//! Attribution: join transcript segments with Discord speaking windows.
//!
//! The core idea (Route C): a transcript segment spanning [t0, t1] belongs to the
//! member(s) whose SPEAKING_START/STOP window overlaps that span. Exactly one
//! overlapping speaker → clean attribution. Multiple → joint attribution ("A + B")
//! until something smarter (voice-embedding disambiguation) lands post-v0.1.

use serde::Serialize;
use std::collections::HashMap;

use crate::presence::Member;

#[derive(Debug, Clone, Serialize)]
pub struct CaptionLine {
    /// None when no speaking window overlapped (e.g. RPC hiccup): render unattributed.
    pub speaker_ids: Vec<String>,
    pub speaker_label: String,
    pub color: String,
    pub text: String,
    pub is_final: bool,
    pub t_start_ms: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct SpeakingSpan {
    pub start_ms: u64,
    /// None while still speaking.
    pub end_ms: Option<u64>,
}

/// Rolling state of who spoke when. Pruned to a small horizon; this never grows unbounded.
#[derive(Default)]
pub struct SpeakingLog {
    spans: HashMap<String, Vec<SpeakingSpan>>,
    horizon_ms: u64,
}

impl SpeakingLog {
    pub fn new() -> Self {
        Self { spans: HashMap::new(), horizon_ms: 30_000 }
    }

    pub fn speaking_start(&mut self, user_id: &str, at_ms: u64) {
        self.spans.entry(user_id.to_string()).or_default().push(SpeakingSpan { start_ms: at_ms, end_ms: None });
        self.prune(at_ms);
    }

    pub fn speaking_stop(&mut self, user_id: &str, at_ms: u64) {
        if let Some(spans) = self.spans.get_mut(user_id) {
            if let Some(last) = spans.last_mut() {
                if last.end_ms.is_none() {
                    last.end_ms = Some(at_ms);
                }
            }
        }
    }

    /// User ids whose speaking spans overlap [t0, t1].
    pub fn speakers_between(&self, t0: u64, t1: u64) -> Vec<String> {
        let mut out: Vec<String> = self
            .spans
            .iter()
            .filter(|(_, spans)| {
                spans.iter().any(|s| s.start_ms < t1 && s.end_ms.map_or(true, |e| e > t0))
            })
            .map(|(id, _)| id.clone())
            .collect();
        out.sort();
        out
    }

    fn prune(&mut self, now_ms: u64) {
        let cutoff = now_ms.saturating_sub(self.horizon_ms);
        for spans in self.spans.values_mut() {
            spans.retain(|s| s.end_ms.map_or(true, |e| e >= cutoff));
        }
        self.spans.retain(|_, spans| !spans.is_empty());
    }
}

/// Attribute a finalized utterance per word: consecutive words with the same
/// speaker set merge into one line, so "A talks, B interjects" renders as two
/// correctly-labeled lines instead of one joint "A + B" line. Words with no
/// overlapping speaker inherit the previous run (VAD pre-roll, ring latency).
/// With no word timings, falls back to whole-utterance attribution — output is
/// then identical to v1.
pub fn attribute_final(
    log: &SpeakingLog,
    roster: &HashMap<String, Member>,
    text: &str,
    words: &[crate::stt::Word],
    t0: u64,
    t1: u64,
) -> Vec<CaptionLine> {
    if words.is_empty() {
        return vec![attribute(log, roster, text, true, t0, t1)];
    }
    // Runs of consecutive words sharing a speaker set.
    let mut runs: Vec<(Vec<String>, Vec<&str>, u64)> = Vec::new(); // (ids, words, run_t0)
    for w in words {
        let mut ids = log.speakers_between(w.t0_ms, w.t1_ms.max(w.t0_ms + 1));
        if ids.is_empty() {
            if let Some(last) = runs.last() {
                ids = last.0.clone();
            }
        }
        match runs.last_mut() {
            Some((last_ids, texts, _)) if *last_ids == ids => texts.push(&w.text),
            _ => runs.push((ids, vec![&w.text], w.t0_ms)),
        }
    }
    // Merge a leading unattributed run into the following one.
    if runs.len() > 1 && runs[0].0.is_empty() {
        let (_, texts, run_t0) = runs.remove(0);
        let first = &mut runs[0];
        let mut merged = texts;
        merged.extend(first.1.iter().copied());
        first.1 = merged;
        first.2 = run_t0;
    }
    runs.into_iter()
        .map(|(ids, texts, run_t0)| {
            let joined = texts.join(" ");
            let mut line = line_for_ids(&ids, roster, &joined, true, run_t0);
            line.speaker_ids = ids;
            line
        })
        .collect()
}

fn line_for_ids(
    ids: &[String],
    roster: &HashMap<String, Member>,
    text: &str,
    is_final: bool,
    t_start_ms: u64,
) -> CaptionLine {
    let names: Vec<&str> = ids
        .iter()
        .filter_map(|id| roster.get(id).map(|m| m.display_name.as_str()))
        .collect();
    let (label, color) = match ids {
        [] => ("?".to_string(), "#9BA0AE".to_string()),
        [one] => {
            let m = roster.get(one);
            (
                m.map(|m| m.display_name.clone()).unwrap_or_else(|| "?".into()),
                m.map(|m| m.color.clone()).unwrap_or_else(|| "#9BA0AE".into()),
            )
        }
        _ => (names.join(" + "), "#E9EAF0".to_string()),
    };
    CaptionLine {
        speaker_ids: ids.to_vec(),
        speaker_label: label,
        color,
        text: text.to_string(),
        is_final,
        t_start_ms,
    }
}

/// Build a display line for a transcript segment given the current roster.
pub fn attribute(
    log: &SpeakingLog,
    roster: &HashMap<String, Member>,
    text: &str,
    is_final: bool,
    t0: u64,
    t1: u64,
) -> CaptionLine {
    let ids = log.speakers_between(t0, t1);
    line_for_ids(&ids, roster, text, is_final, t0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: &str, name: &str) -> Member {
        Member { id: id.into(), display_name: name.into(), color: "#57F287".into(), avatar_url: None, muted: false }
    }

    #[test]
    fn single_speaker_gets_the_line() {
        let mut log = SpeakingLog::new();
        log.speaking_start("1", 1000);
        log.speaking_stop("1", 3000);
        let roster = HashMap::from([("1".to_string(), member("1", "Marina"))]);
        let line = attribute(&log, &roster, "push B", true, 1200, 2800);
        assert_eq!(line.speaker_label, "Marina");
        assert_eq!(line.speaker_ids, vec!["1"]);
    }

    #[test]
    fn overlapping_speakers_are_joint() {
        let mut log = SpeakingLog::new();
        log.speaking_start("1", 1000);
        log.speaking_stop("1", 3000);
        log.speaking_start("2", 2000);
        log.speaking_stop("2", 4000);
        let roster = HashMap::from([
            ("1".to_string(), member("1", "Marina")),
            ("2".to_string(), member("2", "Lucas")),
        ]);
        let line = attribute(&log, &roster, "yeah", true, 2200, 2900);
        assert_eq!(line.speaker_ids.len(), 2);
        assert!(line.speaker_label.contains('+'));
    }

    #[test]
    fn open_span_still_matches() {
        let mut log = SpeakingLog::new();
        log.speaking_start("1", 1000);
        let roster = HashMap::from([("1".to_string(), member("1", "Marina"))]);
        let line = attribute(&log, &roster, "still talking", false, 1500, 2000);
        assert_eq!(line.speaker_label, "Marina");
    }

    #[test]
    fn no_overlap_is_unattributed() {
        let log = SpeakingLog::new();
        let roster = HashMap::new();
        let line = attribute(&log, &roster, "ghost words", true, 100, 200);
        assert!(line.speaker_ids.is_empty());
        assert_eq!(line.speaker_label, "?");
    }

    fn word(text: &str, t0: u64, t1: u64) -> crate::stt::Word {
        crate::stt::Word { text: text.into(), t0_ms: t0, t1_ms: t1 }
    }

    #[test]
    fn interjection_splits_into_two_lines() {
        // Marina talks 0–4s; Lucas interjects 2.5–3.2s.
        let mut log = SpeakingLog::new();
        log.speaking_start("1", 0);
        log.speaking_stop("1", 2400);
        log.speaking_start("2", 2500);
        log.speaking_stop("2", 3200);
        let roster = HashMap::from([
            ("1".to_string(), member("1", "Marina")),
            ("2".to_string(), member("2", "Lucas")),
        ]);
        let words = vec![
            word("push", 100, 500),
            word("B", 600, 900),
            word("now", 1000, 1400),
            word("on", 2600, 2800),
            word("it", 2900, 3100),
        ];
        let lines = attribute_final(&log, &roster, "push B now on it", &words, 0, 3200);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].speaker_label, "Marina");
        assert_eq!(lines[0].text, "push B now");
        assert_eq!(lines[1].speaker_label, "Lucas");
        assert_eq!(lines[1].text, "on it");
    }

    #[test]
    fn single_speaker_stays_one_line() {
        let mut log = SpeakingLog::new();
        log.speaking_start("1", 0);
        log.speaking_stop("1", 3000);
        let roster = HashMap::from([("1".to_string(), member("1", "Marina"))]);
        let words = vec![word("hello", 100, 500), word("there", 600, 900)];
        let lines = attribute_final(&log, &roster, "hello there", &words, 0, 1000);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].speaker_label, "Marina");
        assert_eq!(lines[0].text, "hello there");
    }

    #[test]
    fn no_words_falls_back_to_v1_behavior() {
        let mut log = SpeakingLog::new();
        log.speaking_start("1", 0);
        log.speaking_stop("1", 3000);
        let roster = HashMap::from([("1".to_string(), member("1", "Marina"))]);
        let lines = attribute_final(&log, &roster, "hello there", &[], 0, 1000);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "hello there");
        assert_eq!(lines[0].speaker_label, "Marina");
    }

    #[test]
    fn unattributed_gap_words_inherit_previous_run() {
        // Marina 0–1s; gap; words at 1.2s (VAD tail) stay hers.
        let mut log = SpeakingLog::new();
        log.speaking_start("1", 0);
        log.speaking_stop("1", 1000);
        let roster = HashMap::from([("1".to_string(), member("1", "Marina"))]);
        let words = vec![word("go", 100, 400), word("go", 1200, 1400)];
        let lines = attribute_final(&log, &roster, "go go", &words, 0, 1500);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].speaker_label, "Marina");
    }
}
