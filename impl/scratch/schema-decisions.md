# Schema decisions scratchpad

Running log of SQLite schema choices for the Tracker subsystem. Append entries chronologically. Newest at the top.

Use this when:
- A phase adds a column or table — record what and why.
- A phase considered an index but decided against it — record the reasoning.
- Alpha Vantage / IBKR returns surprised the implementer about a field's shape — capture the decision (e.g., "stored as TEXT JSON because of variable shape").

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
