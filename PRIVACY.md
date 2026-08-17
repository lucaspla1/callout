# Privacy

_Last updated: 2026-08-17. This describes the current pre-alpha and calls out known release blockers; it is not approval to distribute the build publicly._

Unmute is local-first by design: audio is captured and transcribed on your machine. There is no Unmute cloud account, cloud STT, telemetry, analytics, crash-reporting service, or auto-updater. Current pre-alpha builds still have privacy defects listed below and must not be represented as release-ready.

## What Unmute listens to

Only the Discord desktop app's audio output — the voices in your voice channel. Per-process capture means Unmute does not open your microphone or intentionally capture the game, music, or another app. Audio normally lives in memory just long enough to be transcribed. The optional debug mode described below is an explicit exception.

Known pre-alpha defect: production stderr currently includes transcript text and Discord identity details. Stderr may be retained by a terminal, launcher, CI runner, or crash collection outside Unmute. This must be removed and regression-tested before external distribution.

## Every network connection the app makes

| Endpoint | When | Why |
|---|---|---|
| `huggingface.co` | First run | Downloads the Whisper speech model(s): ~190 MB on Windows, ~765 MB on macOS (which adds a larger model for higher-quality finals) |
| `github.com` | First run | Downloads the speaker-identification model (~26 MB, from the sherpa-onnx releases) |
| `discord.com` (`/api/oauth2/token`) | Sign-in and token refresh | Standard Discord OAuth token exchange (PKCE — no app secret involved) |
| `cdn.discordapp.com` | While in a channel | Fetches avatars of channel members, only when your speaker-label setting shows avatars |

That's the entire list. Models come straight from their publishers — Unmute doesn't proxy or mirror them. Talking to the Discord client itself happens over local IPC on your machine, not the internet.

## What's stored on disk

Everything lives in one folder:

- **macOS:** `~/Library/Application Support/app.callout.desktop/`
- **Windows:** `%APPDATA%\app.callout.desktop\`

| File | Contents |
|---|---|
| `settings.json` | Overlay preferences and language selection. Nothing sensitive. |
| `tokens.json` | Discord OAuth access and refresh tokens. They are currently plaintext with filesystem permissions, which is a release blocker. They must move to macOS Keychain or Windows Credential Manager (or equivalent OS-protected encryption), with migration/revocation handling, before distribution. |
| `voiceprints.json` | Numeric voice embeddings used to distinguish speakers. They are not raw audio or intended to retain spoken words, but may be used to distinguish or identify a person, so Unmute treats them as sensitive biometric data. Current automatic persistence without separate informed opt-in is a release blocker. **Delete this file to make Unmute forget stored embeddings.** |
| `unmute-diag.log` | Structural diagnostic events. The intended contract forbids transcripts, display/channel names, Discord IDs, tokens, or voiceprint values; current stderr leaks remain a separate known defect. |
| `models/` | The downloaded speech and speaker models. |
| `debug-audio/` | **Only exists if you set the `CALLOUT_DEBUG_AUDIO=1` environment variable.** Then the app saves WAV recordings of transcribed audio for debugging. This is real recording of your channel — read the consent note below before enabling it. Off by default; public builds should remove or separately gate it with an explicit warning. |

## How to wipe everything

1. Delete the folder above — settings, token, voiceprints, models, all of it.
2. Uninstall the app.
3. In Discord: User Settings → Authorized Apps → remove Unmute (revokes the token on Discord's side).

## Consent and etiquette

Rules for listening, captioning, biometric processing, and recording vary by jurisdiction and context. This policy does not determine whether a user is legally entitled to process a conversation.

- **Tell your channel you use captions and voice attribution.** It is good practice and may be legally required. Persistent voice enrollment must remain disabled until a compliant consent and deletion design is approved.
- **Recording is different.** Some jurisdictions require every participant's consent to record a conversation. Unmute stores no audio and no transcripts by default — but if you enable `CALLOUT_DEBUG_AUDIO=1`, you are recording, and those rules can apply to you. Get consent first.

---

This document is a plain-language description of how the software works, not legal advice. Questions or spot something off? [Open an issue](https://github.com/lucaspla1/unmute/issues).
