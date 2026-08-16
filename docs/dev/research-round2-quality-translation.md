# Research round 2: caption quality, translation recipe, and the steal list

Date: 2026-08-15 (autonomous session). Two deep-dive agents: (1) SOTA live-caption
techniques + local translation; (2) source-level mining of anarlog (ex-Hyprnote)
and Meetily. Full reports in the session log; this is the actionable synthesis.

## ⚠️ Granola "open source": refuted — and a safety warning

Granola is **closed source**. The GitHub org `Granola-AI` is a squatter
distributing a password-protected archive via an off-GitHub download page —
classic malware pattern. **Never download from it.** What people actually mean
by "open-source Granola" is `fastrepl/anarlog` (MIT — literally an anagram of
granola, tagline "Granola, rearranged") and `Zackriya-Solutions/meeting-minutes`
(Meetily, MIT). Both were mined below.

## Shipped from this round (already in the repo)

- **Probability hallucination gate** (the numbers everyone ships): finals drop on
  `mean token p < 0.4` (LocalVocal's production gate) or
  `no_speech_prob > 0.6 && avg_logprob < -1.0` (OpenAI's silence test);
  partials drop on the mean-p gate. Gated finals are logged with their stats.
- **Ban-list expansion**: Amara.org credit lines in any position (the #1
  documented PT hallucination), ES credits, plus anarlog's single-word noise
  blocklist ("you"/"thank you"/♪/obrigado/gracias) and trailing-"..." strip.
- Convergent validation: anarlog independently implements our
  restricted-language detection (argmax of `lang_detect` over user-selected
  languages only). We got there first by a few months of their git history.

## Next quality upgrades, ranked (effort → impact)

1. **LocalAgreement-2 committed-prefix partials** (~1 day). Freeze words agreed
   by two consecutive partial decodes; render frozen words solid, the tail
   dimmed; fast-commit tokens with p > 0.95 (WhisperLiveKit's trick). Kills
   partial flicker; prerequisite for sane translation. Targets: ~0.45s
   hypothesis / ~1.7s confirmed per-word latency (WhisperKit's shipping numbers).
2. **Two-model decode** (~1 day): keep small-q5_1 greedy for partials; add
   `ggml-large-v3-turbo-q5_0` (574 MB) with beam-5 for finals only —
   ~0.4–1.0s final latency on M2/M3, big PT/ES accuracy jump. Ship as a
   "High accuracy" toggle (8 GB Macs stay on small). Try `flash_attn(true)`
   but A/B on PT first (anarlog disables it on macOS citing crashes).
3. **VAD chunk shaping from anarlog** (~1–2 days): adaptive negative threshold
   (hard to end a chunk before min duration, easy after target), short-chunk
   merging, forced split with context carry-over. Fixes fragment finals.
4. **Parakeet-TDT-0.6b-v3 int8 via plain ort** (~3–5 days): Meetily proves the
   path — istupakov's ONNX export + a ~150-line TDT greedy decoder,
   25 languages incl. PT, 80ms-resolution token timestamps (feeds our
   attribution), much faster than whisper. The credible v0.2 engine
   (alternative to Nemotron whose Rust runtimes are still young).

## Translation recipe (decided)

- **Engine**: `ct2rs = "0.10"` (CTranslate2 bindings, MIT, active), features
  `["sentencepiece", "accelerate", "ruy"]` on macOS / `["sentencepiece", "ruy"]`
  on Windows. Builds CTranslate2 from source: **CMake + C++17 in CI**. CPU-only
  (no Metal) — irrelevant at Marian sizes: tens of ms per caption line.
- **Models**: convert Helsinki-NLP to CT2 int8 (one-time, `ct2-transformers-converter`):
  - en→pt: `opus-mt-tc-big-en-pt` (~235 MB int8) — **`>>pob<<` tag selects pt-BR!**
  - en→es: `opus-mt-tc-big-en-es` (~235 MB)
  - pt→en + es→en in ONE model: `opus-mt-tc-bible-big-roa-en` (~240 MB)
  - All CC-BY-4.0. Same-day prototype path: Argos `.argosmodel` files are
    ready-made CT2 dirs (66–285 MB, lower quality) — unzip and point ct2rs.
- **Firefox/Bergamot models: ruled out** (bergamot-only binary format, no
  converter input published).
- **UX**: translate finals always (beam 2–4); for partials translate only the
  LocalAgreement committed prefix, debounced ≥400ms — never the dimmed tail.
  Render as a second line.

## The anarlog steal list (MIT, port-with-attribution)

1. **Downloader upgrade path** for ours: parallel 8MB range chunks (8-way),
   `.part-{generation}` naming, partial validation + 1MB-boundary truncation
   on resume, crc32 verification, monotonic 0–99% progress via AtomicU8 CAS,
   friendly error mapping. Also: consider mirroring model files on our own
   bucket like their hyprnote S3 (HF rate-limit immunity).
2. **`token_beg` logit-filter callback** (bans the `[_BEG_]` token every step —
   kills the empty/looping segment failure mode) + `temperature_inc(0.2)`.
3. **Model-manager idle eviction**: unload whisper contexts after N idle
   minutes (frees ~700 MB when captions idle).
4. **Voiceprint span selection** (their `crates/voiceprint` solves our exact
   problem): enroll only 1.5–10s spans, subtract overlapped-speech intervals
   entirely ("overlapped speech contaminates embeddings"), top-3 spans per
   speaker by duration.
5. **Interactive islands on the click-through overlay**: named-rect registry +
   20Hz cursor hit-test flipping `set_ignore_cursor_events` — hover controls
   (pause, size, pin) without stealing game clicks.
6. **DTLN-aec + SyncProbe alignment** (`crates/aec`, `crates/audio-sync`) —
   the entire echo-cancellation feature for the day we mix the user's mic.
7. Debug trick: env-gated per-chunk WAV dump for "why did it say that" reports.

## Meetily takeaways

Messier codebase (stub DSP, committed backup files) but three real things:
the ort-based Parakeet integration above, zero-padding (never sample-hold) on
buffer underruns, and two frontend patterns worth copying: typewriter reveal on
finalized text (~15ms/char, hides finalize latency) and a 2px confidence dot
(color by mean token p — accessibility-friendly uncertainty display).
