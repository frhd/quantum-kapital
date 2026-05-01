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

    // Pin the count: catches accidental tool-router drops. 11 read tools +
    // 5 write tools added in Phase 02 (add_ticker, archive_ticker,
    // write_research_note, write_morning_pack, ack_alert).
    assert_eq!(
        names.len(),
        16,
        "expected 16 registered MCP tools, got {}: {:?}",
        names.len(),
        names
    );

    // Phase-02 invariant: the only "write" verbs allowed are the closed
    // set below. A future write tool whose name starts with `write_` /
    // `add_` / `archive_` / `ack_` *must* be added here AND reviewed for
    // surveillance-only compliance — never an order-placement primitive.
    let allowed_writes: &[&str] = &[
        "add_ticker",
        "archive_ticker",
        "write_research_note",
        "write_morning_pack",
        "ack_alert",
    ];
    for name in &names {
        let n = name.to_ascii_lowercase();
        let looks_like_write = n.starts_with("add_")
            || n.starts_with("archive_")
            || n.starts_with("write_")
            || n.starts_with("ack_");
        if looks_like_write {
            assert!(
                allowed_writes.contains(&name.as_str()),
                "WRITE TOOL '{name}' not in the Phase-02 allowed-writes list. \
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
