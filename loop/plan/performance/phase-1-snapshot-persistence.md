# Phase 1 — daily account snapshots persisted to SQLite

> Part of [Portfolio performance analytics](master.md). See index for invariants.

**Status:** todo

**Depends on:** none (foundation phase)

**Goal:** Stand up the storage and capture path so the app starts accumulating one `(account, date)` row per trading day. Adds migration `V14__performance_snapshots.sql` (creates `account_snapshots` + an empty-on-purpose `cash_flows` table for forward compat), a new `services/performance/` module split into `mod.rs` / `types.rs` / `repository.rs` / `snapshot.rs`, an EOD scheduler hook that fires the snapshot at ~16:01 ET on trading days, a startup catch-up if today's row is missing, and a debug-grade `performance_capture_snapshot_now` Tauri command for manual triggering.

## Files

- New: `src-tauri/src/storage/migrations/V14__performance_snapshots.sql` — migration with both tables, indexes, and `(account, date)` unique constraint on `account_snapshots`. `cash_flows` ships empty in v1.
- New: `src-tauri/src/services/performance/mod.rs` — `PerformanceService { db: Arc<Db>, ibkr: Arc<dyn IbkrClientTrait> }` constructor + re-exports. Stays under 100 lines.
- New: `src-tauri/src/services/performance/types.rs` — `AccountSnapshot`, `CashFlow`, `Currency` (alias), `SnapshotSource` (`LiveSnapshot | FlexQuery`), `CashFlowSource` (`Manual | FlexQuery`), `CashFlowKind` (closed enum). All `serde` for the Tauri boundary; `rusqlite::types::FromSql` impls for the enums.
- New: `src-tauri/src/services/performance/repository.rs` — `insert_snapshot`, `latest_snapshot(account)`, `snapshots_in_range(account, from, to)`, `cash_flows_in_range(account, from, to)`, `has_snapshot_for_today(account, today_et)`, `connected_accounts(...)`. Pure SQL using the pooled `Db`.
- New: `src-tauri/src/services/performance/snapshot.rs` — `capture_snapshot(account) -> Result<AccountSnapshot>`, `capture_all_accounts(today_et) -> Result<Vec<AccountSnapshot>>`, `capture_now_if_missing(today_et) -> Result<Option<AccountSnapshot>>`. Calls `IbkrClient::get_account_summary`, parses tags, writes via `repository::insert_snapshot`.
- New: `src-tauri/src/ibkr/commands/performance.rs` — `performance_capture_snapshot_now(account: Option<String>, state: State<Arc<PerformanceService>>) -> Result<AccountSnapshot, String>`. Read commands (`performance_get_curve`, `performance_get_metrics`) ship in Phase 2.
- Touches: `src-tauri/src/services/mod.rs` — `pub mod performance;` re-export.
- Touches: `src-tauri/src/ibkr/commands/mod.rs` — re-export `performance_capture_snapshot_now`.
- Touches: `src-tauri/src/services/eod_scheduler/*.rs` — at the top of each tick body on trading days, call `performance.capture_all_accounts(today_et).await` and log per-account success/failure. Trading-day gate via existing `utils/market_calendar`.
- Touches: `src-tauri/src/lib.rs` (`run`) — construct `PerformanceService { db, ibkr }`, `app.manage(Arc::new(svc.clone()))`, register `performance_capture_snapshot_now`, fire `capture_now_if_missing(today_et).await` once on startup (best-effort; log + continue on error so a not-yet-connected IBKR doesn't block boot).

## Tools / endpoints exposed

| Command | Wraps |
|---|---|
| `performance_capture_snapshot_now(account?)` | `PerformanceService::capture_snapshot` for a single account, or `capture_all_accounts` when `account` is `None`. Debug-grade — manual trigger; users won't normally call it. |

## Reuse (no new business logic this phase)

- `IbkrClient::get_account_summary` (`src-tauri/src/ibkr/client/mod.rs`, ~L343) — single source for live `NetLiquidation`, `TotalCashValue`, `GrossPositionValue` tags. Already returns `Vec<AccountSummary>` with `tag` / `value: String` / `currency`.
- `Db` pool from `src-tauri/src/storage/mod.rs` — same `r2d2 + rusqlite` pattern every other service uses.
- `utils/market_calendar` — for the "is today a trading day" gate in the scheduler hook.
- `IbkrClientTrait` + `MockIbkrClient` (`src-tauri/src/ibkr/mocks.rs`) — exclusive test seam for `snapshot.rs`.
- Service-composition pattern from `src-tauri/src/services/agent_morning_packs/` and `src-tauri/src/services/predictions/` — mirror their `mod.rs` shape (struct + constructor + thin re-exports; logic lives in sibling files).
- Refinery migration runner from `src-tauri/src/storage/migrations.rs` — `V14` is the next free number after the existing `V13__executions.sql`.

## Decisions to make in this phase

- **Tag matching.** Case-sensitive on `tag == "NetLiquidation"` etc. (matches `mcp/tools/account_summary.rs`). Missing required tag → log + skip the snapshot for that account; do **not** write a partial row.
- **Currency selection.** Read from each tag's `currency` field. If the three tags disagree across an account in a single fetch, log error + skip. v1 doesn't normalise.
- **Date determination.** Single helper `today_et() -> NaiveDate` colocated in `services/performance/mod.rs` if `utils/market_calendar` doesn't already expose one. Always derived from `chrono::Utc::now()` shifted to `America/New_York`.
- **Pre-market warm-up.** EOD hook only fires the snapshot on the EOD tick (≈16:01 ET), not the intraday or auto-scanner ticks. If the EOD scheduler doesn't currently fire at exactly 16:01, snapshot at whatever time it does fire and document; if it fires before market close, escalate (we may need a tiny standalone scheduler — see Gotchas).
- **UPSERT clause.** `INSERT INTO account_snapshots(...) VALUES (...) ON CONFLICT(account, date) DO UPDATE SET net_liquidation = excluded.net_liquidation, total_cash = excluded.total_cash, gross_position_value = excluded.gross_position_value, currency = excluded.currency, source = excluded.source, captured_at = excluded.captured_at` — preserves the row's `id` PK. **Not** `INSERT OR REPLACE`, which rotates the PK.
- **Account scope on `capture_all_accounts`.** Fetch from `IbkrClient` directly each tick (don't cache the account list — IBKR's connected-accounts set is ephemeral state). If empty list, log + return `Ok(vec![])`.

## Exit criteria

- `cargo test services::performance::` green:
  - Snapshot writer (mocked IBKR) produces one row per call.
  - Repository round-trip: insert → `latest_snapshot` returns it; `snapshots_in_range` filters by date correctly.
  - Idempotent UPSERT: insert same `(account, date)` twice → 1 row, `captured_at` updated, `id` unchanged.
  - Missing-tag fail-soft: `MockIbkrClient` returning only `NetLiquidation` (no `TotalCashValue`) → `capture_snapshot` returns `Err`, no row written.
  - Schema forward-compat: insert one row with `source = 'live_snapshot'` and one with `source = 'flex_query'` in the same range → both round-trip cleanly.
- With `pnpm tauri dev` + IBKR connected: invoke `performance_capture_snapshot_now` from the dev console (or a temporary debug button) → exactly one row in `tracker.sqlite::account_snapshots`. Re-invoke → still one row, `captured_at` updated, `id` unchanged.
- Restart app after 16:01 ET on a trading day with no row for today → catch-up writes a row; `/tmp/qk-tauri.log` shows the capture log line (`performance::snapshot captured account=DU... date=YYYY-MM-DD net_liq=...`).
- Pre-commit clean: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `prettier --check`, `eslint`.
- Every new Rust file < 300 lines.

## Gotchas

- `AccountSummary.value` is a `String`. Parse with `.parse::<f64>()` and propagate errors. A bad parse on a single tag should log + skip the whole snapshot, not write garbage.
- The EOD scheduler's tick interval is config-driven (`intraday_tick_interval_secs` for the intraday one; the EOD one has its own cadence). Make sure the snapshot step runs on the **EOD** tick, not the intraday tick. If the EOD scheduler doesn't currently fire at exactly 16:01 ET, two options: (a) snapshot at whatever time the EOD scheduler does fire and document the choice, or (b) fall back to a tiny standalone scheduler `services/performance_scheduler/` mirroring `services/social_sentiment_scheduler/`. Prefer (a) for v1.
- Tests must not touch a real `Db` or live IBKR. Use `Db::open_in_memory()` if it exists, or the existing `mcp::tools::test_support::make_db()` helper. Wire `MockIbkrClient` (`src-tauri/src/ibkr/mocks.rs`) for the snapshot writer's IBKR dependency.
- `ON CONFLICT(account, date) DO UPDATE` requires the unique constraint to be **named** the same as the conflict target. Either declare `UNIQUE(account, date)` inline (anonymous) and use `ON CONFLICT(account, date)`, or attach a name. Verify with `EXPLAIN`-style debug if the upsert silently inserts duplicates.
- Startup catch-up: if IBKR is not yet connected (`pnpm tauri dev` first launch typically takes 1–2 s to connect), the call returns an error. Treat as best-effort: log a warning, return `Ok(None)`, and let the next EOD tick (or the next startup) capture.
- `ibkr.connected_accounts()` may return an empty set during the connection handshake. The capture_all_accounts impl should treat empty as a non-error.
- `chrono` `America/New_York` requires `chrono_tz` (or `chrono`'s `tzfile` feature). Confirm the dependency is already present from existing market-calendar code; do not add a new crate.
- Don't surface `performance_capture_snapshot_now` in production UI in this phase. It's debug-only. The `Capture snapshot now` button in the Performance tab lands in Phase 3.
