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
    let names: Vec<&str> = ids
        .iter()
        .filter_map(|id| roster.get(id).map(|m| m.display_name.as_str()))
        .collect();
    let (label, color) = match ids.as_slice() {
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
        speaker_ids: ids,
        speaker_label: label,
        color,
        text: text.to_string(),
        is_final,
        t_start_ms: t0,
    }
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
}
