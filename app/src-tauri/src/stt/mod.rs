//! Speech-to-text: PCM chunks in → rolling Partial / stabilized Final caption
//! events out, timestamped on the shared clock. See docs/dev/stt-engine.md.
//!
//! v0.1 engine: VAD-gated chunked whisper.cpp (multilingual small-q5_1).
//! MockStt fakes the same event stream for CALLOUT_MOCK=1 demos.

pub mod fbank;
mod gate;
pub mod voiceid;
#[cfg(target_os = "macos")]
mod whisper_engine;

use serde::Serialize;
use tokio::sync::mpsc;

use crate::capture::PcmChunk;
use crate::CaptionsStatus;

/// One recognized word with absolute shared-clock timing (from whisper token
/// timestamps). Empty `words` on a Final means timing was unavailable —
/// consumers must fall back to whole-utterance attribution.
#[derive(Debug, Clone, Serialize)]
pub struct Word {
    pub text: String,
    pub t0_ms: u64,
    pub t1_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SttEvent {
    /// Provisional text for the utterance in progress; replaces the previous Partial.
    Partial { text: String, t_start_ms: u64 },
    /// Utterance finalized; immutable afterwards. Carries the utterance audio
    /// for voiceprint enrollment/matching (internal only — never serialized).
    Final {
        text: String,
        words: Vec<Word>,
        #[serde(skip)]
        pcm: Vec<f32>,
        t_start_ms: u64,
        t_end_ms: u64,
    },
}

/// A decode job produced by the VAD gate for the engine worker.
pub(crate) enum Job {
    /// Droppable: skip if the worker is busy.
    Partial { pcm: Vec<f32>, t_start_ms: u64 },
    /// Never dropped.
    Final { pcm: Vec<f32>, t_start_ms: u64, t_end_ms: u64 },
}

/// Feeds capture chunks into the VAD gate (called on the capture thread).
pub struct SttFeed {
    gate: gate::Gate,
}

impl SttFeed {
    pub fn feed(&mut self, chunk: PcmChunk) {
        self.gate.feed(&chunk);
    }
}

/// Spawn the whisper pipeline: returns the capture-side feeder and the caption
/// event stream. Model load happens on the worker thread (status events tell
/// the UI). `languages` is read per utterance: empty = auto, one = pin,
/// several = auto restricted to that set. macOS-only until the Windows backend lands.
#[cfg(target_os = "macos")]
pub fn spawn_whisper(
    model_path: std::path::PathBuf,
    languages: std::sync::Arc<std::sync::RwLock<crate::settings::Settings>>,
    status_tx: tokio::sync::mpsc::UnboundedSender<CaptionsStatus>,
) -> (SttFeed, mpsc::UnboundedReceiver<SttEvent>) {
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let (job_tx, job_rx) = crossbeam_channel::bounded::<Job>(4);
    whisper_engine::spawn_worker(model_path, languages, job_rx, event_tx, status_tx);
    (SttFeed { gate: gate::Gate::new(job_tx) }, event_rx)
}

#[cfg(not(target_os = "macos"))]
pub fn spawn_whisper(
    _model_path: std::path::PathBuf,
    _languages: std::sync::Arc<std::sync::RwLock<crate::settings::Settings>>,
    status_tx: tokio::sync::mpsc::UnboundedSender<CaptionsStatus>,
) -> (SttFeed, mpsc::UnboundedReceiver<SttEvent>) {
    let (_event_tx, event_rx) = mpsc::unbounded_channel();
    let (job_tx, _job_rx) = crossbeam_channel::bounded::<Job>(4);
    let _ = status_tx.send(CaptionsStatus::SttError {
        message: "STT is not implemented on this platform yet".into(),
    });
    (SttFeed { gate: gate::Gate::new(job_tx) }, event_rx)
}

/// Development stand-in: canned phrases as word-by-word partials + finals.
pub struct MockStt;

const PHRASES: &[&str] = &[
    "careful, two pushing right side",
    "I'm going B, cover me",
    "nice shot!",
    "rotating now, need thirty seconds",
    "they have no ult, go go go",
];

impl MockStt {
    pub fn start(now_ms: impl Fn() -> u64 + Send + 'static) -> mpsc::UnboundedReceiver<SttEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        tauri::async_runtime::spawn(async move {
            let mut i = 0usize;
            loop {
                let phrase = PHRASES[i % PHRASES.len()];
                let t_start = now_ms();
                let words: Vec<&str> = phrase.split(' ').collect();
                for n in 1..=words.len() {
                    if tx
                        .send(SttEvent::Partial { text: words[..n].join(" "), t_start_ms: t_start })
                        .is_err()
                    {
                        return;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(320)).await;
                }
                let _ = tx.send(SttEvent::Final {
                    text: phrase.to_string(),
                    words: Vec::new(),
                    pcm: Vec::new(),
                    t_start_ms: t_start,
                    t_end_ms: now_ms(),
                });
                tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
                i += 1;
            }
        });
        rx
    }
}
