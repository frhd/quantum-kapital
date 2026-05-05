# Phase 1 — Persist executions to SQLite

> Part of [Behavioral assessment via MCP](master.md). See master for invariants.

**Status:** done (commit d0e0c7a, 2026-05-05)

**Depends on:** —

**Goal:** Stand up an `executions` SQLite table fed by an idempotent UPSERT path; an `ExecutionsIngestor` background worker that drains live IBKR every 5 min during market hours; an extension to the existing `AccountReader::executions` so today's calls hit live IBKR (priming the store) and prior-day calls read straight from the store. This unblocks Phases 2, 4, 6, all of which need multi-day fill history.

**Why this comes first:** IBKR's `reqExecutions` endpoint only returns the current TWS-day. Without persistence there is no "yesterday" — so `get_trade_legs(yesterday)` (Phase 2) and `eod_review.py`'s "score yesterday's fills" (Phase 4) and `get_trader_profile`'s 30-day window (Phase 6) all return empty. Forward-only history is acceptable; the store starts collecting on the day this phase ships and there is no API path to backfill prior fills.

This phase is a folded-in version of the deferred `phase-4-persistence.md` from the retired plan. The deferred spec is the source material; this phase tightens it for TDD step-by-step execution.

## End-state for this phase

- `executions` SQLite table exists, indexed for `(account, exec_time)` scans and a partial index on rows missing commissions.
- `ExecutionsStore::record(rows)` writes new rows and patches late-arriving commission/realized_pnl onto existing rows without overwriting populated values.
- `ExecutionsStore::query(account, date, symbol?)` returns rows for the ET trading day, account-isolated.
- `ExecutionsIngestor` long-running task drains live IBKR on app start and every 5 min during 04:00–20:00 ET; idle outside.
- `AccountReader::executions(account, date)` production impl reads from the store for `date < today (ET)`, drains live IBKR + records + returns store rows for `date == today`, and returns empty for `date > today`.
- All existing Phase 1/2/3 tests of the retired plan continue to pass (no regression to `get_executions` MCP tool's wire shape).

## Files

**Create:**
- `src-tauri/src/storage/migrations/V13__executions.sql` — new table + indexes (schema below).
- `src-tauri/src/services/executions/mod.rs` — module root, re-exports.
- `src-tauri/src/services/executions/store.rs` — `ExecutionsStore` (writer + reader).
- `src-tauri/src/services/executions/ingest.rs` — `ExecutionsIngestor` (background task).
- `src-tauri/src/services/executions/tests.rs` — unit tests for store + ingestor.

**Modify:**
- `src-tauri/src/services/mod.rs` — register `pub mod executions;`.
- `src-tauri/src/mcp/ibkr_seam.rs` — extend the production impl of `AccountReader::executions` to use the store. The trait shape does not change.
- `src-tauri/src/lib.rs` — wire `ExecutionsStore` and `ExecutionsIngestor` into the service composition; spawn the ingestor task on app start (mirror the pattern used by `services/mcp_audit/` or another long-lived worker).

## Reuse

- Existing `IbkrExecution` shape in `src-tauri/src/ibkr/types/orders.rs` (already extended in Phase 1 of the retired plan with all option/commission fields).
- Existing migration runner in `src-tauri/src/storage/migrations.rs` (refinery; just add the next-numbered SQL file).
- Existing connection pool in `src-tauri/src/storage/mod.rs` (r2d2).
- Existing service-composition pattern in `src-tauri/src/lib.rs::run` (look at `mcp_audit` or `social_sentiment_scheduler` for a long-lived worker peer).
- Existing `chrono_tz::America::New_York` for ET-day boundary math (used in `parse_ibkr_exec_time` at `ibkr/client/orders.rs`).

## Schema (V13__executions.sql)

```sql
-- V13__executions.sql
-- Persists IBKR execution fills so the assessment stack (Phases 2/4/6) can
-- query multi-day history. Forward-only; no backfill from IBKR is possible.

CREATE TABLE executions (
    exec_id              TEXT    PRIMARY KEY,
    account              TEXT    NOT NULL,
    symbol               TEXT    NOT NULL,
    contract_type        TEXT    NOT NULL,    -- "STK" | "OPT" | ...
    expiry               TEXT,                -- "YYYY-MM-DD" | NULL (non-options)
    strike               REAL,
    "right"              TEXT,                -- "C" | "P" | NULL
    multiplier           TEXT,                -- "100" for standard equity opts
    side                 TEXT    NOT NULL,    -- "bought" | "sold"
    qty                  REAL    NOT NULL,
    avg_price            REAL    NOT NULL,
    currency             TEXT,
    exec_time            TEXT    NOT NULL,    -- ISO 8601 UTC
    order_id             INTEGER NOT NULL,
    commission           REAL,                -- NULL until report arrives
    realized_pnl         REAL,                -- NULL for opening legs / unreported
    commission_currency  TEXT,
    ingested_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    commission_patched_at TEXT
);

CREATE INDEX idx_executions_account_time
    ON executions(account, exec_time);

CREATE INDEX idx_executions_account_symbol_time
    ON executions(account, symbol, exec_time);

CREATE INDEX idx_executions_pending_commission
    ON executions(account, exec_time)
    WHERE commission IS NULL;
```

> **`right` is a SQL keyword in some dialects** — quoted with double quotes to keep refinery's parser and rusqlite happy. Test the migration on a fresh DB before assuming it lints clean.

## Tasks

### Task 1: Create the migration file

**Files:**
- Create: `src-tauri/src/storage/migrations/V13__executions.sql`

- [ ] **Step 1: Write the migration**

Paste the schema block above verbatim into the new file.

- [ ] **Step 2: Run the migration test**

```bash
cd src-tauri && cargo test storage::migrations -- --nocapture
```

Expected: PASS — refinery picks up the new file and applies it on a fresh DB.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/storage/migrations/V13__executions.sql
git commit -m "feat(storage): add V13 executions table for fill persistence"
```

### Task 2: ExecutionsStore — write path skeleton

**Files:**
- Create: `src-tauri/src/services/executions/mod.rs`
- Create: `src-tauri/src/services/executions/store.rs`
- Modify: `src-tauri/src/services/mod.rs` — add `pub mod executions;`

- [ ] **Step 1: Write the failing test for idempotent insert**

Create `src-tauri/src/services/executions/tests.rs`:

```rust
//! Integration tests for ExecutionsStore. Uses an in-memory SQLite DB
//! built via the existing test_support::make_db() helper.

use super::store::ExecutionsStore;
use crate::ibkr::types::{ExecutionSide, IbkrExecution};
use crate::mcp::tools::test_support::make_db;
use chrono::{NaiveDate, TimeZone, Utc};

fn stk(exec_id: &str, account: &str, qty: f64, price: f64) -> IbkrExecution {
    IbkrExecution {
        symbol: "TSLA".to_string(),
        side: ExecutionSide::Bought,
        qty,
        avg_price: price,
        exec_time: Utc.with_ymd_and_hms(2026, 5, 4, 14, 30, 0).unwrap(),
        order_id: 1,
        exec_id: exec_id.to_string(),
        account: account.to_string(),
        contract_type: "STK".to_string(),
        expiry: None,
        strike: None,
        right: None,
        multiplier: None,
        commission: Some(0.65),
        realized_pnl: None,
        currency: Some("USD".to_string()),
        commission_currency: Some("USD".to_string()),
    }
}

#[tokio::test]
async fn store_upserts_idempotently() {
    let (_tmp, db) = make_db();
    let store = ExecutionsStore::new(db);
    let row = stk("E1", "DU123", 100.0, 250.0);

    store.record(&[row.clone()]).await.expect("first record");
    store.record(&[row.clone()]).await.expect("second record");

    let rows = store
        .query("DU123", NaiveDate::from_ymd_opt(2026, 5, 4).unwrap(), None)
        .await
        .expect("query ok");
    assert_eq!(rows.len(), 1, "expected 1 row, got {}", rows.len());
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cd src-tauri && cargo test services::executions::tests::store_upserts_idempotently 2>&1 | tail -20
```

Expected: FAIL — `ExecutionsStore` does not exist.

- [ ] **Step 3: Implement the minimal store with `record` and `query`**

Create `src-tauri/src/services/executions/mod.rs`:

```rust
//! Executions store + ingest worker. Persists IBKR fills so the
//! assessment stack can query multi-day history. Forward-only.

pub mod store;
pub use store::ExecutionsStore;

#[cfg(test)]
mod tests;
```

Create `src-tauri/src/services/executions/store.rs`:

```rust
//! `ExecutionsStore` — writer + reader for the `executions` table.
//!
//! Idempotent UPSERT keyed on `exec_id`. Late-arriving commission
//! reports patch existing rows without overwriting populated values
//! (first non-NULL commission wins).

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use chrono_tz::America::New_York;
use rusqlite::{params, OptionalExtension};
use std::sync::Arc;

use crate::ibkr::types::{ExecutionSide, IbkrExecution};
use crate::storage::Db;

#[derive(Debug, Default, Clone)]
pub struct RecordSummary {
    pub inserted: usize,
    pub commission_patched: usize,
    pub skipped_redundant: usize,
}

#[derive(Clone)]
pub struct ExecutionsStore {
    db: Arc<Db>,
}

impl ExecutionsStore {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    /// UPSERT a batch of fills.
    ///
    /// - New `exec_id` ⇒ INSERT.
    /// - Existing `exec_id` with `commission IS NULL` and incoming `commission IS Some(_)` ⇒ UPDATE commission/realized_pnl/commission_currency, stamp `commission_patched_at`.
    /// - Existing `exec_id` with `commission IS Some(_)` ⇒ no-op (first non-NULL wins).
    pub async fn record(&self, rows: &[IbkrExecution]) -> Result<RecordSummary, rusqlite::Error> {
        if rows.is_empty() {
            return Ok(RecordSummary::default());
        }
        let owned: Vec<IbkrExecution> = rows.to_vec();
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || {
            let mut conn = db.get().expect("db conn");
            let tx = conn.transaction()?;
            let mut summary = RecordSummary::default();
            for row in &owned {
                let existing: Option<Option<f64>> = tx
                    .query_row(
                        "SELECT commission FROM executions WHERE exec_id = ?1",
                        params![row.exec_id],
                        |r| r.get::<_, Option<f64>>(0),
                    )
                    .optional()?;
                match existing {
                    None => {
                        tx.execute(
                            "INSERT INTO executions (
                                exec_id, account, symbol, contract_type, expiry,
                                strike, \"right\", multiplier, side, qty, avg_price,
                                currency, exec_time, order_id, commission,
                                realized_pnl, commission_currency
                             ) VALUES (
                                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                                ?12, ?13, ?14, ?15, ?16, ?17
                             )",
                            params![
                                row.exec_id,
                                row.account,
                                row.symbol,
                                row.contract_type,
                                row.expiry.map(|d| d.format("%Y-%m-%d").to_string()),
                                row.strike,
                                row.right,
                                row.multiplier,
                                side_to_str(&row.side),
                                row.qty,
                                row.avg_price,
                                row.currency,
                                row.exec_time.to_rfc3339(),
                                row.order_id,
                                row.commission,
                                row.realized_pnl,
                                row.commission_currency,
                            ],
                        )?;
                        summary.inserted += 1;
                    }
                    Some(None) if row.commission.is_some() => {
                        tx.execute(
                            "UPDATE executions
                             SET commission = ?1,
                                 realized_pnl = COALESCE(?2, realized_pnl),
                                 commission_currency = COALESCE(?3, commission_currency),
                                 commission_patched_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
                             WHERE exec_id = ?4",
                            params![row.commission, row.realized_pnl, row.commission_currency, row.exec_id],
                        )?;
                        summary.commission_patched += 1;
                    }
                    _ => {
                        summary.skipped_redundant += 1;
                    }
                }
            }
            tx.commit()?;
            Ok(summary)
        })
        .await
        .expect("blocking task")
    }

    /// Read fills for an ET trading day, optionally filtered by symbol.
    pub async fn query(
        &self,
        account: &str,
        date: NaiveDate,
        symbol: Option<&str>,
    ) -> Result<Vec<IbkrExecution>, rusqlite::Error> {
        let (start_utc, end_utc) = et_day_bounds_utc(date);
        let account = account.to_string();
        let symbol = symbol.map(|s| s.to_string());
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || {
            let conn = db.get().expect("db conn");
            let sql = match &symbol {
                Some(_) => "SELECT exec_id, account, symbol, contract_type, expiry, strike,
                                   \"right\", multiplier, side, qty, avg_price, currency,
                                   exec_time, order_id, commission, realized_pnl,
                                   commission_currency
                            FROM executions
                            WHERE account = ?1 AND symbol = ?2
                              AND exec_time >= ?3 AND exec_time < ?4
                            ORDER BY exec_time ASC",
                None => "SELECT exec_id, account, symbol, contract_type, expiry, strike,
                                \"right\", multiplier, side, qty, avg_price, currency,
                                exec_time, order_id, commission, realized_pnl,
                                commission_currency
                         FROM executions
                         WHERE account = ?1
                           AND exec_time >= ?2 AND exec_time < ?3
                         ORDER BY exec_time ASC",
            };
            let mut stmt = conn.prepare(sql)?;
            let rows = if let Some(sym) = symbol {
                stmt.query_map(
                    params![account, sym, start_utc.to_rfc3339(), end_utc.to_rfc3339()],
                    map_row,
                )?
                .collect::<Result<Vec<_>, _>>()?
            } else {
                stmt.query_map(
                    params![account, start_utc.to_rfc3339(), end_utc.to_rfc3339()],
                    map_row,
                )?
                .collect::<Result<Vec<_>, _>>()?
            };
            Ok(rows)
        })
        .await
        .expect("blocking task")
    }

    /// Count fills with `commission IS NULL` since the given ET day. Observability hook.
    pub async fn pending_commission_count(
        &self,
        account: &str,
        since: NaiveDate,
    ) -> Result<usize, rusqlite::Error> {
        let (start_utc, _) = et_day_bounds_utc(since);
        let account = account.to_string();
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || {
            let conn = db.get().expect("db conn");
            let n: i64 = conn.query_row(
                "SELECT COUNT(*) FROM executions
                 WHERE account = ?1 AND commission IS NULL AND exec_time >= ?2",
                params![account, start_utc.to_rfc3339()],
                |r| r.get(0),
            )?;
            Ok(n as usize)
        })
        .await
        .expect("blocking task")
    }
}

fn side_to_str(side: &ExecutionSide) -> &'static str {
    match side {
        ExecutionSide::Bought => "bought",
        ExecutionSide::Sold => "sold",
    }
}

fn parse_side(s: &str) -> ExecutionSide {
    match s {
        "sold" => ExecutionSide::Sold,
        _ => ExecutionSide::Bought,
    }
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<IbkrExecution> {
    let exec_time_str: String = row.get(12)?;
    Ok(IbkrExecution {
        exec_id: row.get(0)?,
        account: row.get(1)?,
        symbol: row.get(2)?,
        contract_type: row.get(3)?,
        expiry: row
            .get::<_, Option<String>>(4)?
            .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
        strike: row.get(5)?,
        right: row.get(6)?,
        multiplier: row.get(7)?,
        side: parse_side(&row.get::<_, String>(8)?),
        qty: row.get(9)?,
        avg_price: row.get(10)?,
        currency: row.get(11)?,
        exec_time: DateTime::parse_from_rfc3339(&exec_time_str)
            .expect("rfc3339")
            .with_timezone(&Utc),
        order_id: row.get(13)?,
        commission: row.get(14)?,
        realized_pnl: row.get(15)?,
        commission_currency: row.get(16)?,
    })
}

/// Convert an ET trading day to a `[start_utc, end_utc)` half-open range.
/// Handles DST correctly via `chrono_tz::America::New_York`.
fn et_day_bounds_utc(date: NaiveDate) -> (DateTime<Utc>, DateTime<Utc>) {
    let day_start_naive = date.and_hms_opt(0, 0, 0).unwrap();
    let next_day_naive = (date + chrono::Duration::days(1)).and_hms_opt(0, 0, 0).unwrap();
    let start_et = New_York
        .from_local_datetime(&day_start_naive)
        .single()
        .expect("unambiguous start");
    let end_et = New_York
        .from_local_datetime(&next_day_naive)
        .single()
        .expect("unambiguous end");
    (start_et.with_timezone(&Utc), end_et.with_timezone(&Utc))
}
```

> **`Db` import path:** check whether `crate::storage::Db` is the right name in this codebase. If the connection pool type is named differently (e.g. `DbPool`), substitute that. Look at any existing `services/<name>/store.rs` or `agent_morning_packs.rs` for the canonical import.

- [ ] **Step 4: Run the test to verify it passes**

```bash
cd src-tauri && cargo test services::executions::tests::store_upserts_idempotently
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/executions/ src-tauri/src/services/mod.rs
git commit -m "feat(executions): ExecutionsStore with idempotent UPSERT"
```

### Task 3: Late commission patch

**Files:**
- Modify: `src-tauri/src/services/executions/tests.rs`

- [ ] **Step 1: Write the failing test**

Append to `tests.rs`:

```rust
#[tokio::test]
async fn store_patches_commission_on_late_arrival() {
    let (_tmp, db) = make_db();
    let store = ExecutionsStore::new(db);
    let mut row = stk("E2", "DU123", 100.0, 250.0);
    row.commission = None;
    row.realized_pnl = None;
    store.record(&[row.clone()]).await.unwrap();

    // Late report arrives.
    row.commission = Some(0.99);
    row.realized_pnl = Some(42.5);
    let summary = store.record(&[row]).await.unwrap();
    assert_eq!(summary.commission_patched, 1);
    assert_eq!(summary.inserted, 0);

    let rows = store
        .query("DU123", NaiveDate::from_ymd_opt(2026, 5, 4).unwrap(), None)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].commission, Some(0.99));
    assert_eq!(rows[0].realized_pnl, Some(42.5));
}
```

- [ ] **Step 2: Run test, verify it passes** (the patch path is already implemented in Task 2)

```bash
cd src-tauri && cargo test services::executions::tests::store_patches_commission_on_late_arrival
```

Expected: PASS.

### Task 4: Don't overwrite a populated commission

**Files:**
- Modify: `src-tauri/src/services/executions/tests.rs`

- [ ] **Step 1: Write the test**

```rust
#[tokio::test]
async fn store_does_not_overwrite_populated_commission() {
    let (_tmp, db) = make_db();
    let store = ExecutionsStore::new(db);
    let mut row = stk("E3", "DU123", 100.0, 250.0);
    row.commission = Some(0.65);
    store.record(&[row.clone()]).await.unwrap();

    row.commission = Some(0.99); // would clobber if not protected
    let summary = store.record(&[row]).await.unwrap();
    assert_eq!(summary.skipped_redundant, 1);
    assert_eq!(summary.commission_patched, 0);

    let rows = store
        .query("DU123", NaiveDate::from_ymd_opt(2026, 5, 4).unwrap(), None)
        .await
        .unwrap();
    assert_eq!(rows[0].commission, Some(0.65));
}
```

- [ ] **Step 2: Run, verify it passes**

```bash
cd src-tauri && cargo test services::executions::tests::store_does_not_overwrite_populated_commission
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/services/executions/tests.rs
git commit -m "test(executions): commission late-patch and no-overwrite invariants"
```

### Task 5: ET-day query across UTC midnight

**Files:**
- Modify: `src-tauri/src/services/executions/tests.rs`

- [ ] **Step 1: Write the test (DST-stable mid-May date so EDT = UTC-4)**

```rust
#[tokio::test]
async fn store_query_filters_by_et_date_across_utc_midnight() {
    let (_tmp, db) = make_db();
    let store = ExecutionsStore::new(db);

    // 23:59 ET on 2026-05-04 (EDT, UTC-4) ⇒ 03:59 UTC on 2026-05-05.
    let mut late_on_4 = stk("LATE", "DU123", 1.0, 1.0);
    late_on_4.exec_time = Utc.with_ymd_and_hms(2026, 5, 5, 3, 59, 0).unwrap();

    // 00:01 ET on 2026-05-05 (EDT) ⇒ 04:01 UTC on 2026-05-05.
    let mut early_on_5 = stk("EARLY", "DU123", 1.0, 1.0);
    early_on_5.exec_id = "EARLY".into();
    early_on_5.exec_time = Utc.with_ymd_and_hms(2026, 5, 5, 4, 1, 0).unwrap();

    store.record(&[late_on_4, early_on_5]).await.unwrap();

    let day4 = store
        .query("DU123", NaiveDate::from_ymd_opt(2026, 5, 4).unwrap(), None)
        .await
        .unwrap();
    let day5 = store
        .query("DU123", NaiveDate::from_ymd_opt(2026, 5, 5).unwrap(), None)
        .await
        .unwrap();
    assert_eq!(day4.len(), 1, "expected LATE on 2026-05-04");
    assert_eq!(day4[0].exec_id, "LATE");
    assert_eq!(day5.len(), 1, "expected EARLY on 2026-05-05");
    assert_eq!(day5[0].exec_id, "EARLY");
}
```

- [ ] **Step 2: Run, verify it passes**

```bash
cd src-tauri && cargo test services::executions::tests::store_query_filters_by_et_date_across_utc_midnight
```

Expected: PASS. If it fails, the DST-aware bounds in `et_day_bounds_utc` are wrong — fix and re-run.

### Task 6: Account isolation

**Files:**
- Modify: `src-tauri/src/services/executions/tests.rs`

- [ ] **Step 1: Write the test**

```rust
#[tokio::test]
async fn store_query_isolates_accounts() {
    let (_tmp, db) = make_db();
    let store = ExecutionsStore::new(db);
    store
        .record(&[
            stk("U1A", "U1", 1.0, 1.0),
            stk("U2A", "U2", 1.0, 1.0),
        ])
        .await
        .unwrap();

    let u1 = store
        .query("U1", NaiveDate::from_ymd_opt(2026, 5, 4).unwrap(), None)
        .await
        .unwrap();
    let u2 = store
        .query("U2", NaiveDate::from_ymd_opt(2026, 5, 4).unwrap(), None)
        .await
        .unwrap();
    assert_eq!(u1.len(), 1);
    assert_eq!(u1[0].account, "U1");
    assert_eq!(u2.len(), 1);
    assert_eq!(u2[0].account, "U2");
}
```

- [ ] **Step 2: Run, verify it passes**

```bash
cd src-tauri && cargo test services::executions::tests::store_query_isolates_accounts
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/services/executions/tests.rs
git commit -m "test(executions): ET-day boundary and account-isolation queries"
```

### Task 7: ExecutionsIngestor — background drain worker

**Files:**
- Create: `src-tauri/src/services/executions/ingest.rs`
- Modify: `src-tauri/src/services/executions/mod.rs` — re-export.

- [ ] **Step 1: Write the failing test for "skip when disconnected"**

Append to `tests.rs`:

```rust
use super::ingest::ExecutionsIngestor;
use crate::ibkr::mocks::MockIbkrClient;
use std::sync::Arc;

#[tokio::test]
async fn ingestor_skips_when_ibkr_disconnected() {
    let (_tmp, db) = make_db();
    let store = Arc::new(ExecutionsStore::new(db));
    let mock = Arc::new(MockIbkrClient::new());
    mock.set_accounts(vec!["DU123".to_string()]).await;
    mock.set_connected(false).await;

    let ingestor = ExecutionsIngestor::new(Arc::clone(&store), mock.clone());
    // One tick should not panic, not error fatally, just log.
    ingestor.tick_once().await;

    let rows = store
        .query("DU123", NaiveDate::from_ymd_opt(2026, 5, 4).unwrap(), None)
        .await
        .unwrap();
    assert!(rows.is_empty());
}
```

- [ ] **Step 2: Run, verify it fails**

```bash
cd src-tauri && cargo test services::executions::tests::ingestor_skips_when_ibkr_disconnected 2>&1 | tail -10
```

Expected: FAIL — `ExecutionsIngestor` does not exist.

- [ ] **Step 3: Implement the ingestor**

Create `src-tauri/src/services/executions/ingest.rs`:

```rust
//! `ExecutionsIngestor` — background task that drains live IBKR fills
//! into the `executions` store every 5 min during market hours.
//!
//! Runs alongside the live `AccountReader::executions(today)` opportunistic
//! refresh; together they make the store eventually-consistent with IBKR
//! within the 5-min poll window.

use chrono::Utc;
use chrono_tz::America::New_York;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;

use crate::ibkr::client::IbkrClientTrait;
use super::store::ExecutionsStore;

const POLL_INTERVAL: Duration = Duration::from_secs(5 * 60);
const MARKET_OPEN_HOUR_ET: u32 = 4;   // pre-market starts 04:00 ET
const MARKET_CLOSE_HOUR_ET: u32 = 20; // after-hours ends 20:00 ET

pub struct ExecutionsIngestor<C: IbkrClientTrait + Send + Sync + 'static> {
    store: Arc<ExecutionsStore>,
    client: Arc<C>,
}

impl<C: IbkrClientTrait + Send + Sync + 'static> ExecutionsIngestor<C> {
    pub fn new(store: Arc<ExecutionsStore>, client: Arc<C>) -> Self {
        Self { store, client }
    }

    /// Spawn the long-lived loop. Returns immediately.
    pub fn spawn(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut tick = interval(POLL_INTERVAL);
            loop {
                tick.tick().await;
                self.tick_once().await;
            }
        });
    }

    /// One drain pass. Public for tests; otherwise called from `spawn`.
    pub async fn tick_once(&self) {
        if !in_market_hours_et() {
            tracing::debug!("executions ingestor idle (outside market hours)");
            return;
        }
        let accounts = match self.client.managed_accounts().await {
            Ok(a) => a,
            Err(e) => {
                tracing::debug!("executions ingestor: managed_accounts: {e}");
                return;
            }
        };
        let today_et = Utc::now().with_timezone(&New_York).date_naive();
        for account in accounts {
            match self.client.executions(&account, today_et).await {
                Ok(rows) => {
                    if !rows.is_empty() {
                        if let Err(e) = self.store.record(&rows).await {
                            tracing::warn!(
                                account = %account,
                                error = %e,
                                "executions ingestor: store.record failed"
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::debug!(account = %account, "executions ingestor: drain: {e}");
                }
            }
        }
    }
}

fn in_market_hours_et() -> bool {
    let now_et = Utc::now().with_timezone(&New_York);
    let h = now_et.hour();
    h >= MARKET_OPEN_HOUR_ET && h < MARKET_CLOSE_HOUR_ET
}
```

Update `mod.rs`:

```rust
pub mod store;
pub mod ingest;

pub use store::{ExecutionsStore, RecordSummary};
pub use ingest::ExecutionsIngestor;

#[cfg(test)]
mod tests;
```

> **`IbkrClientTrait`:** confirm the trait method names match — the existing `MockIbkrClient` test uses `set_connected` and presumably `executions(account, date)`. Check `src-tauri/src/ibkr/mocks.rs` for the exact trait surface and adjust the call sites accordingly.

- [ ] **Step 4: Run the test to verify it passes**

```bash
cd src-tauri && cargo test services::executions::tests::ingestor_skips_when_ibkr_disconnected
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/executions/
git commit -m "feat(executions): ExecutionsIngestor background drain worker"
```

### Task 8: AccountReader — read from store for past days

**Files:**
- Modify: `src-tauri/src/mcp/ibkr_seam.rs` — extend the production impl of `AccountReader::executions`.

- [ ] **Step 1: Read the current production impl**

```bash
grep -n "fn executions" src-tauri/src/mcp/ibkr_seam.rs
```

Note the current body — it forwards to `IbkrClient::executions(account, date)` directly. We're going to layer the store in front of it.

- [ ] **Step 2: Write the failing test** (in `src-tauri/src/mcp/ibkr_seam.rs::tests` or a new file `tests/account_reader_executions.rs`)

```rust
#[tokio::test]
async fn account_reader_serves_past_days_from_store() {
    let (_tmp, db) = make_db();
    let store = Arc::new(ExecutionsStore::new(Arc::clone(&db)));
    // Seed the store with a fill on 2026-05-03.
    let mut prior = stk("PRIOR", "DU123", 100.0, 250.0);
    prior.exec_time = Utc.with_ymd_and_hms(2026, 5, 3, 14, 30, 0).unwrap();
    store.record(&[prior]).await.unwrap();

    let mock = Arc::new(MockIbkrClient::new());
    mock.set_accounts(vec!["DU123".into()]).await;
    // The mock has NO fills loaded — if AccountReader hits live IBKR for
    // a past day, the result will be empty.
    let reader = ProdAccountReader::new(mock, Arc::clone(&store));

    let rows = reader
        .executions("DU123", NaiveDate::from_ymd_opt(2026, 5, 3).unwrap())
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].exec_id, "PRIOR");
}
```

- [ ] **Step 3: Run, verify it fails** (the impl doesn't yet consult the store)

- [ ] **Step 4: Modify the production `AccountReader::executions`**

```rust
async fn executions(
    &self,
    account: &str,
    date: NaiveDate,
) -> Result<Vec<IbkrExecution>, IbkrError> {
    let today_et = Utc::now().with_timezone(&New_York).date_naive();
    if date < today_et {
        // Past day: store is authoritative.
        let rows = self
            .store
            .query(account, date, None)
            .await
            .map_err(|e| IbkrError::Other(format!("executions store: {e}")))?;
        return Ok(rows);
    }
    if date > today_et {
        return Ok(Vec::new());
    }
    // Today: drain live IBKR, prime the store, return store rows.
    let live = self.client.executions(account, date).await?;
    if !live.is_empty() {
        if let Err(e) = self.store.record(&live).await {
            tracing::warn!(error = %e, "executions store record failed; serving live");
            return Ok(live);
        }
    }
    let rows = self
        .store
        .query(account, date, None)
        .await
        .map_err(|e| IbkrError::Other(format!("executions store: {e}")))?;
    Ok(rows)
}
```

The `ProdAccountReader` constructor must take an `Arc<ExecutionsStore>`; thread it through `lib.rs::run`.

- [ ] **Step 5: Run, verify it passes**

```bash
cd src-tauri && cargo test account_reader_serves_past_days_from_store
```

Expected: PASS.

- [ ] **Step 6: Verify ALL existing executions tests still pass (no regression)**

```bash
cd src-tauri && cargo test executions
cd src-tauri && cargo test mcp::tools::executions
```

Expected: all PASS.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/mcp/ibkr_seam.rs
git commit -m "feat(executions): AccountReader serves past days from store"
```

### Task 9: Wire the store + ingestor into the app

**Files:**
- Modify: `src-tauri/src/lib.rs` — construct `ExecutionsStore` after the DB is up; construct + spawn `ExecutionsIngestor`; pass the store into `ProdAccountReader`.

- [ ] **Step 1: Locate the existing service composition**

```bash
grep -n "fn run" src-tauri/src/lib.rs
grep -n "AccountReader\|ProdAccountReader\|mcp::ibkr_seam" src-tauri/src/lib.rs
```

- [ ] **Step 2: Add the wiring**

Inside `run()`, after the DB is constructed and before the MCP handler is built:

```rust
let executions_store = Arc::new(ExecutionsStore::new(Arc::clone(&db)));
let executions_ingestor = Arc::new(
    ExecutionsIngestor::new(Arc::clone(&executions_store), Arc::clone(&ibkr_client))
);
Arc::clone(&executions_ingestor).spawn();
```

Update the `ProdAccountReader` construction site to pass the store:

```rust
let account_reader = ProdAccountReader::new(Arc::clone(&ibkr_client), Arc::clone(&executions_store));
```

- [ ] **Step 3: cargo check + cargo clippy**

```bash
cd src-tauri && cargo check && cargo clippy --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 4: Run the full test suite**

```bash
cd src-tauri && cargo test
```

Expected: all PASS.

- [ ] **Step 5: Manual smoke test**

```bash
pnpm tauri dev   # let it run for 5+ min during US trading hours
# then in another shell:
sqlite3 "$(find ~/.local/share -name 'qk*.sqlite*' | head -1)" \
  "SELECT COUNT(*) FROM executions; SELECT exec_id, symbol, side, qty, commission FROM executions ORDER BY exec_time DESC LIMIT 5;"
```

Expected: row count > 0 if you placed any trades; commissions populated within 5 min of the fill.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(executions): wire store + ingestor into service composition"
```

## Exit criteria

- [ ] `V13__executions.sql` migration runs cleanly on a fresh DB and on an existing DB.
- [ ] All 7 unit tests in `services/executions/tests.rs` pass: idempotent UPSERT, late-commission patch, no-overwrite, ET-day boundary, account isolation, ingestor disconnect-skip, AccountReader serves past from store.
- [ ] `cargo clippy --all-targets -- -D warnings` clean.
- [ ] Manual: `pnpm tauri dev`, run for one trading session, `sqlite3 … 'SELECT COUNT(*) FROM executions'` > 0.
- [ ] Manual: from a Claude Code session the next morning, `mcp__quantum-kapital__get_executions(date='YYYY-MM-DD' /* yesterday */)` returns yesterday's fills (proving past-day persistence works end-to-end).
- [ ] Update `loop/plan/master.md` Phase 1 row to `done (commit <sha>, YYYY-MM-DD)`.
- [ ] Update this file's Status header to `done`.

## Gotchas

- **`right` is a SQL keyword** in some dialects; quote it as `"right"` in every CREATE / SELECT / INSERT / UPDATE statement.
- **Migration ordering.** Confirm `V13` is the next free integer; `V12__ticker_priming.sql` is the latest at the time of writing. If a parallel branch lands a `V13` first, bump to `V14`.
- **Connection pool type.** The code above assumes `crate::storage::Db` is the `Arc`-wrapped pool name. Check the actual import in a peer service (e.g. `services/agent_morning_packs.rs`) and substitute.
- **`tokio::task::spawn_blocking` for SQLite.** SQLite operations are sync; wrap them in `spawn_blocking` to avoid blocking the tokio reactor. The peer services do this — mirror.
- **DST + ET-day query.** `et_day_bounds_utc` uses `chrono_tz::America::New_York`; this handles spring-forward / fall-back automatically, but a unit test that includes a fall-back day (Nov 1, 2026 → ambiguous local time) is worth adding if dogfooding ever surfaces a row missing from a query. v1 doesn't include it because the half-open `[00:00 ET, next-00:00 ET)` range works correctly through DST transitions in chrono_tz.
- **Concurrent writers.** SQLite is single-writer; the ingestor and the live `AccountReader::executions(today)` both call `record()`. Both go through the connection pool; UPSERT is atomic per row inside its transaction. Don't hold transactions across multiple `record()` calls.
- **`ProdAccountReader` API change.** Adding a `store` parameter to its constructor is a breaking change for any test that builds it directly. Update the test fakes (`mcp::tools::test_support`) to either build a fake store or to construct `ProdAccountReader` with the store. The simpler fix: leave the test fake `AccountReader` impl untouched (it bypasses the store entirely) and only add the store to the production impl.
- **Ingest cadence.** 5 min is a reasonable default; if commission lateness exceeds it during dogfooding, drop to 2 min — the IBKR API is fast enough. Don't go below 60 s without rate-limiter review.
- **Forward-only history.** First time the user opens the future Trades tab on yesterday's date, expect "no fills recorded" if the app wasn't running yesterday. The empty-state copy in Phase 7 should distinguish "you didn't trade" from "the store didn't exist yet."
- **`exec_id` collisions.** IBKR's `execution_id` is globally unique within the broker's lifetime. UPSERT-by-PK handles reconnect/replay duplicates correctly.
