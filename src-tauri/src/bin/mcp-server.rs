//! `mcp-server` — stdio↔local-socket bridge for the Quantum Kapital MCP.
//!
//! Claude Code (and any other stdio-speaking MCP client) talks to this
//! binary; this binary just shovels bytes between its own stdin/stdout and a
//! local socket bound by the running Tauri app. All real protocol work
//! happens inside Tauri (see `quantum_kapital_lib::mcp`).
//!
//! Stdout MUST stay reserved for the JSON-RPC stream — diagnostics go to
//! stderr only.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use quantum_kapital_lib::mcp::transport;
use tokio::io::{stdin, stdout};
use tokio::time::{sleep, Instant};

/// Tauri identifier this app ships with — keep in sync with
/// `src-tauri/tauri.conf.json::identifier`. Used to derive the default
/// socket path so Claude Code Just Works against a default-installed app.
const APP_IDENTIFIER: &str = "com.quantyc.qqk";
const SOCKET_FILE_NAME: &str = "mcp.sock";
const ENV_SOCKET_PATH: &str = "QK_MCP_SOCKET";

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let path = resolve_socket_path();

    let mut sock = match connect_with_retry(&path, Duration::from_secs(3)).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "mcp-server: failed to connect to {}: {e}. \
                 Start the Quantum Kapital app and try again.",
                path.display()
            );
            return ExitCode::from(1);
        }
    };

    // Combine stdin (read-only) and stdout (write-only) into a single duplex
    // value so `copy_bidirectional` can shuttle in both directions.
    let mut io = tokio::io::join(stdin(), stdout());
    if let Err(e) = tokio::io::copy_bidirectional(&mut io, &mut sock).await {
        eprintln!("mcp-server: bridge terminated: {e}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn resolve_socket_path() -> PathBuf {
    // Precedence: CLI argv[1] > env var > OS app-data default.
    if let Some(arg) = std::env::args().nth(1) {
        return PathBuf::from(arg);
    }
    if let Ok(v) = std::env::var(ENV_SOCKET_PATH) {
        if !v.is_empty() {
            return PathBuf::from(v);
        }
    }
    let base = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir);
    base.join(APP_IDENTIFIER).join(SOCKET_FILE_NAME)
}

/// Retry connect with exponential backoff (50ms → 100ms → 200ms, capped at
/// 250ms) until either we connect or `total` elapses. The Tauri app may
/// still be initialising when Claude Code spawns the bridge.
async fn connect_with_retry(path: &Path, total: Duration) -> std::io::Result<transport::McpStream> {
    let deadline = Instant::now() + total;
    let mut delay = Duration::from_millis(50);
    let cap = Duration::from_millis(250);
    loop {
        let last_err = match transport::connect(path).await {
            Ok(s) => return Ok(s),
            Err(e) => e,
        };
        let now = Instant::now();
        if now >= deadline {
            return Err(last_err);
        }
        let until_deadline = deadline.saturating_duration_since(now);
        sleep(delay.min(until_deadline)).await;
        delay = (delay * 2).min(cap);
    }
}
