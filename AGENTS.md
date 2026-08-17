# UNMUTE project instructions

## Mission

UNMUTE is a local-first desktop accessibility app that captions Discord voice chat in an always-on-top game overlay and attributes each line to a speaker. Build for deaf and hard-of-hearing gamers first. Preserve low latency, low game impact, privacy, readable chronology, and honest accuracy claims.

Windows is the primary product platform; macOS is the development platform. Never infer Windows/game validation from a macOS or mock-only result.

## Start here

- Read `docs/PROJECT_STATE.md` for the current handoff, verified state, open decisions, and near-term priorities.
- Read `docs/BRAND.md` before changing naming, visual identity, launch copy, or public assets.
- Read `docs/legal/RELEASE_REVIEW.md` before changing authorization, storage, voice identity, logging, packaging, distribution, or release claims.
- Read `docs/TESTING.md` before changing onboarding, packaging, Discord authorization, or user-facing flows.
- Read `docs/QA_CPU.md` before changing STT cadence, thread counts, model selection, capture backpressure, or performance claims.
- Read the relevant guide under `docs/dev/` before changing RPC, capture, STT, or attribution.
- Treat `docs/STRATEGY.md` and old research as historical context when they conflict with `docs/PROJECT_STATE.md` or current code.

## Working agreements

- Repository code, comments, and product documentation are English-first. Conversation with the maintainer may be in pt-BR.
- Keep audio and transcription local. Do not add telemetry, cloud processing, new network endpoints, or new stored data without explicit product approval and a matching `PRIVACY.md` update.
- The `CALLOUT_DEBUG_AUDIO=1` recorder is an explicit, consent-sensitive debugging exception. Keep it opt-in and never enable it silently.
- Never log transcript text, display names, Discord user IDs, tokens, or voiceprint values. Diagnostic logs may contain structural status, timings, counts, and non-identifying error context only.
- Treat voice embeddings as sensitive biometric data. Keep persistent enrollment disabled by default until informed consent, retention, deletion, encryption, and jurisdictional review are designed and approved.
- Store OAuth credentials only in OS-backed secure storage. Plaintext token files are a release blocker and require migration, deletion, and revocation handling.
- Never capture the microphone, game audio, or unrelated processes. Never inject into Discord or a game process.
- Keep tricky comments focused on why and link to evidence or `docs/dev/` when useful.
- Preserve the shared `now_ms` clock through capture, speaking events, STT, and attribution. Mixing clock domains silently corrupts attribution.
- Keep OS-independent logic pure and unit-tested. Keep platform and FFI layers thin and supervised.
- Preserve real-time boundaries: capture-owned callbacks must not block and must avoid unbounded work or allocation.
- A final caption must not be dropped. Partial captions may be coalesced or skipped under pressure.
- Settings changes must round-trip through the Rust default and persistence layer, Tauri commands/events, and both React windows.
- The overlay stays click-through except in move mode. Re-check hide, quit, monitor bounds, and persistence when changing its lifecycle.
- Preserve chronological reading, color-independent speaker identity, and reduced-motion behavior in the overlay.
- Mock and real pipelines must emit identical serialized event shapes. Mock success is not evidence for live Discord, audio timing, or backpressure.
- Inspect `git status` before editing. Preserve unrelated and untracked user work.
- Treat `app.callout.desktop`, its data directories, and `CALLOUT_*` environment variables as compatibility contracts. Do not rename them without a data/token/model/settings/voiceprint migration.
- Work on a `codex/*` branch and use a PR; do not commit directly to `main` unless the maintainer explicitly requests it.
- The repository is currently MIT-licensed. A source-available/noncommercial change is under review; do not change licensing language piecemeal or call a noncommercial license “open source.” See `docs/legal/LICENSING.md`.
- Treat Discord approval as a release gate. Do not present bring-your-own client IDs as a public workaround for the tester limit unless Discord confirms that distribution pattern in writing.

## Verification

Run the smallest relevant checks while iterating, then the complete headless matrix before a PR that changes code:

```bash
cd app && npm ci
cd app && npx tsc --noEmit
cd app && npm run build
cd app/src-tauri && cargo check --locked
cd app/src-tauri && cargo test --locked
```

Do not launch the GUI (`npm run tauri dev` or `cargo run`) unless the user explicitly asks for an interactive check. It opens desktop windows, registers global shortcuts, and may start audio/Discord flows. Model-backed ignored tests require local fixtures and models; follow the QA agent instructions and never download large models silently.

## Multi-agent roles

Use project agents under `.codex/agents/` for bounded, parallel work:

- `tech_lead`: architecture, correctness, performance, and change-risk review.
- `qa`: headless verification, regression analysis, and CPU benchmark planning.
- `branding`: positioning, identity, accessibility messaging, and launch review.
- `legal`: engineering-grade licensing, privacy, platform-policy, trademark, and release hygiene; not legal advice.

Prefer parallel agents for read-heavy audits and test execution. Keep overlapping source edits serialized and owned by one implementation agent.

## Code review rules

- Flag any path that can block capture, lose a final caption, create holes in audio, or mix timestamps.
- Keep the game's dGPU free unless a separately approved and measured backend explicitly changes that constraint.
- Check both Windows and macOS branches when shared traits, events, settings, packaging, or STT signatures change.
- Treat swallowed errors as acceptable only for truly optional UI effects; authorization, capture, model, and persistence failures must surface.
- Require privacy and notice updates in the same change as new endpoints, persisted files, models, or dependencies with attribution obligations.
- Require the product license, third-party notices, model attribution, privacy information, and required source locations in every packaged artifact; validate the artifact rather than only the repository tree.
- Existing model files need integrity verification before load, not only after a first download.
- Distinguish automated evidence from live evidence. Never claim real Discord, in-game overlay behavior, or CPU impact was verified when only mock/CI tests ran.
