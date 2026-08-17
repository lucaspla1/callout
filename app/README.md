# UNMUTE application

This directory contains the Tauri v2 application: a React/TypeScript settings and overlay UI plus the Rust Discord RPC, per-process capture, STT, attribution, persistence, and window lifecycle.

Start with the repository-level [`AGENTS.md`](../AGENTS.md), [`docs/PROJECT_STATE.md`](../docs/PROJECT_STATE.md), and the relevant [`docs/dev/`](../docs/dev/) guide.

## Headless verification

```bash
npm ci
npx tsc --noEmit
npm run build

cd src-tauri
cargo check --locked
cargo test --locked
```

Do not run `npm run tauri dev` or `cargo run` as a routine check: the GUI opens desktop windows, registers global shortcuts, and can start Discord/audio flows. Launch it only for an intentional interactive test.

## IDE setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
