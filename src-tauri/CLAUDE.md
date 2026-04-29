# Backend (src-tauri)

Rust backend for the Tauri 2 app. Cross-cutting rules in `../CLAUDE.md`.

## Common commands

All cargo commands need `--manifest-path src-tauri/Cargo.toml` from the repo root (no workspace at the root). For longer backend sessions, `cd src-tauri/` once and run the bare forms below.

```bash
cargo check
cargo test
cargo test ibkr::                       # by module path
cargo test -- test_specific_function    # single test
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
```

## Layering (`src/`)

```
config/        AppConfig + SettingsState (JSON persisted to OS app-data dir)
events/        EventEmitter → Tauri events; AppEvent enum is the contract with the UI
storage/       SQLite via rusqlite + r2d2 pool, embedded schema.sql + migrations runner
ibkr/          IBKR adapter: client.rs (TWS/Gateway), commands/ (Tauri handlers),
               types/ (domain types per concern), state.rs (IbkrState — the shared root),
               mocks.rs (MockIbkrClient — the IbkrClientTrait test seam)
strategies/    StrategyDetector trait + MarketContext + SetupCandidate + DetectorRegistry,
               one detector per subdir (breakout / episodic_pivot / parabolic_short)
services/      Business logic. Each service is constructed in lib.rs and managed via
               app.manage(...) so Tauri commands can fetch them via State<T>.
middleware/    Cross-cutting: RateLimiter, HistoricalRateLimiter, logging
utils/         Calendar (RTH/holidays), shared helpers
lib.rs         Tauri setup. Wires Db → IbkrState → services → schedulers and registers
               every #[tauri::command] handler.
```

`lib.rs::run` is the source of truth for service composition. Read it before adding a new service — most additions are: define the service in `services/`, construct it in `run()`, `app.manage(...)` it, then add a Tauri command in `ibkr/commands/` that pulls it via `State<Arc<MyService>>`.

## `IbkrState` and stream handles

`ibkr/state.rs` holds `Arc<IbkrClient>`, `Arc<EventEmitter>`, the SQLite `Arc<Db>`, the tracker services, and several stream handles. All long-running streams follow the same pattern: `*_handle: Arc<RwLock<Option<StreamHandle>>>`, with start methods that stop-then-replace. Mirror this pattern for any new stream.

## Tracker subsystem

Watchlist → detectors → LLM enrichment → alerts pipeline:

1. **Schedulers** (`services/eod_scheduler`, `services/intraday_scheduler`) tick on a calendar-aware schedule and call `TrackerRunner`.
2. **`TrackerRunner`** (`services/tracker_runner`) fetches bars (`HistoricalDataService`) and news (`FinancialDataService`), builds `MarketContext`, runs the `DetectorRegistry`, persists `SetupCandidate` rows, drives the state machine, and emits `SetupDetected`.
3. **LLM enrichment** (`services/thesis_generator`, `services/decay_watcher`, `services/news_interpreter`) calls `LlmService` (`services/llm_service`), which enforces a daily USD budget against the `llm_calls` ledger and re-emits enriched events.
4. **State machine** (`services/tracker_state_machine`) owns `watching → in_play → cool_down` transitions per ticker.

SQLite tables (see `src/storage/schema.sql`): `tracked_tickers`, `setups`, `alerts`, `bars_cache`, `news_cache`, `llm_calls`. The pre-existing file-based `cache_service.rs` (JSON, 7-day TTL for fundamentals/projections) is intentionally **not** migrated to SQLite.
