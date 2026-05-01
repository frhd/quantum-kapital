//! Cross-platform local-socket transport for the MCP server.
//!
//! Wraps the `interprocess` crate's tokio local-socket API so the rest of the
//! MCP layer can deal in `&Path`s and forget about the Unix-domain-socket vs.
//! Windows-named-pipe distinction. The accepted stream impls
//! [`tokio::io::AsyncRead`] + [`tokio::io::AsyncWrite`] which is exactly what
//! `rmcp::serve_server` consumes.
//!
//! Caller responsibilities:
//! - On Unix the path is a real filesystem path. If a stale socket file is
//!   left behind by a crashed previous process [`bind`] removes it before
//!   binding; this mirrors what `interprocess`'s
//!   [`ListenerOptions::try_overwrite`] does but keeps the policy explicit
//!   and consistent across platforms.
//! - On Windows the file-system "path" is reinterpreted as a named-pipe
//!   namespace name (e.g. `mcp.sock` becomes `\\.\pipe\mcp.sock`). The bridge
//!   binary always passes a `&Path`; we do the conversion here so callers
//!   stay platform-agnostic.

use std::io;
use std::path::Path;

use interprocess::local_socket::tokio::{prelude::*, Listener, Stream};
use interprocess::local_socket::{GenericFilePath, GenericNamespaced, ListenerOptions, Name};

/// Server-side listener for the MCP local socket.
pub struct McpListener {
    inner: Listener,
}

impl McpListener {
    /// Accept the next inbound MCP connection.
    pub async fn accept(&self) -> io::Result<McpStream> {
        let stream = self.inner.accept().await?;
        Ok(McpStream { inner: stream })
    }
}

/// Bidirectional MCP byte stream, AsyncRead + AsyncWrite + Unpin + Send.
///
/// `rmcp::serve_server` accepts any `S: AsyncRead + AsyncWrite + Send + 'static`
/// via the crate's combined-RW IntoTransport adapter, so this type slots in
/// directly.
pub struct McpStream {
    inner: Stream,
}

impl McpStream {
    /// Consume the wrapper and yield the underlying interprocess stream so
    /// callers can hand it to e.g. `rmcp::serve_server`.
    pub fn into_inner(self) -> Stream {
        self.inner
    }
}

// Delegate AsyncRead / AsyncWrite to the inner Stream so the wrapper is a
// drop-in for any tokio-aware consumer (including the bridge binary's
// `copy_bidirectional`).
impl tokio::io::AsyncRead for McpStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for McpStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<io::Result<usize>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

/// Bind the server end of the local socket to `path`.
///
/// On Unix this is a filesystem path; any stale socket file left by a crashed
/// prior process is removed before bind so the listener doesn't fail with
/// `AddrInUse`. On Windows the path's file-name component is used as the
/// namespaced name (giving `\\.\pipe\<file-name>`).
pub async fn bind(path: &Path) -> io::Result<McpListener> {
    let name = make_name(path)?;
    // Best-effort cleanup of a stale Unix socket file. We do this only for
    // path-style names (fs paths) — namespaced names on Windows do not have a
    // filesystem entry, and even on Linux abstract namespace names wouldn't
    // either. Errors other than NotFound are intentionally ignored: if the
    // file exists but we lack permission to unlink it, the bind below will
    // fail with a clear diagnostic.
    #[cfg(unix)]
    if path.exists() {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    let inner = ListenerOptions::new().name(name).create_tokio()?;
    Ok(McpListener { inner })
}

/// Connect to a local socket previously bound by [`bind`].
pub async fn connect(path: &Path) -> io::Result<McpStream> {
    let name = make_name(path)?;
    let inner = Stream::connect(name).await?;
    Ok(McpStream { inner })
}

/// Convert a `&Path` to an `interprocess` `Name`. On Unix we use the path as
/// a filesystem name; on Windows we use the file-name component as the
/// namespaced (named-pipe) name.
fn make_name(path: &Path) -> io::Result<Name<'_>> {
    #[cfg(unix)]
    {
        path.to_fs_name::<GenericFilePath>()
    }
    #[cfg(windows)]
    {
        let file = path.file_name().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "MCP socket path has no file-name component: {}",
                    path.display()
                ),
            )
        })?;
        file.to_ns_name::<GenericNamespaced>()
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (GenericFilePath, GenericNamespaced);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "local socket transport not supported on this platform",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A bind/connect pair must round-trip a small payload, in both
    /// directions, on the platform's local-socket flavour.
    #[tokio::test]
    async fn bind_connect_roundtrip_small_payload() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("mcp-transport-test.sock");

        let listener = bind(&path).await.expect("bind");

        let server = tokio::spawn({
            async move {
                let mut s = listener.accept().await.expect("accept");
                let mut buf = [0u8; 5];
                s.read_exact(&mut buf).await.expect("read from client");
                assert_eq!(&buf, b"ping\n");
                s.write_all(b"pong\n").await.expect("write to client");
                s.flush().await.expect("flush");
                // Hold the stream alive long enough for the client to read.
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        });

        let mut client = connect(&path).await.expect("connect");
        client.write_all(b"ping\n").await.expect("write to server");
        client.flush().await.expect("flush client");
        let mut buf = [0u8; 5];
        client.read_exact(&mut buf).await.expect("read reply");
        assert_eq!(&buf, b"pong\n");

        server.await.expect("server task completed");
    }

    /// Re-binding the same path after a stale socket file exists must
    /// succeed (the helper removes the corpse on Unix).
    #[tokio::test]
    #[cfg(unix)]
    async fn bind_clears_stale_socket_file() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("stale.sock");
        // Simulate a stale file at the target path (e.g. a crashed prior
        // process that didn't remove its socket on exit).
        std::fs::write(&path, b"junk").expect("seed stale file");
        assert!(path.exists());

        let _listener = bind(&path).await.expect("bind succeeds despite stale file");
        // Successful bind implies the stale file was unlinked + replaced
        // with a fresh socket.
    }
}
