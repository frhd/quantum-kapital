# Phase 1 — Capture commissions and option metadata in `IbkrClient::executions`

> Part of [Trade history visibility](master.md). See index for invariants.

**Status:** in-progress (started 2026-05-04)

**Depends on:** none (foundation phase)

**Goal:** The IBKR adapter's `executions(date)` reader stops dropping
`CommissionReport` events and starts populating commission, realized P&L,
and option contract details on every returned row. No surface beyond the
adapter changes — no MCP tool, no Tauri command, no UI. The exit point
is a `Vec<IbkrExecution>` whose rows answer "what fee did I pay" and
"what option contract was this" without further calls.

## Files

- Touches: `src-tauri/src/ibkr/types/orders.rs` — extend `IbkrExecution`
  with `account: String`, `contract_type: String`,
  `expiry: Option<chrono::NaiveDate>`, `strike: Option<f64>`,
  `right: Option<String>`, `multiplier: Option<String>`,
  `commission: Option<f64>`, `realized_pnl: Option<f64>`,
  `currency: Option<String>`, `commission_currency: Option<String>`.
- Touches: `src-tauri/src/ibkr/client/orders.rs` — replace the
  `IBExecutions::CommissionReport(_) => {}` arm at the existing line
  117-ish; introduce a `HashMap<String, CommissionPatch>` keyed on
  `execution_id`; on stream end, merge buffered patches into the
  collected `IbkrExecution` rows; populate option fields from
  `data.contract.{last_trade_date_or_contract_month, strike, right,
  multiplier}` and `account` from `data.execution.account_number`.
- Touches: `src-tauri/src/ibkr/mocks.rs` — extend `MockIbkrClient`'s
  fixture interface so a test can hand back a list of
  `(IbkrExecution, Option<commission>, Option<realized_pnl>)` tuples or
  a richer canned-stream representation. Existing `executions` test
  helper signature changes to admit commission data.
- Touches: any unit/integration test under `src-tauri/src/ibkr/tests/`
  that constructs `IbkrExecution` literals — they need the new fields
  (default to `None` for the optionals where the test does not care).
- Touches: any caller that constructs `IbkrExecution` outside of the
  IBKR adapter. **Pre-flight check** (do this before coding): grep for
  `IbkrExecution {` across `src-tauri/src/` and confirm only the
  adapter and its tests construct the struct. If a service or MCP tool
  does, that caller goes in this phase's blast radius too.

## Reuse

- `parse_ibkr_exec_time` (existing helper at the bottom of
  `ibkr/client/orders.rs`) — unchanged, used as-is for timestamp parsing.
- `chrono_tz::America::New_York` for the ET-day filter — unchanged.
- `tokio::task::spawn_blocking` for the blocking subscription drain —
  unchanged shape; the merge logic happens inside the same closure.
- `ibapi::orders::Executions` enum (the SDK's stream type) — already
  imported; we just stop ignoring the `CommissionReport` variant.

## Decisions to make in this phase

- **Where do commission and realized P&L live?** Two options:
  - (A) Add fields directly to `IbkrExecution`. Simpler.
  - (B) Introduce a sibling struct (e.g. `IbkrFill`) that pairs an
    `IbkrExecution` with a `CommissionInfo`. Cleaner separation of
    "what filled" vs. "what it cost," but doubles the type churn.
  - **Decision:** (A). Every consumer wants both pieces together; the
    `Option<f64>` on each commission field already encodes the
    "report not received yet" state truthfully.
- **Option `expiry` representation.** SDK gives
  `last_trade_date_or_contract_month` as a string that is `YYYYMMDD`
  for monthly contracts and `YYYYMM` for some quarter/index contracts.
  - **Decision:** parse to `chrono::NaiveDate` when the format is
    `YYYYMMDD`; for `YYYYMM` (no day), fall back to `None` and emit a
    `warn!` log line. v1 of this work is dominated by
    `YYYYMMDD`-shaped equity options.
- **Option `right` normalisation.** Some venues report `"CALL"`/`"PUT"`,
  most report `"C"`/`"P"`.
  - **Decision:** normalize to single-letter `"C"` or `"P"`; anything
    else logs a `warn!` and stores the raw string verbatim.
- **`commission_currency` vs `currency`.** A fill's `currency` is the
  contract's currency; commissions are also in a currency that may
  differ from the contract's (rare for US equities, common for some
  futures). Both are surfaced; no FX conversion happens here.
- **Unmatched commission report (no buffered fill with that
  `execution_id`).** Drop with a `warn!` log including the orphaned
  `execution_id`. This is not a real IBKR scenario inside a single
  drain.
- **Fields named `expiry`, `strike`, `right`, `multiplier` already exist
  on the MCP `Position` DTO** (per the `get_positions` tool docstring).
  Choose the exact same names on `IbkrExecution` so anyone reading the
  two side by side sees the same vocabulary.

## Exit criteria

- `cargo test ibkr::client::orders` (or the existing test module the
  adapter tests live in) is green with these new cases:
  1. **`executions_merges_commission_into_matching_fill`** — fixture
     stream of two `ExecutionData` events with `execution_id`s `e1`,
     `e2`, interleaved with two `CommissionReport` events for `e1` and
     `e2`. Result: both rows have `commission = Some(<expected>)` and
     `realized_pnl = Some(<expected>)`.
  2. **`executions_handles_fill_without_commission_report`** — two
     fills, one commission report. Result: one row's commission is
     `None`, the other's is populated. No panic. No row dropped.
  3. **`executions_preserves_option_contract_fields`** — fill on
     `TSLA  260504C00390000`. Result row has
     `contract_type = "OPT"`, `expiry = Some(NaiveDate from
     2026-05-04)`, `strike = Some(390.0)`, `right = Some("C")`,
     `multiplier = Some("100")`.
  4. **`executions_stock_fill_has_empty_option_fields`** — fill on
     `RDDT`. Result row has `contract_type = "STK"` and all option
     fields are `None`.
  5. **`executions_filters_near_midnight_et`** — fill timestamped
     `20260504  23:59:30 America/New_York`. `executions(2026-05-04)`
     includes it; `executions(2026-05-05)` excludes it.
  6. **`executions_drops_orphan_commission_report_with_warn`** —
     stream contains a `CommissionReport` for an `execution_id` that
     never appeared as `ExecutionData`. Test asserts no panic, the
     orphan does not produce a fictitious row, and a `warn!` was
     emitted (gate via `tracing-test` or assert through a custom
     subscriber).
  7. **`executions_normalises_right_call_put`** — synthetic fill where
     SDK reports `right = "CALL"`. Result row has `right = Some("C")`
     and a warn line is emitted.
- **No live-IBKR test introduced.** Every new test goes through the
  existing mocked `ibapi` gateway path used by sibling tests in
  `ibkr/tests/`.
- **`IbkrExecution` shape change is backwards-friendly:** existing
  fields (`symbol`, `side`, `qty`, `avg_price`, `exec_time`, `order_id`,
  `exec_id`) keep their names and types. New fields are added; nothing
  is renamed.
- **`#[cfg(test)] struct Execution`** at `types/orders.rs:47` is left
  alone (it's a test-only mirror of the SDK raw type and unrelated to
  the public DTO).
- **File-size caps:** `client/orders.rs` is currently around 130 LOC
  for the `executions` method. The merge buffer + option parsing adds
  ~40 LOC. Keep the file under the 300 soft cap; extract a free
  `merge_commission_reports(...)` helper into the same file (or a
  sibling `executions_merge.rs`) if it pushes the cap.
- **Pre-commit clean** (`cargo fmt --check`, `cargo clippy -D warnings`,
  prettier, eslint).

## Gotchas

- **The SDK's `CommissionReport` enum payload field names.** Confirm
  against `~/.cargo/registry/src/.../ibapi-2.11.2/src/orders/`. Field
  names that matter: `execution_id` (string), `commission` (f64),
  `currency` (string), `realized_pnl` (Option<f64>), `yield_redemption_date`
  and friends are not relevant to v1.
- **`data.execution.account_number`** is the canonical account string
  on the `ExecutionData` event; do **not** infer the account from the
  connected account list. Multi-account users place fills on different
  books; the row must say which.
- **`data.contract.last_trade_date_or_contract_month`** can be empty
  for non-derivatives. Don't try to parse a date out of an empty
  string; that's where a `None` belongs.
- **`HashMap` keying.** `execution_id` is a string. Match exactly —
  IBKR does not whitespace-pad it the way it pads `local_symbol`.
- **Drain ordering is not guaranteed.** A `CommissionReport` may
  arrive before or after its `ExecutionData`. The merge buffer must
  accept reports first, then patch when the matching fill comes in
  (or vice versa). Implementation: a single `HashMap<String,
  ExecutionInProgress>` where each entry holds whichever side has
  arrived; finalise on stream end.
- **Test-only `Execution` struct collision.** Don't accidentally
  rename the new fields to match the test struct's field names — they
  are different surfaces.
- **Logging discipline.** A `warn!` per orphan commission is correct;
  a `warn!` per fill is too noisy. Use `debug!` for per-fill flow if
  needed.
- **`MockIbkrClient::executions` signature change.** Any existing
  test that called it through `IbkrClientTrait` needs an update.
  Audit `ibkr/tests/` and `services/*/tests*` before changing the
  signature; the change is intrusive.
