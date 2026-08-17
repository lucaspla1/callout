# Unmute

**Live captions for Discord voice chat, rendered as an in-game overlay — with each line labeled by who said it.**

Built for deaf and hard-of-hearing gamers first; useful to anyone who'd rather read the shotcalls (auditory processing differences, non-native speakers, muted-TV households — the curb-cut effect). The current code is MIT-licensed; licensing for future releases is under review.

```
Marina  careful, two pushing right side
Lucas   I'm going B, cover me▏
```

## Why

Voice chat is still a participation barrier for many deaf and hard-of-hearing PC gamers. Friends type callouts, players miss fast conversations, and phone-based transcription loses speaker identity and game context. Unmute is an attempt to close that gap directly on the player's machine.

## How it works (local-first, no bot or client modification)

Two local data sources, joined by timestamps:

1. **Who is speaking** — the Discord desktop client's local RPC reports voice-channel members and `SPEAKING_START/STOP` events (the same surface Discord's own StreamKit overlay uses).
2. **What is being said** — OS-level per-process audio capture takes *only Discord's* audio (never the game's), which a local speech-to-text model transcribes on CPU/iGPU so your GPU stays with the game.

There is no bot to invite, client modification, or process injection. Unmute uses an authorized local Discord connection for presence and OS-level per-process capture for audio, then transcribes and discards it locally. Platform approval and policy requirements are tracked as release gates.

## Status

**Pre-alpha — implemented, not yet release-validated.** The real local pipeline exists across Discord RPC, per-process audio capture, VAD, whisper.cpp, speaker attribution, overlay rendering, settings, model provisioning, tray behavior, and Windows packaging. The current focus is correctness, whole-app CPU measurement, live Discord/game QA, signing, and Discord approval. See [docs/PROJECT_STATE.md](docs/PROJECT_STATE.md) for the verified handoff and [docs/dev/](docs/dev/) for module guides.

| Milestone | What it proves | Status |
|---|---|---|
| M0 skeleton + mock pipeline | Event plumbing, caption UI | ✅ |
| M1 "who's speaking" (Discord RPC) | Auth (PKCE, no secret), presence + speaking events | ✅ verified live 2026-08-15 |
| M2 captions (capture + VAD + whisper) | Per-process audio, STT latency | 🟡 implemented on macOS/Windows; target-hardware CPU and live quality QA pending |
| M3 attribution (the product moment) | Captions with names | 🟡 implemented and unit-tested; live crosstalk QA pending |
| M4 overlay window, settings, hotkey, onboarding | Livable daily driver | 🟡 core flows implemented; signing, onboarding polish, and release QA pending |

## Building

Prereqs: [Rust](https://rustup.rs), Node 20+, and the [Tauri v2 prerequisites](https://tauri.app/start/prerequisites/) for your OS.

```bash
cd app
npm ci
npm run tauri dev
```

Targets: **Windows 10 (2004)+** is the primary platform; macOS 14.4+ is the development platform and works too.

## Project layout

```
app/               Tauri v2 app (React + TS frontend, Rust backend)
  src-tauri/src/
    rpc/           Discord IPC client (who's in the channel, who's speaking)
    capture/       Per-process audio capture (WASAPI / Core Audio tap)
    stt/           VAD, whisper.cpp, and speaker embeddings
    align.rs       Timestamp attribution: transcript segment → speaker
AGENTS.md          Durable Codex project instructions
.codex/agents/     Tech Lead, QA, Branding, and Legal agent definitions
docs/PROJECT_STATE.md  Current verified handoff and priorities
docs/STRATEGY.md   Product principles, architecture, milestones
docs/dev/          Implementation guides per risky module
research/          Phase-1 market & technology research
```

## Privacy & Legal

The processing architecture is local: Unmute captures Discord output (not the microphone or game), transcribes on-device, and has no cloud STT or telemetry service. This pre-alpha is not ready for external distribution: current builds still need secure OAuth-token storage, privacy-safe logging, consent-safe voice identity, and packaged legal notices. First run downloads speech models from their publishers (Hugging Face / GitHub): ~220 MB on Windows, ~790 MB on macOS. Full details:

- [PRIVACY.md](PRIVACY.md) — what's processed, what's stored where, every network endpoint, how to wipe it all
- [NOTICE.md](NOTICE.md) — third-party model & library licenses and attribution
- [DISCLAIMER.md](DISCLAIMER.md) — as-is software, captions contain errors, not affiliated with Discord Inc. or OpenAI
- [docs/legal/RELEASE_REVIEW.md](docs/legal/RELEASE_REVIEW.md) — current release blockers and remediation gates

## License

[MIT](LICENSE) for the current tree. A possible source-available/noncommercial license for future releases is being evaluated; see [docs/legal/LICENSING.md](docs/legal/LICENSING.md). A commercial restriction would no longer be open source, so the change will not be made piecemeal.
