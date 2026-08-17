# Audio Capture: Discord-Only Loopback (Windows + macOS)

Status: implementation guide, v1 — 2026-08-15
Scope: capture ONLY the Discord client's audio output (not the game, not the mic), deliver
16 kHz mono f32 PCM chunks to the VAD/STT pipeline. Rust, inside the Tauri v2 backend.

| | Windows | macOS |
|---|---|---|
| API | WASAPI **process loopback** (`ActivateAudioInterfaceAsync` + `AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS`) | Core Audio **process tap** (`CATapDescription` + `AudioHardwareCreateProcessTap`) |
| OS floor | Docs say build 20348; works in practice from Win10 2004 (19041) — see §1.1 | API from 14.2; target **14.4+** (TCC fixes; what AudioCap targets) |
| Rust crate | `wasapi` ≥ 0.24 (has it built in) | `cidre` ≥ 0.20 (`core_audio` module, MIT) |
| Native format | Whatever we declare (engine converts); declare 48 kHz / 2ch / f32 | Tap's format — read it, usually 48 kHz / 2ch / f32, can be 44.1 kHz |
| Permission | None | TCC "System Audio Recording" prompt via `NSAudioCaptureUsageDescription` |

---

## 1. Windows — WASAPI Process Loopback

### 1.1 API overview and real OS floor

`ActivateAudioInterfaceAsync` with device id `VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK` and an
`AUDIOCLIENT_ACTIVATION_PARAMS { ActivationType = AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, ProcessLoopbackParams = { TargetProcessId, ProcessLoopbackMode } }`
returns an `IAudioClient` that captures the mix of render streams belonging to one process
(and, with `PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE`, all its descendants). It is
**not bound to any endpoint**: if the user switches headsets, capture continues — a major
advantage over classic endpoint loopback.

MS Learn stamps the structs "minimum: Windows 10 Build 20348", but the feature shipped with
Win10 2004 (19041): OBS's win-capture-audio plugin requires "Windows 10 2004+" and works
there. Treat 19041 as the practical floor, feature-detect at runtime (attempt activation;
on `E_INVALIDARG`/`AUDCLNT_E_UNSUPPORTED_FORMAT`-class failures fall back to endpoint
loopback, §3.6).

- Struct docs: <https://learn.microsoft.com/en-us/windows/win32/api/audioclientactivationparams/ns-audioclientactivationparams-audioclient_process_loopback_params>
- Mode enum: <https://learn.microsoft.com/en-us/windows/win32/api/audioclientactivationparams/ne-audioclientactivationparams-process_loopback_mode>
- MS sample (MIT — safe to adapt): <https://github.com/microsoft/windows-classic-samples/tree/main/Samples/ApplicationLoopback> (overview: <https://learn.microsoft.com/en-us/samples/microsoft/windows-classic-samples/applicationloopbackaudio-sample/>)
- OBS win-capture-audio (**GPL-2.0 — read for behavior, do NOT copy code**): <https://github.com/bozbez/win-capture-audio>

### 1.2 Which PID? Discord is multi-process

Discord is Electron/Chromium: a root `Discord.exe` (browser process) spawns renderer, GPU,
and **utility** children — audio output is rendered by the child utility process running
`audio.mojom.AudioService` (see <https://chromium.googlesource.com/chromium/src/+/main/services/audio/README.md>),
*not* by the root. Therefore:

- Target the **root Discord.exe** with `PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE`.
  Include-tree covers the audio-service child no matter which child it is; this is exactly
  the pattern win-capture-audio and Discord-capture tools use.
- Finding the root with `sysinfo`: the root is the `Discord.exe` whose parent is **not**
  another live `Discord.exe` (its parent is Squirrel's `Update.exe`, often already exited).

```rust
use sysinfo::{ProcessesToUpdate, System};

const NAMES: &[&str] = &["Discord.exe", "DiscordPTB.exe", "DiscordCanary.exe"];

pub fn find_discord_root(sys: &mut System) -> Option<u32> {
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let is_discord = |pid: &sysinfo::Pid| {
        sys.process(*pid)
            .map_or(false, |p| NAMES.iter().any(|n| p.name().eq_ignore_ascii_case(n)))
    };
    sys.processes()
        .iter()
        .filter(|(pid, p)| is_discord(pid) && !p.parent().map_or(false, |pp| is_discord(&pp)))
        .map(|(pid, _)| pid.as_u32())
        .next() // if several variants run, prefer stable Discord.exe first in practice
}
```

Sort candidates so stable > PTB > Canary, or let the user pick when several are running.

### 1.3 Quirks you must design around (all verified)

1. **`GetMixFormat` returns `E_NOTIMPL`** on a process-loopback client — there is no device,
   hence no mix format. You must declare the format yourself and pass
   `AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM` so the engine converts each source stream into it.
   (MS confirmation: <https://github.com/microsoft/Windows-classic-samples/issues/275>,
   <https://learn.microsoft.com/en-us/answers/questions/1125409/loopbackcapture-(-activateaudiointerfaceasync-with>.)
   **Declare 48 000 Hz, 2 ch, f32** (IEEE float). That matches the engine mix on virtually
   all systems (conversion becomes a no-op) and feeds our own high-quality 48k→16k stage
   (§3). The MS sample declares 44.1 kHz/16-bit only because it writes a CD-format WAV.
   Do *not* declare 16 kHz mono directly: it works, but you'd be trusting the engine's
   converter with the anti-aliasing your STT depends on.
2. Also `E_NOTIMPL`/failing on this client: `GetDevicePeriod`, `GetCurrentPadding`,
   `GetService(IAudioClock/IAudioSessionControl)`. Consequence: no padding-based polling;
   read via `GetNextPacketSize`/`GetBuffer` only. (The `wasapi` crate documents exactly this
   restriction set: <https://docs.rs/wasapi/latest/wasapi/struct.AudioClient.html>.)
3. **Event-driven works and is the right mode**: `Initialize(SHARED, LOOPBACK | EVENTCALLBACK
   | AUTOCONVERTPCM, 0, 0, &fmt, NULL)` then `SetEventHandle`. The sample uses an MF async
   callback; a plain thread blocking on `WaitForSingleObject` is equivalent and simpler.
   Polling (~10 ms timer + drain `GetNextPacketSize` loop) also works if you ever need it.
4. **Silence = gaps, not zeros.** When the target renders nothing, expect *no packets*
   (endpoint loopback has the same trait: <https://github.com/PortAudio/portaudio/issues/935>;
   MS words it as "receives silence"). Never derive wall-clock time from sample count;
   timestamp packets at capture (§3.5) and let downstream treat absence as silence. When
   packets do arrive, honor the `AUDCLNT_BUFFERFLAGS_SILENT` flag as a free silence gate.
5. **Discord exit/restart is not a device event.** The client keeps "running" and simply
   delivers nothing; you may or may not see `AUDCLNT_E_DEVICE_INVALIDATED`. Detect via the
   process watcher (§1.2 rescan every ~2 s, or on a `SourceLost` heuristic: N seconds with
   zero packets AND pid dead) → `Stop()`, drop the client, re-find root PID, re-activate.
   New PID ⇒ activation params differ ⇒ full re-activation is mandatory.
6. **Default-device changes are transparent** for process loopback (not endpoint-bound).
   Only the fallback endpoint-loopback path (§3.6) needs `IMMNotificationClient` handling.

### 1.4 Rust path A (recommended): the `wasapi` crate

`wasapi` (HEnquist, MIT, <https://github.com/HEnquist/wasapi-rs>) supports this natively
since it wrapped `ActivateAudioInterfaceAsync`:
`AudioClient::new_application_loopback_client(process_id, include_tree)` — see
<https://docs.rs/wasapi/latest/wasapi/struct.AudioClient.html> and the shipped
**`record_application` example**, which is precisely our use case. Sketch (API of 0.24.x;
mirror the example for exact signatures):

```rust
use wasapi::*;

pub fn run_capture(pid: u32, tx: RawChunkSender, stop: StopFlag) -> Result<(), WasapiError> {
    initialize_mta()?; // COM init for this capture thread
    let mut client = AudioClient::new_application_loopback_client(pid, /*include_tree=*/ true)?;
    let fmt = WaveFormat::new(32, 32, &SampleType::Float, 48_000, 2, None); // declared, not queried
    client.initialize_client(&fmt, &Direction::Capture, &ShareMode::Shared,
        &StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: 0 })?;
    let event = client.set_get_eventhandle()?;
    let capture = client.get_audiocaptureclient()?;
    client.start_stream()?;
    let mut frames = vec![0f32; 4800 * 2]; // reused scratch, 100 ms stereo
    while !stop.is_set() {
        if event.wait_for_event(500).is_err() { continue; } // timeout ≠ error: target is silent
        // drain every pending packet; capture QPC timestamp per packet
        capture.read_from_device(...)?; // GetBuffer/ReleaseBuffer loop under the hood
        tx.push_raw48k_stereo(&frames_read, qpc_ts);
    }
    client.stop_stream()
}
```

### 1.5 Rust path B: `windows` crate directly

Only needed if we outgrow `wasapi`. Follow the MS sample (MIT) 1:1:
1. Pack `AUDIOCLIENT_ACTIVATION_PARAMS` into a `PROPVARIANT` (`VT_BLOB`, `blob.pBlobData`
   pointing at the struct — this is the fiddly part; copy the pattern from the sample).
2. `#[implement(IActivateAudioInterfaceCompletionHandler)]` struct that signals an event in
   `ActivateCompleted`; call `ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
   &IAudioClient::IID, Some(&propvariant), &handler)`; wait; `GetActivateResult`.
3. `Initialize` / `SetEventHandle` / `IAudioCaptureClient` loop as in §1.3.
Note: the completion handler may be invoked on a COM worker thread — keep it trivial
(SetEvent only). Features needed: `Win32_Media_Audio`, `Win32_System_Com_StructuredStorage`,
`Win32_System_Variant`, `Win32_Foundation`, `Win32_System_Threading`.

---

## 2. macOS — Core Audio Process Taps

### 2.1 API overview and the SCK alternative

Since macOS 14.2 the HAL can tap the output of specific processes:
`CATapDescription` (list of process **AudioObjectIDs**, mono/stereo mixdown, mute behavior,
private flag) → `AudioHardwareCreateProcessTap` → tap `AudioObjectID` → put the tap's UUID in
the `kAudioAggregateDeviceTapListKey` of a **private aggregate device** (with the current
default output device in its sub-device list) → `AudioDeviceCreateIOProcIDWithBlock` on the
aggregate → `AudioDeviceStart`; the tap audio arrives as the IOProc's input buffer list.
Canonical reference implementation: **AudioCap** (BSD-2-Clause, adapt freely with attribution):
<https://github.com/insidegui/AudioCap>. Apple docs:
<https://developer.apple.com/documentation/coreaudio/audiohardwarecreateprocesstap(_:_:)>,
<https://developer.apple.com/documentation/coreaudio/catapdescription>.

Alternative considered — **ScreenCaptureKit audio-only** (`SCStreamConfiguration.capturesAudio`
+ `SCContentFilter` per-app): works since macOS 13, but it runs on screen-capture
infrastructure and demands the **Screen Recording** TCC permission — a far scarier prompt
("wants to record your screen") for an accessibility app that never touches pixels, plus the
purple menu-bar indicator. Process taps need only the audio-capture permission and no
indicator. **Decision: process taps; SCK only if we ever need ≤14.3 support.**
(Comparison: <https://dgrlabs.co/blog/2026-04-25-capturing-system-audio-on-macos-in-2026.html>.)

### 2.2 Which process to tap? (Discord is multi-process here too)

Same Chromium architecture: on macOS audio is rendered by a `Discord Helper` utility process,
not the root app process. There is **no include-tree mode** in `CATapDescription` — you tap an
explicit list of HAL process objects. Two translation routes:

- `kAudioHardwarePropertyTranslatePIDToProcessObject` — PID → process AudioObjectID
  (<https://developer.apple.com/documentation/coreaudio/kaudiohardwarepropertytranslatepidtoprocessobject>).
  Fine when you already know the audio-rendering PID — but with Discord you don't, and the
  *root* PID's process object renders nothing.
- **Robust plan (do this):** enumerate `kAudioHardwarePropertyProcessObjectList`, read each
  object's `kAudioProcessPropertyBundleID`, and select every process whose bundle id starts
  with Discord's (`com.hnc.Discord`, `com.hnc.DiscordPTB`, `com.hnc.DiscordCanary` — helpers
  are `com.hnc.Discord.helper*`). Tap **all matches** in one stereo-mixdown tap. Optionally
  inspect `kAudioProcessPropertyIsRunningOutput` for diagnostics, but don't filter on it —
  tap the set, streams that start later inside those processes are included automatically.
- **Re-tap on membership change:** install a property listener on
  `kAudioHardwarePropertyProcessObjectList` (system object). New helper / restarted Discord ⇒
  new process objects ⇒ rebuild tap + aggregate. `CATapDescription` also has
  `processRestoreEnabled` (auto re-tap by bundle id after process exit) on newer OSes —
  enable it when available, but keep the listener as the portable mechanism.

### 2.3 Rust path: `cidre` (recommended)

`cidre` (yury/cidre, MIT, on crates.io — <https://docs.rs/cidre/latest/cidre/core_audio/>)
covers the whole surface in `core_audio`: `System`, `Process`, `TapDesc`, `Tap`/`TapGuard`,
`AggregateDevice`, `DeviceIoProc`, plus a `hardware_tapping` module. Verified constructors on
`TapDesc`: `with_stereo_mixdown_of_processes()`, `with_mono_mixdown_of_processes()`,
`with_stereo_global_tap_excluding_processes()`, `set_mute_behavior()`, `set_private()`, and
`create_process_tap() -> Result<TapGuard>`. Sketch (check docs.rs for exact signatures):

```rust
use cidre::core_audio as ca;

pub struct TapCapture { _tap: ca::TapGuard, agg: ca::AggregateDevice, proc_id: ca::DeviceIoProcId }

pub fn start(bundle_prefix: &str, tx: RawChunkSender) -> anyhow::Result<TapCapture> {
    let procs: Vec<ca::Process> = ca::System::processes()? // kAudioHardwarePropertyProcessObjectList
        .into_iter()
        .filter(|p| p.bundle_id().map_or(false, |b| b.to_string().starts_with(bundle_prefix)))
        .collect();
    anyhow::ensure!(!procs.is_empty(), "Discord has no audio process objects yet");

    let desc = ca::TapDesc::with_stereo_mixdown_of_processes(&procs);
    desc.set_private(true);                       // invisible to other HAL clients
    desc.set_mute_behavior(ca::TapMuteBehavior::Unmuted); // user keeps hearing Discord
    let tap = desc.create_process_tap()?;          // TCC prompt fires here on first run
    let asbd = tap.asbd()?;                        // kAudioTapPropertyFormat — do NOT assume 48k

    // Private aggregate: default output as sub-device + our tap in the tap list.
    let agg = ca::AggregateDevice::builder()      // sub-device: default output UID,
        .tap(&tap)                                 // kAudioAggregateDeviceTapListKey entry,
        .private(true)                             // kAudioAggregateDeviceIsPrivateKey
        .build()?;
    let proc_id = agg.create_io_proc_id(move |_now, input: &ca::AudioBufList, in_ts, _, _| {
        // input holds tap audio in `asbd`; forward + host-time timestamp (in_ts.host_time)
        tx.push_raw(input, &asbd, in_ts.host_time);
    })?;
    agg.start(proc_id)?;
    Ok(TapCapture { _tap: tap, agg, proc_id })
}
```

Teardown order matters (AudioCap): `AudioDeviceStop` → destroy IOProc → destroy aggregate →
destroy tap (TapGuard/Drop impls handle it — keep the guards alive for the session).
Do **not** route the aggregate through `AVAudioEngine`; it fails silently — use the IOProc.
Fallback if cidre ever blocks us: hand-roll with `coreaudio-sys` + `objc2` (CATapDescription
is an ObjC class; the HAL calls are plain C) — that's what AudioCap proves out in Swift.

### 2.4 TCC permission story

- Add `NSAudioCaptureUsageDescription` ("Unmute captions Discord voice chat so you can read
  it in-game.") to the app's Info.plist — in Tauri v2, via an `Info.plist` file next to
  `tauri.conf.json` in `src-tauri/` (Tauri merges it into the bundle).
- The system prompt ("wants to record audio from other applications") appears on the first
  `AudioHardwareCreateProcessTap`. **It only appears for properly signed apps** — an unsigned
  `cargo run` binary just gets errors. Dev workflow: `tauri build`/`tauri dev` with a Developer
  ID or dev certificate; re-test prompts with `tccutil reset All <bundle-id>`.
- There is no public "query permission" API; AudioCap uses a private TCC SPI behind a build
  flag — for us: just attempt tap creation and map the failure to a "grant permission in
  System Settings → Privacy & Security → System Audio Recording" UI hint.

---

## 3. Common pipeline: 48 kHz stereo → 16 kHz mono chunks

Per-platform threads deliver *raw* interleaved f32 + timestamp; everything below is shared.

1. **Downmix first** (`mono[i] = 0.5 * (l + r)`) — halves resampler work. Discord voice is
   effectively mono per speaker; equal-weight average loses nothing intelligibility-wise.
2. **Resample with `rubato`** (HEnquist, MIT/Apache-2, <https://docs.rs/rubato>). v5 API:
   `Fft` resampler for fixed ratios — perfect for 48 000→16 000 (and 44 100→16 000 on odd
   macOS devices; read the actual rate from the tap ASBD / declared format). `Async::new_poly`
   exists if CPU ever matters, but `Fft` at 3:1 is already cheap; **skip hand-rolled linear
   decimation even for v0.1** — aliasing directly hurts STT accuracy and rubato is one
   dependency. Feed `process_into_buffer` fixed 10 ms input blocks (480 frames @48k → 160 @16k).
3. **Chunking:** accumulate resampler output into fixed **20 ms frames (320 samples @16 kHz,
   f32)** — the least-common-denominator for VAD consumers (WebRTC-VAD 10/20/30 ms; Silero v5
   wants 512-sample/32 ms windows, which the VAD stage re-buffers trivially from 20 ms frames).
4. **Ring buffer + backpressure:** OS audio callbacks/threads must never block. Use a bounded
   `tokio::sync::mpsc::Sender<CaptureEvent>` (capacity ~64 chunks ≈ 1.3 s) with `try_send`;
   on `Full`, drop the *oldest* (captions want freshness, not completeness) and bump a
   `dropped_frames` counter surfaced in a debug overlay. Inside the capture thread use a
   plain `Vec`/`heapless` scratch, no allocation per callback.
5. **Silence gating + latency:**
   - Windows: packets flagged `AUDCLNT_BUFFERFLAGS_SILENT` → emit nothing (counter only).
     Both platforms: cheap RMS gate on the 48 kHz block (< −60 dBFS for >300 ms ⇒ suppress
     until sound returns). The real VAD stays downstream; this gate only saves resampler/IPC
     work during the (dominant) silent periods, and process-loopback gaps (§1.3.4) make
     silence free on Windows anyway.
   - Latency: stamp every chunk at the source — Windows `GetBuffer`'s `u64QPCPosition`
     (QueryPerformanceCounter units), macOS the IOProc's `AudioTimeStamp.mHostTime`
     (mach ticks). Convert to a monotonic `Instant`-comparable value; log p50/p95 of
     `now − chunk_ts` per stage (capture→resample→VAD→STT→overlay). Capture-stage budget:
     ~10–30 ms (WASAPI event cadence ≈10 ms; macOS IO buffer 5–20 ms). End-to-end test:
     spawn the test-tone process (§4.3), toggle the tone, measure to caption event.
6. **Fallback mode — "capture everything" (future Zoom/any-app support):**
   - Windows: classic endpoint loopback on the default render device (`wasapi`:
     default render device → capture client with loopback), plus `IMMNotificationClient`
     (crate: `device_notifications` example) to restart on default-device switch.
   - macOS: same tap machinery with
     `TapDesc::with_stereo_global_tap_excluding_processes(&[our_own_process])` — same
     permission, near-zero extra code.
   - Behind the same trait: `CaptureTarget::DefaultOutput` vs `CaptureTarget::Process{..}`.
     Caveat to document for users: fallback captures game audio too, so captions may see music
     (the VAD/STT will mostly ignore it, but Discord-only mode is strictly better).

---

## 4. Module sketch

### 4.1 API (crate-internal, `src-tauri/src/audio/`)

```rust
pub enum CaptureTarget { Process(ProcessSpec), DefaultOutput }
pub struct ProcessSpec { pub root_pid: u32, pub bundle_prefix: String } // pid: win, bundle: mac

pub struct PcmChunk { pub samples: Box<[f32]>, /* 320 = 20 ms @16k mono */ pub ts: CaptureTs }

pub enum CaptureEvent {
    Chunk(PcmChunk),
    Silence { since_ms: u32 },          // gate engaged / loopback gap detected
    SourceLost(SourceLost),             // Discord exited, device invalidated, tap died
    Started { native_rate: u32, native_ch: u16 },
}

pub trait AudioCapture: Send {
    fn start(&mut self, target: CaptureTarget, tx: tokio::sync::mpsc::Sender<CaptureEvent>)
        -> Result<(), CaptureError>;   // spawns the platform thread; non-blocking
    fn stop(&mut self);                // idempotent, joins thread
}
```

Files: `audio/mod.rs` (trait, pipeline types, `pub fn create() -> Box<dyn AudioCapture>`),
`audio/pipeline.rs` (downmix, rubato, chunker, gate — pure, unit-testable),
`audio/wasapi.rs` (`#[cfg(windows)] WasapiProcessCapture` + PID finder),
`audio/coreaudio.rs` (`#[cfg(target_os = "macos")] CoreAudioTapCapture` + process-list watcher),
`audio/supervisor.rs` (restart loop: watch `SourceLost` → poll for Discord → `start` again,
with 1 s→5 s backoff; owns the `sysinfo::System`). The Tauri layer (`lib.rs`) holds the
supervisor in `app.manage(...)`, exposes `start_captions`/`stop_captions` commands, and
forwards `CaptureEvent::SourceLost`/`Started` as Tauri events so the overlay can show
"waiting for Discord…". STT consumes the mpsc receiver inside `tauri::async_runtime`.

### 4.2 Platform notes

- The WASAPI thread: `initialize_mta()` (or STA per wasapi-rs docs) *on that thread*;
  raise priority via AvSetMmThreadCharacteristics("Audio") later if we see jitter.
- The Core Audio "thread" is the HAL's IOProc callback: do nothing there but copy + `try_send`
  to the pipeline (which runs on a normal thread). Keep `TapGuard`/aggregate alive in the impl.

### 4.3 Testing without Discord

- **Any Chromium browser is a perfect Discord stand-in** (same multi-process/audio-service
  layout): capture Chrome/Edge's root PID include-tree (or bundle prefix `com.google.Chrome`
  on macOS) while YouTube plays; verify children's audio is caught — this validates the
  include-tree/bundle-matching logic specifically.
- **Deterministic integration test:** tiny `test-tone` bin in the workspace (`rodio` sine at
  440 Hz); spawn it, capture its PID/bundle, assert ≥95% of 1 s of chunks have dominant energy
  at 440 Hz (Goertzel), then kill it and assert `SourceLost` fires. Requires an audio device →
  `#[ignore]` in CI, run in the manual pre-release checklist (CI runners have no endpoint;
  process loopback still needs the audio engine running).
- Pipeline (`pipeline.rs`) is pure: unit-test downmix/resample/chunker/gate with synthetic
  buffers on every platform in CI.

---

## Sources

- MS: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS / PROCESS_LOOPBACK_MODE / ActivateAudioInterfaceAsync — links in §1.1
- MS ApplicationLoopback sample (MIT): <https://github.com/microsoft/windows-classic-samples/tree/main/Samples/ApplicationLoopback>
- GetMixFormat E_NOTIMPL: <https://github.com/microsoft/Windows-classic-samples/issues/275>, <https://github.com/microsoft/Windows-classic-samples/issues/343>
- `wasapi` crate (process loopback support + `record_application` example): <https://docs.rs/wasapi/latest/wasapi/struct.AudioClient.html>, <https://github.com/HEnquist/wasapi-rs>
- Loopback gap behavior: <https://github.com/PortAudio/portaudio/issues/935>
- OBS win-capture-audio (GPL, reference only): <https://github.com/bozbez/win-capture-audio>
- Apple: AudioHardwareCreateProcessTap, CATapDescription, TranslatePIDToProcessObject — links in §2
- AudioCap (BSD-2-Clause reference impl): <https://github.com/insidegui/AudioCap>
- Process taps vs ScreenCaptureKit: <https://dgrlabs.co/blog/2026-04-25-capturing-system-audio-on-macos-in-2026.html>
- `cidre` core_audio (TapDesc etc.): <https://docs.rs/cidre/latest/cidre/core_audio/>, <https://github.com/yury/cidre>
- Chromium audio service (why include-tree/helpers): <https://chromium.googlesource.com/chromium/src/+/main/services/audio/README.md>
- `rubato`: <https://docs.rs/rubato> · `sysinfo`: <https://docs.rs/sysinfo>
