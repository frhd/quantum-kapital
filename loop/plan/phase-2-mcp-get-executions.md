# Phase 2 — MCP `get_executions` tool

> Part of [Trade history visibility](master.md). See index for invariants.

**Status:** todo

**Depends on:** 1

**Goal:** A new read-only MCP tool, `mcp__quantum-kapital__get_executions`,
returns the day's per-leg fills with commissions and option metadata for
the connected IBKR account. From a Claude Code session the user can ask
"how did I do on TSLA today?" and the tool returns the per-leg structure
in a single call. Mirrors the shape of `get_positions` and
`get_account_summary`: `{account?, date?}` args, `{items, count}`
response, optional account, `resolve_account` ergonomics.

## Files

- New: `src-tauri/src/mcp/tools/executions.rs` — the tool definition.
  Mirrors `positions.rs` structure: `GetExecutionsArgs` derive block,
  `#[tool_router(router = executions_router, vis = "pub(crate)")]` impl
  on `McpHandler`, single `#[tool(name = "get_executions", description =
  "...")]` method body.
- Touches: `src-tauri/src/mcp/tools/mod.rs` — `pub(crate) mod
  executions;` line; surface the router through whatever the existing
  pattern is (mirror how `positions` is exposed).
- Touches: `src-tauri/src/mcp/handler.rs` — wire `executions_router`
  onto `McpHandler` in the existing `tool_router` chain. Order: place
  next to the other read-only tools (`positions`, `account_summary`,
  `quote`) so the catalogue stays grouped by concern.
- Touches: `src-tauri/src/mcp/ibkr_seam.rs` — extend the `AccountReader`
  trait with `async fn executions(&self, account: &str, date:
  chrono::NaiveDate) -> IbkrResult<Vec<ExecutionRow>>;`. Add the
  production impl on the `Arc<IbkrClient>` newtype (currently in the
  same file) — it forwards to `IbkrClient::executions(date)` and
  filters the resulting `Vec<IbkrExecution>` by `account` (since the
  IBKR API may return all managed-account fills in one drain, even
  when filtered server-side). Convert each `IbkrExecution` into the
  wire DTO `ExecutionRow`.
- Touches: `src-tauri/src/mcp/tools/test_support.rs` — add a fake
  `AccountReader` implementation (or extend the existing one) with a
  setter for canned executions: `with_executions(self, account: &str,
  date: NaiveDate, rows: Vec<ExecutionRow>) -> Self`. Mirror the
  existing position fixture setter shape.
- New: `src-tauri/src/mcp/tools/types.rs` (if it exists) or sibling
  `executions_types.rs` — the public `ExecutionRow` DTO used by both
  the seam and the tool. Fields:
  ```
  pub struct ExecutionRow {
      pub exec_id: String,
      pub time: chrono::DateTime<chrono::Utc>,
      pub account: String,
      pub symbol: String,
      pub contract_type: String,         // "STK" | "OPT" | ...
      pub expiry: Option<chrono::NaiveDate>,
      pub strike: Option<f64>,
      pub right: Option<String>,         // "C" | "P"
      pub multiplier: Option<String>,
      pub side: ExecutionSide,           // "bought" | "sold"
      pub qty: f64,
      pub avg_price: f64,
      pub commission: Option<f64>,
      pub realized_pnl: Option<f64>,
      pub currency: Option<String>,
      pub commission_currency: Option<String>,
      pub order_id: i32,
  }
  ```
  Derived: `Debug, Clone, Serialize, Deserialize, JsonSchema`.

## Tools exposed

| Tool | Wraps |
|---|---|
| `get_executions` | `AccountReader::executions` → `IbkrClient::executions(date)` → IBKR `reqExecutions(ExecutionFilter{ specific_dates: [yyyymmdd] })` |

## Reuse

- `resolve_account` helper from `mcp/tools/mod.rs` (used today by both
  `get_positions` and `get_account_summary`).
- `map_tool_result` helper from `mcp/tools/mod.rs`.
- `AccountReader` trait from `mcp/ibkr_seam.rs` — extended, not replaced.
- `tool_router` macro pattern from `mcp/tools/positions.rs`.

## Decisions to make in this phase

- **Default `date` value.** Today, in the **server's local trading
  zone** (ET). The default is computed at call time, not baked into
  the schema. Implementation: when `args.date` is `None`,
  `chrono::Utc::now().with_timezone(&America::New_York).date_naive()`.
- **Past-date behaviour.** Accept any past ISO date but document
  plainly that IBKR returns empty for non-today (until Phase 4 ships).
  Do NOT error on past-date — empty is a correct truthful response.
- **Future-date behaviour.** Accept; IBKR returns empty. Same as past.
- **Pagination.** None. Per-day fill counts for a single user are well
  under any reasonable cap.
- **No `symbol` arg, no `contract_type` arg in v1.** A day's fills are
  small; the agent filters client-side. Add later if usage demands.
- **`ExecutionRow` DTO module.** If `mcp/tools/types.rs` exists,
  put `ExecutionRow` there alongside `Position`. Otherwise put it in
  `executions.rs` and re-export from `tools/mod.rs`.

## Exit criteria

- `cargo test mcp::tools::executions` (or the matching test module
  layout) passes:
  1. **`get_executions_returns_canned_rows_in_chrono_order`** — the
     fake `AccountReader` returns three rows with mixed timestamps;
     the tool result lists them ascending.
  2. **`get_executions_errors_on_multi_account_without_arg`** — fake
     client reports two managed accounts; the tool returns an
     `IbkrError`-mapped MCP error whose message lists both account
     IDs (mirrors the `get_positions` test).
  3. **`get_executions_returns_empty_when_no_fills`** — fake yields
     zero rows; tool returns `{items: [], count: 0}`.
  4. **`get_executions_errors_when_ibkr_disconnected`** — fake yields
     `IbkrError::NotConnected`; tool maps to a clear MCP error with
     a recognisable message ("IBKR connection unavailable" or the
     same wording the sibling tools use).
  5. **`get_executions_passes_option_fields_through`** — fake row
     with `contract_type = "OPT"` and all option fields populated;
     the JSON tool response includes them; a stock row in the same
     batch has them omitted.
  6. **`get_executions_does_not_write_audit`** — call the tool and
     assert `mcp_audit` count is unchanged. (Reuse the audit-test
     helper from sibling read-tool tests.)
- Manual smoke (record in PR notes): from a Claude Code session,
  `mcp__quantum-kapital__get_executions` is callable and returns the
  expected per-leg shape for at least one real fill placed during
  the smoke window.
- Tool `description` argument explicitly says: "Returns the day's
  IBKR executions (fills) for `date` (defaults to today, ET trading
  day). Each row includes commission, realized P&L, and option
  contract metadata when applicable. NOTE: IBKR's reqExecutions
  endpoint only delivers fills for the current TWS-day; querying past
  dates returns an empty list. Errors if the IBKR connection is down.
  Returns `{ items: [ExecutionRow, ...], count: N }`."
- File-size cap: `executions.rs` ≤ 200 LOC. The seam method on
  `ibkr_seam.rs` keeps that file well under 300.
- Pre-commit clean.

## Gotchas

- **`AccountReader::executions` filter.** `IbkrClient::executions(date)`
  returns rows for **all** managed accounts in a single drain. The
  seam method must filter by `account` after fetching. Don't push the
  filter into the IBKR request — the existing API doesn't support a
  per-account server-side filter.
- **`#[tool_router]` ordering.** The handler's
  `#[tool_router]`-derived chain in `handler.rs` has a specific
  insertion shape. Mirror exactly the way `positions_router` is wired
  on `McpHandler` — one router-add line, no other changes to the
  composition.
- **`async_trait` boilerplate.** `AccountReader` uses `async_trait`;
  the new method needs the same decorator on both the trait
  definition and each impl block.
- **`IbkrError → McpError` mapping.** Use `map_tool_result` —
  bypassing it is inconsistent with `get_positions` and breaks the
  uniform error surface the agents rely on.
- **No `mcp_audit` row.** Confirm the audit policy in
  `services/mcp_audit/`: read-only tools must not write. The
  `does_not_write_audit` test locks this in.
- **`ExecutionRow` is the wire DTO, NOT `IbkrExecution`.** The
  conversion lives in the production seam impl. Don't expose
  `IbkrExecution` over the wire — we want a stable wire shape that
  doesn't track IBKR adapter refactors.
- **`JsonSchema` derive.** `chrono::NaiveDate` and `DateTime<Utc>`
  need the `chrono` feature on `schemars` (already enabled in this
  project; verify by grepping `Cargo.toml`).
- **`exec_id` uniqueness within a day.** IBKR's `execution_id` is
  globally unique, so the wire response will not have duplicates;
  no de-dup logic required in the tool body.
