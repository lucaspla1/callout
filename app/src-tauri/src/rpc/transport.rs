//! Finding and connecting to the local Discord client's IPC endpoint.
//! See docs/dev/discord-rpc.md §1.1: sockets are named discord-ipc-{0..9}; each
//! running flavor (Stable/PTB/Canary) takes the first free index.

#[cfg(unix)]
pub type Conn = tokio::net::UnixStream;
#[cfg(windows)]
pub type Conn = tokio::net::windows::named_pipe::NamedPipeClient;

pub async fn connect_any() -> Option<Conn> {
    for n in 0..10 {
        for path in candidate_paths(n) {
            if let Some(conn) = try_connect(&path).await {
                return Some(conn);
            }
        }
    }
    None
}

#[cfg(unix)]
async fn try_connect(path: &str) -> Option<Conn> {
    tokio::net::UnixStream::connect(path).await.ok()
}

#[cfg(windows)]
async fn try_connect(path: &str) -> Option<Conn> {
    // TODO(M4): handle ERROR_PIPE_BUSY with a short retry.
    tokio::net::windows::named_pipe::ClientOptions::new().open(path).ok()
}

#[cfg(windows)]
fn candidate_paths(n: u32) -> Vec<String> {
    vec![format!(r"\\.\pipe\discord-ipc-{n}")]
}

#[cfg(unix)]
fn candidate_paths(n: u32) -> Vec<String> {
    let bases: Vec<String> = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .filter(|v| !v.is_empty())
        .chain(std::iter::once("/tmp".to_string()))
        .collect();
    let mut out = Vec::new();
    for base in bases {
        let base = base.trim_end_matches('/');
        out.push(format!("{base}/discord-ipc-{n}"));
        // Sandboxed Discord on Linux.
        out.push(format!("{base}/snap.discord/discord-ipc-{n}"));
        out.push(format!("{base}/app/com.discordapp.Discord/discord-ipc-{n}"));
    }
    out
}
