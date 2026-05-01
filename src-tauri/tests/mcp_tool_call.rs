//! End-to-end Step 3 integration test: spawn the `mcp-server` bridge as a
//! subprocess, hand-roll a stdio MCP client against it, and verify a
//! `tools/call get_llm_budget_status` round-trips through bridge → unix
//! socket → in-process rmcp server → handler → seeded `LlmService`.
//!
//! The bridge binary is built automatically by Cargo when this integration
//! test is run because the test references `env!("CARGO_BIN_EXE_mcp-server")`.

use std::process::Stdio;
use std::time::Duration;

use quantum_kapital_lib::mcp;
use rmcp::serve_server;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time::timeout;

/// Drive an MCP `initialize` + `tools/call get_llm_budget_status` exchange
/// through the spawned bridge and assert the returned spend numbers match
/// the seed.
#[tokio::test(flavor = "multi_thread")]
async fn tools_call_get_llm_budget_status_through_bridge() {
    // 1) Bind the MCP listener at a temp path. Must be BEFORE the bridge is
    //    spawned so the bridge's first connect attempt usually succeeds
    //    immediately (its retry loop exists for the real Tauri lifecycle,
    //    not this happy-path test).
    let dir = tempfile::TempDir::new().expect("tempdir");
    let socket_path = dir.path().join("mcp.sock");
    let db_path = dir.path().join("test.sqlite");

    let listener = mcp::transport::bind(&socket_path)
        .await
        .expect("bind listener");

    // 2) Pre-seed an McpHandler with $0.75 spent against a $2.00 budget.
    let handler = mcp::handler::test_handler_with_seeded_spend(&db_path, 0.75, 2.00)
        .await
        .expect("seed handler");

    // 3) Accept ONE connection (the bridge), then hand it to rmcp's server
    //    loop. rmcp owns the stream until the client disconnects, so we run
    //    the whole serve future in a background task.
    let server = tokio::spawn(async move {
        let stream = listener.accept().await.expect("accept bridge");
        // serve_server consumes any S: AsyncRead + AsyncWrite + Send + 'static.
        let running = serve_server(handler, stream)
            .await
            .expect("serve_server initialize");
        // Wait for the client (bridge) to disconnect.
        let _ = running.waiting().await;
    });

    // 4) Spawn the bridge subprocess. CARGO_BIN_EXE_<name> is set by Cargo
    //    when running an integration test in the same package as the
    //    binary, and points at the freshly-built executable.
    let bin = env!("CARGO_BIN_EXE_mcp-server");
    let mut child = Command::new(bin)
        .arg(&socket_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn mcp-server bridge");

    let mut stdin = child.stdin.take().expect("bridge stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("bridge stdout"));

    // 5) MCP initialize handshake.
    let initialize = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": { "name": "qk-integration-test", "version": "0.0.1" }
        }
    });
    write_line(&mut stdin, &initialize).await;
    let init_resp = read_line(&mut stdout).await;
    assert_eq!(init_resp["id"], 1, "initialize response: {init_resp}");
    assert!(
        init_resp.get("result").is_some(),
        "initialize had no result: {init_resp}"
    );

    // Spec requires the client to send `notifications/initialized` after a
    // successful initialize before issuing any other request.
    let initialized = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized",
    });
    write_line(&mut stdin, &initialized).await;

    // 6) tools/call get_llm_budget_status.
    let tool_call = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "get_llm_budget_status",
            "arguments": {}
        }
    });
    write_line(&mut stdin, &tool_call).await;
    let tool_resp = read_line(&mut stdout).await;
    assert_eq!(tool_resp["id"], 2, "tool call response: {tool_resp}");
    let result = tool_resp
        .get("result")
        .unwrap_or_else(|| panic!("tools/call had no result: {tool_resp}"));
    let body = result
        .get("structuredContent")
        .unwrap_or_else(|| panic!("tools/call result missing structuredContent: {result}"));

    let spent = body["spent_usd"].as_f64().expect("spent_usd is f64");
    let budget = body["budget_usd"].as_f64().expect("budget_usd is f64");
    let remaining = body["remaining_usd"]
        .as_f64()
        .expect("remaining_usd is f64");
    assert!((spent - 0.75).abs() < 1e-9, "spent_usd = {spent}");
    assert!((budget - 2.00).abs() < 1e-9, "budget_usd = {budget}");
    assert!(
        (remaining - 1.25).abs() < 1e-9,
        "remaining_usd = {remaining}"
    );

    // 7) Clean shutdown: closing stdin lets the bridge's copy_bidirectional
    //    drain and exit naturally.
    drop(stdin);
    let _ = timeout(Duration::from_secs(5), child.wait()).await;
    let _ = child.start_kill();
    server.abort();
    let _ = server.await;
}

async fn write_line<W: AsyncWriteExt + Unpin>(w: &mut W, msg: &Value) {
    let mut bytes = serde_json::to_vec(msg).expect("serialize");
    bytes.push(b'\n');
    w.write_all(&bytes).await.expect("write to bridge stdin");
    w.flush().await.expect("flush bridge stdin");
}

async fn read_line<R: AsyncBufReadExt + Unpin>(r: &mut R) -> Value {
    let mut line = String::new();
    let n = timeout(Duration::from_secs(5), r.read_line(&mut line))
        .await
        .expect("read_line did not time out")
        .expect("read_line io");
    assert!(
        n > 0,
        "EOF on bridge stdout while expecting a JSON-RPC line"
    );
    serde_json::from_str(line.trim_end()).unwrap_or_else(|e| panic!("parse {line:?}: {e}"))
}
