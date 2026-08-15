# Callout <sub>(working title)</sub>

**Live captions for Discord voice chat, rendered as an in-game overlay — with each line labeled by who said it.**

Free and open source. Built for deaf and hard-of-hearing gamers first; useful to anyone who'd rather read the shotcalls (auditory processing differences, non-native speakers, muted-TV households — the curb-cut effect).

```
Marina  careful, two pushing right side
Lucas   I'm going B, cover me▏
```

## Why

Xbox has shipped party-chat captions since 2021. PS5 since launch. Switch 2 since 2025. Discord — where PC gaming actually talks — has shipped nothing, with accessibility requests open since 2020. Deaf gamers today get kicked from lobbies, ask friends to type, or point a phone at their speakers. This app closes that gap without waiting for Discord.

## How it works (no bot, no ToS gray zones)

Two local data sources, joined by timestamps:

1. **Who is speaking** — the Discord desktop client's local RPC reports voice-channel members and `SPEAKING_START/STOP` events (the same surface Discord's own StreamKit overlay uses).
2. **What is being said** — OS-level per-process audio capture takes *only Discord's* audio (never the game's), which a local speech-to-text model transcribes on CPU/iGPU so your GPU stays with the game.

No bot to invite, no client mods, no process injection (nothing for anti-cheat to dislike), and audio never leaves your machine — transcribe and discard.

## Status

**M0 — skeleton.** The Tauri app runs an end-to-end mock pipeline (fake presence + fake transcription → attribution → caption UI). Real Discord RPC (M1) and real capture + whisper (M2) slot in behind the same traits. See [docs/STRATEGY.md](docs/STRATEGY.md) for the milestone plan and [docs/dev/](docs/dev/) for module implementation guides.

| Milestone | What it proves | Status |
|---|---|---|
| M0 skeleton + mock pipeline | Event plumbing, caption UI | ✅ |
| M1 "who's speaking" (Discord RPC) | Auth (PKCE, no secret), presence + speaking events | ✅ verified live 2026-08-15 |
| M2 captions (capture + VAD + whisper) | Per-process audio, STT latency | ⏳ in progress |
| M3 attribution (the product moment) | Captions with names | ⏳ lands with M2 (attribution logic already tested) |
| M4 overlay window, settings, hotkey, onboarding | Livable daily driver | — |

## Building

Prereqs: [Rust](https://rustup.rs), Node 20+, and the [Tauri v2 prerequisites](https://tauri.app/start/prerequisites/) for your OS.

```bash
cd app
npm install
npm run tauri dev
```

Targets: **Windows 10 (2004)+** is the primary platform; macOS 14.4+ is the development platform and works too.

## Project layout

```
app/               Tauri v2 app (React + TS frontend, Rust backend)
  src-tauri/src/
    presence.rs    Discord RPC client (who's in the channel, who's speaking)
    capture.rs     Per-process audio capture (WASAPI / Core Audio tap)
    stt.rs         Speech-to-text engine trait (whisper.cpp first)
    align.rs       Timestamp attribution: transcript segment → speaker
docs/STRATEGY.md   Product principles, architecture, milestones
docs/dev/          Implementation guides per risky module
research/          Phase-1 market & technology research
```

## License

[MIT](LICENSE). Contributions welcome once M1 lands.
