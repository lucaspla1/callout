# Unmute — Build Strategy

> Live captions for Discord voice chat, rendered as an in-game overlay, with per-speaker labels.
> Last updated: 2026-08-17 · Current handoff: [`PROJECT_STATE.md`](PROJECT_STATE.md) · Phase-1 market/tech research: [`research/fase-1-panorama-e-stack.md`](../research/fase-1-panorama-e-stack.md)

## Product principles

1. **The accessibility core is free, forever.** Local transcription, speaker labels, and the overlay never sit behind a paywall. (Sustainability comes from donations and optional convenience — see below.)
2. **Local-first.** Audio never leaves the machine by default. Transcribe-and-discard; no audio storage.
3. **Minimize platform and anti-cheat risk by construction.** No client mods, process injection, or bot voice-receive. Use authorized local Discord RPC for presence and OS-level per-process audio capture; treat current Discord policy and app approval as explicit release gates.
4. **Gaming is the wedge, not the ceiling.** The capture layer is source-agnostic so a future "caption any app" mode (Zoom, Meet, videos) is a feature flag away — but v0.1 is Discord-only, done well.
5. **English-first product; multilingual engine.** UI and docs in English. The STT default model is multilingual (English + Portuguese work day one); UI strings externalized from day one so localization is cheap later.
6. **Windows is the target; macOS is the dev platform.** All platform-specific code lives behind traits, so macOS support is nearly free — but Windows is what we optimize, test, and ship first.

## Architecture (Route C from phase-1 research)

No bot. No ML speaker diarization. Two local data sources, joined by timestamps:

```
Discord client ──(RPC/IPC)──► presence: who's in the channel, SPEAKING_START/STOP per user_id
Discord.exe audio ──(per-process loopback)──► capture ──► VAD ──► stt: partial/final text + timestamps
                                                                        │
                          align: overlap transcript segments with speaking windows
                                                                        │
                          overlay: Tauri transparent click-through window, per-speaker lines
```

### Modules (Rust workspace, `app/src-tauri/`)

| Module | Responsibility | Platform notes |
|---|---|---|
| `presence` | Discord RPC client: auth, selected voice channel, member roster (name/avatar), speaking events | Cross-platform (named pipe / unix socket) |
| `capture` | Per-process audio of Discord only → 16 kHz mono PCM stream | `WasapiProcessCapture` (Win 10 2004+) / `CoreAudioTapCapture` (macOS 14.4+) behind an `AudioCapture` trait |
| `stt` | `SttEngine` trait: `feed(pcm)` → `Partial`/`Final(text, t0, t1)` events | v0.1: VAD-chunked whisper.cpp (multilingual). v0.2: true-streaming engine (Nemotron-3.5 / sherpa-onnx) behind the same trait |
| `align` | Assign each transcript segment to speaker(s) whose speaking window overlaps it | Pure logic; unit-testable with recorded fixtures |
| `overlay` | Transparent, always-on-top, click-through caption window; global hotkey toggle | Tauri v2 window flags; second-monitor placement option |
| `settings` | Position/size/font/opacity/language/engine; first-run flow | Tauri settings window (same React app) |

Implementation guides for the three risky modules live in [`docs/dev/`](dev/): `discord-rpc.md`, `audio-capture.md`, `stt-engine.md`.

## Milestones

Ordered so each one de-risks the scariest remaining unknown and is demoable on its own:

- **M0 — Skeleton.** Tauri v2 app scaffolded; Rust workspace with empty modules; CI building Windows + macOS artifacts.
- **M1 — "Who's speaking" overlay (RPC only, no audio).** Join a voice channel → overlay shows members and lights up whoever is speaking, with name + color. *This alone is Overlayed-parity and already useful. De-risks: RPC auth, event stream, overlay rendering over a game.*
- **M2 — Captions, unattributed.** Discord audio → VAD → whisper → rolling caption line in the overlay. *De-risks: per-process capture, STT latency on CPU while a game runs.*
- **M3 — Attribution.** Merge M1 + M2: each caption line carries the speaker's name and color. *This is the distinctive product moment.*
- **M4 — Livable.** Settings (position/size/opacity/font/language), global hotkey, first-run onboarding (Discord app authorization), caption history scrollback, graceful states (Discord not running, not in a channel).
- **v0.1 — Public release.** GitHub release with signed binaries, README with a 30-second demo GIF, announcement to the accessibility community (Can I Play That covered CaptionsRush; they'll care about a free OSS alternative).

Out of scope for v0.1: bot mode, cloud STT, translation, non-Discord sources, Mac polish, auto-update.

## Known constraints (from phase-1 research — don't relearn these)

- **Discord RPC for unapproved apps only authorizes the app owner's/testers' accounts.** Fine for development; wide distribution needs Discord app approval (Overlayed got it — there's precedent). Track as a v0.1-release blocker.
- **Per-process loopback taps decrypted local audio** — DAVE E2EE (mandatory since Mar 2026) is irrelevant to this route. Never build on bot voice-receive for the core path.
- **Never inject into game processes.** Transparent topmost window covers borderless/windowed (the modern default). Detect exclusive fullscreen and coach the user; second monitor is the universal fallback.
- **The dGPU belongs to the game.** STT runs on CPU (quantized small models) or iGPU; benchmark with a game actually running before tuning quality up.
- **Consent UX**: visible "captions active" state; no audio persisted, ever, in v0.x.

## Sustainability (not the point, but the door stays open)

Models that fit an accessibility-first app, in order of fit:

1. **Donations/sponsorship** — GitHub Sponsors + Ko-fi from day one (costs nothing to add). Craig (Discord's recorder bot) has run on Patreon for years; precedent exists in this exact niche.
2. **Grants** — accessibility and OSS funds (NLnet, GitHub Accelerator, platform accessibility funds). A shipped, documented, used project is what makes these applications credible.
3. **Optional hosted convenience, later** — a paid "cloud boost" subscription (managed cloud STT for people who want max accuracy with zero setup; BYO-key stays free). Open-core done right: the paid thing is convenience, never access.
4. **Not doing**: ads or paywalled core captions. Licensing and commercial-exception policy are under review; accessibility access must remain the product constraint. See [`legal/LICENSING.md`](legal/LICENSING.md).

## Decision log

| Date | Decision |
|---|---|
| 2026-08-15 | Product/docs English-first (largest audience); conversation with maintainer may be pt-BR |
| 2026-08-15 | Skip phase-2 user validation for now; build the MVP (maintainer's call) |
| 2026-08-15 | Route C (RPC + per-process loopback); bot mode deferred |
| 2026-08-15 | Windows-first target, macOS as dev platform via trait abstraction |
| 2026-08-15 | Tauri v2 + Rust; MIT license; "Callout" as working title (rename before release) |
| 2026-08-15 | v0.1 STT: VAD-chunked multilingual whisper.cpp; true-streaming engine is v0.2 behind the same trait |
| 2026-08-17 | Product name changed to Unmute; run trademark/name screening before public beta |
| 2026-08-17 | Favor the chronological chip-less caption-pill layout; roster-attached speech bubbles rejected for overlapping conversation |
| 2026-08-17 | Future licensing is under review: commercial restrictions would be source-available, not open source |
