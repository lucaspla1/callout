# QA and CPU benchmark baseline

_Snapshot: 2026-08-17. Results are evidence for this commit and machine, not a general product claim._

## Release verdict

The local macOS headless matrix is green, and the Windows installer workflow has passed its compile, faster-than-realtime STT, install, idle, mock-pipeline, and uninstall gates. UNMUTE is not release-ready because Windows unit-test CI is red, final-job/backpressure correctness is not proven, and whole-app CPU/latency has not been measured during a real Discord call and game.

## Verified checks

Local environment: macOS 27.0, Apple M5 arm64, 16 GB RAM.

| Check | Result |
|---|---|
| `cd app && npx tsc --noEmit` | Pass |
| `cd app && npm run build` | Pass; JS 223.24 kB, 68.09 kB gzip |
| `cd app/src-tauri && cargo fmt --all -- --check` | Pass |
| `cd app/src-tauri && cargo check --locked --offline` | Pass with five warnings |
| `cd app/src-tauri && cargo test --locked --offline` | Pass; 31 passed, 4 ignored |
| `cd app/src-tauri && cargo clippy --all-targets -- -D warnings` | Fail; 14 diagnostics, not currently a CI gate |
| Release mock smoke | Presence, partials, and finals flowed without a crash |
| Whisper small/Metal ignored benchmark | 138 ms for 5 s audio; RTF 0.028; passes `< 0.8` gate |

There are no frontend unit-test or lint scripts yet.

Public CI evidence:

- macOS CI at `700f4c0` passed typecheck, `cargo check`, and `cargo test`.
- Windows CI at `700f4c0` failed `cargo test` because a platform-specific model test expects three active models even though Windows intentionally uses two.
- The manually dispatched Windows build at `c8d00d6` passed installer build, clang toolchain proof, faster-than-realtime STT gate, silent install, no-Discord idle smoke, mock pipeline, and uninstall.

## Directional CPU observations

These observations are useful for forming hypotheses but are not acceptance benchmarks:

- Release, Discord closed: the host process median was about 0.5% CPU, observed range 0–1.2%, about 937 MB RSS, and 21–22 threads after warm-up.
- The high idle RSS is consistent with loading both small and turbo models even without Discord audio.
- Mock captions active: the host process averaged about 1.9% CPU after warm-up and about 90 MB RSS.
- Host-only numbers undercount the Tauri application: WebKit GPU/WebContent processes are separate. Mock snapshots put host plus XPC processes around 2.7–4.7% CPU and roughly 250 MB RSS.
- The no-Discord path repeatedly emits/logs `WaitingForDiscordAudio` every two seconds; this is measurable idle churn but not yet proven to be a meaningful CPU cause.

Static profiling candidates, not proven causes:

- per-callback allocation/ownership churn in Core Audio;
- repeated copying of a growing utterance for partial jobs;
- `Vec::drain` frame rebuffering;
- duplicated one-second timers in two WebViews;
- a permanently visible transparent overlay/WebView.

## Correctness blockers before performance conclusions

1. Fix the Windows `models::tests::missing_lists_absent_models` expectation and keep a regression assertion for the platform-specific active set.
2. Redesign the bounded STT job path so a capture-owned thread never blocks and multiple queued Finals are never collapsed into one.
3. Add tests for consecutive Finals, full queues, partial coalescing, and slow-worker backpressure.
4. Align the six-second Windows partial tail with its true timestamp and a stable-prefix strategy.
5. Emit an explicit utterance-ended/clear-partial signal when a Final is filtered so stale partial text cannot stick.
6. Remove transcript text, names, IDs, and voiceprint details from diagnostic output before collecting or sharing logs.

Measuring CPU before these fixes risks optimizing a pipeline that loses data or stalls capture.

## Reproducible benchmark protocol

Use a Release build of one commit. Record exact hardware, OS build, Discord version, model hashes/backend, build toolchain, whisper thread count, display setup, and game settings. Warm up for 30 seconds, then run three 120-second samples per scenario.

### Scenarios

1. UNMUTE idle with Discord closed.
2. Discord open, user not in a call.
3. Connected call with silence.
4. Deterministic single-speaker English playback.
5. Deterministic single-speaker pt-BR playback.
6. Long utterance and overlapping speakers.
7. Overlay visible versus hidden with the same mock event stream.
8. Discord restart/reconnect and post-reconnect steady state.
9. Repeat the deterministic call while a representative borderless Windows game is running.

Use owned/licensed deterministic audio for the benchmark. Do not enable or distribute real-channel debug recordings without informed consent.

### Measurements

- whole application CPU, including WebView/GPU child processes or macOS coalition;
- RSS and model-load memory over time;
- energy impact where the OS exposes it;
- partial and final decode p50/p95;
- speech-end to caption-visible p50/p95;
- expected versus emitted Finals;
- job queue depth, coalesced partials, and capture blocks dropped;
- game average FPS and 1% lows;
- dGPU utilization on Windows.

On macOS, use Instruments Time Profiler/Energy Log and include the application coalition. `top -pid Unmute` alone is insufficient. On Windows, use a process-tree-aware profiler plus PresentMon or an equivalent frame-time capture.

### Provisional gates

- no lost Final and no capture-thread blocking under the stress fixture;
- macOS p95 speech-end to Final `< 1.2 s` on the documented target class;
- Windows p95 speech-end to Final `< 2.0 s`;
- Windows average whole-app CPU `< 15%` during continuous speech on the documented mid-range target;
- no UNMUTE workload on the game's dGPU;
- no material regression in game 1% lows versus the same call with UNMUTE closed.

Do not publish CPU or game-impact marketing claims until the target hardware, scenarios, and complete process tree have passed.

## Coverage gaps requiring live QA

- real Discord authorization, presence, reconnect, and self-exclusion;
- macOS Core Audio and Windows WASAPI capture;
- actual crosstalk attribution and pt-BR caption quality;
- transparent overlay click-through, move mode, hotkey conflicts, and persistence over a game;
- first-run model downloads and OS permission prompts;
- macOS 14.4 minimum target and representative Windows gaming hardware;
- signed application/installer behavior.
