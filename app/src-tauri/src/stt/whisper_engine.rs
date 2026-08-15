//! whisper.cpp worker: one context, one thread, VAD-cut utterances in,
//! Partial/Final text out. Decode parameters and the audio_ctx latency trick
//! per docs/dev/stt-engine.md §3.3.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use super::{Job, SttEvent};
use crate::capture::TARGET_RATE;
use crate::settings::Settings;
use crate::CaptionsStatus;

/// whisper misbehaves on very short inputs; pad to at least ~1.1 s.
const MIN_SAMPLES: usize = (TARGET_RATE as usize * 11) / 10;

pub fn spawn_worker(
    model_path: PathBuf,
    settings: Arc<RwLock<Settings>>,
    job_rx: crossbeam_channel::Receiver<Job>,
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
            let _ = status_tx.send(CaptionsStatus::SttReady);

            let threads = std::thread::available_parallelism()
                .map(|n| n.get().saturating_sub(2).clamp(2, 6))
                .unwrap_or(4) as i32;
            let mut last_final = String::new();
            // Language is decided once per utterance and cached for its partials.
            let mut utt_key: u64 = u64::MAX;
            let mut utt_lang: Option<String> = None;

            while let Ok(mut job) = job_rx.recv() {
                // Coalesce a backlog: prefer the newest job; never skip a Final.
                while let Ok(newer) = job_rx.try_recv() {
                    job = match (job, newer) {
                        (Job::Final { .. } | Job::Partial { .. }, f @ Job::Final { .. }) => f,
                        (f @ Job::Final { .. }, Job::Partial { .. }) => f,
                        (Job::Partial { .. }, p @ Job::Partial { .. }) => p,
                    };
                }
                let (pcm, t_start_ms) = match &job {
                    Job::Partial { pcm, t_start_ms } | Job::Final { pcm, t_start_ms, .. } => {
                        (pcm.clone(), *t_start_ms)
                    }
                };
                if t_start_ms != utt_key {
                    utt_key = t_start_ms;
                    let allowed = settings.read().map(|s| s.languages.clone()).unwrap_or_default();
                    utt_lang = choose_language(&mut state, &pcm, &allowed, threads);
                }
                match job {
                    Job::Partial { pcm, t_start_ms } => {
                        if let Some(text) = decode(&mut state, &pcm, utt_lang.as_deref(), threads) {
                            if !text.is_empty() {
                                let _ = event_tx.send(SttEvent::Partial { text, t_start_ms });
                            }
                        }
                    }
                    Job::Final { pcm, t_start_ms, t_end_ms } => {
                        if let Some(text) = decode(&mut state, &pcm, utt_lang.as_deref(), threads) {
                            // Hallucination guards: empty or exact repeat of the
                            // previous final (whisper's classic noise output).
                            if !text.is_empty() && text != last_final {
                                last_final = text.clone();
                                let _ = event_tx.send(SttEvent::Final { text, t_start_ms, t_end_ms });
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
    let mut padded;
    let samples = if pcm.len() < MIN_SAMPLES {
        padded = pcm.to_vec();
        padded.resize(MIN_SAMPLES, 0.0);
        &padded[..]
    } else {
        pcm
    };

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some(lang.unwrap_or("auto")));
    params.set_no_context(true);
    params.set_single_segment(true);
    params.set_suppress_blank(true);
    params.set_suppress_nst(true);
    params.set_temperature(0.0);
    params.set_no_timestamps(true);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_n_threads(threads);
    // The latency trick: shrink the encoder context to the real audio length.
    let len_s = samples.len() as f32 / TARGET_RATE as f32;
    let audio_ctx = ((len_s / 30.0 * 1500.0) as i32 + 128).clamp(512, 1500);
    params.set_audio_ctx(audio_ctx);

    if let Err(e) = state.full(params, samples) {
        eprintln!("[stt] decode failed: {e}");
        return None;
    }
    let n = state.full_n_segments();
    let mut text = String::new();
    for i in 0..n {
        if let Some(seg) = state.get_segment(i) {
            if let Ok(s) = seg.to_str_lossy() {
                text.push_str(&s);
            }
        }
    }
    Some(text.trim().to_string())
}
