//! Silero-VAD gate with hysteresis: turns the continuous 16 kHz stream into
//! discrete utterances, emitting droppable Partial jobs while speech runs and
//! one non-droppable Final job at each endpoint. Parameters tuned for gaming
//! callouts (docs/dev/stt-engine.md §2).

use std::collections::VecDeque;
use std::time::Instant;

use voice_activity_detector::VoiceActivityDetector;

use super::mailbox::{FinalJob, JobMailboxTx, PartialJob, PartialPublish};
use crate::capture::{PcmChunk, TARGET_RATE};
use crate::CaptionsStatus;

const VAD_FRAME: usize = 512; // Silero v5 native @ 16 kHz (32 ms)
const WINDOWS_PARTIAL_EVERY_MS: u32 = 1_600;
const WINDOWS_PARTIAL_TAIL_S: usize = 6;

fn partial_every_ms(windows: bool) -> u32 {
    if windows {
        WINDOWS_PARTIAL_EVERY_MS
    } else {
        600
    }
}

fn partial_tail_samples(windows: bool) -> Option<usize> {
    windows.then_some(WINDOWS_PARTIAL_TAIL_S * TARGET_RATE as usize)
}

/// Copy only the audio the Partial decoder will consume. Keeping the windowing
/// on the capture side avoids cloning an entire growing utterance every tick.
fn copy_partial_window(
    pcm: &[f32],
    utterance_start_ms: u64,
    max_samples: Option<usize>,
) -> (Vec<f32>, u64) {
    let start = max_samples
        .map(|limit| pcm.len().saturating_sub(limit))
        .unwrap_or(0);
    let pcm_start_ms = utterance_start_ms + start as u64 * 1_000 / TARGET_RATE as u64;
    (pcm[start..].to_vec(), pcm_start_ms)
}

pub struct VadConfig {
    pub enter_threshold: f32,
    pub exit_threshold: f32,
    pub min_speech_ms: u32,
    pub endpoint_silence_ms: u32,
    pub pre_roll_ms: u32,
    pub max_utterance_s: f32,
    pub partial_every_ms: u32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            enter_threshold: 0.55,
            exit_threshold: 0.35,
            min_speech_ms: 128,
            endpoint_silence_ms: 400,
            pre_roll_ms: 240,
            max_utterance_s: 12.0,
            // CPU decode (Windows) can't keep the Metal cadence: each partial
            // re-decodes the whole utterance, so pace them ~2x slower there.
            partial_every_ms: partial_every_ms(cfg!(windows)),
        }
    }
}

enum State {
    Silence,
    Speech {
        silence_ms: u32,
        since_partial_ms: u32,
    },
}

pub struct Gate {
    vad: Option<VoiceActivityDetector>,
    cfg: VadConfig,
    state: State,
    /// Rebuffer to Silero's 512-sample frames.
    vad_buf: Vec<f32>,
    vad_buf_t0: u64,
    pre_roll: VecDeque<f32>,
    utterance: Vec<f32>,
    utt_start_ms: u64,
    job_tx: JobMailboxTx,
    status_tx: tokio::sync::mpsc::UnboundedSender<CaptionsStatus>,
    utterance_id: u64,
    partial_replacements: u32,
    mailbox_failed: bool,
    dbg_frames: u64,
    dbg_max_prob: f32,
}

impl Gate {
    pub(crate) fn new(
        job_tx: JobMailboxTx,
        status_tx: tokio::sync::mpsc::UnboundedSender<CaptionsStatus>,
    ) -> Self {
        let vad = VoiceActivityDetector::builder()
            .sample_rate(TARGET_RATE as i64)
            .chunk_size(VAD_FRAME)
            .build()
            .map_err(|e| eprintln!("[stt] VAD init failed: {e}"))
            .ok();
        Self {
            vad,
            cfg: VadConfig::default(),
            state: State::Silence,
            vad_buf: Vec::with_capacity(VAD_FRAME * 2),
            vad_buf_t0: 0,
            pre_roll: VecDeque::new(),
            utterance: Vec::new(),
            utt_start_ms: 0,
            job_tx,
            status_tx,
            utterance_id: 0,
            partial_replacements: 0,
            mailbox_failed: false,
            dbg_frames: 0,
            dbg_max_prob: 0.0,
        }
    }

    pub fn feed(&mut self, chunk: &PcmChunk) {
        if self.vad_buf.is_empty() {
            self.vad_buf_t0 = chunk.t_start_ms;
        }
        self.vad_buf.extend_from_slice(&chunk.samples);
        while self.vad_buf.len() >= VAD_FRAME {
            let frame: Vec<f32> = self.vad_buf.drain(..VAD_FRAME).collect();
            let frame_t0 = self.vad_buf_t0;
            self.vad_buf_t0 += (VAD_FRAME as u64) * 1000 / TARGET_RATE as u64;
            self.on_frame(&frame, frame_t0);
        }
    }

    fn on_frame(&mut self, frame: &[f32], t0: u64) {
        if self.mailbox_failed {
            return;
        }
        let Some(vad) = &mut self.vad else { return };
        let prob = vad.predict(frame.iter().copied());
        let frame_ms = (frame.len() as u32) * 1000 / TARGET_RATE;
        let mut mailbox_closed = false;

        self.dbg_frames += 1;
        self.dbg_max_prob = self.dbg_max_prob.max(prob);
        if self.dbg_frames % 200 == 0 {
            eprintln!(
                "[stt] vad 6.4s: max_prob={:.2} in_speech={}",
                self.dbg_max_prob,
                matches!(self.state, State::Speech { .. })
            );
            self.dbg_max_prob = 0.0;
        }

        match &mut self.state {
            State::Silence => {
                if prob > self.cfg.enter_threshold {
                    self.utterance_id = self.utterance_id.wrapping_add(1).max(1);
                    self.partial_replacements = 0;
                    let pre_roll_ms = (self.pre_roll.len() as u64) * 1000 / TARGET_RATE as u64;
                    self.utt_start_ms = t0.saturating_sub(pre_roll_ms);
                    self.utterance.clear();
                    self.utterance.extend(self.pre_roll.iter());
                    self.utterance.extend_from_slice(frame);
                    self.state = State::Speech {
                        silence_ms: 0,
                        since_partial_ms: 0,
                    };
                } else {
                    // Keep the pre-roll ring filled.
                    self.pre_roll.extend(frame.iter().copied());
                    let cap = (self.cfg.pre_roll_ms as usize) * TARGET_RATE as usize / 1000;
                    while self.pre_roll.len() > cap {
                        self.pre_roll.pop_front();
                    }
                }
            }
            State::Speech {
                silence_ms,
                since_partial_ms,
            } => {
                self.utterance.extend_from_slice(frame);
                *silence_ms = if prob < self.cfg.exit_threshold {
                    *silence_ms + frame_ms
                } else {
                    0
                };
                *since_partial_ms += frame_ms;

                let utt_ms = (self.utterance.len() as u64) * 1000 / TARGET_RATE as u64;
                let endpoint = *silence_ms >= self.cfg.endpoint_silence_ms
                    || utt_ms as f32 / 1000.0 >= self.cfg.max_utterance_s;

                if endpoint {
                    let speech_ms = utt_ms.saturating_sub(*silence_ms as u64);
                    if speech_ms >= self.cfg.min_speech_ms as u64 {
                        // "cap" endings chop speech mid-word — in the field log
                        // they are the smoking gun for a stalled silence clock.
                        let pcm = std::mem::take(&mut self.utterance);
                        let t_end_ms = self.utt_start_ms + utt_ms;
                        // Capture must never wait for inference. Finals have a
                        // dedicated unbounded FIFO and remain strictly ordered.
                        if self
                            .job_tx
                            .publish_final(FinalJob {
                                utterance_id: self.utterance_id,
                                pcm,
                                t_start_ms: self.utt_start_ms,
                                t_end_ms,
                                queued_at: Instant::now(),
                            })
                            .is_err()
                        {
                            mailbox_closed = true;
                        }
                        crate::diag::log(&format!(
                            "utterance: duration_ms={utt_ms} ended_by={} partials_replaced={} final_queue_depth={}",
                            if *silence_ms >= self.cfg.endpoint_silence_ms {
                                "endpoint"
                            } else {
                                "cap"
                            },
                            self.partial_replacements,
                            self.job_tx.final_depth()
                        ));
                    } else {
                        self.utterance.clear(); // click/cough — discard
                    }
                    self.pre_roll.clear();
                    self.state = State::Silence;
                } else if *since_partial_ms >= self.cfg.partial_every_ms {
                    *since_partial_ms = 0;
                    let (pcm, pcm_start_ms) = copy_partial_window(
                        &self.utterance,
                        self.utt_start_ms,
                        partial_tail_samples(cfg!(windows)),
                    );
                    match self.job_tx.publish_partial(PartialJob {
                        utterance_id: self.utterance_id,
                        pcm,
                        utterance_start_ms: self.utt_start_ms,
                        pcm_start_ms,
                        pcm_end_ms: self.utt_start_ms + utt_ms,
                        queued_at: Instant::now(),
                    }) {
                        Ok(PartialPublish::Queued) => {}
                        Ok(PartialPublish::Replaced) => {
                            self.partial_replacements += 1;
                        }
                        Err(_) => mailbox_closed = true,
                    }
                }
            }
        }

        if mailbox_closed && !self.mailbox_failed {
            self.mailbox_failed = true;
            let _ = self.status_tx.send(CaptionsStatus::SttError {
                message: "speech decoder stopped unexpectedly".into(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_balanced_profile_reduces_partial_frequency() {
        assert_eq!(partial_every_ms(true), 1_600);
        assert_eq!(partial_tail_samples(true), Some(96_000));
        assert_eq!(partial_every_ms(false), 600);
        assert_eq!(partial_tail_samples(false), None);
    }

    #[test]
    fn partial_window_has_the_timestamp_of_its_first_sample() {
        let pcm = vec![0.0; 8 * TARGET_RATE as usize];
        let (tail, tail_start_ms) = copy_partial_window(
            &pcm,
            10_000,
            Some(WINDOWS_PARTIAL_TAIL_S * TARGET_RATE as usize),
        );
        assert_eq!(tail.len(), 6 * TARGET_RATE as usize);
        assert_eq!(tail_start_ms, 12_000);
    }

    #[test]
    fn short_partial_keeps_the_utterance_timestamp() {
        let pcm = vec![0.0; 2 * TARGET_RATE as usize];
        let (tail, tail_start_ms) = copy_partial_window(
            &pcm,
            10_000,
            Some(WINDOWS_PARTIAL_TAIL_S * TARGET_RATE as usize),
        );
        assert_eq!(tail.len(), pcm.len());
        assert_eq!(tail_start_ms, 10_000);
    }
}
