//! Speaker voiceprints ("the app learns your friends"): embeddings from a
//! WeSpeaker ONNX model, auto-enrolled from solo speech, matched by cosine
//! similarity to resolve ambiguous ("N speaking") caption lines.
//! Design: docs/dev/speaker-attribution-2.md, layer 2.

use std::collections::HashMap;
use std::path::PathBuf;

use ort::session::Session;
use ort::value::Tensor;
use serde::{Deserialize, Serialize};

use super::fbank;

/// Similarity floor for reassigning a joint line to one speaker…
pub const MATCH_THRESHOLD: f32 = 0.40;
/// …and how much better than the runner-up the winner must be.
pub const MATCH_MARGIN: f32 = 0.08;
/// Solo speech shorter than this doesn't update a voiceprint.
pub const MIN_ENROLL_MS: u64 = 1_500;
/// Segments shorter than this aren't worth embedding for a match.
pub const MIN_MATCH_MS: u64 = 700;
/// Cap embedding input (cost control); the voice is stable within seconds.
const MAX_EMBED_SAMPLES: usize = 8 * 16_000;

pub struct VoiceId {
    session: Session,
}

impl VoiceId {
    pub fn load(model_path: &std::path::Path) -> Result<Self, String> {
        let session = Session::builder()
            .and_then(|b| b.commit_from_file(model_path))
            .map_err(|e| format!("speaker model load: {e}"))?;
        Ok(Self { session })
    }

    /// L2-normalized speaker embedding of a 16 kHz mono segment.
    pub fn embed(&mut self, pcm: &[f32]) -> Option<Vec<f32>> {
        let pcm = &pcm[..pcm.len().min(MAX_EMBED_SAMPLES)];
        let feats = fbank::compute(pcm);
        if feats.len() < 30 {
            return None; // < ~300ms of frames — not a reliable voice sample
        }
        let t = feats.len();
        let mut input = ndarray::Array3::<f32>::zeros((1, t, fbank::NUM_MELS));
        for (i, row) in feats.iter().enumerate() {
            for (m, v) in row.iter().enumerate() {
                input[[0, i, m]] = *v;
            }
        }
        let inputs = ort::inputs![Tensor::from_array(input).ok()?];
        let outputs = self.session.run(inputs).ok()?;
        let (_, value) = outputs.iter().next()?;
        let arr = value.try_extract_array::<f32>().ok()?;
        let mut emb: Vec<f32> = arr.iter().copied().collect();
        let norm = emb.iter().map(|v| v * v).sum::<f32>().sqrt();
        if !norm.is_finite() || norm < f32::EPSILON {
            return None;
        }
        for v in emb.iter_mut() {
            *v /= norm;
        }
        Some(emb)
    }
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return -1.0;
    }
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[derive(Serialize, Deserialize, Default)]
struct StoredPrint {
    /// Running mean of enrolled embeddings (re-normalized on use).
    centroid: Vec<f32>,
    samples: u32,
}

/// Per-user voiceprints, persisted locally. Derived vectors only — never audio.
pub struct VoiceStore {
    path: PathBuf,
    prints: HashMap<String, StoredPrint>,
}

impl VoiceStore {
    pub fn load(path: PathBuf) -> Self {
        let prints = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { path, prints }
    }

    pub fn enroll(&mut self, user_id: &str, emb: &[f32]) {
        let entry = self.prints.entry(user_id.to_string()).or_default();
        if entry.centroid.len() != emb.len() {
            entry.centroid = emb.to_vec();
            entry.samples = 1;
        } else {
            let n = entry.samples as f32;
            for (c, v) in entry.centroid.iter_mut().zip(emb) {
                *c = (*c * n + v) / (n + 1.0);
            }
            entry.samples = entry.samples.saturating_add(1);
        }
        if let Ok(json) = serde_json::to_string(&self.prints) {
            let _ = std::fs::write(&self.path, json);
        }
    }

    pub fn samples(&self, user_id: &str) -> u32 {
        self.prints.get(user_id).map_or(0, |p| p.samples)
    }

    pub fn similarity(&self, user_id: &str, emb: &[f32]) -> Option<f32> {
        let p = self.prints.get(user_id)?;
        if p.centroid.is_empty() {
            return None;
        }
        let norm = p.centroid.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm < f32::EPSILON {
            return None;
        }
        let normalized: Vec<f32> = p.centroid.iter().map(|v| v / norm).collect();
        Some(cosine(&normalized, emb))
    }
}

/// Decision rule for reassigning an ambiguous line, given (user, similarity)
/// for each candidate that has a voiceprint. Pure — unit-tested.
pub fn pick_by_similarity(mut scored: Vec<(String, f32)>) -> Option<String> {
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    match scored.as_slice() {
        [] => None,
        [(id, s)] => (*s >= MATCH_THRESHOLD).then(|| id.clone()),
        [(id, s1), (_, s2), ..] => {
            (*s1 >= MATCH_THRESHOLD && s1 - s2 >= MATCH_MARGIN).then(|| id.clone())
        }
    }
}

/// End-to-end discrimination check against the real ONNX model + fixtures:
///   CALLOUT_AB_DIR=<dir> cargo test --release -- --ignored voice_discriminates
/// Fixtures: en1/en2 = same voice (Samantha), pt1/pt2 = Luciana, es1 = Mónica.
#[cfg(test)]
mod model_tests {
    use super::*;

    fn read_wav(path: &std::path::Path) -> Vec<f32> {
        let bytes = std::fs::read(path).expect("read wav");
        let pos = bytes.windows(4).position(|w| w == b"data").expect("data chunk") + 8;
        bytes[pos..]
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
            .collect()
    }

    #[test]
    #[ignore]
    fn voice_discriminates_speakers() {
        let Some(dir) = std::env::var_os("CALLOUT_AB_DIR") else {
            eprintln!("CALLOUT_AB_DIR not set; skipping");
            return;
        };
        let home = std::env::var("HOME").unwrap();
        let model = std::path::PathBuf::from(home).join(
            "Library/Application Support/app.callout.desktop/models/speaker/wespeaker_en_voxceleb_resnet34_LM.onnx",
        );
        let mut vid = VoiceId::load(&model).expect("model");
        let dir = std::path::PathBuf::from(dir);
        let emb = |name: &str, vid: &mut VoiceId| {
            vid.embed(&read_wav(&dir.join(name))).expect("embedding")
        };
        let en1 = emb("en1.wav", &mut vid);
        let en2 = emb("en2.wav", &mut vid);
        let pt1 = emb("pt1.wav", &mut vid);
        let pt2 = emb("pt2.wav", &mut vid);
        let es1 = emb("es1.wav", &mut vid);

        let same_en = cosine(&en1, &en2);
        let same_pt = cosine(&pt1, &pt2);
        let cross1 = cosine(&en1, &pt1);
        let cross2 = cosine(&pt1, &es1);
        let cross3 = cosine(&en2, &es1);
        eprintln!("[voice-ab] same-speaker: en={same_en:.3} pt={same_pt:.3}");
        eprintln!("[voice-ab] cross-speaker: en/pt={cross1:.3} pt/es={cross2:.3} en/es={cross3:.3}");
        assert!(same_en > cross1 + 0.15, "en1/en2 vs en1/pt1: {same_en:.3} vs {cross1:.3}");
        assert!(same_pt > cross2 + 0.15, "pt1/pt2 vs pt1/es1: {same_pt:.3} vs {cross2:.3}");
        assert!(
            same_en >= MATCH_THRESHOLD && same_pt >= MATCH_THRESHOLD,
            "same-speaker similarity below MATCH_THRESHOLD ({MATCH_THRESHOLD})"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_of_identical_is_one() {
        let v = vec![0.6, 0.8];
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn store_enrolls_and_averages() {
        let dir = std::env::temp_dir().join(format!("callout-vs-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("voiceprints.json");
        let _ = std::fs::remove_file(&path);
        let mut store = VoiceStore::load(path.clone());
        store.enroll("1", &[1.0, 0.0]);
        store.enroll("1", &[0.0, 1.0]);
        assert_eq!(store.samples("1"), 2);
        // Centroid (0.5, 0.5) normalized → similarity with (1,0) ≈ 0.707.
        let sim = store.similarity("1", &[1.0, 0.0]).unwrap();
        assert!((sim - 0.707).abs() < 0.01, "sim {sim}");
        // Persists across load.
        let store2 = VoiceStore::load(path);
        assert_eq!(store2.samples("1"), 2);
    }

    #[test]
    fn pick_requires_threshold_and_margin() {
        assert_eq!(
            pick_by_similarity(vec![("a".into(), 0.6), ("b".into(), 0.3)]),
            Some("a".to_string())
        );
        // Below threshold.
        assert_eq!(pick_by_similarity(vec![("a".into(), 0.3), ("b".into(), 0.1)]), None);
        // Margin too small — stay ambiguous.
        assert_eq!(pick_by_similarity(vec![("a".into(), 0.5), ("b".into(), 0.47)]), None);
        // Single candidate above threshold wins.
        assert_eq!(pick_by_similarity(vec![("a".into(), 0.45)]), Some("a".to_string()));
        assert_eq!(pick_by_similarity(vec![]), None);
    }
}
