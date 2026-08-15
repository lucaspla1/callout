//! Per-process audio capture of the Discord client → 16 kHz mono f32 chunks.
//!
//! Platform backends (see docs/dev/audio-capture.md):
//!   - macOS 14.4+: Core Audio process tap on all `com.hnc.Discord*` HAL process
//!     objects, mono mixdown ([`coreaudio`]).
//!   - Windows 10 2004+: WASAPI process loopback, include-process-tree (M2.5, stub).
//!
//! The capture supervisor runs on its own thread, restarts on Discord restarts,
//! and hands conditioned chunks to a sink closure (the STT gate).

pub mod pipeline;

#[cfg(target_os = "macos")]
pub mod coreaudio;

#[cfg(windows)]
pub mod wasapi {
    //! TODO(M2.5): WASAPI process loopback per docs/dev/audio-capture.md §1.
}

pub const TARGET_RATE: u32 = 16_000;
/// 20 ms @ 16 kHz — least-common-denominator frame for the VAD/STT consumers.
pub const CHUNK_SAMPLES: usize = 320;

#[derive(Debug, Clone)]
pub struct PcmChunk {
    /// 16 kHz mono samples in [-1, 1]; always `CHUNK_SAMPLES` long.
    pub samples: Vec<f32>,
    /// Shared-clock (`now_ms`) time of the first sample.
    pub t_start_ms: u64,
}

/// Spawns the platform capture supervisor. `sink` is called on a capture-owned
/// thread with conditioned chunks; it must be fast (hand off to a channel/worker).
#[cfg(target_os = "macos")]
pub fn spawn(
    now_ms: impl Fn() -> u64 + Send + Sync + Clone + 'static,
    status_tx: tokio::sync::mpsc::UnboundedSender<crate::CaptionsStatus>,
    sink: impl FnMut(PcmChunk) + Send + 'static,
) {
    coreaudio::spawn_supervisor("com.hnc.Discord", now_ms, status_tx, sink);
}

#[cfg(not(target_os = "macos"))]
pub fn spawn(
    _now_ms: impl Fn() -> u64 + Send + Sync + 'static,
    status_tx: tokio::sync::mpsc::UnboundedSender<crate::CaptionsStatus>,
    _sink: impl FnMut(PcmChunk) + Send + 'static,
) {
    let _ = status_tx.send(crate::CaptionsStatus::CaptureError {
        message: "audio capture is not implemented on this platform yet".into(),
    });
}
