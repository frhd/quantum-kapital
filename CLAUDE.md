# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview
Quantum Kapital is a cross-platform algorithmic trading application built with Tauri (Rust) and React (TypeScript) that integrates with Interactive Brokers (IBKR) API for portfolio management and automated trading.

## Key Development Commands

### Frontend Development
```bash
# Install dependencies
pnpm install

# Run development server (Vite + Tauri)
pnpm tauri dev

# Build for production
pnpm tauri build

# Run frontend only (without Tauri)
pnpm dev

# Build frontend only
pnpm build
```

### Rust/Tauri Development
```bash
# Check Rust code
cargo check --manifest-path src-tauri/Cargo.toml

# Run tests
cargo test --manifest-path src-tauri/Cargo.toml

# Run specific IBKR tests
cargo test --manifest-path src-tauri/Cargo.toml ibkr::

# Run a single test by name
cargo test --manifest-path src-tauri/Cargo.toml test_name_here

# Run tests in a specific module
cargo test --manifest-path src-tauri/Cargo.toml ibkr::tests::client_tests

# Format Rust code
cargo fmt --manifest-path src-tauri/Cargo.toml

# Lint Rust code
cargo clippy --manifest-path src-tauri/Cargo.toml
```

### Environment Setup
```bash
# Optional: Configure Alpha Vantage API for real fundamental data
cd src-tauri
cp .env.example .env
# Edit .env and add your API key: ALPHA_VANTAGE_API_KEY=your_key_here
```

## Architecture

### Frontend Architecture
The React frontend (`/src`) uses a modular structure:
- **Component Library**: shadcn/ui-style components in `src/shared/components/ui/` (Alert, Badge, Button, Card, Input, Label, Skeleton, Table, Tabs)
- **State Management**: React hooks with Tauri command invocation via `@tauri-apps/api`
- **Styling**: Tailwind CSS with custom gradient themes
- **TypeScript**: Strict mode with path mapping (`@/*` → `./src/*`)
- **Icons**: Lucide React
- **Structure**:
  - `app/`: Main application entry and layout
  - `features/`: Feature-based modules (connection, portfolio, analysis, scanner)
  - `shared/`: Reusable components, utilities, hooks, types, and API layer

### Backend Architecture
The Rust backend (`/src-tauri/src`) follows a layered architecture:
- **Core Modules**:
  - `ibkr/`: IBKR API integration layer
    - `client.rs`: IBKR TWS/Gateway connection using `ibapi` crate
    - `commands/`: Tauri command handlers modularized by domain
      - `connection.rs`: Connection management commands
      - `accounts.rs`: Account-related commands (including daily P&L stream lifecycle)
      - `market_data.rs`: Market data subscription commands
      - `trading.rs`: Order placement commands
      - `analysis.rs`: Fundamental data and projection commands
      - `scanner.rs`: Market scanner stream lifecycle commands
      - `tracker.rs`: Tracker subsystem commands (Phase 02 added `tracker_fetch_bars`; Phase 03 added `tracker_get_news`; Phase 04 added watchlist CRUD: `tracker_add` / `tracker_remove` / `tracker_list` / `tracker_get` / `tracker_set_tags` / `tracker_set_status`; Phase 10 added `tracker_run_now` and `tracker_get_setups`; Phase 13 added `tracker_start_scheduler` / `tracker_stop_scheduler`)
    - `types/`: Type definitions modularized by domain (account, connection, fundamentals, historical, market_data, news, orders, positions, scanner, tracker)
    - `state.rs`: Application state management with Tokio async runtime
    - `error.rs`: Custom error types with thiserror
    - `mocks.rs`: MockIbkrClient for test-driven development
    - `tests/`: Comprehensive test modules (`api_interface_tests.rs`, `client_tests.rs`, `command_tests.rs`, `integration_tests.rs`)
  - `services/`: Business logic layer
    - `account_service.rs`: Account management operations
    - `market_service.rs`: Market data operations
    - `trading_service.rs`: Trading operations
    - `financial_data_service.rs` + `financial_data_service/news.rs`: Alpha Vantage fundamental data integration. Phase 03 added a `news` submodule for `NEWS_SENTIMENT` with SQLite-backed cache (`news_cache` table, 60-min default TTL), HTTP transport seam (`NewsHttp` trait blanket-impl'd by `ReqwestNewsHttp`), and injectable `NewsClock` for deterministic tests. Service is best-effort: rate-limit `Note`/`Information` responses, transport failures, or missing API key fall back to the cached payload (or empty `Vec`) and only log a `warn!` — never propagate as an error. Filters items to the requested symbol via `ticker_sentiment[].ticker`. `FinancialDataService::fetch_news_sentiment` requires `with_db(Arc<Db>)` to be set first.
    - `projection_service.rs`: Forward-looking financial projection logic
    - `cache_service.rs`: In-memory caching for fundamentals/projections
    - `tracker_service/`: Watchlist persistence over the `tracked_tickers` table (added Phase 04; extended Phase 12). `TrackerService::new(db: Arc<Db>)`. CRUD surface: `add` (returns `TrackerError::AlreadyTracked` on PK conflict), `remove` (idempotent), `list(status_filter)`, `get`, `set_tags`, `set_status(symbol, status, in_play_until, cool_down_until)` (Phase 12 widened the signature to take `cool_down_until` — passing `None` clears it), `touch_last_checked`. Phase 12 added `count_active_setups(symbol) -> usize` and `update_setup_status(id, status, reason?, invalidated_at?) -> Setup` so the state machine can drive setup-row lifecycle without leaking SQL through the runner. Symbols are normalized to uppercase. `tags` and `source_meta` round-trip as JSON columns; status is stored as a snake_case string but transitions are NOT enforced here — `tracker_state_machine/` owns the rules. Phase 10 added setup-row CRUD over the `setups` table: `insert_setup(symbol, &SetupCandidate) -> Setup`, `list_setups(symbol?, since?) -> Vec<Setup>` (orders `detected_at DESC, id DESC`), `get_setup(id) -> Option<Setup>`, and `recent_duplicate(symbol, strategy, direction, within: Duration) -> Option<i64>` for the runner's dedup check. `direction` columns are persisted as the lowercase strings `"long"` / `"short"`; setup status defaults to `"active"` and decodes through `SetupStatus::parse`.
    - `tracker_runner/` (Phase 10): `TrackerRunner` glues bars + news + the detector registry together for a single command-callable surface. Constructor `TrackerRunner::new(db, tracker, state_machine: Arc<TrackerStateMachine>, bars: Arc<dyn BarsFetcher>, news: Arc<dyn NewsFetcher>, registry: Arc<DetectorRegistry>)`. `BarsFetcher` is blanket-impl'd for `HistoricalDataService`; `NewsFetcher` is blanket-impl'd for `FinancialDataService` (the news arm collapses errors to an empty `Vec` with a `warn!` so a missing API key never propagates). Public API: `context_for(symbol) -> OwnedMarketContext` (mandatory daily 200-day fetch; intraday Min15 for today and 24h news are best-effort; fundamentals + live quote intentionally `None` since no current detector reads them); `run_for(symbol) -> Vec<Setup>` (dispatches `registry.evaluate_all` against the borrowed `MarketContext` from `OwnedMarketContext::as_borrowed`, persists hits with 24h dedup keyed on `(symbol, strategy, direction)`, calls `state_machine.on_setup_detected` after each successful persist (Phase 12 — flips the ticker into `SetupActive` and extends `in_play_until`), touches `last_checked_at` on success, logs detector / persistence / state-machine errors as `warn!`); `run_all() -> Vec<RunResult>` iterates the watchlist excluding `CoolDown` rows and surfaces per-symbol failures inside individual `RunResult { symbol, setups, error }` entries instead of short-circuiting. `DUPLICATE_WINDOW = 24h`, `DAILY_LOOKBACK_DAYS = 200`, `INTRADAY_BAR_SIZE = Min15`. Wired in `lib.rs::run` as `Arc<TrackerRunner>` alongside a state-managed `Arc<FinancialDataService>` (constructed once with the env-supplied `ALPHA_VANTAGE_API_KEY` + shared `Arc<Db>`).
    - `tracker_state_machine/` (Phase 12): `TrackerStateMachine` codifies the watchlist lifecycle (`Watching → InPlay → SetupActive → CoolDown → Watching`) without enforcing it inside `TrackerService` (CRUD stays dumb). Constructor `TrackerStateMachine::new(db, tracker)` uses a real wall clock; `with_clock(db, tracker, Clock::Fixed(now))` is gated on `#[cfg(test)]` for deterministic trading-day math. Public API: `record_scanner_hit(symbol, meta?)` and `record_manual_flag(symbol)` promote `Watching|InPlay → InPlay` with `in_play_until = trading_days_after_close(now, IN_PLAY_TRADING_DAYS=3)`, no-op on hotter states (`SetupActive` / `CoolDown`), and fold `meta` into `source_meta` when provided; `on_setup_detected(symbol, setup_id)` promotes any non-`CoolDown` row to `SetupActive` and re-stamps `in_play_until` (called by `TrackerRunner` after every persisted hit); `mark_invalidated(setup_id, reason)` and `mark_completed(setup_id)` update the `setups` row's `status`/`invalidation_reason`/`invalidated_at` and only flip the ticker to `CoolDown` (with `cool_down_until = trading_days_after_close(now, COOL_DOWN_TRADING_DAYS=5)`) once `count_active_setups(symbol) == 0` — so a ticker with two live setups stays `SetupActive` until the *second* invalidation; `expire_ttls(now) -> usize` runs a single atomic SQL update flipping any row whose `in_play_until` or `cool_down_until` is `<= now` back to `Watching` (and clears both columns), idempotent across repeat calls; `active_in_play_symbols() -> Vec<String>` returns `status IN ('in_play', 'setup_active')` for Phase 14's intraday scheduler. `StateMachineError::SetupNotFound(id)` is mapped from the underlying `TrackerError::NotFound`. Module carries `#![allow(dead_code)]` since `record_manual_flag` / `mark_*` / `expire_ttls` / `active_in_play_symbols` only have production callers in Phase 13 (EOD scheduler), Phase 14 (intraday scheduler), and Phase 18 (LLM decay-watcher); 12 unit tests in `tests.rs` (controlled `Clock::Fixed`, `tempfile`-backed DB) exercise every transition including the trading-days-not-calendar-days TTL semantics. Wired into `IbkrState::state_machine` and consumed by `TrackerRunner` (post-persist hook) and the `tracker_add` command (auto-promotes scanner-sourced rows to `InPlay`).
    - `eod_scheduler/` (Phase 13): `EodScheduler` is the long-running background task that wakes once a minute, checks whether the wall clock is inside a 5-minute window starting at 16:05 ET on a US equity trading day (weekend + holiday aware via `utils::market_calendar::is_holiday`), and — exactly once per trading day — calls `runner.run_all()`, then `state_machine.expire_ttls(now)`, then emits `AppEvent::MorningPackReady { date }`. Constructor `EodScheduler::new(runner, state_machine, emitter)` uses a real wall clock; `with_clock(..., Clock::Fixed(now))` is gated on `#[cfg(test)]` for deterministic window math. Public API: `tick() -> Result<Option<EodTickOutcome>, String>` runs one scheduling pass synchronously (returns `Ok(None)` on no-op, `Ok(Some({ date, run_results, expired }))` on a real run); `spawn(self: Arc<Self>) -> StreamHandle` kicks off the tokio loop using `tokio::time::interval(60s)` and returns a handle suitable for `IbkrState::eod_handle`. The window is `[16:05, 16:10)` ET; `last_run_date` (`Arc<RwLock<Option<NaiveDate>>>`) guards against double-firing inside the window. `MorningPackReady` carries an empty `date: String` (ISO-8601) — Phase 20's daily ranker fills the payload. Wired in `lib.rs::run` as `app.manage(Arc<EodScheduler>)` (auto-start is intentionally off — opt-in via the Tauri command pair). 8 unit tests in `tests.rs` cover the window/weekend/holiday/dedup branches plus IbkrState handle replacement and drop.
    - `historical_data_service/`: Historical bars fetcher with SQLite cache (added Phase 02)
      - `mod.rs`: `HistoricalDataService` with cache-first reads, write-through, in-flight dedup via `tokio::sync::Mutex<HashMap<key, Arc<Mutex<()>>>>`, partial-range gap fetch for daily bars, intraday cache invalidation at session rollover. Exposes `HistoricalDataFetcher` trait (blanket-impl'd by `IbkrClient`) + injectable `Clock` for tests.
      - `tests.rs`: 9 unit tests covering cache hit/miss, partial-range fetch, daily-vs-intraday TTL, rate-limiter accounting, dedup, and bit-equal SQLite round-trip
      - `Lookback` enum: `Days(u32)` for daily bars, `TradingDay(NaiveDate)` for intraday
  - `middleware/`: Cross-cutting concerns
    - `rate_limit.rs`: API rate limiting (default 50 req/sec; tracing is initialized in `lib.rs::run`)
    - `historical_rate_limit.rs`: Sliding 60-second window for IBKR historical-data calls (default 6 req/min); separate from the 50 req/sec general limiter
  - `events/`: Event system
    - `emitter.rs`: Event emitter for frontend notifications. `AppEvent::MorningPackReady { date: String }` was added in Phase 13 and is emitted by the EOD scheduler at 16:05 ET on each trading day. The `date` is an ISO-8601 ET trading-day stamp; the payload is empty for now and Phase 20's daily ranker will fill it in.
  - `config/`: Application configuration
    - `settings.rs`: Configuration management
  - `storage/`: SQLite layer for the Tracker subsystem (added Phase 01)
    - `mod.rs`: `Db` (r2d2 pool wrapper) + async `with_conn` helper around `tokio::task::spawn_blocking`
    - `schema.sql`: Embedded baseline schema (`tracked_tickers`, `setups`, `alerts`, `bars_cache`, `news_cache`, `llm_calls` + `idx_setups_symbol`, `idx_setups_status_detected`)
    - `migrations.rs`: Idempotent `CREATE TABLE IF NOT EXISTS` runner invoked at startup
    - `error.rs`: `StorageError` (`Sqlite`, `Pool`, `Migration`, `Serde`, `Join`)
    - PRAGMAs (`journal_mode=WAL`, `foreign_keys=ON`, `synchronous=NORMAL`) applied per pooled connection via `SqliteConnectionManager::with_init`
    - DB lives at `app_local_data_dir()/tracker.sqlite`; `Arc<Db>` is both `app.manage`d in `lib.rs::run` and held on `IbkrState` (Phase 04 wired `IbkrState::db` + `IbkrState::tracker: Arc<TrackerService>`; Phase 12 added `IbkrState::state_machine: Arc<TrackerStateMachine>`, constructed in `IbkrState::new`; Phase 13 added `IbkrState::eod_handle: Arc<RwLock<Option<StreamHandle>>>` plus `start_eod_scheduler` / `stop_eod_scheduler` methods, mirroring the scanner / daily-P&L stream pattern)
    - `tracked_tickers.cool_down_until INTEGER` column added in Phase 12 — separate from `in_play_until` (different semantics: cool-down rules out re-entry, in-play accelerates intraday checks). `migrations.rs` runs an idempotent `add_column_if_missing` (inspects `PRAGMA table_info`) so existing `tracker.sqlite` files pick up the column on next launch; `schema.sql` includes it for fresh DBs.
    - `bars_cache` (Phase 02) is read/written exclusively through `HistoricalDataService` — composite PK `(symbol, bar_size, bar_time)` is the only index; writes use `INSERT OR REPLACE` for idempotency
  - `strategies/`: Strategy detector framework (added Phase 06). Pure types + trait + registry; production detectors registered in Phase 07 (breakout), Phase 08 (episodic pivot), Phase 09 (parabolic short); Phase 10 wired the registry into `TrackerRunner` via `default_registry()` so runs persist to the `setups` table.
    - `trait_def.rs`: `StrategyDetector` async trait (`Send + Sync`) with `name`, `tag`, `timeframe`, `min_lookback_days`, `evaluate(&MarketContext) -> Result<Option<SetupCandidate>, DetectorError>`. `DetectorError` (thiserror): `InsufficientBars { needed, available }`, `IntradayBarsRequired` (added Phase 08), `InvalidInput`, `Internal`.
    - `context.rs`: `MarketContext<'a>` envelope holding `&[HistoricalBar]` (daily + optional intraday), `Option<&FundamentalData>`, `&[NewsItem]`, `Option<&MarketDataSnapshot>`, `now: DateTime<Utc>`. Borrows everything — caller owns the data.
    - `candidate.rs`: `SetupCandidate`, `Direction { Long, Short }` (snake_case serde), `TargetLevel { label, price }`, and `targets_for_risk_profile(direction, trigger, stop) -> Result<Vec<TargetLevel>, &'static str>` helper that emits 2R/3R targets (errors on `trigger == stop`).
    - `registry.rs`: `DetectorRegistry` stores `Vec<Arc<dyn StrategyDetector>>`. `evaluate_all(ctx)` and `evaluate_for_tags(ctx, &[StrategyTag])` run detectors sequentially in registration order, returning `Vec<DetectorOutcome>` (each holds detector name + `Result`) — errors are collected, never short-circuit. Phase 10 wires `default_registry()` into `TrackerRunner` so a single Tauri call evaluates every registered detector against fresh bars / news.
    - `default_registry()` (in `strategies/mod.rs`, added Phase 07; expanded Phase 08 and Phase 09): seeds a registry with all production detectors (currently `BreakoutDetector`, `EpisodicPivotDetector`, `ParabolicShortDetector`); registration order is the canonical evaluation order.
    - `indicators.rs` (Phase 07): pure helpers `atr(bars, period)`, `rsi(closes, period)`, `swing_low(bars, period)`, `swing_high(bars, period)`. Wilder smoothing seeded with SMA of the first `period` samples; out-of-range inputs return `None`. Flat-input RSI is `Some(50.0)` by convention; up-only is `Some(100.0)`.
    - `breakout/` (Phase 07): long-only `BreakoutDetector` (`min_lookback_days = 30`). Fires when today's close ≥ 20-day prior-high close, volume ≥ 1.5× the 20-day prior-window average, and RSI(14) `< 80`. Stop = `max(swing_low_10, trigger − 1×ATR(14))`. Targets are 2R/3R via `targets_for_risk_profile`. Conviction is a logistic of the volume multiple (`k = 1.2`, midpoint at 1.5×). Returns `Ok(None)` (not error) on degenerate flat-line bars where risk distance would be zero. `raw_signals` JSON exposes `lookback_high`, `volume_multiple`, `atr_14`, `swing_low_10`, `rsi_14`.
    - `episodic_pivot/` (Phase 08): bidirectional `EpisodicPivotDetector` (`timeframe = Min15`, `min_lookback_days = 5`). Requires `intraday_bars`; raises `DetectorError::IntradayBarsRequired` when missing. Computes gap = `(today.open − yesterday.close) / yesterday.close` against the daily series; gates on `|gap| ≥ 4%`. Picks sentiment from the news item with the highest `ticker_sentiment.relevance_score` for the symbol; gates on `|score| ≥ 0.15`. Direction: gap-up + bullish → `Long`; gap-down + bearish → `Short` (continuation); gap-up + bearish → `Short` (fade); gap-down + bullish → no setup. Volume confirmation: sum of first 30 minutes of intraday volume (first 2 Min15 bars) must be `≥ yesterday.volume`. Trigger = today's RTH open. Stops: long → `yesterday.close` (pre-gap); short → highest intraday high seen so far. Targets via `targets_for_risk_profile` (2R / 3R). Conviction = `0.4·norm(|gap|, 0.04..0.10) + 0.4·norm(|sent|, 0.15..0.50) + 0.2·norm(vol_ratio, 1..3)`, clamped `[0, 1]`. `raw_signals` JSON: `gap_pct`, `sentiment_score`, `volume_ratio`, `first_30min_volume`, `prior_day_volume`.
    - `parabolic_short/` (Phase 09): short-only `ParabolicShortDetector` (`timeframe = Min15`, `min_lookback_days = 25`; internal gate is 21 bars — what ATR(20) actually requires). Requires `intraday_bars`; raises `DetectorError::IntradayBarsRequired` when missing. Daily-side gates: ≥ 3 strict-up consecutive days walking back from the latest bar; min per-day move across the streak `≥ 5%`; cumulative move (today.close vs the close just before the streak) `≥ 40%`; `(close − ma_20) / atr_20 ≥ 2.0` (MA(20) is a simple mean of the last 20 closes; ATR(20) uses Wilder smoothing); RSI(14) `≥ 80`. Trigger = close of the first 15-min intraday bar where `close < open`. Stop = `max(high)` of today's intraday bars so far (session high). Targets are 2R/3R via `targets_for_risk_profile(Short, …)`. Conviction = `0.3·norm(consec, 3..6) + 0.3·norm(cumul, 0.40..0.80) + 0.2·norm(atr_dist, 2..4) + 0.2·norm(rsi, 80..95)`, clamped `[0, 1]`. Returns `Ok(None)` (not error) on degenerate inputs (`prior_close ≤ 0`, `atr_20 == 0`, or stop `≤` trigger). `raw_signals` JSON: `consec_days`, `cumulative_move`, `atr_distance`, `rsi_14`, `min_per_day_move`, `ma_20`, `atr_20`.
    - `tests.rs`: 5 unit tests cover registry ordering, tag filtering, error collection, and target math; `breakout/tests.rs` adds 10 table-driven detector tests; `episodic_pivot/tests.rs` adds 12 table-driven detector tests; `parabolic_short/tests.rs` adds 11 table-driven detector tests; `indicators.rs` carries 11 inline tests including the Wilder-1978 RSI reference fixture.
    - Module-level `#![allow(dead_code, unused_imports)]` is intentional: the framework's public surface is consumed by Phase 07–09 detectors and Phase 13/14 schedulers.
  - `utils/`: Shared utilities
    - `market_calendar/` (Phase 11; expanded Phase 12): US equity market calendar helpers — `is_rth_open(now: DateTime<Utc>)`, `is_holiday(NaiveDate)`, `next_open_at(now)`, `next_close_at(now)`, `eod_sweep_target(NaiveDate) -> 16:05 ET`, `trading_days_after(NaiveDate, n: u32) -> NaiveDate` (skips weekends + holidays; `n = 0` returns the input unchanged), and `trading_days_after_close(now: DateTime<Utc>, n: u32) -> DateTime<Utc>` (anchored to the ET date of `now`, returns 16:00 ET on the target date as UTC — used by the Phase 12 state machine for `in_play_until` / `cool_down_until` stamping). ET is hardcoded as `FixedOffset::west_opt(5 * 3600)` (EST, no DST switching for the MVP — see module-level TODO). Holiday list (`holidays.rs`) is a sorted `&[NaiveDate]` covering 2025–2028 (NYSE full-day closes only, no half-days); `is_holiday` uses `binary_search` so the array MUST stay sorted, and the list needs annual top-up. Module carries `#![allow(dead_code)]` because `next_close_at` / `eod_sweep_target` will be consumed by the Phase 13/14 schedulers. Not registered in `lib.rs` — call sites use `crate::utils::market_calendar::*` directly.
- **Entry Points**:
  - `main.rs`: Application entry
  - `lib.rs`: Tauri setup, command registration, and state initialization

### Key Integration Points

1. **Tauri Commands**: All functionality exposed through commands registered in `lib.rs` via `tauri::generate_handler![]`:
   - `ibkr_connect`: Establish connection to TWS/Gateway
   - `ibkr_disconnect`: Close connection
   - `ibkr_get_connection_status`: Check connection state
   - `ibkr_get_accounts`: Retrieve account list
   - `ibkr_get_account_summary`: Get account metrics
   - `ibkr_get_positions`: Fetch current positions
   - `ibkr_start_daily_pnl` / `ibkr_stop_daily_pnl`: Subscribe/unsubscribe daily P&L stream (single shared `StreamHandle` in `IbkrState::daily_pnl_handle`)
   - `ibkr_subscribe_market_data`: Real-time quotes
   - `ibkr_place_order`: Submit orders
   - `ibkr_get_fundamental_data`: Fetch fundamental data (via Alpha Vantage or mock)
   - `ibkr_generate_projections`: Generate forward-looking scenario projections
   - `ibkr_generate_projection_results`: Run projection scenarios and return computed results
   - `ibkr_get_cached_tickers`: List tickers currently cached in `cache_service`
   - `ibkr_start_scanner` / `ibkr_stop_scanner`: Start/stop a market scanner stream (single shared handle in `IbkrState::scanner_handle`; results pushed via `EventEmitter`)
   - `tracker_fetch_bars`: Fetch historical bars with SQLite cache + 6 req/min rate limit (Phase 02)
   - `tracker_get_news(symbol, lookback_hours) -> Vec<NewsItem>`: Fetch Alpha Vantage NEWS_SENTIMENT with SQLite cache; falls back to cached/empty on rate-limit, transport failure, or missing API key (Phase 03)
   - `tracker_add(symbol, source, sourceMeta, tags, notes) -> TrackedTicker`: Insert new watchlist row; rejects duplicates with `AlreadyTracked` (Phase 04)
   - `tracker_remove(symbol)`: Delete watchlist row (idempotent — non-existent symbol returns `Ok(())`) (Phase 04)
   - `tracker_list(status?)` / `tracker_get(symbol)`: Read watchlist, optionally filtered by status (Phase 04)
   - `tracker_set_tags(symbol, tags)` / `tracker_set_status(symbol, status, inPlayUntil?, coolDownUntil?)`: Update tags or status; both return the refreshed row, error `NotFound` if missing. Phase 12 widened `tracker_set_status` to accept `coolDownUntil` (passing `null` clears it) — most callers should drive lifecycle through the Phase 12 state machine instead of poking this command directly.
   - `tracker_add` (Phase 04; updated Phase 12): when `source = "scanner"` the command auto-promotes the new row to `InPlay` via `state_machine.record_scanner_hit` (and folds `sourceMeta` into the row's `source_meta`); manual / news rows stay `Watching` until a detector hit fires `state_machine.on_setup_detected`.
   - `tracker_run_now(symbol?) -> Vec<RunResult>` (Phase 10): when a symbol is provided, runs every registered detector against fresh bars / news and persists hits; with `null` it iterates the watchlist (skipping `CoolDown` rows) and never short-circuits on per-symbol failures — each entry's `error` is populated instead. Hits are deduplicated against `(symbol, strategy, direction)` over a 24h window.
   - `tracker_get_setups(symbol?, since?) -> Vec<Setup>` (Phase 10): reads the `setups` table ordered by `detected_at DESC, id DESC`. Both filters are optional and combine with `AND`.
   - `tracker_start_scheduler` / `tracker_stop_scheduler` (Phase 13): start or stop the EOD scheduler. The scheduler is constructed once in `lib.rs::run` and `app.manage`d as `Arc<EodScheduler>`; the handle lives on `IbkrState::eod_handle`. Calling `start` twice replaces the existing handle (mirrors the scanner stream pattern). Phase 14 will hang the intraday scheduler off the same command pair.
   - `get_settings` / `update_settings` / `get_settings_path`: Configuration management (in `config::commands`)

   Streaming commands (daily P&L, scanner, EOD scheduler) follow a "replace any existing subscription" pattern: starting a new stream stops the previous one. See `IbkrState::start_*` / `stop_*` in `ibkr/state.rs`.

2. **State Management**: The `IbkrState` (managed by Tauri) maintains the IBKR client connection and is accessed across commands using Tauri's state management. Initialized in `lib.rs` setup with configuration.

3. **Event System**: The `EventEmitter` (in `events/emitter.rs`) enables server-to-client push notifications for real-time updates (market data, order status, etc.). The app handle is set during Tauri setup.

4. **Service Layer**: Business logic is encapsulated in service modules that interact with the IBKR client, providing a clean separation between API integration and command handlers.

5. **Async Operations**: All IBKR operations use Tokio for async handling, ensuring non-blocking UI updates.

6. **Type Safety**: Types are defined in Rust (organized in `ibkr/types/`) and must be matched in TypeScript when invoking commands.

## IBKR Connection Requirements
- TWS or IB Gateway must be running locally
- Default connection: `127.0.0.1:4004`
- API access must be enabled in TWS/Gateway settings
- Client ID: 100 (configurable in app)

## Alpha Vantage API Integration
The application integrates with Alpha Vantage API for real fundamental data (revenue, net income, EPS, analyst estimates) used in forward-looking projections:

### Configuration
- API key stored in `src-tauri/.env`: `ALPHA_VANTAGE_API_KEY=your_key_here`
- Get free API key at: https://www.alphavantage.co/support/#api-key
- Free tier: 25 calls/day (~8 ticker lookups, using 3 endpoints per ticker)
- Graceful fallback to mock data when API unavailable or key not set

### Data Sources
The app fetches 3 endpoints per ticker symbol:
1. **OVERVIEW**: Company metrics, P/E ratio, shares outstanding
2. **INCOME_STATEMENT**: Historical revenue and net income
3. **EARNINGS**: Annual/quarterly EPS data and estimates

### Development Workflow
- **With API key**: Real fundamental data for tickers
- **Without API key**: Automatic fallback to mock data
- **Rate limit exceeded**: Transparent fallback with warning log
- See `ALPHA_VANTAGE_SETUP.md` for detailed setup instructions

## Component Structure
- UI components follow shadcn/ui patterns with Radix UI primitives
- All UI components are in `src/shared/components/ui/` with corresponding TypeScript types
- Layout components are in `src/shared/components/layout/`
- Feature-specific components are in their respective feature directories (e.g., `src/features/portfolio/components/`)
- Use existing component patterns when adding new features
- Tailwind classes are merged using `cn()` utility from `src/shared/lib/utils.ts`

## Error Handling
- Rust errors are converted to string messages for frontend display
- Use the custom error types in `error.rs` for consistent error handling
- Frontend should handle command failures gracefully with try/catch blocks

## Testing Approach
- For Rust: Use `cargo test` with unit tests in respective modules
- For React: No test framework is configured — frontend changes are verified manually in `pnpm tauri dev`
- Integration testing: Test Tauri commands with mock IBKR responses

## Code Quality and Pre-commit Hooks

### Pre-commit Setup
The project uses pre-commit hooks to ensure code quality before commits:

```bash
# Install pre-commit (cross-platform via pipx)
pipx install pre-commit
# or: pip install --user pre-commit
# macOS alternative: brew install pre-commit

# Install hooks in the repository
pre-commit install

# Run hooks manually on all files
pre-commit run --all-files
```

### Configured Hooks
- **cargo fmt --check**: Ensures Rust code formatting compliance
- **cargo clippy**: Runs Rust linter with warnings as errors (-D warnings)
- **prettier --check**: Checks JS/TS/CSS/JSON formatting (config in `.prettierrc.json`)
- **eslint**: Lints `.ts`/`.tsx` source (flat config in `eslint.config.js`); blocks errors only — pre-existing warnings are tracked but non-blocking
- **trailing-whitespace**: Removes trailing whitespace
- **end-of-file-fixer**: Ensures files end with newline
- **check-merge-conflict**: Prevents committing merge conflict markers
- **check-yaml**: Validates YAML syntax
- **check-toml**: Validates TOML syntax

### Frontend Scripts
- `pnpm lint` / `pnpm lint:fix`: ESLint across `.ts`/`.tsx`
- `pnpm format` / `pnpm format:check`: Prettier across `src/` + root configs (Rust workspace and `*.md` excluded via `.prettierignore`)
- `pnpm typecheck`: `tsc --noEmit`

### Development Workflow
The pre-commit hooks will automatically run when you commit, preventing commits that don't meet quality standards. If hooks fail:
1. Fix the reported issues
2. Stage the fixes with `git add`
3. Commit again

Common issues and fixes:
- **Rust formatting**: Run `cargo fmt --manifest-path src-tauri/Cargo.toml`
- **Clippy warnings**: Fix the specific warnings reported
- **JS/TS formatting**: Run `pnpm format`
- **ESLint errors**: Run `pnpm lint:fix` for auto-fixable issues, otherwise address reported errors
- **Trailing whitespace**: Pre-commit will fix automatically

## Backend Development Workflow

### Adding New IBKR Features
When adding new IBKR functionality, follow this layered approach:

1. **Define Types**: Add type definitions in `src-tauri/src/ibkr/types/` organized by domain
2. **Implement Service Logic**: Add business logic in appropriate service module (`services/account_service.rs`, `services/market_service.rs`, or `services/trading_service.rs`)
3. **Create Tauri Commands**: Add command handlers in `src-tauri/src/ibkr/commands/` organized by domain (connection, accounts, market_data, trading)
4. **Register Commands**: Add command to `tauri::generate_handler![]` macro in `lib.rs`
5. **Write Tests**: Use MockIbkrClient for unit tests in `src-tauri/src/ibkr/tests/`

### Test-Driven Development
Follow the test-driven approach:
1. Write tests first using MockIbkrClient in appropriate test module
2. Implement mock behavior in `mocks.rs`
3. Run tests: `cargo test --manifest-path src-tauri/Cargo.toml ibkr::`
4. Implement real IBKR client functionality in `client.rs`
5. Implement service layer logic
6. Create and register Tauri commands

## Frontend Feature Organization
The frontend follows a feature-based architecture in `/src/features/`:
- `connection/`: IBKR connection management (ConnectionSettings, ConnectionStatus)
- `portfolio/`: Account and position management (AccountSummary, AccountDetails, StockPositions, OptionPositions)
- `analysis/`: Fundamental data analysis and forward projections (integrated with Alpha Vantage API)
- `scanner/`: Market scanner UI consuming the streaming scanner backend; each row exposes "Analyze" (deep-links to analysis via `pendingSymbol`) and "Add to tracker" (Phase 05) actions
- `tracker/` (Phase 05): Watchlist UI over the `tracker_*` Tauri commands. `TrackerTab` composes `Watchlist` (table with inline tag editing + remove) and a status filter; mounted as a top-level tab with an unread-count badge in `App.tsx`. `AddToTrackerDialog` is a portal-rendered modal triggered by the manual "Add" button or the scanner row's "Add to tracker"; on duplicate it surfaces an "already tracked" message. `useWatchlist(refreshKey)` owns fetch/CRUD against `ibkrApi.tracker.*` and re-fetches whenever `refreshKey` changes (App.tsx bumps it after every dialog submission). Source-of-truth types live in `src/features/tracker/types.ts` mirroring the Rust enums in snake_case (`watching` / `in_play` / `setup_active` / `cool_down`; `manual` / `scanner` / `news`).

Each feature contains its own components, hooks, and (where relevant) types. Real-time market data and order placement are exposed as backend Tauri commands but do not have dedicated feature directories yet.

When creating new features:
- Place shared components in `src/shared/components/ui/`
- Feature-specific components go in their feature directory
- Use the `Table` component from `shared/components/ui/table.tsx` for data tables
- Use `Skeleton` component for loading states
- Use `Alert` component for error/success messages
- API calls to Tauri commands should be placed in `src/shared/api/` (see `src/shared/api/ibkr.ts` for existing IBKR API wrapper)
