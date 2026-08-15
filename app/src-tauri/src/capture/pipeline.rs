//! Pure signal conditioning: native-rate mono blocks → 16 kHz mono 20 ms chunks
//! with shared-clock timestamps. No OS APIs — unit-testable everywhere.

use rubato::{Fft, FixedSync, Resampler};

use super::{PcmChunk, CHUNK_SAMPLES, TARGET_RATE};

/// Feed-in block size at the native rate (10 ms @ 48 kHz). The resampler is
/// configured FixedSync::Input on this size; we buffer arbitrary input to it.
const IN_BLOCK: usize = 480;

pub struct Conditioner {
    resampler: Option<Fft<f32>>, // None when native rate is already 16 kHz
    in_buf: Vec<f32>,
    out_buf: Vec<f32>,
    /// Samples emitted at 16 kHz since anchor.
    emitted: u64,
    anchor_ms: u64,
    now_ms: Box<dyn Fn() -> u64 + Send + Sync>,
}

impl Conditioner {
    pub fn new(
        native_rate: u32,
        now_ms: impl Fn() -> u64 + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let anchor_ms = now_ms();
        let resampler = if native_rate == TARGET_RATE {
            None
        } else {
            Some(
                Fft::<f32>::new(
                    native_rate as usize,
                    TARGET_RATE as usize,
                    IN_BLOCK,
                    1,
                    FixedSync::Input,
                )
                .map_err(|e| format!("resampler init ({native_rate} Hz): {e}"))?,
            )
        };
        Ok(Self {
            resampler,
            in_buf: Vec::with_capacity(IN_BLOCK * 4),
            out_buf: Vec::with_capacity(CHUNK_SAMPLES * 8),
            emitted: 0,
            anchor_ms,
            now_ms: Box::new(now_ms),
        })
    }

    /// Feed native-rate mono samples; returns zero or more finished 20 ms chunks.
    pub fn feed(&mut self, mono: &[f32]) -> Vec<PcmChunk> {
        match &mut self.resampler {
            None => self.out_buf.extend_from_slice(mono),
            Some(_) => {
                self.in_buf.extend_from_slice(mono);
                self.drain_resampler();
            }
        }
        self.take_chunks()
    }

    fn drain_resampler(&mut self) {
        use rubato::audioadapter_buffers::direct::InterleavedSlice;
        use rubato::audioadapter_buffers::owned::InterleavedOwned;
        let Some(rs) = &mut self.resampler else { return };
        while self.in_buf.len() >= rs.input_frames_next() {
            let need = rs.input_frames_next();
            let input = InterleavedSlice::new(&self.in_buf[..need], 1, need)
                .expect("input adapter");
            let out_frames = rs.output_frames_next();
            let mut output = InterleavedOwned::<f32>::new(0.0, 1, out_frames);
            match rs.process_into_buffer(&input, &mut output, None) {
                Ok((consumed, produced)) => {
                    self.in_buf.drain(..consumed);
                    let raw = output.take_data();
                    self.out_buf.extend_from_slice(&raw[..produced]);
                }
                Err(e) => {
                    eprintln!("[capture] resampler error: {e}");
                    self.in_buf.clear();
                    return;
                }
            }
        }
    }

    fn take_chunks(&mut self) -> Vec<PcmChunk> {
        let mut out = Vec::new();
        while self.out_buf.len() >= CHUNK_SAMPLES {
            let samples: Vec<f32> = self.out_buf.drain(..CHUNK_SAMPLES).collect();
            let expected_ms = self.anchor_ms + self.emitted * 1000 / TARGET_RATE as u64;
            // Slew the anchor if the audio clock drifted vs. the shared clock
            // (device clock skew, dropped packets). Keeps caption timestamps
            // alignable with Discord speaking events over long sessions.
            let now = (self.now_ms)();
            let drift = now.saturating_sub(expected_ms);
            if drift > 300 {
                self.anchor_ms += drift;
            }
            let t_start_ms = self.anchor_ms + self.emitted * 1000 / TARGET_RATE as u64;
            self.emitted += CHUNK_SAMPLES as u64;
            out.push(PcmChunk { samples, t_start_ms });
        }
        out
    }
}

/// Downmix an interleaved or per-buffer multi-channel block to mono in place.
pub fn downmix_interleaved(interleaved: &[f32], channels: usize, out: &mut Vec<f32>) {
    out.clear();
    if channels <= 1 {
        out.extend_from_slice(interleaved);
        return;
    }
    let frames = interleaved.len() / channels;
    let gain = 1.0 / channels as f32;
    for f in 0..frames {
        let mut acc = 0.0f32;
        for c in 0..channels {
            acc += interleaved[f * channels + c];
        }
        out.push(acc * gain);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_at_16k_chunks_and_timestamps() {
        let mut c = Conditioner::new(16_000, || 1000).unwrap();
        let chunks = c.feed(&vec![0.5f32; 800]); // 50 ms
        assert_eq!(chunks.len(), 2); // 2 × 20 ms, 10 ms remains buffered
        assert_eq!(chunks[0].samples.len(), CHUNK_SAMPLES);
        assert_eq!(chunks[0].t_start_ms, 1000);
        assert_eq!(chunks[1].t_start_ms, 1020);
    }

    #[test]
    fn resamples_48k_to_16k() {
        let mut c = Conditioner::new(48_000, || 0).unwrap();
        // 1 s of a 1 kHz sine at 48 kHz.
        let sine: Vec<f32> = (0..48_000)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 48_000.0).sin())
            .collect();
        let mut total = 0usize;
        for block in sine.chunks(1024) {
            total += c.feed(block).iter().map(|ch| ch.samples.len()).sum::<usize>();
        }
        // ~16k samples out (resampler delay swallows a tail); generous bounds.
        assert!(total > 14_000 && total <= 16_320, "got {total}");
    }

    #[test]
    fn downmix_averages_channels() {
        let mut out = Vec::new();
        downmix_interleaved(&[1.0, 0.0, 0.0, 1.0], 2, &mut out);
        assert_eq!(out, vec![0.5, 0.5]);
    }
}
