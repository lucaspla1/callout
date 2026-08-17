//! Kaldi-style 80-dim log-mel filterbank features for the speaker-embedding
//! model (WeSpeaker-compatible: 25ms/10ms frames, Povey window, no dither,
//! per-utterance CMN). Close-enough to kaldi for cosine discrimination; not
//! bit-exact.

use rustfft::{num_complex::Complex, FftPlanner};

pub const NUM_MELS: usize = 80;
const SAMPLE_RATE: f32 = 16_000.0;
const FRAME_LEN: usize = 400; // 25 ms
const FRAME_SHIFT: usize = 160; // 10 ms
const FFT_SIZE: usize = 512;
const LOW_FREQ: f32 = 20.0;
const HIGH_FREQ: f32 = 7_600.0;

fn mel(hz: f32) -> f32 {
    1127.0 * (1.0 + hz / 700.0).ln()
}

/// Triangular mel filterbank: NUM_MELS x (FFT_SIZE/2+1), computed once.
fn mel_banks() -> Vec<Vec<(usize, f32)>> {
    let n_bins = FFT_SIZE / 2 + 1;
    let mel_lo = mel(LOW_FREQ);
    let mel_hi = mel(HIGH_FREQ);
    let step = (mel_hi - mel_lo) / (NUM_MELS as f32 + 1.0);
    (0..NUM_MELS)
        .map(|m| {
            let left = mel_lo + m as f32 * step;
            let center = left + step;
            let right = center + step;
            let mut taps = Vec::new();
            for bin in 0..n_bins {
                let freq = bin as f32 * SAMPLE_RATE / FFT_SIZE as f32;
                let fm = mel(freq);
                let w = if fm > left && fm < right {
                    if fm <= center {
                        (fm - left) / step
                    } else {
                        (right - fm) / step
                    }
                } else {
                    0.0
                };
                if w > 0.0 {
                    taps.push((bin, w));
                }
            }
            taps
        })
        .collect()
}

/// pcm (16 kHz mono, [-1,1]) → frames of NUM_MELS log-mel values, CMN applied.
pub fn compute(pcm: &[f32]) -> Vec<[f32; NUM_MELS]> {
    if pcm.len() < FRAME_LEN {
        return Vec::new();
    }
    let n_frames = 1 + (pcm.len() - FRAME_LEN) / FRAME_SHIFT;
    let banks = mel_banks();
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    // Povey window.
    let window: Vec<f32> = (0..FRAME_LEN)
        .map(|n| {
            let hann = 0.5
                - 0.5 * (2.0 * std::f32::consts::PI * n as f32 / (FRAME_LEN as f32 - 1.0)).cos();
            hann.powf(0.85)
        })
        .collect();

    let mut feats: Vec<[f32; NUM_MELS]> = Vec::with_capacity(n_frames);
    let mut buf = vec![Complex::new(0.0f32, 0.0); FFT_SIZE];
    // Kaldi scales int16 samples; match its energy floor expectations.
    const SCALE: f32 = 32_768.0;
    for f in 0..n_frames {
        let frame = &pcm[f * FRAME_SHIFT..f * FRAME_SHIFT + FRAME_LEN];
        let mean: f32 = frame.iter().sum::<f32>() / FRAME_LEN as f32;
        // DC removal + pre-emphasis + window.
        let mut prev = frame[0] - mean;
        for (i, item) in buf.iter_mut().enumerate().take(FFT_SIZE) {
            *item = if i < FRAME_LEN {
                let s = frame[i] - mean;
                let pre = s - 0.97 * prev;
                prev = s;
                Complex::new(pre * window[i] * SCALE, 0.0)
            } else {
                Complex::new(0.0, 0.0)
            };
        }
        fft.process(&mut buf);
        let mut row = [0.0f32; NUM_MELS];
        for (m, taps) in banks.iter().enumerate() {
            let mut acc = 0.0f32;
            for &(bin, w) in taps {
                acc += w * buf[bin].norm_sqr();
            }
            row[m] = acc.max(f32::EPSILON).ln();
        }
        feats.push(row);
    }
    // Per-utterance cepstral mean normalization (what WeSpeaker applies).
    let mut means = [0.0f32; NUM_MELS];
    for row in &feats {
        for (m, v) in row.iter().enumerate() {
            means[m] += v;
        }
    }
    for m in means.iter_mut() {
        *m /= feats.len() as f32;
    }
    for row in feats.iter_mut() {
        for (m, v) in row.iter_mut().enumerate() {
            *v -= means[m];
        }
    }
    feats
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_count_and_shape() {
        let pcm = vec![0.1f32; 16_000]; // 1 s
        let feats = compute(&pcm);
        assert_eq!(feats.len(), 1 + (16_000 - 400) / 160); // 98 frames
        assert!(feats.iter().all(|r| r.iter().all(|v| v.is_finite())));
    }

    #[test]
    fn cmn_zeroes_the_mean() {
        let pcm: Vec<f32> = (0..16_000)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 16_000.0).sin() * 0.3)
            .collect();
        let feats = compute(&pcm);
        let mean: f32 = feats.iter().map(|r| r[10]).sum::<f32>() / feats.len() as f32;
        assert!(mean.abs() < 1e-3, "post-CMN mean {mean}");
    }

    #[test]
    fn too_short_input_is_empty() {
        assert!(compute(&[0.0; 100]).is_empty());
    }
}
