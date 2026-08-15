# Attribution 2.0 — resolving overlapping speakers

Status: design, 2026-08-15. Prereq reading: `discord-rpc.md` (speaking events), `stt-engine.md`.

## The problem

Attribution v1 joins a whole VAD utterance against every speaker whose SPEAKING
window overlaps it. When B interjects while A talks, the entire line renders as
"A + B". Frequent in real calls; the maintainer flagged it after the first live
session.

## Three layers, ordered by cost

### Layer 1 — finer-grained timestamp alignment (no ML, do first)

Most "overlaps" are *interleaved*, not acoustically simultaneous: A speaks
0–4 s, B interjects at 2.5–3.0 s. Whisper can emit per-token/segment timestamps
(currently disabled for speed on partials; enable on **finals** only). Split the
utterance into word spans and attribute each span against the speaking windows:

- span overlaps exactly one speaker → that speaker;
- span overlaps several → candidate set for Layer 2 (or joint label).

Render as separate caption lines per speaker run. Expected to resolve the
majority of today's "A + B" lines. Cost: ~a day; finals-only so latency is
untouched.

### Layer 2 — voiceprints, auto-enrolled ("the app learns your friends")

The insight: **solo speech is free labeled training data.** Whenever exactly one
speaking window covers a segment (the common case), we know who spoke and we
have their clean 16 kHz audio. Extract a speaker embedding and update that
user's voiceprint (running average per Discord user id, persisted locally).
Nobody records anything; after a few minutes of normal conversation every
regular teammate has a voiceprint.

On a genuinely ambiguous span (Layer 1 found 2–3 candidates), embed the span
and pick the candidate voiceprint with highest cosine similarity — the search
space is tiny (the 2–3 lit-up users, not the whole roster), which keeps accuracy
high even with a small model. Below a confidence threshold, keep the honest
joint label ("A + B").

An explicit "record this friend" button (the maintainer's original idea) stays
as an optional booster/repair for voices the auto-enrollment gets wrong — not a
requirement.

**Tech:** ECAPA-TDNN / WeSpeaker speaker-embedding model in ONNX (~20–80 MB,
192-d embeddings). `ort` is already in our tree (via voice_activity_detector);
alternatives: sherpa-onnx's bundled speaker-id models + frontend via `sherpa-rs`.
Embedding a 1–2 s span is cheap CPU work (~ms). Privacy: voiceprints are
192-float vectors stored locally (`voiceprints.json`), derived data, never audio;
delete-on-request per user id in settings.

### Layer 3 — the ceiling: truly simultaneous speech

Two voices mixed in the same samples can't be perfectly split by attribution —
the audio itself is a mixture, and the transcript interleaves. Options, honest
about tradeoffs:

- accept joint labels for hard simultaneity (v1 behavior, now rare);
- **bot mode** (per-user Opus streams — see phase-1 research): perfect
  separation, perfect attribution, at the cost of inviting a bot + the
  undocumented voice-receive API. Already planned as the optional
  "high fidelity" mode; this is what makes it worth building eventually.

## Expectations to set

With Layers 1+2 and ~5 enrolled friends: interjections resolve cleanly;
short simultaneous bursts mostly resolve to the dominant voice; sustained
scream-overs stay approximate (joint label). That matches how human captioners
handle crosstalk, and bot mode exists for anyone who needs perfection.
