# Testing Unmute

Thanks for helping test Unmute — live captions for Discord voice chat, with each line labeled by who said it. Built for deaf and hard-of-hearing gamers first and currently MIT-licensed.

This guide gets you from zero to captions. It's honest about the rough edges: builds are unsigned (your OS will warn you), and Discord currently limits who can connect, so Lucas must add you as an approved tester before the authorization flow will work.

## What you need

- **Windows 10 (version 2004) or newer**, or **macOS 14.4 (Sonoma) or newer**. These floors are real: the per-app audio capture Unmute uses didn't exist in the OS before them.
- **The Discord desktop app**, installed and logged in. Discord in a browser won't work — Unmute talks to the desktop client on your machine (the same local interface Discord's own StreamKit overlay uses).
- **Internet on first launch** — Unmute downloads its speech-recognition models once (~220 MB on Windows, ~790 MB on macOS).
- A download link for the build. Grab it from [Releases](https://github.com/lucaspla1/unmute/releases) if one is published; otherwise Lucas will send you the build directly.

## Step 1 — Get access to connect (one-time)

Discord gates the local-RPC interface: until an app is approved by Discord, only the app's owner and up to 50 invited testers can authorize it.

### Ask Lucas to add you as a tester

1. Send Lucas your **exact Discord username** — the unique lowercase one from your profile (click your avatar, bottom-left of Discord; e.g. `mariana_plays`). Not your display name, and no `#1234` tag — those are gone.
2. Lucas invites that username in the Discord developer portal.
3. **Check the email inbox attached to your Discord account** (spam folder too) for an invitation from Discord, and click **Accept**. Make sure the browser you accept in is logged into that same Discord account.
4. Done — you're on the tester list and Unmute's default connection will work for you.

If the email never arrives, double-check the username you sent, and ask Lucas to re-invite. There are 50 slots total, so if you stop testing, tell Lucas so the slot can go to someone else.

The code has a `CALLOUT_CLIENT_ID` override for controlled development with a Discord application the operator is authorized to use. It is not a documented public-distribution workaround: obtain written guidance from Discord before offering or promoting that flow to bypass the tester limit.

## Step 2 — Install

The builds are currently **unsigned** — Lucas is an individual developer and hasn't bought code-signing certificates yet — so both operating systems will warn you. The code is open source and the builds come from public CI, so you can see exactly what you're running.

### Windows

1. Run the installer (`Unmute_…_x64-setup.exe`).
2. SmartScreen will likely show "**Windows protected your PC**". Click **More info**, then **Run anyway**. (SmartScreen flags any new unsigned program it hasn't seen enough times — it's a reputation check, not a virus verdict.)
3. The install is per-user (into `%LOCALAPPDATA%\Unmute`) — no admin prompt needed.

### macOS

1. Open the download and move **Unmute.app** to **Applications**.
2. First open depends on your macOS version:
   - **macOS 14 (Sonoma):** right-click (Control-click) Unmute.app → **Open** → **Open**.
   - **macOS 15 (Sequoia) and newer:** Apple removed the right-click trick. Double-click Unmute (it will be blocked — that's expected), then open **System Settings → Privacy & Security**, scroll to the **Security** section, and click **Open Anyway** next to the Unmute message, then confirm. The button only sticks around for a while after a blocked attempt, so do this right after trying to open the app.
   - Comfortable in a terminal? This does the same thing in one line: `xattr -d com.apple.quarantine /Applications/Unmute.app`
3. You only do this once — later launches are normal.

## Step 3 — First run

1. Start **Discord** (desktop) and join a voice channel with a friend, or just be logged in.
2. Start **Unmute**. It will download the speech models (one-time; progress is shown).
3. Discord itself pops up an authorization window: *Unmute wants access to your account* — the requested access is your username/avatar and the local voice-status interface. Click **Authorize**. Unmute waits up to 5 minutes for you to click.
4. Join a voice channel and talk — captions should appear in the overlay, labeled with the speaker's name.

**If authorization fails with an error** instead of connecting: you're probably not on the tester list yet (Door A not finished) — or the `CALLOUT_CLIENT_ID` you set (Door B) has a typo or is missing the Public Client toggle. Fix, restart Unmute, try again.

## Using it

Captions appear in an always-on-top overlay. It's click-through, so it never steals your mouse from the game.

| Action | Windows | macOS |
|---|---|---|
| Show / hide captions | `Ctrl+Shift+C` | `⌘⇧C` |
| Move the overlay | `Ctrl+Shift+O` | `⌘⇧O` |

**Moving:** press the move shortcut once — the overlay becomes grabbable; drag it wherever you like; press the shortcut again to lock it in place (the position is remembered). It's `O`, not `M`, because Discord's own global mute hotkey owns `Ctrl/⌘+Shift+M`.

There's also a tray/menu-bar icon with **Open Unmute**, **Show / hide overlay**, and **Quit**.

## Known limitations (please don't file these — we know)

- **Unsigned builds** → the SmartScreen/Gatekeeper hoops above. Proper signing is planned.
- **Windows captions are less accurate than macOS for now.** Windows runs a smaller speech model entirely on CPU; macOS additionally runs a larger model for the final caption text. Closing this gap is on the roadmap.
- **Exclusive-fullscreen games hide the overlay.** Set your game to **borderless windowed** (most competitive games default to it) and the overlay stays visible.
- **Discord voice only.** Unmute captions Discord's audio, not in-game voice chat or other apps.
- The connection is capped at **50 invited testers** until Discord approves the app (that's Discord's policy, not ours).

## Reporting issues

File bugs at <https://github.com/lucaspla1/unmute/issues>. The perfect report has:

1. What you did, what you expected, what happened (screenshots welcome).
2. Your OS version and Discord flavor (Stable/PTB/Canary).
3. The diagnostic log, found here:
   - **Windows:** `%APPDATA%\app.callout.desktop\unmute-diag.log`
   - **macOS:** `~/Library/Application Support/app.callout.desktop/unmute-diag.log`

The diag log contains **timings and health numbers only — never transcript text, names, or IDs**, so it's safe to attach publicly. One catch: it's wiped each time Unmute starts, so **copy it right after the problem happens**, before relaunching.

## Privacy, in one line

Your audio is captured from Discord only, transcribed **on your machine**, and immediately discarded — nothing you say ever leaves your computer. Details: [PRIVACY.md](https://github.com/lucaspla1/unmute/blob/main/PRIVACY.md).
