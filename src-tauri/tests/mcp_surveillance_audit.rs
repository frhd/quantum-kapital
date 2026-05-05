//! Surveillance-only audit guard for the MCP tool registry.
//!
//! Quantum Kapital's MCP surface is surveillance-only by project rule
//! (`CLAUDE.md`): the tracker pipeline must never call order-placement code
//! paths, and the MCP server must never expose order-placement tools to an
//! external LLM. This integration test enumerates every tool registered on
//! `McpHandler` and refuses to ship if any tool name looks like an
//! order-placement primitive — a build-time guard against future regressions
//! per cross-phase verification §5 of the Phase 1 roadmap.
//!
//! Lives under `tests/` (Cargo integration test) so it runs in plain
//! `cargo test`, shows up as its own binary in CI, and exercises the
//! cross-crate `pub(crate)`-equivalent surface (`tool_names` is `pub` on
//! `McpHandler`, reachable through the library crate).

use quantum_kapital_lib::mcp;

/// Predicates that flag an MCP tool name as an order-placement primitive.
///
/// Lower-cased, exact-prefix and substring checks. The substring guards
/// (`"order"`, `"trade"`, `"execute"`) are deliberately broad — read-only
/// surveillance tools in this codebase use snake_case verbs + nouns and
/// none legitimately contain those tokens. False positives are caught by
/// code review; a future read-only `get_order_book` would need explicit
/// reviewer approval (and a relaxation of this predicate).
///
/// Note on `starts_with("place_")` vs `starts_with("place")`: the
/// underscore-anchored form catches the canonical `place_order` /
/// `place_bracket_order` shape without false-positiving on innocent
/// words like `placeholder` or `placement_score`. The accompanying
/// substring guard `contains("order")` fully covers the
/// no-trailing-underscore degenerate case (a hypothetical `place`
/// tool that actually places orders would still trip on the rest of
/// the predicate set anyway, since the only sensible name for such a
/// tool would also include "order" / "trade" / "execute"). Same
/// reasoning for `cancel_` / `modify_` / `submit_`.
fn is_blocked(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    // Read-only tools whose names happen to share a substring with the
    // order-placement vocabulary. `get_executions` returns past fills —
    // a read, not a write — but the noun "executions" trips the
    // `contains("execute")` guard. Keep the broad guard for everything
    // else; allowlist by exact name here.
    if matches!(
        n.as_str(),
        "get_executions" | "get_trade_legs" | "get_trade_review" | "write_trade_review"
    ) {
        return false;
    }
    n.starts_with("place_")
        || n.starts_with("cancel_")
        || n.starts_with("modify_")
        || n.starts_with("submit_")
        || n.contains("order")
        || n.contains("execute")
        || n.contains("trade")
}

/// Sanity-check the audit predicates themselves so they don't bit-rot.
/// Plain `#[test]` (no async) — pure logic.
#[test]
fn audit_predicates_block_known_order_names() {
    for bad in [
        "place_order",
        "cancel_order",
        "modify_order",
        "submit_order",
        "execute_trade",
        "BUY_ORDER",
        "place_bracket_order",
        "trade_now",
        "execute_strategy",
    ] {
        assert!(is_blocked(bad), "{bad} should have been blocked");
    }
    for good in [
        "get_quote",
        "get_positions",
        "get_account_summary",
        "get_executions",
        "get_trade_legs",
        "get_trade_review",
        "write_trade_review",
        "run_scanner",
        "list_watchlist",
        "get_llm_budget_status",
        "get_setups",
        "get_alerts",
        "get_news",
        "get_bars",
        "get_fundamentals",
        "get_watchlist",
    ] {
        assert!(!is_blocked(good), "{good} should have been allowed");
    }
}

/// Enumerate every tool registered on `McpHandler` and refuse to ship if
/// any name looks like an order-placement primitive. Also pins the count
/// so an accidental tool drop fails the build too.
#[tokio::test(flavor = "multi_thread")]
async fn mcp_tool_registry_is_surveillance_only() {
    // Use the same cross-crate-visible constructor `tests/mcp_tool_call.rs`
    // uses. It seeds an LLM-spend row but the audit only needs the
    // composed `ToolRouter`; the seeded state is harmless.
    let dir = tempfile::TempDir::new().expect("tempdir");
    let db_path = dir.path().join("audit.sqlite");
    let handler = mcp::handler::test_handler_with_seeded_spend(&db_path, 0.0, 1.0)
        .await
        .expect("seed handler");

    let names = handler.tool_names();

    // Pin the count: catches accidental tool-router drops. Phase 1 (11
    // reads) + Phase 02 (5 writes: add_ticker, archive_ticker,
    // write_research_note, write_morning_pack, ack_alert) + Phase 3 (1
    // read: get_sentiment) + Phase 4 (1 read: get_candidates, 1 write:
    // promote_candidate) + Phase 6 (1 write: mark_alert_enriched) +
    // Phase 7 (2 reads: get_morning_pack, get_outcomes; 1 write:
    // append_journal_entry) + Phase 8 (3 reads: get_calibration_stats,
    // get_prediction_history, get_cost_attribution) + AV strip-out
    // Phase 4 (1 write: set_fundamentals — operator-curated reference
    // data, never market actions) + Trade-history Phase 2 (2 reads:
    // get_executions, get_trade_legs) + Behavioral-assessment Phase 3
    // (1 read: get_watchlist_briefing) = 30.
    assert_eq!(
        names.len(),
        30,
        "expected 30 registered MCP tools, got {}: {:?}",
        names.len(),
        names
    );

    // The only "write" verbs allowed are the closed set below. A future
    // write tool whose name starts with `write_` / `add_` / `archive_` /
    // `ack_` / `promote_` / `mark_` / `append_` / `set_` *must* be
    // added here AND reviewed for surveillance-only compliance — never
    // an order-placement primitive.
    let allowed_writes: &[&str] = &[
        "add_ticker",
        "archive_ticker",
        "write_research_note",
        "write_morning_pack",
        "ack_alert",
        "promote_candidate",
        "mark_alert_enriched",
        "append_journal_entry",
        "set_fundamentals",
    ];
    for name in &names {
        let n = name.to_ascii_lowercase();
        let looks_like_write = n.starts_with("add_")
            || n.starts_with("archive_")
            || n.starts_with("write_")
            || n.starts_with("ack_")
            || n.starts_with("promote_")
            || n.starts_with("mark_")
            || n.starts_with("append_")
            || n.starts_with("set_");
        if looks_like_write {
            assert!(
                allowed_writes.contains(&name.as_str()),
                "WRITE TOOL '{name}' not in the allowed-writes list. \
                 Add it explicitly here and confirm it does NOT mutate orders \
                 or live positions; see CLAUDE.md."
            );
        }
    }

    for name in &names {
        assert!(
            !is_blocked(name),
            "SURVEILLANCE-ONLY VIOLATION: MCP tool '{name}' looks like an order-placement primitive.\n\
             Quantum Kapital's MCP surface is surveillance-only — adding order tools requires\n\
             explicit project-level approval and removing this guard. See CLAUDE.md."
        );
    }
}

/// Per master-plan invariant #3 (Trade-history visibility roadmap): the
/// `get_executions` tool file must never reach into the order-placement
/// surface. A textual gate is sufficient — it catches the obvious slip
/// (an `OrderRequest` import, a `place_order` call, anything from
/// `ibkr/commands/trading.rs`) at build time before the binary ships.
#[test]
fn executions_tool_does_not_import_order_placement() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/mcp/tools/executions.rs");
    let src =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    for needle in [
        "OrderRequest",
        "place_order",
        "ibkr::commands::trading",
        "ibkr/commands/trading",
    ] {
        assert!(
            !src.contains(needle),
            "SURVEILLANCE-ONLY VIOLATION: `{}` references `{needle}`. \
             `get_executions` is read-only by master-plan invariant #3.",
            path.display()
        );
    }
}

/// Belt-and-braces ripgrep gate: no source file under `mcp/tools/` is
/// allowed to call `place_order` directly. Catches a future tool file
/// that bypasses `executions.rs` entirely.
#[test]
fn no_mcp_tool_calls_place_order() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/mcp/tools");
    for entry in
        std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
    {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        assert!(
            !src.contains("place_order"),
            "SURVEILLANCE-ONLY VIOLATION: {} calls `place_order`. \
             MCP tools must not place orders.",
            path.display()
        );
    }
}
