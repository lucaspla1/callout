# UNMUTE project state

_Snapshot: 2026-08-17. This is the durable handoff from the Claude-era work to Codex. Update it when a verified fact or product decision changes._

## Product

UNMUTE is a local-first Tauri desktop app that captions Discord voice chat in a transparent in-game overlay and labels captions by speaker. The primary audience is deaf and hard-of-hearing PC gamers. Secondary users include people with auditory-processing differences, non-native speakers, and anyone playing without game audio.

Core product promises:

- Discord output only; never the microphone, game, or unrelated apps.
- On-device capture, VAD, transcription, and speaker attribution.
- No saved audio or transcript by default.
- Chronological captions with clear speaker identity and low screen occlusion.
- No Discord or game-process injection.
- CPU/iGPU work must leave headroom for the game.

## Repository and sources of truth

- Canonical repository: `https://github.com/lucaspla1/unmute`.
- The existing workspace is that repository; do not create a second “UNMUTE Codex” copy.
- `AGENTS.md` contains durable Codex working rules.
- `.codex/agents/` defines the Tech Lead, QA, Branding, and Legal reviewers.
- `docs/dev/` contains implementation research and module guides.
- `docs/STRATEGY.md` and `research/` preserve product history, but some milestone language predates the current implementation.

At the migration snapshot, `main` was `700f4c0` and matched `origin/main`. The untracked `mockups/branding/` directory predates the migration and must be preserved as user work.

## Architecture

- `app/src-tauri/src/lib.rs`: Tauri setup, window/tray/hotkey lifecycle, commands, and the pipeline join loop.
- `app/src-tauri/src/rpc/`: local Discord IPC, PKCE authorization, subscriptions, presence, and speaking events.
- `app/src-tauri/src/capture/`: macOS Core Audio per-process capture, Windows WASAPI process loopback, and pure 16 kHz conditioning.
- `app/src-tauri/src/stt/`: Silero VAD gate, whisper.cpp worker, language selection, and WeSpeaker voice embeddings.
- `app/src-tauri/src/align.rs`: timestamp-based speaker attribution.
- `app/src-tauri/src/models.rs`: resumable, SHA-256-pinned model provisioning.
- `app/src/`: settings window, overlay, shared Tauri event state, speaker identity, and CSS.

The implementation has moved beyond the old “M0 skeleton” README label: real RPC, macOS capture, Windows capture, STT, attribution, overlay layouts, settings, tray behavior, model provisioning, and packaging code exist. That does not mean v0.1 is accepted: live user testing, performance measurement, Discord approval, signing, and release polish remain.

## Current design handoff

The last attached Claude conversation iterated `mockups/overlay-variants.html` over `mockups/fortnite.png`.

- Six layout variants exist.
- The current favorite is variant 6: the existing stacked caption pills without the always-visible participant chips.
- Participant chips were considered redundant during normal speech. A possible follow-up is to show join/leave/mute chips only as short-lived events; this was proposed, not approved.
- The roster-plus-speech-bubble concept was rejected as a direction because spatial placement obscures chronological reading during overlapping conversation.
- Speaker identity versus screen occlusion remains the central layout trade-off.

No mockup is a final launch asset. The Fortnite screenshot and platform-derived color choices require rights/brand review before public marketing use.

## CPU and Windows evidence

Claude implemented several Windows-oriented reductions in commits around `eade5a3` and `a798265`:

- skip the large-v3-turbo final model on Windows and use the small model for finals;
- cap Windows whisper work at four threads;
- pace Windows partials at 1100 ms instead of 600 ms;
- decode only the trailing six seconds for Windows partials;
- build whisper.cpp with clang-cl, Ninja, AVX2, and C++ exceptions;
- gate the Windows workflow on a faster-than-realtime decode plus install, idle, mock-pipeline, and uninstall checks.

Verified automated evidence:

- The manually dispatched Windows build for `c8d00d6` completed the installer, toolchain proof, STT speed gate, install smoke test, mock pipeline, and uninstall checks.
- macOS typecheck, `cargo check`, and `cargo test` were green at `700f4c0`.

Not yet verified:

- real whole-app CPU usage before versus after the changes;
- CPU impact while an actual game and Discord call are active;
- real caption latency/quality on target Windows hardware;
- crosstalk, long monologues, reconnects, and sustained backpressure.

Known migration-snapshot issues to address in a separate technical change:

- CI is red on Windows because `models::tests::missing_lists_absent_models` assumes all three models are active, while Windows intentionally filters the turbo model.
- The STT backlog coalescer can replace one queued Final with a newer Final despite the “never skip a Final” invariant.
- The VAD gate uses a blocking send for Final jobs on a capture-owned thread; this needs a bounded non-blocking design that still preserves finals.
- Windows partials decode only a six-second tail but retain the original utterance start timestamp, which can misalign partial text and speaker attribution.
- A filtered Final does not explicitly clear the frontend partial, so low-confidence, repeated, or hallucination-gated utterances can leave stale text visible.
- Runtime stderr currently includes final transcript text and voice-refinement identity details. That conflicts with the documented “transcripts only on screen” privacy posture; diagnostics must become structural and non-identifying.
- The pipeline can emit transient Tauri state before React listeners mount, and broadcast lag errors are ignored without resynchronizing presence; add snapshot/resync paths.
- Downloaded models are hash-checked before the initial rename, but pre-existing model files are accepted by `is_file()` without revalidation before load.
- Windows whisper builds currently require AVX2. Document the CPU floor, detect support, or provide a compatible fallback before public distribution.

Compatibility note: the visible product is UNMUTE, but `app.callout.desktop`, existing data directories, Rust/npm package names, and `CALLOUT_*` environment variables are legacy runtime contracts. Rename them only with an explicit migration for tokens, models, settings, logs, and voiceprints.

## Release and legal state

- The 2026-08-17 legal-hygiene audit is **RED for public/commercial release** while development may continue. See `docs/legal/RELEASE_REVIEW.md`.
- The repository is currently MIT-licensed. MIT explicitly permits commercial use and sale.
- The maintainer chose licensing direction B on 2026-08-17: preserve personal, nonprofit, community, and internal workplace accessibility use while blocking resale, white-labeling, paid embedding, and competing products/services. Exact counsel-reviewed source-available terms are still pending; `LICENSE` remains MIT until the change can be made atomically. See `docs/legal/LICENSING.md`.
- Discord app approval is a release dependency. The existing `CALLOUT_CLIENT_ID` override can support controlled development with an app the tester is authorized to operate, but it must not be marketed as a public workaround for Discord's tester limit without written confirmation from Discord.
- OAuth tokens are currently persisted in plaintext, voice embeddings are enrolled and persisted without a separate opt-in, and stderr contains transcripts/identifiers. These contradict the intended privacy posture and block external distribution.
- Existing packaged artifacts do not include the product license, privacy policy, or complete third-party/model notices. A per-target dependency inventory and packaged-artifact gate are required.
- UNMUTE is only a working title: an active US registration for the same word in adjacent audio/software services requires professional trademark clearance or a rename before brand investment or public beta.
- Privacy, notices, disclaimer, terms, tester onboarding, and Discord approval documentation exist, but must be reviewed together whenever data, endpoints, dependencies, models, distribution, or licensing change.
- Public marketing must not imply Discord, Epic Games, Fortnite, OpenAI, or model-publisher affiliation.

## Near-term order of work

1. Land the Codex context and agent migration without mixing in runtime changes.
2. Restore green Windows CI and add regression coverage for platform-specific model sets.
3. Fix final-job/backpressure correctness and the privacy release blockers: personal logs, persistent voiceprints, and plaintext OAuth tokens.
4. Run a reproducible before/after CPU and latency benchmark on representative Windows gaming hardware, then perform live Discord/game QA.
5. Complete the per-target license inventory and make packaged builds contain verified licenses, notices, provenance, and privacy links.
6. Make and document the licensing decision, then update every legal and package reference atomically.
7. Clear or replace the working name, establish a distinct accessible palette, and replace third-party gameplay imagery in public launch assets.
8. Update launch claims from measured evidence, obtain Discord guidance/approval, sign builds, and only then start a small tester rollout.
