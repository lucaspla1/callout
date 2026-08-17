# Privacy

Unmute is local-first by design: audio is captured on your machine, transcribed on your machine, and thrown away. There is no cloud, no account, no telemetry, no analytics, no crash reporting, and no auto-updater phoning home. This page is the complete story — if the app ever does more than what's written here, that's a bug; please report it.

## What Unmute listens to

Only the Discord desktop app's audio output — the voices in your voice channel. Per-process capture means Unmute never opens your microphone and never hears your game, music, or any other app. Audio lives in memory just long enough to be transcribed, then it's gone. Transcripts exist only on your screen; they are not saved.

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
| `tokens.json` | Your Discord OAuth token. Planned: move into the OS keychain once builds are code-signed (unsigned builds break keychain access across updates). |
| `voiceprints.json` | Small numeric voice fingerprints used to tell speakers apart when several people talk at once. These are derived vectors — not audio, and audio can't be reconstructed from them. They never leave your machine. **Delete this file to make Unmute forget all voices.** |
| `models/` | The downloaded speech and speaker models. |
| `debug-audio/` | **Only exists if you set the `CALLOUT_DEBUG_AUDIO=1` environment variable.** Then the app saves WAV recordings of transcribed audio for debugging. This is real recording of your channel — read the consent note below before enabling it. Off by default; nothing is written otherwise. |

## How to wipe everything

1. Delete the folder above — settings, token, voiceprints, models, all of it.
2. Uninstall the app.
3. In Discord: User Settings → Authorized Apps → remove Unmute (revokes the token on Discord's side).

## Consent and etiquette

Live captioning that stores nothing is assistive listening — you're reading what you were already entitled to hear. Still:

- **Tell your channel you use captions.** It's good manners, it normalizes accessibility tools, and mis-transcriptions make more sense to everyone when people know captions are in play.
- **Recording is different.** Some jurisdictions require every participant's consent to record a conversation. Unmute stores no audio and no transcripts by default — but if you enable `CALLOUT_DEBUG_AUDIO=1`, you are recording, and those rules can apply to you. Get consent first.

---

This document is a plain-language description of how the software works, not legal advice. Questions or spot something off? [Open an issue](https://github.com/lucaspla1/unmute/issues).
