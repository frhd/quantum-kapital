# Trade history visibility → per-leg fills, commissions, and a Today's Trades panel: ~1.5 weeks

## Context

The IBKR adapter already has `IbkrClient::executions(date)` at
`src-tauri/src/ibkr/client/orders.rs:66`, wrapping the `ibapi` crate's
`client.executions(ExecutionFilter)`. The data is drained but **half of it
is silently dropped**: the `IBExecutions::CommissionReport(_) => {}` arm at
`orders.rs:117` discards every commission/realized-P&L event, and the
`IbkrExecution` row carries no contract-type, no option fields
(`expiry`/`strike`/`right`/`multiplier`), and no `account` — just symbol,
side, qty, avg_price, time, order_id, exec_id.

The result is observable end-to-end. When the user asked today "I traded
TSLA options — how did I do?", the only reachable surface was
`get_positions.realized_pnl` aggregated by closed legs (the three TSLA
0DTE call rows that are now `position=0`). That works for closed-out
contracts but tells us nothing per-fill: no commissions, no entry vs exit,
no time, no per-leg structure of the spread the user actually traded.

There is also no MCP tool, no Tauri command, and no UI surface for fills.
The data exists in the backend; it just terminates inside the IBKR
adapter and never propagates outward.

**Inversion.** Today executions are an opaque method on `IbkrClient` whose
output is incomplete and unused. End state: per-leg fills with fully
populated commissions, realized P&L, and option-contract metadata are a
read-only MCP tool **and** a "Today's Trades" tab in the desktop app, both
served by the same `AccountReader` seam used by `get_positions` and
`get_account_summary`. Persistence is intentionally deferred — the
in-memory same-day model from IBKR's `reqExecutions` covers the
intraday/EOD use case the user is asking for; a Phase 4 storage layer is
scoped but only triggered when multi-day visibility becomes a real ask.

## End-state architecture

| Subsystem | Responsibility |
|---|---|
| **`IbkrClient::executions`** (existing, extended in Phase 1) | Drains `ExecutionData` and `CommissionReport` events; merges by `execution_id` within a single subscription drain; populates option contract fields from `data.contract`. Returns `Vec<IbkrExecution>` with `commission`, `realized_pnl`, contract type, and option metadata. |
| **`IbkrExecution`** (existing, extended in Phase 1) | Per-leg row. Gains `account`, `contract_type`, `expiry`, `strike`, `right`, `multiplier`, `commission`, `realized_pnl`, `currency`. All commission/option fields are `Option<...>` — `None` means "not (yet) reported by IBKR," not "zero." |
| **`AccountReader::executions`** (new method on existing `mcp::ibkr_seam` trait, Phase 2) | Production seam that MCP tools call. Forwards to `IbkrClient::executions`, filters by `account`. The fake impl in `mcp::tools::test_support` is what unit tests use. |
| **`get_executions` MCP tool** (new, Phase 2) | Read-only. Args `{account?, date?}`. Returns `{items: [ExecutionRow], count}`. Mirrors `get_positions` shape and the `resolve_account` ergonomics. No `mcp_audit` row (read-only, consistent with sibling read tools). |
| **`get_executions_for_date` Tauri command** (new, Phase 3) | Frontend's path to the same `AccountReader::executions`. Same DTO shape as the MCP tool. |
| **Trades tab** (new, Phase 3) | New `src/features/trades/` feature folder. Date picker (defaults to today), summary banner (gross P&L, fees, net P&L, fill count), symbol-grouped collapsible list with per-leg detail. |
| **(Phase 4, deferred) Executions store** | SQLite table keyed by `exec_id`; idempotent UPSERT; commission late-arrivals patch existing rows. Lifts the IBKR same-day-only constraint. Forward-only — no historical backfill. |

## Hard invariants

1. **Surveillance-only stays.** `get_executions` is read-only. No phase
   may add an order-placement code path. The MCP tool surface stays
   "read-only + `ack_alert`" per the project-level rule in `CLAUDE.md`.
2. **MCP tools go through `AccountReader`** (the production seam in
   `src-tauri/src/mcp/ibkr_seam.rs`), not the test-only `IbkrClientTrait`.
   Phase 2 extends `AccountReader` with `executions(account, date)` and
   adds both the production impl (forwards to `IbkrClient`) and the test
   fake.
3. **No new LLM call sites.** This program is data plumbing. No phase
   may add a `LlmService` consumer. (Tempting future direction: "ask
   Claude to summarize the day." That is a separate program with a
   separate budget review and is explicitly out of scope here.)
4. **Commissions and realized P&L are pass-through, not derived.**
   Whatever IBKR's `CommissionReport` carries we record verbatim; we
   never compute a synthetic commission or recover a missing one.
   `Option<f64>` represents truth: `None` ↔ "not reported."
5. **Time-zone discipline.** Tool/command args take ET trading-day ISO
   dates (`YYYY-MM-DD`). Backend returns timestamps as UTC ISO 8601.
   Presentation TZ is the FE's job.
6. **Trait seams unchanged for tests.** No test ever spawns a real IBKR
   socket. `MockIbkrClient` (existing, `src-tauri/src/ibkr/mocks.rs`)
   covers the `IbkrClient::executions` drain logic. The `AccountReader`
   fake in `mcp::tools::test_support` covers the MCP layer.
7. **Phases 1–3 are stateless w.r.t. fills.** Each call to `executions`
   re-fetches from the live IBKR subscription. No memoisation, no cache,
   no on-disk row. Persistence is a deliberate Phase 4 decision; it is
   not snuck in through a "tiny LRU."
8. **Pre-commit sacred.** `cargo fmt --check`, `cargo clippy -D warnings`,
   `prettier --check`, `eslint`. Never `--no-verify` per `CLAUDE.md`.
9. **File-size caps respected.** Rust soft 300 / hard 500. TS/TSX soft
   200 / hard 350. Past hard cap requires `// allow-large-file:` per
   `CONTRIBUTING.md`.

Violating the letter of these rules is violating the spirit.

## Defaults committed (overridable per-phase)

- **Date format in tool/command args:** ISO 8601 `YYYY-MM-DD`,
  interpreted as the **ET trading day**. Same convention as the existing
  `parse_ibkr_exec_time` helper in `ibkr/client/orders.rs`.
- **Account resolution:** reuse the existing `resolve_account` helper
  from `mcp/tools/mod.rs`. Optional `account` arg; defaults to the sole
  managed account; errors with the available IDs when multiple accounts
  are connected. Mirrors `get_positions` and `get_account_summary`.
- **Currency handling:** USD only for v1. Surface IBKR's reported
  `currency` field on each row but no FX conversion.
- **Option contract identity:** the canonical 5-tuple
  `(symbol, expiry, strike, right, multiplier)`. Never group by
  `local_symbol` (IBKR pads it with double spaces, e.g.
  `"TSLA  260504C00390000"`). The DTO emits parsed
  `expiry: Option<NaiveDate>`, not the raw `YYYYMMDD` string.
- **Commission + realized P&L serialization:** `Option<f64>`. `None` means
  "not (yet) reported." A literal `0.0` is real (free trade or zero P&L).
- **Empty days:** `{items: [], count: 0}`. Not an error.
- **Sort order:** ascending by `exec_time`. Reads as the day's narrative.
- **Sign convention for `realized_pnl`:** IBKR's convention — gross of
  the closing leg's commission. Net P&L per group = `sum(realized_pnl) -
  sum(commission)`. The math lives at the FE / aggregation layer; the
  raw row stays IBKR-shaped.
- **No `symbol` filter arg in v1.** A day's fills are typically a small
  set; the agent or FE filters client-side. Add the arg later if usage
  demonstrates the need.

## Phase index

| Phase | File | Depends on | Status |
|---|---|---|---|
| 1. Capture commissions + extend `IbkrExecution` | [phase-1-capture-commissions.md](phase-1-capture-commissions.md) | — | done (commit f35421a, 2026-05-04) |
| 2. MCP `get_executions` tool | [phase-2-mcp-get-executions.md](phase-2-mcp-get-executions.md) | 1 | done (commit 540078f, 2026-05-04) |
| 3. FE Today's Trades panel + Tauri command | [phase-3-trades-panel.md](phase-3-trades-panel.md) | 2 | in-progress (started 2026-05-04) |
| 4. (optional, deferred) Executions persistence layer | [phase-4-persistence.md](phase-4-persistence.md) | 1 | todo (deferred — schedule when multi-day visibility becomes a real ask) |

> **Status convention:** `todo` | `in-progress (started YYYY-MM-DD)` | `done (commit <sha>, YYYY-MM-DD)`. Update both this table AND the phase file's `**Status:**` header at phase start and exit. Don't start a phase whose dependencies aren't `done`.

## Critical files

| Concern | Path |
|---|---|
| IBKR executions drain (the bug + extension) | `src-tauri/src/ibkr/client/orders.rs` |
| IBKR execution DTO (extended in Phase 1) | `src-tauri/src/ibkr/types/orders.rs` |
| IBKR mock client (test seam) | `src-tauri/src/ibkr/mocks.rs` |
| IBKR client trait + production type | `src-tauri/src/ibkr/client/mod.rs` |
| MCP IBKR seam (production) | `src-tauri/src/mcp/ibkr_seam.rs` |
| MCP handler (router chain) | `src-tauri/src/mcp/handler.rs` |
| MCP tool registry + helpers | `src-tauri/src/mcp/tools/mod.rs` |
| MCP test fakes / fixtures | `src-tauri/src/mcp/tools/test_support.rs` |
| Reference MCP tools (mirror these) | `src-tauri/src/mcp/tools/positions.rs`, `account_summary.rs` |
| Tauri commands (existing trading) | `src-tauri/src/ibkr/commands/trading.rs` |
| Tauri command registration | `src-tauri/src/lib.rs` |
| FE feature folder conventions | `src/CLAUDE.md` |
| FE feature folder peers (mirror these) | `src/features/portfolio/`, `src/features/candidates/` |
| Storage migrations directory (Phase 4 only) | `src-tauri/src/storage/migrations/` |
| Repo-level rules | `CLAUDE.md`, `src-tauri/CLAUDE.md` |

## Sequencing + cadence

- **Day 1–2 (Phase 1):** IBKR reader gets the commission merge fix and
  `IbkrExecution` is extended with the contract+commission fields.
  Backend-only; no MCP/UI changes. Visible win: a unit test feeds the
  three-leg TSLA case from today's drain and the resulting rows include
  `commission`, `realized_pnl`, and option metadata.
- **Day 3–4 (Phase 2):** `AccountReader::executions` lands on the seam,
  the `get_executions` MCP tool ships. Visible win: from a Claude Code
  session, the same TSLA question gets answered with per-leg fills in
  one tool call — including each leg's commission and the option strike
  ladder.
- **Day 5–7 (Phase 3):** FE Trades tab. Visible win: open the tab during
  the trading day; symbol-grouped layout shows today's fills with net
  realized P&L and total fees. Date picker is wired but only "today"
  has data until Phase 4.
- **Deferred (Phase 4):** Persistence. Schedule when (a) the user asks
  for prior-day visibility in the panel, or (b) a downstream consumer
  (an agent loop, attribution service) needs multi-day fill history.

Phases 1–3 are sequential. Phase 4 depends only on Phase 1 and could in
principle run in parallel with 2 or 3, but doing so adds review surface
without changing the user-visible product, so the recommendation is to
ship 1–3 first and decide on 4 after a week of dogfooding.

## Cross-phase verification

1. **Tracer-bullet (Phase 2 exit):** the user places at least one real
   or paper trade. From a Claude Code session, calling
   `mcp__quantum-kapital__get_executions` returns ≥1 row whose
   `commission` and (for closing legs) `realized_pnl` are populated.
   For an option fill, `expiry`, `strike`, `right`, `multiplier` are
   all present. The total of `realized_pnl - commission` across the
   day's TSLA rows matches what the user can see in their TWS Trade
   Log to ±$0.01. Verified manually and recorded in the PR notes.
2. **Tracer-bullet (Phase 3 exit):** with the same fills, opening the
   Trades tab shows the symbol-grouped layout with totals matching the
   MCP tool's per-row sum.
3. **CI invariant — surveillance-only:** a test in
   `src-tauri/src/mcp/tools/` (or a top-level `tests/`) asserts that
   `executions.rs` does not import `OrderRequest`, `place_order`, or
   anything from `src-tauri/src/ibkr/commands/trading.rs`. A ripgrep
   gate asserts no source file under `src-tauri/src/mcp/tools/` calls
   `place_order`.
4. **CI invariant — commission merge correctness:** a unit test in
   `ibkr/client/orders.rs` feeds N canned `ExecutionData` events
   interleaved with M ≤ N `CommissionReport` events; asserts every
   fill row appears, exactly M rows have `commission = Some(...)`, and
   any unmatched fill has `commission = None`. No panic, no row loss.
5. **CI invariant — option contract identity:** a unit test asserts
   that for a `contract_type = "OPT"` row the option fields are all
   `Some(...)` and for `"STK"` they are all `None`.
6. **CI invariant — no live IBKR, no real `claude` subprocess:** all
   tests use either `MockIbkrClient` (unit tests on the drain) or the
   `AccountReader` fake (MCP-layer tests). The Phase 3 FE tests use
   fixture data — no Tauri command actually executes during `vitest`.
7. **CI invariant — read-only audit:** a test asserts that calling
   `get_executions` writes zero rows to `mcp_audit` (consistent with
   `get_positions` and `get_account_summary`).

## Open risks

- **Commission report lateness within a single drain.** IBKR sometimes
  emits the `CommissionReport` for a fill several events after the
  `ExecutionData` for the same `execution_id`. Within one drain this is
  fine — we read the entire subscription before returning, so a buffer
  keyed on `execution_id` reconciles them. The risk window is a fill at
  16:00:00 ET whose commission posts at 16:00:02; if the caller's
  `executions(today)` returns at 16:00:01 the row will lack commission.
  Mitigation: the DTO carries `Option<f64>` and the FE renders "—" for
  unknown commissions, which is the truthful presentation. Phase 4
  persistence makes the next drain patch the stored row. — owned by
  Phase 1 (semantics) and Phase 3 (presentation).
- **`Execution` DTO collision.** `src-tauri/src/ibkr/types/orders.rs:47`
  already defines a `#[cfg(test)]` `Execution` struct mapping the SDK
  raw type. The new public DTO must not collide. Phase 2 names its
  wire DTO `ExecutionRow` so it's clear it's the row-in-a-fills-table
  shape, distinct from the raw SDK mirror. — owned by Phase 2.
- **Multi-leg / spread orders.** A combo or spread produces multiple
  `ExecutionData` events (one per leg), each with its own `exec_id`.
  Each row stands alone, which is the right shape for our per-leg
  surface. The FE's per-symbol grouping naturally surfaces the legs
  together; no special "combo" handling required for v1. — owned by
  Phase 3.
- **IBKR's same-day-only window.** `reqExecutions` returns only the
  current TWS-day. Without persistence, "yesterday" returns empty.
  Phase 2's tool accepts any ISO date, but the doc string must say
  plainly that IBKR only delivers today's data, so a Claude Code agent
  doesn't over-trust historical date queries. Phase 4 lifts this
  constraint. — owned by Phase 2 (doc), Phase 4 (lift).
- **`Position.realized_pnl` overlap.** `get_positions` already exposes
  lifetime realized P&L per contract row. Once `get_executions` lands,
  callers asking "today's TSLA P&L" have two ways to compute it that
  are NOT equivalent: positions = lifetime per-contract, executions =
  the day's drain. Both tools' doc strings must call out the
  distinction. — owned by Phase 2.
- **Time-zone boundary.** A fill at 23:59:30 ET on day D — does
  `executions(D+1)` see it? The existing `parse_ibkr_exec_time` parses
  IBKR's timestamps through `chrono_tz::America::New_York` and the
  filter compares `exec_time.with_timezone(&New_York).date_naive()` to
  the requested date, so the answer is "no, it stays under D." Phase 1
  unit tests must include a near-midnight ET fill to lock this. —
  owned by Phase 1.
- **Multi-account.** Same handling as positions/account_summary; the
  `resolve_account` helper takes care of the single-vs-multi case.
  The Trades tab gets an account picker only when the user actually
  has more than one account; otherwise the picker is hidden. —
  owned by Phase 2 + Phase 3.
- **Tauri-FE type drift.** Rust `ExecutionRow` and TS `ExecutionRow`
  must agree. The repo does not currently use `ts-rs` or similar
  generation (per `src/CLAUDE.md`). Manual mirror is the convention;
  Phase 3 adds a focused unit test that round-trips a fixture through
  serde and the TS type to catch field renames early. — owned by
  Phase 3.
- **Persistence is an architectural escalation.** Phase 4 introduces a
  migration, an ingest worker, and a multi-account query layer. It is
  marked optional and explicitly deferred. The decision to land it
  should be triggered by a concrete user/consumer ask, not by
  speculation. — flagged here.

## Out of scope

- **Tax-lot accounting / cost basis.** IBKR's Activity Statement is
  authoritative; we do not duplicate it.
- **Order entry from the panel.** Surveillance-only.
- **Multi-currency P&L aggregation.** USD only in v1.
- **Real-time fills via streaming subscriptions** (as opposed to
  request/response). The existing `executions(filter)` request is
  sufficient at the cadence the panel and agent need.
- **Backfill of pre-Phase-4 history from IBKR.** The API does not
  support it.
