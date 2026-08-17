//! Structural diagnostics for field troubleshooting: capture health, decode
//! timings, utterance boundaries. Written to stderr AND a log file in the app
//! data dir, so "send me the file" replaces terminal gymnastics.
//! PRIVACY: never log transcript text, names, or ids here — numbers only.

use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static FILE: OnceLock<Mutex<std::fs::File>> = OnceLock::new();
static T0: OnceLock<Instant> = OnceLock::new();

/// Call once at startup. Truncates the previous run's log.
pub fn init(data_dir: &std::path::Path) {
    let _ = T0.set(Instant::now());
    let path = data_dir.join("unmute-diag.log");
    if let Ok(f) = std::fs::File::create(&path) {
        let _ = FILE.set(Mutex::new(f));
        log(&format!("diag start · v{}", env!("CARGO_PKG_VERSION")));
    }
}

pub fn log(line: &str) {
    let t = T0.get().map(|t| t.elapsed().as_secs_f32()).unwrap_or(0.0);
    let stamped = format!("[{t:8.1}s] {line}");
    eprintln!("[diag] {stamped}");
    if let Some(f) = FILE.get() {
        if let Ok(mut f) = f.lock() {
            let _ = writeln!(f, "{stamped}");
            let _ = f.flush();
        }
    }
}
