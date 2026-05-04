# Phase 4 — (optional, deferred) Executions persistence layer

> Part of [Trade history visibility](master.md). See index for invariants.

**Status:** todo (deferred — schedule when multi-day visibility becomes a real ask)

**Depends on:** 1

**Goal:** Persist fills to SQLite with idempotent UPSERT keyed on
`exec_id` so the panel and MCP tool can answer multi-day questions
("how did I do on TSLA Tuesday?"). Late-arriving `CommissionReport`
events patch the stored row when the next drain runs. Forward-only —
the store starts collecting the day this phase ships; pre-Phase-4
history cannot be backfilled because IBKR's API doesn't expose it.

This phase is **optional**. Schedule it when (a) the user asks for
prior-day visibility in the Trades tab, or (b) a downstream consumer
(a future agent loop, an attribution service) needs multi-day fill
history. Until then, Phases 1–3 are sufficient for the intraday/EOD
review use case.

## Files

- New: `src-tauri/src/storage/migrations/V<N>__executions.sql` — pick
  the next free version number from `storage/migrations/` at the
  time of execution. Schema (final field set; mirrors Phase 1's
  `IbkrExecution`):
  ```sql
  CREATE TABLE executions (
      exec_id TEXT PRIMARY KEY,
      account TEXT NOT NULL,
      symbol TEXT NOT NULL,
      contract_type TEXT NOT NULL,        -- "STK" | "OPT" | ...
      expiry TEXT,                        -- "YYYY-MM-DD" | NULL
      strike REAL,
      right TEXT,                         -- "C" | "P" | NULL
      multiplier TEXT,
      side TEXT NOT NULL,                 -- "bought" | "sold"
      qty REAL NOT NULL,
      avg_price REAL NOT NULL,
      currency TEXT,
      exec_time TEXT NOT NULL,            -- ISO 8601 UTC
      order_id INTEGER NOT NULL,
      commission REAL,                    -- NULL until report arrives
      realized_pnl REAL,                  -- NULL until report arrives
      commission_currency TEXT,
      ingested_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
      commission_patched_at TEXT          -- NULL until first commission patch
  );
  CREATE INDEX idx_executions_account_time ON executions(account, exec_time);
  CREATE INDEX idx_executions_account_symbol_time ON executions(account, symbol, exec_time);
  CREATE INDEX idx_executions_pending_commission ON executions(account, exec_time)
      WHERE commission IS NULL;
  ```
- New: `src-tauri/src/services/executions/mod.rs` — `ExecutionsStore`:
  - `record(rows: &[IbkrExecution]) -> Result<RecordSummary>` —
    UPSERT by `exec_id`. ON CONFLICT updates `commission`,
    `realized_pnl`, `commission_currency` only when the existing row
    has them as `NULL` (don't overwrite a populated commission with a
    later report's). Stamps `commission_patched_at` on first
    commission write.
  - `query(account: &str, date: NaiveDate, symbol: Option<&str>) ->
    Result<Vec<ExecutionRow>>` — date is interpreted as ET trading
    day; the impl converts to a UTC range before scanning.
  - `pending_commission_count(account: &str, since: NaiveDate) ->
    Result<usize>` — observability hook for the ingest worker's
    "should I poll again" decision.
- New: `src-tauri/src/services/executions/ingest.rs` —
  `ExecutionsIngestor`:
  - On app start: drain `account_reader.executions(today)` once and
    `record(...)`.
  - During market hours (04:00–20:00 ET, covers extended): every
    5 min, drain again and `record(...)`. Idempotent UPSERT means
    repeated records are safe; the late-commission patch path only
    writes when the existing row's `commission IS NULL`.
  - Outside market hours: idle.
  - Errors logged but never crash the worker; missing IBKR connection
    skips a tick with a `debug!` log.
- Touches: Phase 2's production `AccountReader::executions` impl.
  Now reads from the store first; falls back to live IBKR for today's
  fresh-data window. Algorithm:
  - If `date == today (ET)`: drain live IBKR, record into store,
    return store rows. (Live drain primes the store on every call,
    making the worker's polling a defence-in-depth rather than the
    only path.)
  - If `date < today (ET)`: read from store only.
  - If `date > today (ET)`: empty.
- Touches: Phase 3's `TradesPage` empty-state caption — drop the
  "Prior days will populate once the executions store ships" line
  when this phase is done. Update the page's date picker to allow
  any date the store knows about.
- Touches: `src-tauri/src/lib.rs` — register the ingestor with the
  existing service composition (look at how `services/mcp_audit/`
  or another long-running worker is constructed).
- Touches: master plan — update Phase 4 status, document the
  per-day "fees pending" rate observed during dogfooding.

## Reuse

- Existing migration runner pattern (`storage/migrations/`).
- Existing service composition pattern (the LLM service, news
  interpreter, and tracker workers all follow the same shape — pick
  a peer that runs as a long-lived background task and mirror it).
- The connection pool in `src-tauri/src/storage/` (existing).
- Phase 1's `IbkrExecution` shape — bound directly into SQL via
  `rusqlite` named parameters.
- Phase 2's `AccountReader` trait — extended only by changing the
  body of the production impl; the trait shape doesn't change.

## Decisions to make in this phase

- **Migration number.** Next free integer in
  `storage/migrations/`. Confirm at phase start.
- **Forward-only history.** No backfill from IBKR (the API can't).
  No backfill from a TWS Activity Statement export (out of scope —
  separate program if ever needed). The store starts on the day
  this phase ships.
- **Ingest cadence.** App-start drain + 5-min poll during 04:00–20:00
  ET + an opportunistic refresh on every `executions` call for the
  current ET day. Outside market hours: idle.
- **UPSERT semantics on commission.** First non-NULL commission wins;
  subsequent commission reports for the same `exec_id` are ignored
  with a `debug!` log. This protects against the rare case where
  IBKR re-sends a report with a different value (very rare but
  observed historically — the broker's bookkeeping is the source of
  truth, but we don't want a later poll to clobber an earlier
  commission with a different number unless we also bump
  `commission_patched_at`).
- **Multi-account isolation.** Account is a column; queries always
  filter by it. The ingest worker drains all managed accounts in one
  pass and dispatches to rows by `account_number`.
- **Schema evolution.** If Phase 1's `IbkrExecution` gains a field
  later, this migration plus a follow-up migration handle it. v1
  schema covers everything Phase 1 produces.

## Exit criteria

- Manual: stop `pnpm tauri dev`, restart, open Trades tab on
  yesterday's date — fills appear (assuming the app was running
  yesterday and ingested them). Today's date continues to behave as
  in Phase 3.
- Tests:
  1. **`store_upserts_idempotently`** — call `record` twice with the
     same `Vec<IbkrExecution>`; assert `SELECT count(*) FROM
     executions` is the row count, not 2× the row count.
  2. **`store_patches_commission_on_late_arrival`** — insert fill
     with `commission = None`; later, insert the same fill (same
     `exec_id`) with `commission = Some(0.65)`; assert the row's
     commission is now `0.65` and `commission_patched_at` is set.
  3. **`store_does_not_overwrite_populated_commission`** — insert
     fill with `commission = Some(0.65)`; later insert the same fill
     with `commission = Some(0.99)`; assert the row's commission
     stayed `0.65` and a `debug!` was emitted.
  4. **`store_query_filters_by_et_date_across_utc_midnight`** — fill
     at 23:59 ET on 2026-05-04 (UTC ≈ 03:59 the next day).
     `query(account, 2026-05-04, None)` includes it;
     `query(account, 2026-05-05, None)` excludes it.
  5. **`store_query_isolates_accounts`** — fills for accounts `U1`
     and `U2`; query for `U1` returns only `U1` rows.
  6. **`ingestor_skips_when_ibkr_disconnected`** — fake reader
     returns `IbkrError::NotConnected`; ingestor logs a `debug!`
     and proceeds without erroring.
  7. **`account_reader_executions_serves_past_days_from_store`** —
     production impl receives a date < today; asserts no live IBKR
     call is made and rows come from the store.
- Migration runs cleanly on a fresh DB and on an existing DB with
  the prior set of migrations applied.
- File-size caps: split `services/executions/` into
  `mod.rs` + `ingest.rs` + `query.rs` if the writer/reader paths
  approach the 300 soft cap together.
- Pre-commit clean.

## Gotchas

- **Migration ordering.** Cargo build runs all migrations; ensure the
  new file's number is strictly greater than every existing one.
  Don't reuse a number from an in-flight branch.
- **DST + ET-day query.** Convert ET-day to a UTC range
  (`day_start_et → day_end_et`, both with the correct DST offset
  for that calendar day) and scan `exec_time` in `[start, end)`.
  Test against a fall-back / spring-forward day.
- **Concurrent writes.** SQLite is single-writer. The ingestor and
  the live `AccountReader::executions(today)` both call `record(...)`.
  Both go through the existing connection pool; UPSERT is atomic per
  row. Don't hold transactions across multiple calls.
- **Commission-late metric.** The phase exit criteria don't require
  it, but instrumenting the rate of "fill present, commission still
  NULL after T+5 min" is a valuable observability signal. Add a
  Prometheus-or-equivalent counter only if the rest of the app
  already exposes them; otherwise log to `tracing` with a structured
  field for grep-based dashboarding.
- **PII / data sensitivity.** `executions` is the same sensitivity
  as the existing `tracker.sqlite` (personal trading data). No new
  privacy considerations; storage path is the same SQLite file.
- **Tax-lot accounting.** Out of scope. The store records fills,
  not lots. IBKR's Activity Statement is authoritative for tax
  purposes.
- **Time-zone storage convention.** Store timestamps as ISO 8601
  UTC strings (already the format we use elsewhere in this DB);
  parse on read.
- **`exec_id` collisions across reconnects.** IBKR's `execution_id`
  is globally unique within the broker's lifetime. UPSERT-by-PK
  handles any duplicate stream from a reconnect/replay correctly.
- **Forward-only consequences.** The Trades tab will show partial
  history at first (only days since this phase shipped). The page's
  empty-state for older dates should say "no recorded fills for
  {date}" — not "no fills for {date}" — so the user understands
  the distinction between "I didn't trade" and "the store didn't
  exist yet."
