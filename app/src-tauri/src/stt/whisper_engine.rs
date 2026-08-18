//! whisper.cpp worker: one context, one thread, VAD-cut utterances in,
//! Partial/Final text out. Decode parameters and the audio_ctx latency trick
//! per docs/dev/stt-engine.md §3.3.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use super::mailbox::{DecodeJob, JobMailboxRx};
use super::{SttEvent, Word};
use crate::capture::TARGET_RATE;
use crate::settings::Settings;
use crate::CaptionsStatus;

/// whisper misbehaves on very short inputs; pad to at least ~1.1 s.
const MIN_SAMPLES: usize = (TARGET_RATE as usize * 11) / 10;

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
            whisper_rs::install_logging_hooks();
            let ctx = match WhisperContext::new_with_params(
                &model_path,
                WhisperContextParameters::default(),
            ) {
                Ok(c) => c,
                Err(e) => {
                    let _ = status_tx.send(CaptionsStatus::SttError {
                        message: format!("failed to load whisper model: {e}"),
                    });
                    return;
                }
            };
            let mut state = match ctx.create_state() {
                Ok(s) => s,
                Err(e) => {
                    let _ = status_tx.send(CaptionsStatus::SttError {
                        message: format!("failed to create whisper state: {e}"),
                    });
                    return;
                }
            };
            // Second, bigger context for finals: partials stay fast on the small
            // model, finals get re-decoded on large-v3-turbo with beam search —
            // the biggest available quality jump for pt/es. Falls back to the
            // small model when the file is absent or fails to load.
            let turbo = turbo_path.filter(|p| p.is_file()).and_then(|p| {
                let t0 = std::time::Instant::now();
                match WhisperContext::new_with_params(&p, WhisperContextParameters::default())
                    .and_then(|c| {
                        // Keep the context alive alongside its state.
                        let s = c.create_state()?;
                        Ok((c, s))
                    }) {
                    Ok(pair) => {
                        eprintln!("[stt] finals model loaded in {:?}", t0.elapsed());
                        Some(pair)
                    }
                    Err(e) => {
                        eprintln!("[stt] finals model failed to load ({e}); using small for finals");
                        None
                    }
                }
            });
            let mut turbo_state = turbo.map(|(ctx, state)| (ctx, state));
            eprintln!(
                "[stt] finals engine: {}",
                if turbo_state.is_some() { "large-v3-turbo (beam)" } else { "small (greedy)" }
            );
            let _ = status_tx.send(CaptionsStatus::SttReady);

            let available_threads = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4);
            let threads = decode_thread_count(available_threads, cfg!(windows));
            crate::diag::log(&format!(
                "stt profile: decode_threads={threads} available_threads={available_threads} partial_cadence_ms={} partial_tail_s={}",
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
                        let p_t0 = std::time::Instant::now();
                        let decoded =
                            decode(&mut state, &job.pcm, utt_lang.as_deref(), threads);
                        let decode_ms = p_t0.elapsed().as_millis();
                        crate::diag::log(&format!(
                            "partial decode: queue_ms={queue_ms} decode_ms={decode_ms} audio_ms={audio_ms} utterance_span_ms={} threads={threads} window_offset_ms={}",
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
                        if let Some(dir) = debug_audio_dir(&model_path) {
                            write_wav_16k_mono(&dir.join(format!("utt_{t_start_ms}.wav")), &pcm);
                        }
                        let decode_t0 = std::time::Instant::now();
                        // Finals decode on the big model (beam search) when
                        // available; partials stay on the small one. For
                        // multi-language users, re-pick the language on the FULL
                        // utterance — more audio = reliable detection.
                        let (fin_state, beam) = match turbo_state.as_mut() {
                            Some((_ctx, s)) => (s, true),
                            None => (&mut state, false),
                        };
                        let allowed =
                            settings.read().map(|s| s.languages.clone()).unwrap_or_default();
                        // A Final may be the utterance's first job. Pick once
                        // from its full audio; for several allowed languages,
                        // this also deliberately refreshes a Partial's guess.
                        utt_lang = choose_language(fin_state, &pcm, &allowed, threads);
                        // Finals also collect word timings (proven not to change
                        // the text — see ab_tests below) for per-word attribution.
                        if let Some((text, words)) = decode_final(
                            fin_state,
                            &pcm,
                            utt_lang.as_deref(),
                            threads,
                            t_start_ms,
                            beam,
                        ) {
                            // Hallucination guards: empty, exact repeat of the
                            // previous final, a known noise hallucination, or a
                            // low-confidence decode (probability gates).
                            let stats = collect_stats(fin_state);
                            let decode_ms = decode_t0.elapsed().as_millis();
                            let audio_ms = pcm.len() as u64 * 1_000 / TARGET_RATE as u64;
                            crate::diag::log(&format!(
                                "final decode: queue_ms={queue_ms} decode_ms={decode_ms} audio_ms={audio_ms} threads={threads} model={}",
                                if beam { "turbo" } else { "small" },
                            ));
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
    run_full(state, pcm, lang, threads, false, false)?;
    Some(collect_text(state))
}

/// Final decode: word timings on the shared clock (empty if degenerate →
/// whole-utterance attribution fallback), optionally with beam search when
/// running on the big finals model.
fn decode_final(
    state: &mut whisper_rs::WhisperState,
    pcm: &[f32],
    lang: Option<&str>,
    threads: i32,
    base_ms: u64,
    beam: bool,
) -> Option<(String, Vec<Word>)> {
    run_full(state, pcm, lang, threads, true, beam)?;
    let text = collapse_repeats(&collect_text(state));
    let words = collect_words(state, base_ms);
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

fn run_full(
    state: &mut whisper_rs::WhisperState,
    pcm: &[f32],
    lang: Option<&str>,
    threads: i32,
    token_timestamps: bool,
    beam: bool,
) -> Option<()> {
    let mut padded;
    let samples = if pcm.len() < MIN_SAMPLES {
        padded = pcm.to_vec();
        padded.resize(MIN_SAMPLES, 0.0);
        &padded[..]
    } else {
        pcm
    };

    // Beam search on finals (quality); greedy on partials (speed).
    let strategy = if beam {
        SamplingStrategy::BeamSearch {
            beam_size: 5,
            patience: 1.0,
        }
    } else {
        SamplingStrategy::Greedy { best_of: 1 }
    };
    let mut params = FullParams::new(strategy);
    params.set_language(Some(lang.unwrap_or("auto")));
    params.set_no_context(true);
    params.set_single_segment(true);
    params.set_suppress_blank(true);
    params.set_suppress_nst(true);
    params.set_temperature(0.0);
    // Finals need timestamp processing on, or token t0/t1 come back -1 and
    // word-level attribution silently falls back (text is unaffected either
    // way — proven byte-identical by ab_tests).
    params.set_no_timestamps(!token_timestamps);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_n_threads(threads);
    // The latency trick: shrink the encoder context to the real audio length —
    // PARTIALS ONLY. Beam-search finals get the full context: trimmed context
    // degrades cross-attention for beams and was producing decoder loops on
    // Portuguese ("X. X. X.").
    if beam {
        params.set_audio_ctx(0); // 0 = whisper default (full context)
    } else {
        let len_s = samples.len() as f32 / TARGET_RATE as f32;
        let audio_ctx = ((len_s / 30.0 * 1500.0) as i32 + 128).clamp(512, 1500);
        params.set_audio_ctx(audio_ctx);
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

/// A/B harness: proves that enabling token timestamps does not change the
/// transcribed text (same decode, timing is a post-hoc computation).
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

    /// Forensic test for the "fluent but unrelated Portuguese" bug: decodes a
    /// clean PT fixture through the EXACT production finals path (turbo, beam,
    /// full context), then the same audio with holes punched in it (simulating
    /// capture drops under backpressure).
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

        let (clean, _) = decode_final(&mut state, &pcm, Some("pt"), 4, 0, true).expect("decode");
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
        let (holed, _) =
            decode_final(&mut state, &chopped, Some("pt"), 4, 0, true).expect("decode");
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
