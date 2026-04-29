# Schema decisions scratchpad

Running log of SQLite schema choices for the Tracker subsystem. Append entries chronologically. Newest at the top.

Use this when:
- A phase adds a column or table — record what and why.
- A phase considered an index but decided against it — record the reasoning.
- Alpha Vantage / IBKR returns surprised the implementer about a field's shape — capture the decision (e.g., "stored as TEXT JSON because of variable shape").

---

### 2026-04-29 — Phase 01 — SQLite foundation landed

**Change:** `src-tauri/src/storage/` (Db + r2d2 pool + embedded `schema.sql` + migrations runner). All six baseline tables + two indexes created up front. PRAGMAs set: `journal_mode=WAL`, `foreign_keys=ON`, `synchronous=NORMAL`, applied via `SqliteConnectionManager::with_init` so every pooled connection enforces them (per-connection, not just one-shot).
**Why WAL:** Better concurrent reader/writer behavior for the tracker (intraday writer + UI reader). `synchronous=NORMAL` is the recommended pair with WAL — durable enough for surveillance data, fewer fsyncs than FULL.
**Why per-connection PRAGMA init:** `foreign_keys` is connection-local in SQLite; setting it once on the migrating connection is not enough for pool checkouts. `with_init` makes it idempotent and ubiquitous.
**Deviations from baseline:** none. Schema copied verbatim from design doc (lines 117–189).
**Deferred indexes:** still none on `news_cache.fetched_at` or `llm_calls.called_at` — Phase 03 / Phase 16 to decide.
**Migration impact:** none (additive, first-time creation).
**Cross-references:** `src-tauri/src/storage/schema.sql`, `src-tauri/src/storage/mod.rs`, `src-tauri/src/storage/tests.rs` (9 tests, all green).

---

## Template for new entries

```
### YYYY-MM-DD — Phase NN — <one-line summary>

**Change:** ...
**Why:** ...
**Migration impact:** none / additive / destructive (if destructive, escalate)
**Cross-references:** related code paths, tests, prompt versions
```

---

## Initial schema baseline (Phase 01)

The baseline schema covers all phases up front; later phases add data, not tables. Tables: `tracked_tickers`, `setups`, `alerts`, `bars_cache`, `news_cache`, `llm_calls`. See `src-tauri/src/storage/schema.sql` after Phase 01 lands.

Indexes baked in at Phase 01:
- `idx_setups_symbol`
- `idx_setups_status_detected`
- `bars_cache` PRIMARY KEY `(symbol, bar_size, bar_time)` — covers the dominant access pattern (range scan for one symbol+timeframe)

Open questions for later phases to resolve:
- Whether `news_cache` needs an `idx_news_fetched_at` for backfill queries (Phase 03 will decide).
- Whether `llm_calls.cost_usd` should be `INTEGER` (cents × 1000) for exact arithmetic vs `REAL` for ergonomics (Phase 16).
- WAL mode setting and `PRAGMA journal_mode` choice (Phase 01 will decide and log here).
