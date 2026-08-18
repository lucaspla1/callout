# Testing Unmute

Thanks for helping test Unmute — live captions for Discord voice chat, with each line labeled by who said it. Built for deaf and hard-of-hearing gamers first, free and open source.

This guide gets you from zero to captions. It's honest about the rough edges: builds are unsigned (your OS will warn you), and Discord currently limits who can connect (there's a step where Lucas adds you as a tester — or a self-serve alternative if you'd rather not wait).

## What you need

- **Windows 10 (version 2004) or newer**, or **macOS 14.4 (Sonoma) or newer**. These floors are real: the per-app audio capture Unmute uses didn't exist in the OS before them.
- **The Discord desktop app**, installed and logged in. Discord in a browser won't work — Unmute talks to the desktop client on your machine (the same local interface Discord's own StreamKit overlay uses).
- **Internet on first launch** — Unmute downloads up to ~790 MB of speech and speaker models once. If the optional high-quality model cannot be downloaded, captions still start with the smaller local model.
- **About 1.2 GB of working memory for speech recognition** when the optional quality model is loaded (roughly 370 MB with the smaller-model fallback). This is an experimental quality/CPU tradeoff we are measuring on Windows hardware.
- A download link for the build. Grab it from [Releases](https://github.com/lucaspla1/unmute/releases) if one is published; otherwise Lucas will send you the build directly.

## Step 1 — Get access to connect (one-time)

Discord gates the local-RPC interface: until an app is approved by Discord, only the app's owner and up to 50 invited testers can authorize it. So before your first launch, pick one of two doors:

### Door A (easiest): ask Lucas to add you as a tester

1. Send Lucas your **exact Discord username** — the unique lowercase one from your profile (click your avatar, bottom-left of Discord; e.g. `mariana_plays`). Not your display name, and no `#1234` tag — those are gone.
2. Lucas invites that username in the Discord developer portal.
3. **Check the email inbox attached to your Discord account** (spam folder too) for an invitation from Discord, and click **Accept**. Make sure the browser you accept in is logged into that same Discord account.
4. Done — you're on the tester list and Unmute's default connection will work for you.

If the email never arrives, double-check the username you sent, and ask Lucas to re-invite. There are 50 slots total, so if you stop testing, tell Lucas so the slot can go to someone else.

### Door B (self-serve): bring your own Discord app ID

No waiting on anyone: you create your own (free, empty) Discord application, and Unmute connects as *that* app. Discord always lets an application's owner authorize their own app, so there's no tester list to be on. Takes about two minutes:

1. Go to <https://discord.com/developers/applications> and click **New Application**. Name it anything — that name is what Discord's consent popup will show you (e.g. "My Unmute").
2. No bot, no special permissions, nothing to enable — except one thing: open the **OAuth2** page and
   - turn **Public Client** **on**,
   - add `http://127.0.0.1` under **Redirects** (it's never actually visited; Discord just wants one registered).
   Save changes.
3. On **General Information**, copy the **Application ID** (a long number).
4. Tell Unmute to use it, via the `CALLOUT_CLIENT_ID` environment variable:

   **Windows** — open Command Prompt and run (paste your own ID):

   ```
   setx CALLOUT_CLIENT_ID 123456789012345678
   ```

   Close the terminal, then start Unmute normally (Start menu). If it doesn't take, sign out of Windows and back in once.

   **macOS** — quickest (lasts until you log out/reboot; re-run it after a reboot):

   ```
   launchctl setenv CALLOUT_CLIENT_ID 123456789012345678
   ```

   then quit and relaunch Unmute. If you prefer something permanent, add
   `export CALLOUT_CLIENT_ID=123456789012345678` to `~/.zshrc` and launch the app from a terminal with
   `/Applications/Unmute.app/Contents/MacOS/Unmute` (Finder/Dock launches don't read `.zshrc`, hence the `launchctl` route above).

5. Launch Unmute — the Discord consent popup will show your app's name. Approve it and you're set. (A settings-screen field for this is planned so the env var won't be needed forever.)

Switching between Door A and Door B later is fine — Unmute just asks for a fresh Discord authorization.

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
- The default connection is capped at **50 testers** until Discord approves the app (that's Door A/B above, and it's Discord's policy, not ours).

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
