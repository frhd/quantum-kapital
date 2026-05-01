//! Phase 1 / Step 4 — local-socket MCP listener.
//!
//! Owns the accept loop that hands each inbound connection to
//! [`rmcp::serve_server`]. Lifecycle follows the project's standard pattern:
//! [`McpServer::start`] returns a [`StreamHandle`] the runtime stores on
//! `IbkrState::mcp_handle` and stops on app shutdown.
//!
//! Cancellation strategy: the accept loop wraps each `listener.accept()` in
//! a short [`tokio::time::timeout`] and re-checks the shutdown flag between
//! attempts. This matches the existing polling-with-timeout pattern used by
//! the streaming subscriptions in `ibkr/client/streams.rs` and avoids relying
//! on the `interprocess` accept future being cancel-safe under
//! [`tokio::select!`].
//!
//! Per-connection tasks are spawned independently so a slow client can't
//! block other connections; on shutdown we stop accepting but let in-flight
//! sessions drain naturally (they exit when the client closes its side).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rmcp::serve_server;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::ibkr::client::StreamHandle;
use crate::mcp::handler::McpHandler;
use crate::mcp::transport;

/// Poll cadence for the accept loop. Bounds how long we wait between
/// shutdown-flag checks; small enough that app exit feels prompt, large
/// enough that idle CPU stays at zero.
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(500);

pub struct McpServer {
    handler: McpHandler,
    socket_path: PathBuf,
}

impl McpServer {
    pub fn new(handler: McpHandler, socket_path: PathBuf) -> Self {
        Self {
            handler,
            socket_path,
        }
    }

    /// Bind the listener and spawn the accept loop on a tokio task. The
    /// returned [`StreamHandle`] keeps the loop alive; dropping it without
    /// calling [`StreamHandle::stop`] just leaks the task (the OS reaps the
    /// socket on process exit).
    pub async fn start(self) -> std::io::Result<StreamHandle> {
        if let Some(parent) = self.socket_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let listener = transport::bind(&self.socket_path).await?;
        info!("MCP server listening on {}", self.socket_path.display());

        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_task = Arc::clone(&shutdown);
        let handler = self.handler;

        let join: JoinHandle<()> = tokio::spawn(async move {
            loop {
                if shutdown_task.load(Ordering::Relaxed) {
                    break;
                }
                match tokio::time::timeout(ACCEPT_POLL_INTERVAL, listener.accept()).await {
                    Err(_elapsed) => {
                        // Timeout — re-check shutdown flag and try again.
                        continue;
                    }
                    Ok(Err(e)) => {
                        warn!("MCP accept failed: {e}");
                        // Brief pause to avoid a hot spin if accept keeps
                        // failing immediately (e.g. transient resource
                        // exhaustion).
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    Ok(Ok(stream)) => {
                        let conn_handler = handler.clone();
                        tokio::spawn(async move {
                            match serve_server(conn_handler, stream.into_inner()).await {
                                Ok(running) => {
                                    if let Err(e) = running.waiting().await {
                                        warn!("MCP connection ended with error: {e}");
                                    }
                                }
                                Err(e) => {
                                    warn!("MCP serve_server failed during init: {e}");
                                }
                            }
                        });
                    }
                }
            }
            info!("MCP server stopped");
        });

        Ok(StreamHandle::new("mcp-server", shutdown, join))
    }
}
