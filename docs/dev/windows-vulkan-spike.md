# Windows Vulkan A/B

This spike tests one hypothesis only: moving whisper.cpp inference from CPU to
Vulkan can reduce Unmute's speech-time CPU load without hurting caption quality
or game frametimes. It is not a release-default decision.

## Builds

- **A / CPU:** `0.1.2`, commit `4cb22e2`.
- **B / Vulkan:** `0.1.3`, artifact commit `01e9d4b`, built with Cargo feature
  `windows-vulkan`.

The artifact commit temporarily substituted `windows-build.yml` because GitHub
can dispatch only a workflow path already present on the default branch. The
following branch commit restores the production workflow; never merge or
cherry-pick the disposable workflow definition from `01e9d4b`.

Both builds use the same Small, Turbo, and speaker models, adaptive selection,
thread limit, partial cadence, and model directory. Do not bundle models or
change decoding policy during this A/B.

The B build records `active=vulkan` plus adapter metadata in
`%APPDATA%\app.callout.desktop\unmute-diag.log`. An `active=cpu` line means the
runtime fallback worked, but the GPU A/B is invalid. Set `UNMUTE_FORCE_CPU=1`
only when explicitly testing that fallback.

## Test order

Keep the Windows version, AMD driver, Discord call, language setting, game
resolution/FPS cap, power plan, and model files fixed. Warm each build with one
discarded phrase, then measure repeated 2–3 minute samples in `A-B-B-A-B-A`
order:

1. Discord call, silence.
2. Discord call, identical Portuguese speech corpus.
3. Reproducible game scene, silence.
4. Same scene and corpus with capped FPS, then with GPU utilization near 95%.
5. Twenty-minute game/call/speech soak for the winner.

Capture Unmute CPU/private memory, total CPU, GPU/VRAM, FPS, 1% low, p99
frametime, and the diagnostic log. The corpus must include `foi mal`, negation,
numbers, directions, continuous 5–6 second speech, and 30 seconds of silence.

## Promotion gates

- Speech CPU median B <= 70% of A; silent-call regression <= 1 percentage point.
- Final queue p95 <= 250 ms and queue + decode p95 <= 2 seconds.
- WER no more than 0.02 absolute worse than A; no regression in negation,
  numbers, or directions; no duplicated or silence-generated captions.
- Average game FPS >= 98% of A, 1% low >= 95% of A, and p99 frametime no more
  than 5% or 1 ms worse.
- Private memory <= 1.5 GB, extra VRAM <= 1.25 GB, and total VRAM < 90%.
- No crash, driver reset, device-lost error, stuck caption, or memory growth over
  100 MB during the soak.

Passing only with capped FPS is not enough for a default. In that case Vulkan
may remain opt-in or activate only when GPU headroom is available.

## Distribution decision

Keep the small online installer during private alpha. Bundling roughly 790 MB
of models changes download/update behavior but not quality or CPU, so evaluate
an offline model pack only after the backend and model set are stable.
