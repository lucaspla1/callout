//! Silero-VAD gate with hysteresis: turns the continuous 16 kHz stream into
//! discrete utterances, emitting droppable Partial jobs while speech runs and
//! one non-droppable Final job at each endpoint. Parameters tuned for gaming
//! callouts (docs/dev/stt-engine.md §2).

use std::collections::VecDeque;

use voice_activity_detector::VoiceActivityDetector;

use super::Job;
use crate::capture::{PcmChunk, TARGET_RATE};

const VAD_FRAME: usize = 512; // Silero v5 native @ 16 kHz (32 ms)

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
            partial_every_ms: 600,
        }
    }
}

enum State {
    Silence,
    Speech { silence_ms: u32, since_partial_ms: u32 },
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
    job_tx: crossbeam_channel::Sender<Job>,
    dbg_frames: u64,
    dbg_max_prob: f32,
}

impl Gate {
    pub fn new(job_tx: crossbeam_channel::Sender<Job>) -> Self {
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
        let Some(vad) = &mut self.vad else { return };
        let prob = vad.predict(frame.iter().copied());
        let frame_ms = (frame.len() as u32) * 1000 / TARGET_RATE;

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
                    let pre_roll_ms = (self.pre_roll.len() as u64) * 1000 / TARGET_RATE as u64;
                    self.utt_start_ms = t0.saturating_sub(pre_roll_ms);
                    self.utterance.clear();
                    self.utterance.extend(self.pre_roll.iter());
                    self.utterance.extend_from_slice(frame);
                    self.state = State::Speech { silence_ms: 0, since_partial_ms: 0 };
                } else {
                    // Keep the pre-roll ring filled.
                    self.pre_roll.extend(frame.iter().copied());
                    let cap = (self.cfg.pre_roll_ms as usize) * TARGET_RATE as usize / 1000;
                    while self.pre_roll.len() > cap {
                        self.pre_roll.pop_front();
                    }
                }
            }
            State::Speech { silence_ms, since_partial_ms } => {
                self.utterance.extend_from_slice(frame);
                *silence_ms = if prob < self.cfg.exit_threshold { *silence_ms + frame_ms } else { 0 };
                *since_partial_ms += frame_ms;

                let utt_ms = (self.utterance.len() as u64) * 1000 / TARGET_RATE as u64;
                let endpoint = *silence_ms >= self.cfg.endpoint_silence_ms
                    || utt_ms as f32 / 1000.0 >= self.cfg.max_utterance_s;

                if endpoint {
                    let speech_ms = utt_ms.saturating_sub(*silence_ms as u64);
                    if speech_ms >= self.cfg.min_speech_ms as u64 {
                        let pcm = std::mem::take(&mut self.utterance);
                        let t_end_ms = self.utt_start_ms + utt_ms;
                        // Finals are never dropped; blocking send is fine off the RT thread.
                        let _ = self.job_tx.send(Job::Final {
                            pcm,
                            t_start_ms: self.utt_start_ms,
                            t_end_ms,
                        });
                    } else {
                        self.utterance.clear(); // click/cough — discard
                    }
                    self.pre_roll.clear();
                    self.state = State::Silence;
                } else if *since_partial_ms >= self.cfg.partial_every_ms {
                    *since_partial_ms = 0;
                    // Droppable: if the worker is busy, skip this tick.
                    let _ = self.job_tx.try_send(Job::Partial {
                        pcm: self.utterance.clone(),
                        t_start_ms: self.utt_start_ms,
                    });
                }
            }
        }
    }
}
