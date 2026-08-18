//! whisper.cpp worker: one worker thread, a small context plus an optional
//! Turbo finals context, and VAD-cut utterances in → Partial/Final text out.
//! Decode parameters follow docs/dev/stt-engine.md §3.3.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

#[cfg(all(target_os = "windows", feature = "windows-vulkan"))]
use std::sync::atomic::{AtomicBool, Ordering};

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use super::mailbox::{DecodeJob, JobMailboxRx};
use super::{SttEvent, Word};
use crate::capture::TARGET_RATE;
use crate::settings::Settings;
use crate::CaptionsStatus;

#[cfg(all(target_os = "windows", feature = "windows-vulkan"))]
static VULKAN_BACKEND_CONFIRMED: AtomicBool = AtomicBool::new(false);

#[cfg(all(target_os = "windows", feature = "windows-vulkan"))]
struct WhisperNativeLogger;

#[cfg(all(target_os = "windows", feature = "windows-vulkan"))]
impl log::Log for WhisperNativeLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let message = record.args().to_string();
        if message.contains("using Vulkan") && message.contains(" backend") {
            // whisper.cpp prints this immediately before initialization. Keep
            // it as a candidate until a possible synchronous failure log has
            // had a chance to clear it and context creation returns.
            VULKAN_BACKEND_CONFIRMED.store(true, Ordering::Release);
        } else if message.contains("failed to initialize Vulkan") && message.contains(" backend") {
            VULKAN_BACKEND_CONFIRMED.store(false, Ordering::Release);
            crate::diag::log("native backend confirmation: cpu (Vulkan init failed)");
        } else if message.contains("no GPU found") {
            VULKAN_BACKEND_CONFIRMED.store(false, Ordering::Release);
            crate::diag::log("native backend confirmation: cpu (no GPU found)");
        }
    }

    fn flush(&self) {}
}

#[cfg(all(target_os = "windows", feature = "windows-vulkan"))]
static WHISPER_NATIVE_LOGGER: WhisperNativeLogger = WhisperNativeLogger;

#[cfg(all(target_os = "windows", feature = "windows-vulkan"))]
fn install_whisper_logging() {
    VULKAN_BACKEND_CONFIRMED.store(false, Ordering::Release);
    if log::set_logger(&WHISPER_NATIVE_LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    } else {
        crate::diag::log("native backend confirmation unavailable: logger already installed");
    }
    whisper_rs::install_logging_hooks();
}

#[cfg(not(all(target_os = "windows", feature = "windows-vulkan")))]
fn install_whisper_logging() {
    whisper_rs::install_logging_hooks();
}

/// whisper misbehaves on very short inputs; pad to at least ~1.1 s.
const MIN_SAMPLES: usize = (TARGET_RATE as usize * 11) / 10;
const WINDOWS_TURBO_FINAL_MAX_MS: u64 = 6_000;
const WINDOWS_FINAL_BACKLOG_MS: u128 = 1_600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecodeModel {
    Small,
    Turbo,
}

impl DecodeModel {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Turbo => "turbo",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchMode {
    Greedy,
    Beam { size: i32 },
}

impl SearchMode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Greedy => "greedy",
            Self::Beam { size: 3 } => "beam3",
            Self::Beam { size: 5 } => "beam5",
            Self::Beam { .. } => "beam",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AudioContext {
    Trimmed,
    Full,
}

impl AudioContext {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Trimmed => "trimmed",
            Self::Full => "full",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DecodeProfile {
    model: DecodeModel,
    search: SearchMode,
    context: AudioContext,
}

const SMALL_GREEDY_TRIMMED: DecodeProfile = DecodeProfile {
    model: DecodeModel::Small,
    search: SearchMode::Greedy,
    context: AudioContext::Trimmed,
};
const SMALL_BEAM3_FULL: DecodeProfile = DecodeProfile {
    model: DecodeModel::Small,
    search: SearchMode::Beam { size: 3 },
    context: AudioContext::Full,
};
const TURBO_GREEDY_TRIMMED: DecodeProfile = DecodeProfile {
    model: DecodeModel::Turbo,
    search: SearchMode::Greedy,
    context: AudioContext::Trimmed,
};
const TURBO_BEAM5_FULL: DecodeProfile = DecodeProfile {
    model: DecodeModel::Turbo,
    search: SearchMode::Beam { size: 5 },
    context: AudioContext::Full,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FinalReason {
    Endpoint,
    Cap,
    Backlog,
    LongAudio,
}

impl FinalReason {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Endpoint => "endpoint",
            Self::Cap => "cap",
            Self::Backlog => "backlog",
            Self::LongAudio => "long_audio",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FinalSelection {
    profile: DecodeProfile,
    reason: FinalReason,
    timing: WordTiming,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WordTiming {
    Token,
    Utterance,
}

impl WordTiming {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Token => "token",
            Self::Utterance => "utterance",
        }
    }
}

fn final_selection(profile: DecodeProfile, reason: FinalReason) -> FinalSelection {
    // Turbo + trimmed encoder context + token timestamps has a deterministic
    // decoder-loop mode on 5–8 s Portuguese utterances. Without token timings,
    // the same profile keeps its quality/speed advantage and does not repeat.
    // Empty words deliberately selects the existing whole-utterance speaker
    // attribution fallback.
    let timing = if profile == TURBO_GREEDY_TRIMMED {
        WordTiming::Utterance
    } else {
        WordTiming::Token
    };
    FinalSelection {
        profile,
        reason,
        timing,
    }
}

fn select_final_profile(
    windows: bool,
    turbo_available: bool,
    ended_by_cap: bool,
    queue_ms: u128,
    audio_ms: u64,
) -> FinalSelection {
    if windows {
        if ended_by_cap {
            return final_selection(SMALL_GREEDY_TRIMMED, FinalReason::Cap);
        }
        if queue_ms > WINDOWS_FINAL_BACKLOG_MS {
            return final_selection(SMALL_GREEDY_TRIMMED, FinalReason::Backlog);
        }
        if audio_ms > WINDOWS_TURBO_FINAL_MAX_MS {
            return final_selection(SMALL_GREEDY_TRIMMED, FinalReason::LongAudio);
        }
        return final_selection(
            if turbo_available {
                TURBO_GREEDY_TRIMMED
            } else {
                SMALL_BEAM3_FULL
            },
            FinalReason::Endpoint,
        );
    }

    final_selection(
        // Preserve the existing macOS behavior: Turbo beam5 when its optional
        // finals model is loaded, otherwise the small greedy fallback.
        if turbo_available {
            TURBO_BEAM5_FULL
        } else {
            SMALL_GREEDY_TRIMMED
        },
        if ended_by_cap {
            FinalReason::Cap
        } else {
            FinalReason::Endpoint
        },
    )
}

fn realtime_factor(decode_ms: u128, audio_ms: u64) -> f64 {
    decode_ms as f64 / audio_ms.max(1) as f64
}

/// whisper's classic hallucinations on noise/beeps/silence (YouTube training
/// artifacts) — "Thanks for watching." shows up on every join beep. Matched
/// against the normalized whole line.
fn is_known_hallucination(text: &str) -> bool {
    let norm: String = text
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_ascii_punctuation() && *c != '！' && *c != '。')
        .collect::<String>()
        .trim()
        .to_string();
    const PREFIXES: &[&str] = &[
        "thanks for watching",
        "thank you for watching",
        "thank you so much for watching",
        "obrigado por assistir",
        "obrigada por assistir",
        "gracias por ver",
        "legendas pela comunidade",
        // PT subtitle-credit family — "Legenda por Sônia Ruberti" etc. shows up
        // on Discord's mute/unmute beeps.
        "legenda por",
        "legendas por",
        "legendado por",
        "legendas e revisão",
        "subtítulos realizados por",
        "subtitulos realizados por",
        "sous-titres",
    ];
    // Subtitle-credit hallucinations mention amara.org anywhere in the line.
    norm.contains("amaraorg") || PREFIXES.iter().any(|p| norm.starts_with(p))
}

/// Aggregate token statistics for hallucination gating (skips special tokens).
struct DecodeStats {
    mean_p: f32,
    avg_logprob: f32,
    no_speech_prob: f32,
}

fn collect_stats(state: &whisper_rs::WhisperState) -> DecodeStats {
    let (mut sum_p, mut sum_plog, mut n) = (0.0f32, 0.0f32, 0u32);
    let mut no_speech = 0.0f32;
    for s in 0..state.full_n_segments() {
        let Some(seg) = state.get_segment(s) else {
            continue;
        };
        no_speech = no_speech.max(seg.no_speech_probability());
        for t in 0..seg.n_tokens() {
            let Some(token) = seg.get_token(t) else {
                continue;
            };
            if let Ok(piece) = token.to_str_lossy() {
                if piece.starts_with("[_") || piece.starts_with("<|") {
                    continue;
                }
            }
            let data = token.token_data();
            sum_p += data.p;
            sum_plog += data.plog;
            n += 1;
        }
    }
    let n = n.max(1) as f32;
    DecodeStats {
        mean_p: sum_p / n,
        avg_logprob: sum_plog / n,
        no_speech_prob: no_speech,
    }
}

/// The gates every serious chunked-whisper deployment ships (OpenAI defaults +
/// LocalVocal's mean-probability threshold). See docs/dev research notes.
fn is_low_confidence(stats: &DecodeStats) -> bool {
    (stats.no_speech_prob > 0.6 && stats.avg_logprob < -1.0) || stats.mean_p < 0.4
}

/// CALLOUT_DEBUG_AUDIO=1 → every final's PCM is dumped as a WAV next to the
/// models, so "why did it transcribe that?" becomes listenable evidence.
fn debug_audio_dir(model_path: &std::path::Path) -> Option<PathBuf> {
    if std::env::var("CALLOUT_DEBUG_AUDIO").ok().as_deref() != Some("1") {
        return None;
    }
    let data_root = model_path.ancestors().nth(3)?; // models/whisper/file.bin → data dir
    let dir = data_root.join("debug-audio");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn write_wav_16k_mono(path: &std::path::Path, pcm: &[f32]) {
    let n = pcm.len() as u32;
    let data_bytes = n * 2;
    let mut out = Vec::with_capacity(44 + data_bytes as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&16_000u32.to_le_bytes());
    out.extend_from_slice(&32_000u32.to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    let _ = std::fs::write(path, out);
}

fn decode_thread_count(available: usize, windows: bool) -> i32 {
    // Leave more headroom on Windows for Discord and the game. Three worker
    // threads is the balanced profile: two risks falling behind, while four
    // leaves too little CPU headroom during continuous speech.
    let max_threads = if windows { 3 } else { 6 };
    available.saturating_sub(2).clamp(2, max_threads) as i32
}

#[derive(Debug, Clone)]
struct BackendChoice {
    label: &'static str,
    use_gpu: bool,
    gpu_device: i32,
    vram_mb: usize,
}

impl BackendChoice {
    const fn cpu() -> Self {
        Self {
            label: "cpu",
            use_gpu: false,
            gpu_device: 0,
            vram_mb: 0,
        }
    }

    fn context_params(&self) -> WhisperContextParameters<'static> {
        let mut params = WhisperContextParameters::default();
        params.use_gpu(self.use_gpu).gpu_device(self.gpu_device);
        params
    }
}

#[cfg(all(target_os = "windows", feature = "windows-vulkan"))]
fn preferred_backend() -> BackendChoice {
    if std::env::var_os("UNMUTE_FORCE_CPU").is_some() {
        eprintln!("[stt] UNMUTE_FORCE_CPU set; Vulkan disabled for this run");
        return BackendChoice::cpu();
    }

    let devices = whisper_rs::vulkan::list_devices();
    for device in &devices {
        eprintln!(
            "[stt] Vulkan device {}: {} ({} MiB total, {} MiB free)",
            device.id,
            device.name,
            device.vram.total / (1024 * 1024),
            device.vram.free / (1024 * 1024),
        );
    }

    // ggml orders discrete adapters before integrated ones. Do not rank by
    // reported memory: an iGPU's shared system heap can appear larger than a
    // real dGPU's VRAM and silently invalidate the gaming-PC A/B.
    devices
        .into_iter()
        .next()
        .map(|device| {
            let device_name = device.name.trim().replace(['\r', '\n'], " ");
            crate::diag::log(&format!(
                "vulkan adapter: device={} vram_mb={} name={device_name}",
                device.id,
                device.vram.total / (1024 * 1024),
            ));
            BackendChoice {
                label: "vulkan",
                use_gpu: true,
                gpu_device: device.id,
                vram_mb: device.vram.total / (1024 * 1024),
            }
        })
        .unwrap_or_else(|| {
            eprintln!("[stt] no Vulkan device found; using CPU fallback");
            BackendChoice::cpu()
        })
}

#[cfg(all(target_os = "windows", not(feature = "windows-vulkan")))]
fn preferred_backend() -> BackendChoice {
    BackendChoice::cpu()
}

#[cfg(target_os = "macos")]
fn preferred_backend() -> BackendChoice {
    BackendChoice {
        label: "metal",
        use_gpu: true,
        gpu_device: 0,
        vram_mb: 0,
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn preferred_backend() -> BackendChoice {
    BackendChoice::cpu()
}

#[cfg(all(target_os = "windows", feature = "windows-vulkan"))]
fn confirmed_backend(requested: &BackendChoice, loaded: BackendChoice) -> BackendChoice {
    if requested.use_gpu && loaded.use_gpu {
        if VULKAN_BACKEND_CONFIRMED.load(Ordering::Acquire) {
            crate::diag::log("native backend confirmation: vulkan");
            loaded
        } else {
            eprintln!(
                "[stt] Vulkan device was enumerated but whisper.cpp did not confirm the backend; treating the active context as CPU"
            );
            BackendChoice::cpu()
        }
    } else {
        loaded
    }
}

#[cfg(not(all(target_os = "windows", feature = "windows-vulkan")))]
fn confirmed_backend(_requested: &BackendChoice, loaded: BackendChoice) -> BackendChoice {
    loaded
}

#[cfg(all(target_os = "windows", feature = "windows-vulkan"))]
fn begin_backend_probe(backend: &BackendChoice) {
    if backend.use_gpu {
        VULKAN_BACKEND_CONFIRMED.store(false, Ordering::Release);
    }
}

#[cfg(not(all(target_os = "windows", feature = "windows-vulkan")))]
fn begin_backend_probe(_backend: &BackendChoice) {}

fn load_model(
    path: &Path,
    backend: &BackendChoice,
) -> Result<(WhisperContext, whisper_rs::WhisperState), String> {
    begin_backend_probe(backend);
    let context = WhisperContext::new_with_params(path, backend.context_params())
        .map_err(|error| format!("context: {error}"))?;
    let state = context
        .create_state()
        .map_err(|error| format!("state: {error}"))?;
    Ok((context, state))
}

pub fn spawn_worker(
    model_path: PathBuf,
    turbo_path: Option<PathBuf>,
    settings: Arc<RwLock<Settings>>,
    mut job_rx: JobMailboxRx,
    event_tx: tokio::sync::mpsc::UnboundedSender<SttEvent>,
    status_tx: tokio::sync::mpsc::UnboundedSender<CaptionsStatus>,
) {
    std::thread::Builder::new()
        .name("callout-whisper".into())
        .spawn(move || {
            let _ = status_tx.send(CaptionsStatus::LoadingModel);
            install_whisper_logging();
            let requested_backend = preferred_backend();
            let (_ctx, mut state, loaded_backend) = match load_model(
                &model_path,
                &requested_backend,
            ) {
                Ok((context, state)) => (context, state, requested_backend.clone()),
                Err(gpu_error) if requested_backend.use_gpu => {
                    eprintln!(
                        "[stt] {} model load failed ({gpu_error}); retrying on CPU",
                        requested_backend.label
                    );
                    let cpu = BackendChoice::cpu();
                    match load_model(&model_path, &cpu) {
                        Ok((context, state)) => (context, state, cpu),
                        Err(cpu_error) => {
                            let _ = status_tx.send(CaptionsStatus::SttError {
                                message: format!(
                                    "failed to load whisper model on {} ({gpu_error}) and CPU ({cpu_error})",
                                    requested_backend.label
                                ),
                            });
                            return;
                        }
                    }
                }
                Err(error) => {
                    let _ = status_tx.send(CaptionsStatus::SttError {
                        message: format!("failed to load whisper model: {error}"),
                    });
                    return;
                }
            };
            let active_backend = confirmed_backend(&requested_backend, loaded_backend);
            crate::diag::log(&format!(
                "stt backend: requested={} active={} gpu_device={} gpu_vram_mb={}",
                requested_backend.label,
                active_backend.label,
                active_backend.gpu_device,
                active_backend.vram_mb,
            ));
            // Optional second context for high-quality finals. Model choice,
            // search strategy, and encoder context are selected independently
            // per Final below; partials always remain on the small model.
            let turbo = turbo_path.filter(|p| p.is_file()).and_then(|p| {
                let t0 = std::time::Instant::now();
                match load_model(&p, &active_backend) {
                    Ok(pair) => {
                        let confirmed =
                            confirmed_backend(&active_backend, active_backend.clone());
                        if active_backend.use_gpu && !confirmed.use_gpu {
                            eprintln!(
                                "[stt] finals model did not initialize Vulkan; using small for finals"
                            );
                            return None;
                        }
                        eprintln!("[stt] finals model loaded in {:?}", t0.elapsed());
                        Some(pair)
                    }
                    Err(e) => {
                        eprintln!("[stt] finals model failed to load ({e}); using small for finals");
                        None
                    }
                }
            });
            let mut turbo_state = turbo;
            eprintln!(
                "[stt] finals models: {}",
                if turbo_state.is_some() {
                    "small + large-v3-turbo"
                } else {
                    "small only"
                }
            );
            let _ = status_tx.send(CaptionsStatus::SttReady);

            let available_threads = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4);
            let threads = decode_thread_count(available_threads, cfg!(windows));
            crate::diag::log(&format!(
                "stt profile: backend={} decode_threads={threads} available_threads={available_threads} partial_model=small partial_search=greedy partial_context=trimmed partial_cadence_ms={} partial_tail_s={}",
                active_backend.label,
                if cfg!(windows) { 1_600 } else { 600 },
                if cfg!(windows) { 6 } else { 0 }
            ));
            let mut last_final = String::new();
            // Language is decided once per utterance and cached for its partials.
            let mut utt_key: u64 = u64::MAX;
            let mut utt_lang: Option<String> = None;

            while let Ok(job) = job_rx.recv_next() {
                let (utterance_id, queue_ms) = match &job {
                    DecodeJob::Partial(job) => {
                        (job.utterance_id, job.queued_at.elapsed().as_millis())
                    }
                    DecodeJob::Final(job) => {
                        (job.utterance_id, job.queued_at.elapsed().as_millis())
                    }
                };
                let new_utterance = utterance_id != utt_key;
                if new_utterance {
                    utt_key = utterance_id;
                    utt_lang = None;
                }
                match job {
                    DecodeJob::Partial(job) => {
                        let decode_t0 = std::time::Instant::now();
                        if new_utterance {
                            let allowed = settings
                                .read()
                                .map(|s| s.languages.clone())
                                .unwrap_or_default();
                            utt_lang =
                                choose_language(&mut state, &job.pcm, &allowed, threads);
                        }
                        let audio_ms =
                            job.pcm.len() as u64 * 1_000 / TARGET_RATE as u64;
                        let decoded = decode(&mut state, &job.pcm, utt_lang.as_deref(), threads);
                        let decode_ms = decode_t0.elapsed().as_millis();
                        let rtf = realtime_factor(decode_ms, audio_ms);
                        crate::diag::log(&format!(
                            "partial decode: model={} search={} context={} reason=live queue_ms={queue_ms} decode_ms={decode_ms} audio_ms={audio_ms} rtf={rtf:.3} utterance_span_ms={} threads={threads} window_offset_ms={}",
                            SMALL_GREEDY_TRIMMED.model.as_str(),
                            SMALL_GREEDY_TRIMMED.search.as_str(),
                            SMALL_GREEDY_TRIMMED.context.as_str(),
                            job.pcm_end_ms.saturating_sub(job.utterance_start_ms),
                            job.pcm_start_ms.saturating_sub(job.utterance_start_ms),
                        ));
                        if let Some(text) = decoded {
                            let stats = collect_stats(&state);
                            if !text.is_empty()
                                && !is_known_hallucination(&text)
                                && stats.mean_p >= 0.4
                            {
                                let _ = event_tx.send(SttEvent::Partial {
                                    text,
                                    t_start_ms: job.pcm_start_ms,
                                    t_end_ms: job.pcm_end_ms,
                                });
                            }
                        }
                    }
                    DecodeJob::Final(job) => {
                        let pcm = job.pcm;
                        let t_start_ms = job.t_start_ms;
                        let t_end_ms = job.t_end_ms;
                        let audio_ms = pcm.len() as u64 * 1_000 / TARGET_RATE as u64;
                        let selection = select_final_profile(
                            cfg!(windows),
                            turbo_state.is_some(),
                            job.ended_by_cap,
                            queue_ms,
                            audio_ms,
                        );
                        if let Some(dir) = debug_audio_dir(&model_path) {
                            write_wav_16k_mono(&dir.join(format!("utt_{t_start_ms}.wav")), &pcm);
                        }
                        let decode_t0 = std::time::Instant::now();
                        let fin_state = match selection.profile.model {
                            DecodeModel::Small => &mut state,
                            DecodeModel::Turbo => &mut turbo_state
                                .as_mut()
                                .expect("Turbo profile requires a loaded Turbo state")
                                .1,
                        };
                        let allowed =
                            settings.read().map(|s| s.languages.clone()).unwrap_or_default();
                        // A Final may be the utterance's first job. Pick once
                        // from its full audio; for several allowed languages,
                        // this also deliberately refreshes a Partial's guess.
                        utt_lang = choose_language(fin_state, &pcm, &allowed, threads);
                        // Most Finals collect per-word timings. The Windows
                        // Turbo/trimmed profile deliberately uses the existing
                        // whole-utterance fallback; see final_selection().
                        let decoded = decode_final(
                            fin_state,
                            &pcm,
                            utt_lang.as_deref(),
                            threads,
                            t_start_ms,
                            selection.profile,
                            selection.timing,
                        );
                        let decode_ms = decode_t0.elapsed().as_millis();
                        let rtf = realtime_factor(decode_ms, audio_ms);
                        crate::diag::log(&format!(
                            "final decode: model={} search={} context={} timestamps={} reason={} queue_ms={queue_ms} decode_ms={decode_ms} audio_ms={audio_ms} rtf={rtf:.3} threads={threads}",
                            selection.profile.model.as_str(),
                            selection.profile.search.as_str(),
                            selection.profile.context.as_str(),
                            selection.timing.as_str(),
                            selection.reason.as_str(),
                        ));
                        if let Some((text, words)) = decoded {
                            // Hallucination guards: empty, exact repeat of the
                            // previous final, a known noise hallucination, or a
                            // low-confidence decode (probability gates).
                            let stats = collect_stats(fin_state);
                            if is_low_confidence(&stats) {
                                eprintln!(
                                    "[stt] gated final (mean_p={:.2} no_speech={:.2} logprob={:.2})",
                                    stats.mean_p, stats.no_speech_prob, stats.avg_logprob
                                );
                            } else if !text.is_empty()
                                && text != last_final
                                && !is_known_hallucination(&text)
                                && !is_noise_final(&text)
                            {
                                last_final = text.clone();
                                let _ = event_tx.send(SttEvent::Final {
                                    text,
                                    words,
                                    pcm,
                                    t_start_ms,
                                    t_end_ms,
                                });
                            }
                        }
                    }
                }
            }
        })
        .expect("spawn whisper thread");
}

/// Pick the decode language per the user's setting: [] = full auto (None),
/// [one] = hard pin, [several] = whisper's language detector restricted to
/// that set (best of the allowed probabilities).
fn choose_language(
    state: &mut whisper_rs::WhisperState,
    pcm: &[f32],
    allowed: &[String],
    threads: i32,
) -> Option<String> {
    match allowed {
        [] => None,
        [one] => Some(one.clone()),
        several => {
            let mut padded;
            let samples = if pcm.len() < MIN_SAMPLES {
                padded = pcm.to_vec();
                padded.resize(MIN_SAMPLES, 0.0);
                &padded[..]
            } else {
                pcm
            };
            state.pcm_to_mel(samples, threads as usize).ok()?;
            let (_, probs) = state.lang_detect(0, threads as usize).ok()?;
            several
                .iter()
                .filter_map(|code| {
                    let id = whisper_rs::get_lang_id(code)?;
                    let p = probs.get(id as usize).copied().unwrap_or(0.0);
                    Some((code.clone(), p))
                })
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(code, _)| code)
        }
    }
}

fn decode(
    state: &mut whisper_rs::WhisperState,
    pcm: &[f32],
    lang: Option<&str>,
    threads: i32,
) -> Option<String> {
    run_full(state, pcm, lang, threads, false, SMALL_GREEDY_TRIMMED)?;
    Some(collect_text(state))
}

/// Final decode: word timings on the shared clock (empty if degenerate →
/// whole-utterance attribution fallback). Model selection happens in the
/// worker; this function applies the independently selected search/context.
fn decode_final(
    state: &mut whisper_rs::WhisperState,
    pcm: &[f32],
    lang: Option<&str>,
    threads: i32,
    base_ms: u64,
    profile: DecodeProfile,
    timing: WordTiming,
) -> Option<(String, Vec<Word>)> {
    let token_timestamps = timing == WordTiming::Token;
    run_full(state, pcm, lang, threads, token_timestamps, profile)?;
    let text = collapse_repeats(&collect_text(state));
    let words = if token_timestamps {
        collect_words(state, base_ms)
    } else {
        Vec::new()
    };
    Some((text, words))
}

/// Collapse the "X. X. X." decoder-loop leak: consecutive identical sentences
/// become one. (Doesn't touch legitimate repeats separated by other speech.)
fn collapse_repeats(text: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for part in text.split_inclusive(['.', '!', '?']) {
        let t = part.trim().trim_end_matches(['.', '!', '?']).trim();
        if t.is_empty() {
            continue;
        }
        let dup = out
            .last()
            .map(|l| {
                l.trim()
                    .trim_end_matches(['.', '!', '?'])
                    .trim()
                    .eq_ignore_ascii_case(t)
            })
            .unwrap_or(false);
        if !dup {
            out.push(part);
        }
    }
    if out.is_empty() {
        return text.trim().to_string();
    }
    out.join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn collect_text(state: &whisper_rs::WhisperState) -> String {
    let mut text = String::new();
    for i in 0..state.full_n_segments() {
        if let Some(seg) = state.get_segment(i) {
            if let Ok(s) = seg.to_str_lossy() {
                text.push_str(&s);
            }
        }
    }
    let mut text = text.trim().to_string();
    // Trailing "..." runs are a whisper noise tic (anarlog strips them too).
    while text.ends_with("..") {
        text.pop();
    }
    text.trim().to_string()
}

/// Single-word noise outputs whisper emits on breaths/music — never worth a line.
/// (anarlog ships the same blocklist.)
fn is_noise_final(text: &str) -> bool {
    matches!(
        text.trim()
            .to_lowercase()
            .trim_end_matches(['.', '!'])
            .trim(),
        // "música"/"music" are whisper's [music] tags leaking as words on beeps.
        "you"
            | "thank you"
            | "♪"
            | "obrigado"
            | "obrigada"
            | "gracias"
            | "música"
            | "musica"
            | "music"
    )
}

/// Group whisper's subword tokens into words with absolute-clock spans.
/// Token t0/t1 are centiseconds relative to the fed buffer.
fn collect_words(state: &whisper_rs::WhisperState, base_ms: u64) -> Vec<Word> {
    let mut words: Vec<Word> = Vec::new();
    for s in 0..state.full_n_segments() {
        let Some(seg) = state.get_segment(s) else {
            continue;
        };
        for t in 0..seg.n_tokens() {
            let Some(token) = seg.get_token(t) else {
                continue;
            };
            let Ok(piece) = token.to_str_lossy() else {
                continue;
            };
            // Skip special tokens ("[_BEG_]", "<|pt|>", …).
            if piece.starts_with("[_") || piece.starts_with("<|") {
                continue;
            }
            let data = token.token_data();
            let (t0, t1) = (data.t0, data.t1);
            if t0 < 0 || t1 < t0 {
                return Vec::new(); // degenerate timings → whole-utterance fallback
            }
            let t0_ms = base_ms + (t0 as u64) * 10;
            let t1_ms = base_ms + (t1 as u64) * 10;
            let starts_word = piece.starts_with(' ') || words.is_empty();
            if starts_word {
                words.push(Word {
                    text: piece.trim_start().to_string(),
                    t0_ms,
                    t1_ms,
                });
            } else if let Some(last) = words.last_mut() {
                last.text.push_str(&piece);
                last.t1_ms = last.t1_ms.max(t1_ms);
            }
        }
    }
    words.retain(|w| !w.text.is_empty());
    words
}

fn trimmed_audio_ctx(samples: usize) -> i32 {
    let len_s = samples as f32 / TARGET_RATE as f32;
    ((len_s / 30.0 * 1500.0) as i32 + 128).clamp(512, 1500)
}

fn run_full(
    state: &mut whisper_rs::WhisperState,
    pcm: &[f32],
    lang: Option<&str>,
    threads: i32,
    token_timestamps: bool,
    profile: DecodeProfile,
) -> Option<()> {
    let mut padded;
    let samples = if pcm.len() < MIN_SAMPLES {
        padded = pcm.to_vec();
        padded.resize(MIN_SAMPLES, 0.0);
        &padded[..]
    } else {
        pcm
    };

    let strategy = match profile.search {
        SearchMode::Greedy => SamplingStrategy::Greedy { best_of: 1 },
        SearchMode::Beam { size } => SamplingStrategy::BeamSearch {
            beam_size: size,
            patience: 1.0,
        },
    };
    let mut params = FullParams::new(strategy);
    params.set_language(Some(lang.unwrap_or("auto")));
    params.set_no_context(true);
    params.set_single_segment(true);
    params.set_suppress_blank(true);
    params.set_suppress_nst(true);
    params.set_temperature(0.0);
    params.set_temperature_inc(0.2);
    // Token timestamps enable word attribution. The Windows Turbo/trimmed
    // profile deliberately disables them because they can also change Turbo's
    // decoded text and trigger repeated sentence sequences; Small and the
    // full-context Turbo path keep them enabled.
    params.set_no_timestamps(!token_timestamps);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_n_threads(threads);
    match profile.context {
        // The CPU latency path: scale encoder work to the actual input.
        AudioContext::Trimmed => params.set_audio_ctx(trimmed_audio_ctx(samples.len())),
        // Beam finals use the model's full encoder context. The repo previously
        // observed PT decoder loops when beam search shared the trimmed context.
        AudioContext::Full => params.set_audio_ctx(0),
    }
    if token_timestamps {
        params.set_token_timestamps(true);
    }

    if let Err(e) = state.full(params, samples) {
        eprintln!("[stt] decode failed: {e}");
        return None;
    }
    Some(())
}

/// A/B harness: checks that enabling token timestamps does not change Small's
/// transcribed text. Turbo/trimmed is covered separately because timestamps do
/// affect its decoder and are disabled in that Windows production profile.
/// Run explicitly with fixtures:
///   CALLOUT_AB_DIR=<dir-with-wavs> cargo test --release -- --ignored ab_token
/// CI speed gate: the small model must decode faster than realtime on the
/// build machine, or captions can't keep up with speech (field-measured 15x
/// slower when ggml is built by MSVC without AVX2/FMA — kernel speed is
/// content-independent, so synthetic audio measures it fine).
///   CALLOUT_BENCH_MODEL=<ggml-small.bin> cargo test --release -- --ignored small_model
#[cfg(test)]
mod speed_gate {
    use super::*;
    use whisper_rs::{WhisperContext, WhisperContextParameters};

    #[test]
    fn partial_profile_is_small_greedy_trimmed() {
        assert_eq!(
            SMALL_GREEDY_TRIMMED,
            DecodeProfile {
                model: DecodeModel::Small,
                search: SearchMode::Greedy,
                context: AudioContext::Trimmed,
            }
        );
    }

    #[test]
    fn windows_short_endpoint_prefers_turbo_then_small_beam_fallback() {
        let turbo = select_final_profile(true, true, false, 1_600, 6_000);
        assert_eq!(turbo.profile, TURBO_GREEDY_TRIMMED);
        assert_eq!(turbo.reason, FinalReason::Endpoint);
        assert_eq!(turbo.timing, WordTiming::Utterance);

        let no_turbo = select_final_profile(true, false, false, 1_600, 6_000);
        assert_eq!(no_turbo.profile, SMALL_BEAM3_FULL);
        assert_eq!(no_turbo.reason, FinalReason::Endpoint);
        assert_eq!(no_turbo.timing, WordTiming::Token);
    }

    #[test]
    fn windows_cap_long_audio_and_backlog_fall_back_to_small_greedy() {
        let cap = select_final_profile(true, true, true, 2_000, 12_000);
        assert_eq!(cap.profile, SMALL_GREEDY_TRIMMED);
        assert_eq!(cap.reason, FinalReason::Cap);
        assert_eq!(cap.timing, WordTiming::Token);

        let backlog = select_final_profile(true, true, false, 1_601, 4_000);
        assert_eq!(backlog.profile, SMALL_GREEDY_TRIMMED);
        assert_eq!(backlog.reason, FinalReason::Backlog);

        let long = select_final_profile(true, true, false, 0, 6_001);
        assert_eq!(long.profile, SMALL_GREEDY_TRIMMED);
        assert_eq!(long.reason, FinalReason::LongAudio);
    }

    #[test]
    fn non_windows_keeps_existing_turbo_beam5_policy() {
        let turbo = select_final_profile(false, true, false, 9_000, 30_000);
        assert_eq!(turbo.profile, TURBO_BEAM5_FULL);
        assert_eq!(turbo.reason, FinalReason::Endpoint);

        let fallback = select_final_profile(false, false, false, 0, 3_000);
        assert_eq!(fallback.profile, SMALL_GREEDY_TRIMMED);
    }

    #[test]
    fn trimmed_context_is_bounded_and_tracks_audio_length() {
        assert_eq!(trimmed_audio_ctx(0), 512);
        assert_eq!(trimmed_audio_ctx(5 * TARGET_RATE as usize), 512);
        assert_eq!(trimmed_audio_ctx(12 * TARGET_RATE as usize), 728);
        assert_eq!(trimmed_audio_ctx(30 * TARGET_RATE as usize), 1_500);
    }

    #[test]
    fn balanced_windows_profile_caps_decode_at_three_threads() {
        assert_eq!(decode_thread_count(16, true), 3);
        assert_eq!(decode_thread_count(8, true), 3);
        assert_eq!(decode_thread_count(4, true), 2);
        assert_eq!(decode_thread_count(8, false), 6);
    }

    #[test]
    #[ignore]
    fn small_model_decodes_faster_than_realtime() {
        let Ok(model) = std::env::var("CALLOUT_BENCH_MODEL") else {
            eprintln!("CALLOUT_BENCH_MODEL not set; skipping");
            return;
        };
        let ctx = WhisperContext::new_with_params(&model, WhisperContextParameters::default())
            .expect("load small model");
        let mut state = ctx.create_state().expect("state");
        const SECS: usize = 5;
        let pcm: Vec<f32> = (0..SECS * TARGET_RATE as usize)
            .map(|i| (i as f32 * 0.05).sin() * 0.01)
            .collect();
        let t0 = std::time::Instant::now();
        let threads = decode_thread_count(
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4),
            cfg!(windows),
        );
        let _ = decode(&mut state, &pcm, Some("en"), threads);
        let rtf = t0.elapsed().as_secs_f32() / SECS as f32;
        eprintln!(
            "[bench] realtime factor: {rtf:.3} ({}ms for {SECS}s audio, {threads} threads)",
            t0.elapsed().as_millis()
        );
        assert!(
            rtf < 0.8,
            "small-model decode slower than realtime (rtf={rtf:.2}) — ggml built without SIMD?"
        );
    }
}

#[cfg(test)]
mod ab_tests {
    use super::*;

    #[derive(Clone, Copy, Debug)]
    enum QualityMode {
        GreedyTrimmed,
        GreedyFull,
        BeamFull(i32),
    }

    fn read_wav_16k_mono(path: &std::path::Path) -> Vec<f32> {
        let bytes = std::fs::read(path).expect("read wav");
        // Find the "data" chunk (fixtures come from afconvert, LEI16@16000 mono).
        let pos = bytes
            .windows(4)
            .position(|w| w == b"data")
            .expect("data chunk")
            + 8;
        bytes[pos..]
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
            .collect()
    }

    fn decode_quality_mode(
        state: &mut whisper_rs::WhisperState,
        pcm: &[f32],
        mode: QualityMode,
        threads: i32,
        token_timestamps: bool,
    ) -> String {
        let strategy = match mode {
            QualityMode::GreedyTrimmed | QualityMode::GreedyFull => {
                SamplingStrategy::Greedy { best_of: 1 }
            }
            QualityMode::BeamFull(beam_size) => SamplingStrategy::BeamSearch {
                beam_size,
                patience: 1.0,
            },
        };
        let mut params = FullParams::new(strategy);
        params.set_language(Some("pt"));
        params.set_no_context(true);
        params.set_single_segment(true);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);
        params.set_temperature(0.0);
        params.set_temperature_inc(0.2);
        params.set_no_timestamps(!token_timestamps);
        if token_timestamps {
            params.set_token_timestamps(true);
        }
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_n_threads(threads);
        match mode {
            QualityMode::GreedyTrimmed => {
                let len_s = pcm.len() as f32 / TARGET_RATE as f32;
                let audio_ctx = ((len_s / 30.0 * 1500.0) as i32 + 128).clamp(512, 1500);
                params.set_audio_ctx(audio_ctx);
            }
            QualityMode::GreedyFull | QualityMode::BeamFull(_) => params.set_audio_ctx(0),
        }
        state.full(params, pcm).expect("quality decode");
        collapse_repeats(&collect_text(state))
    }

    fn normalize_pt(text: &str) -> Vec<String> {
        text.to_lowercase()
            .chars()
            .map(|c| match c {
                'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
                'é' | 'è' | 'ê' | 'ë' => 'e',
                'í' | 'ì' | 'î' | 'ï' => 'i',
                'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
                'ú' | 'ù' | 'û' | 'ü' => 'u',
                'ç' => 'c',
                c if c.is_alphanumeric() => c,
                _ => ' ',
            })
            .collect::<String>()
            .split_whitespace()
            .map(str::to_string)
            .collect()
    }

    fn word_errors(expected: &[String], actual: &[String]) -> usize {
        let mut previous: Vec<usize> = (0..=actual.len()).collect();
        let mut current = vec![0; actual.len() + 1];
        for (i, expected_word) in expected.iter().enumerate() {
            current[0] = i + 1;
            for (j, actual_word) in actual.iter().enumerate() {
                current[j + 1] = (previous[j + 1] + 1)
                    .min(current[j] + 1)
                    .min(previous[j] + usize::from(expected_word != actual_word));
            }
            std::mem::swap(&mut previous, &mut current);
        }
        previous[actual.len()]
    }

    /// Diagnostic A/B for Windows quality decisions. Fixtures named
    /// `pt01_v1.wav` … `pt10_vN.wav` map to the phrase list below.
    ///
    /// CALLOUT_AB_DIR=<fixtures> \
    /// CALLOUT_SMALL_MODEL=<ggml-small-q5_1.bin> \
    /// CALLOUT_TURBO_MODEL=<ggml-large-v3-turbo-q5_0.bin> \
    /// cargo test --release -- --ignored ab_windows_quality --nocapture
    #[test]
    #[ignore]
    fn ab_windows_quality_profiles_portuguese() {
        let Some(dir) = std::env::var_os("CALLOUT_AB_DIR") else {
            eprintln!("CALLOUT_AB_DIR not set; skipping");
            return;
        };
        let Ok(small_model) = std::env::var("CALLOUT_SMALL_MODEL") else {
            eprintln!("CALLOUT_SMALL_MODEL not set; skipping");
            return;
        };
        let Ok(turbo_model) = std::env::var("CALLOUT_TURBO_MODEL") else {
            eprintln!("CALLOUT_TURBO_MODEL not set; skipping");
            return;
        };
        const EXPECTED: &[&str] = &[
            "foi mal",
            "abre a porta",
            "cuidado pela direita",
            "não vai pelo meio",
            "tem dois no B",
            "ele está atrás de você",
            "pega minha arma",
            "vamos rotacionar agora",
            "são quinze segundos",
            "não atira ainda",
        ];

        let mut wavs: Vec<_> = std::fs::read_dir(dir)
            .expect("fixture dir")
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.extension().is_some_and(|ext| ext == "wav")
                    && path
                        .file_stem()
                        .is_some_and(|stem| stem.to_string_lossy().contains("_v"))
            })
            .collect();
        wavs.sort();
        assert!(!wavs.is_empty(), "no ptNN_vN.wav fixtures found");

        let mut profiles = vec![
            (
                "small-greedy-trimmed",
                small_model.as_str(),
                QualityMode::GreedyTrimmed,
                true,
            ),
            (
                "small-beam3-full",
                small_model.as_str(),
                QualityMode::BeamFull(3),
                true,
            ),
            (
                "turbo-greedy-trimmed-ts",
                turbo_model.as_str(),
                QualityMode::GreedyTrimmed,
                true,
            ),
            (
                "turbo-greedy-trimmed-no-ts",
                turbo_model.as_str(),
                QualityMode::GreedyTrimmed,
                false,
            ),
        ];
        if std::env::var("CALLOUT_AB_SLOW").ok().as_deref() == Some("1") {
            profiles.extend([
                (
                    "turbo-greedy-full",
                    turbo_model.as_str(),
                    QualityMode::GreedyFull,
                    true,
                ),
                (
                    "turbo-beam3-full",
                    turbo_model.as_str(),
                    QualityMode::BeamFull(3),
                    true,
                ),
                (
                    "turbo-beam5-full",
                    turbo_model.as_str(),
                    QualityMode::BeamFull(5),
                    true,
                ),
            ]);
        }

        let profile_filter = std::env::var("CALLOUT_AB_PROFILE").ok();
        for (profile, model, mode, token_timestamps) in profiles {
            if profile_filter
                .as_ref()
                .is_some_and(|filter| !profile.contains(filter))
            {
                continue;
            }
            let ctx = WhisperContext::new_with_params(model, WhisperContextParameters::default())
                .expect("load quality model");
            let mut state = ctx.create_state().expect("quality state");
            let warmup = read_wav_16k_mono(&wavs[0]);
            let _ = decode_quality_mode(&mut state, &warmup, mode, 3, token_timestamps);

            let mut errors = 0usize;
            let mut reference_words = 0usize;
            let mut decode_ms = 0u128;
            for wav in &wavs {
                let stem = wav.file_stem().unwrap().to_string_lossy();
                let phrase_index: usize = stem[2..4].parse().expect("ptNN fixture name");
                let expected = EXPECTED[phrase_index - 1];
                let pcm = read_wav_16k_mono(wav);
                let started = std::time::Instant::now();
                let actual = decode_quality_mode(&mut state, &pcm, mode, 3, token_timestamps);
                let elapsed_ms = started.elapsed().as_millis();
                let expected_words = normalize_pt(expected);
                let actual_words = normalize_pt(&actual);
                let item_errors = word_errors(&expected_words, &actual_words);
                errors += item_errors;
                reference_words += expected_words.len();
                decode_ms += elapsed_ms;
                if item_errors > 0 || phrase_index == 1 {
                    eprintln!(
                        "[quality] {profile} {stem}: {elapsed_ms}ms errors={item_errors} expected={expected:?} actual={actual:?}"
                    );
                }
            }
            let wer = errors as f32 / reference_words as f32;
            eprintln!(
                "[quality-summary] {profile}: WER={wer:.3} errors={errors}/{reference_words} mean_decode_ms={} fixtures={}",
                decode_ms / wavs.len() as u128,
                wavs.len()
            );
        }
    }

    /// Forensic test for the "fluent but unrelated Portuguese" bug: decodes a
    /// clean PT fixture through the macOS Turbo/beam5/full-context path, then
    /// the same audio with holes punched in it (simulating capture drops under
    /// backpressure).
    ///   CALLOUT_AB_DIR=<fixtures> cargo test --release -- --ignored pt_finals --nocapture
    #[test]
    #[ignore]
    fn pt_finals_path_forensics() {
        let Some(dir) = std::env::var_os("CALLOUT_AB_DIR") else {
            eprintln!("CALLOUT_AB_DIR not set; skipping");
            return;
        };
        let home = std::env::var("HOME").unwrap();
        let turbo = std::path::PathBuf::from(&home).join(
            "Library/Application Support/app.callout.desktop/models/whisper/ggml-large-v3-turbo-q5_0.bin",
        );
        let ctx = WhisperContext::new_with_params(&turbo, WhisperContextParameters::default())
            .expect("turbo model");
        let mut state = ctx.create_state().expect("state");
        let pcm = read_wav_16k_mono(&std::path::PathBuf::from(&dir).join("pt1.wav"));

        let (clean, _) = decode_final(
            &mut state,
            &pcm,
            Some("pt"),
            4,
            0,
            TURBO_BEAM5_FULL,
            WordTiming::Token,
        )
        .expect("decode");
        eprintln!("[forensics] clean:   {clean:?}");

        // Punch holes: drop every other 80ms block — spliced audio, like a
        // capture channel dropping blocks under backpressure.
        let block = 1280usize; // 80ms @ 16k
        let chopped: Vec<f32> = pcm
            .chunks(block)
            .enumerate()
            .filter(|(i, _)| i % 2 == 0)
            .flat_map(|(_, c)| c.iter().copied())
            .collect();
        let (holed, _) = decode_final(
            &mut state,
            &chopped,
            Some("pt"),
            4,
            0,
            TURBO_BEAM5_FULL,
            WordTiming::Token,
        )
        .expect("decode");
        eprintln!("[forensics] chopped: {holed:?}");

        let clean_lower = clean.to_lowercase();
        assert!(
            clean_lower.contains("cuidado") && clean_lower.contains("direita"),
            "turbo+beam+full-ctx mangled CLEAN pt audio: {clean:?}"
        );
    }

    #[test]
    #[ignore]
    fn ab_token_timestamps_do_not_change_text() {
        let Some(dir) = std::env::var_os("CALLOUT_AB_DIR") else {
            eprintln!("CALLOUT_AB_DIR not set; skipping");
            return;
        };
        let model = dirs_model_path();
        let ctx = WhisperContext::new_with_params(&model, WhisperContextParameters::default())
            .expect("load model");
        let mut state = ctx.create_state().expect("state");

        let mut wavs: Vec<_> = std::fs::read_dir(dir)
            .expect("fixture dir")
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "wav"))
            .collect();
        wavs.sort();
        assert!(!wavs.is_empty(), "no fixtures found");

        for wav in wavs {
            let name = wav.file_name().unwrap().to_string_lossy().to_string();
            let lang = &name[..2]; // fixtures are named en*/pt*/es*
            let pcm = read_wav_16k_mono(&wav);

            let t = std::time::Instant::now();
            let baseline = decode_with(&mut state, &pcm, lang, false);
            let base_ms = t.elapsed().as_millis();
            let t = std::time::Instant::now();
            let with_ts = decode_with(&mut state, &pcm, lang, true);
            let ts_ms = t.elapsed().as_millis();

            eprintln!("[ab] {name}: baseline {base_ms}ms | +token_ts {ts_ms}ms");
            eprintln!("[ab]   A: {baseline:?}");
            eprintln!("[ab]   B: {with_ts:?}");
            assert_eq!(baseline, with_ts, "text changed for {name}");

            // The timing config must also yield real, ordered word spans.
            let words = collect_words(&state, 0);
            assert!(!words.is_empty(), "no word timings for {name}");
            let spread = words.last().unwrap().t1_ms.saturating_sub(words[0].t0_ms);
            assert!(
                spread > 500,
                "degenerate spans for {name}: spread {spread}ms"
            );
            let dump: Vec<String> = words
                .iter()
                .take(6)
                .map(|w| format!("{}@{}-{}", w.text, w.t0_ms, w.t1_ms))
                .collect();
            eprintln!("[ab]   words: {} …", dump.join(" "));
        }
    }

    fn dirs_model_path() -> std::path::PathBuf {
        let home = std::env::var("HOME").unwrap();
        std::path::PathBuf::from(home).join(
            "Library/Application Support/app.callout.desktop/models/whisper/ggml-small-q5_1.bin",
        )
    }

    fn decode_with(
        state: &mut whisper_rs::WhisperState,
        pcm: &[f32],
        lang: &str,
        token_timestamps: bool,
    ) -> String {
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(lang));
        params.set_no_context(true);
        params.set_single_segment(true);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);
        params.set_temperature(0.0);
        // Timing config: token timestamps only compute real values with
        // timestamp processing enabled — mirror production's decode_final.
        params.set_no_timestamps(!token_timestamps);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_n_threads(4);
        let len_s = pcm.len() as f32 / TARGET_RATE as f32;
        let audio_ctx = ((len_s / 30.0 * 1500.0) as i32 + 128).clamp(512, 1500);
        params.set_audio_ctx(audio_ctx);
        if token_timestamps {
            params.set_token_timestamps(true);
        }
        state.full(params, pcm).expect("decode");
        let mut text = String::new();
        for i in 0..state.full_n_segments() {
            if let Some(seg) = state.get_segment(i) {
                if let Ok(s) = seg.to_str_lossy() {
                    text.push_str(&s);
                }
            }
        }
        text.trim().to_string()
    }
}
