# Schema decisions scratchpad

Running log of SQLite schema choices for the Tracker subsystem. Append entries chronologically. Newest at the top.

Use this when:
- A phase adds a column or table — record what and why.
- A phase considered an index but decided against it — record the reasoning.
- Alpha Vantage / IBKR returns surprised the implementer about a field's shape — capture the decision (e.g., "stored as TEXT JSON because of variable shape").

---

### 2026-04-29 — Phase 20 — Morning pack persistence

**Change:** Added new table `morning_packs (date TEXT PRIMARY KEY, payload TEXT NOT NULL, generated_at INTEGER NOT NULL)`. Stores the full ranked `MorningPack` JSON keyed by ET trading day. `INSERT ... ON CONFLICT(date) DO UPDATE` ensures the latest run wins, so a user can re-run the EOD sweep and get a fresh pack without polluting history.
**Why a separate table (vs. a sibling column on `setups`):** The pack is a per-day artifact, not per-setup. Multiple setups participate in one pack and one setup never appears in two packs (same trading day). A keyed-by-date table is the natural shape and lets the `tracker_get_morning_pack` command serve `Option<MorningPack>` with a single PK lookup.
**Why JSON payload:** The `RankedSetup { setup_id, rank, why_top_pick }` shape is bounded and rarely queried by sub-field — frontend always reads the whole pack. Storing a normalized `morning_pack_entries` join table would force a 5-row materialization per render with no upside.
**Migration impact:** additive. The table uses `CREATE TABLE IF NOT EXISTS` in `schema.sql` so the existing migration runner picks it up on next launch — no `add_column_if_missing` needed.
**Cross-references:** `src-tauri/src/storage/schema.sql`, `src-tauri/src/services/daily_ranker/{mod,tests}.rs` (8 unit tests, all green), `src-tauri/src/services/eod_scheduler/mod.rs` (calls `rank_today` after `expire_ttls`), `src-tauri/src/ibkr/commands/tracker.rs::tracker_get_morning_pack`.

---

### 2026-04-29 — Phase 19 — News interpreter verdict persistence

**Change:** Added `news_cache.news_verdict_json TEXT` (nullable). Stores the full `NewsVerdict` struct (`tone`, `ep_worthy`, `parabolic_risk`, `summary`) as a serialized JSON object on the same row as the cached news payload.
**Why a sibling column on `news_cache`:** keeps verdict + raw items co-located and write-coherent — a fresh news fetch and its interpretation belong in one row keyed by symbol. Avoids a second per-symbol table that would always be joined back. Verdict-less rows (LLM disabled, budget exhausted, fresh row before interpreter has run) simply leave the column NULL; the EP detector falls back to AV's per-ticker sentiment in that case.
**Migration impact:** additive. `schema.sql` updated for fresh DBs; `migrations.rs` runs an idempotent `add_column_if_missing(&tx, "news_cache", "news_verdict_json", "TEXT")` so existing `tracker.sqlite` files pick up the column on next launch. No data backfill needed — verdicts populate on the next news refresh.
**Cross-references:** `src-tauri/src/storage/{schema.sql, migrations.rs}`, `src-tauri/src/services/news_interpreter/{mod,tests}.rs`, `src-tauri/src/services/financial_data_service/news.rs` (read/write helpers extended with verdict column), `src-tauri/src/strategies/episodic_pivot/detector.rs` (consumes verdict tone when present).

---

### 2026-04-29 — Phase 17 — Thesis generator persistence

**Change:** Added `setups.thesis_json TEXT` (nullable) to hold the full structured `Thesis` (markdown + conviction + invalidation_levels[] + risk_notes) as a serialized JSON object. Markdown still lives in `setups.thesis` for backwards-compatibility and for the existing `Setup.thesis: Option<String>` wire surface.
**Why two columns:** Keeping `thesis` as the markdown-only convenience preserves the existing `SetupDetected.thesis: Option<String>` event payload (frontend reads it directly to fill toast / row preview) without forcing a JSON parse on every render. `thesis_json` carries the fuller LLM output (conviction grade, multi-level invalidation list, risk flags) for components that want the structured data — Phase 21's AlertFeed and Phase 20's daily ranker would otherwise need a sibling table.
**Migration impact:** additive. `schema.sql` updated for fresh DBs; `migrations.rs` runs an idempotent `add_column_if_missing(&tx, "setups", "thesis_json", "TEXT")` so existing `tracker.sqlite` files pick up the column on next launch. No data backfill needed — existing rows stay `NULL` until the next runner pass regenerates the thesis.
**Cross-references:** `src-tauri/src/storage/{schema.sql, migrations.rs}`, `src-tauri/src/ibkr/types/tracker.rs::Setup` (added `thesis_json: Option<serde_json::Value>`), `src-tauri/src/services/tracker_service/mod.rs::update_setup_thesis`, `src-tauri/src/services/thesis_generator/{mod,tests}.rs` (8 unit tests + 1 runner integration test, all green).

---

### 2026-04-29 — Phase 12 — Tracker status state machine

**Change:** Added `tracked_tickers.cool_down_until INTEGER` (nullable). Stored separately from `in_play_until` rather than reusing the column — different semantics (cool-down rules out re-entry, in-play accelerates intraday checks) and easier queries (`expire_ttls` checks both with a single `OR` filter).
**Why separate column:** A single `ttl_until` would force every read to also know which state we're in to interpret it; with two columns the SQL is self-describing and the state machine's reset path can `SET in_play_until = NULL, cool_down_until = NULL` unconditionally.
**Migration impact:** additive. `schema.sql` updated for fresh DBs; `migrations.rs` runs an idempotent `add_column_if_missing` (inspects `PRAGMA table_info`) so existing `tracker.sqlite` files pick up the column on next launch.
**Cross-references:** `src-tauri/src/storage/schema.sql`, `src-tauri/src/storage/migrations.rs`, `src-tauri/src/services/tracker_state_machine/{mod,tests}.rs` (12 tests, all green).

---

### 2026-04-29 — Phase 02 — Historical bars service landed

**Change:** `bars_cache` writes/reads now go through `services::historical_data_service::HistoricalDataService`. No schema changes — composite PK `(symbol, bar_size, bar_time)` proved sufficient. Service uses `INSERT OR REPLACE` for idempotent writes.
**Why no separate index:** The dominant access pattern is `WHERE symbol=? AND bar_size=? AND bar_time BETWEEN ? AND ? ORDER BY bar_time ASC`, which the composite PK already covers. A SQLite primary key on a non-INTEGER table is itself a B-tree index — adding `(symbol, bar_size, bar_time DESC)` would be redundant for ascending scans. Re-evaluate if scanners ever need the most-recent N bars cheaply.
**Migration impact:** none.
**Cross-references:** `src-tauri/src/services/historical_data_service/mod.rs`, `src-tauri/src/services/historical_data_service/tests.rs` (9 tests, all green), `src-tauri/src/middleware/historical_rate_limit.rs`.

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
