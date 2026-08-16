# Research: a meetings mode? (live captions/translation for Zoom/Meet/Teams)

Date: 2026-08-15. Question from the maintainer: Brazilians working for foreign
companies would benefit from reading meetings live (in English or translated to
Portuguese). Is there tech to pull from Granola-style projects? Is someone
already doing this? Should this project absorb the use case — or pivot to it?

Two parallel research passes (market landscape; open-source tech). Full agent
reports in the session log; this file is the synthesis.

## Market verdict (condensed)

- **Same-language captions are commoditized.** EN→EN reading is free on Zoom,
  Teams, Meet, Windows 11 Live Captions, Chrome.
- **Live EN→PT is the paywall — and the attendee can't pay it.** All three
  platforms gate translated captions behind employer-level licensing (Teams
  Premium/Copilot, Zoom Business Plus/add-on, Workspace Business Standard+).
  The Brazilian IC whose employer didn't buy the tier has no self-serve native
  option. That licensing asymmetry is the entire niche.
- **Notetakers (Otter/Granola/Fireflies/Fathom/tl;dv) don't serve it** —
  they optimize post-meeting notes; only Notta sells live translation (add-on),
  Krisp's interpreter is enterprise-sales-only.
- **The overlay niche exists and is crowded with fragile clones**: Seagull
  ($69.99/yr, cloud-metered hours), Whisperr, MirrorCaption, JotMe, NotchLive,
  plus SEO micro-SaaS (some marketing directly in pt-BR). Nearly all are thin
  cloud wrappers with hour-metering. **The open flank is "local/private,
  flat-priced, polished"** — only NotchLive attempts it.
- **The platforms are absorbing the category from above fast** (Teams
  Interpreter GA, Meet speech translation GA + Gemini 3.5 Live Translate
  preview, Zoom AI Companion 3.0, Apple Live Translation). Graveyard of
  head-on competitors: Skype Translator, Vowel, Web Captioner, Airgram.
- **Honest sizing**: a sustainable indie business ($50–100/user/yr band), not
  venture-scale, with a window that narrows every platform release.

## Tech verdict (condensed)

- **Nothing to pull.** Nobody surveyed has a better pipeline than ours —
  Granola itself ($1.5B valuation) uses *cloud* STT (Deepgram/AssemblyAI);
  our per-process capture → VAD → local whisper → overlay is architecturally
  ahead on privacy and already built.
- A meetings mode needs exactly two new pieces:
  1. **Capture-target picker** (Zoom/Meet/Teams/any app) — our capture layer
     was designed for this (`CaptureTarget`), days of work.
  2. **Translation layer**: whisper's built-in `task=translate` gives
     X→English free today (a toggle). English→Portuguese needs local MT:
     **`ct2rs` (MIT) + Opus-MT/Firefox Marian student models — ~17–60 MB per
     language pair, int8, <50 ms per caption line on CPU** (the Firefox
     Translations stack). Avoid NLLB (CC-BY-NC). MADLAD-400 (Apache-2.0,
     1.65 GB q4) as the quality tier for post-meeting text.
- **Repos to study (MIT, not depend on)**: fastrepl/anarlog (ex-Hyprnote —
  Tauri+Rust sibling; vendorable `aec`/`agc`/`denoise` crates matter the day
  we mix mic + system audio) and Zackriya-Solutions/meetily (mixing/ducking
  patterns; Parakeet ONNX live-STT benchmark). Avoid: screenpipe (relicensed
  commercial), Amurex/Whishper (AGPL), Vexa (bot/server architecture).
- **Granola's product lesson inverts for us**: they *removed* live output
  because users watched the AI instead of the meeting. For accessibility and
  for non-native readers, live text IS the product — which is why nobody in
  the notes market competes on live caption quality.

## Decision

**Don't migrate. Absorb.**

1. **The accessibility/Discord wedge stays the product.** It's underserved,
   mission-defining, differentiated (per-speaker attribution via RPC exists
   nowhere else), and no platform giant is circling it — unlike meetings.
2. **Ship "caption any app" as a mode, not a pivot** (post-v0.1): capture
   target picker + `task=translate` toggle costs days and serves the
   maintainer's own meetings use case immediately. No speaker names in that
   mode (no RPC equivalent) — acceptable: the reader wants the words.
3. **EN→PT local translation (ct2rs + Opus-MT) is the roadmap item that makes
   the meetings mode genuinely valuable to Brazilians** — and it also serves
   the core accessibility product (deaf pt-BR gamers reading English
   teammates in Portuguese).
4. **Positioning if we ever market the mode**: local, private, nothing joins
   the call, flat/free — exactly the flank the cloud-metered indies leave
   open, and it inherits credibility from the accessibility mission.
