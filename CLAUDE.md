# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A Tauri 2 desktop app that pairs an Interactive Brokers connection with a strategy-driven **Tracker** subsystem (watchlist → detectors → LLM reasoning → alerts). Surveillance only — order placement exists but is not wired into the tracker. The implementation is phase-driven; the canonical plan is `impl.md` plus `impl/phase-*.md`.

## Common commands

Frontend / Tauri:

```bash
pnpm tauri dev            # full app (Vite + Rust backend, hot reload)
pnpm dev                  # frontend only (no Tauri shell)
pnpm tauri build          # production binaries → src-tauri/target/release/
pnpm typecheck            # tsc --noEmit
pnpm lint                 # eslint .
pnpm format               # prettier --write .
```

Rust backend (always pass `--manifest-path src-tauri/Cargo.toml` — running `cargo` from the repo root won't find it):

```bash
cargo check   --manifest-path src-tauri/Cargo.toml
cargo test    --manifest-path src-tauri/Cargo.toml
cargo test    --manifest-path src-tauri/Cargo.toml ibkr::                       # by module path
cargo test    --manifest-path src-tauri/Cargo.toml -- test_specific_function    # single test
cargo fmt     --manifest-path src-tauri/Cargo.toml
cargo clippy  --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
```

Pre-commit (`.pre-commit-config.yaml`) runs `cargo fmt --check`, `cargo clippy -D warnings`, `prettier --check`, and `eslint` on every commit. **Never bypass with `--no-verify`** — fix the underlying issue. If clippy fails on code you didn't touch, that's a real regression to investigate.

## Architecture

### Backend layering (`src-tauri/src/`)

```
config/        AppConfig + SettingsState (JSON persisted to OS app-data dir)
events/        EventEmitter → Tauri events; AppEvent enum is the contract with the UI
storage/       SQLite via rusqlite + r2d2 pool, embedded schema.sql + migrations runner
ibkr/          IBKR adapter: client.rs (TWS/Gateway), commands/ (Tauri handlers),
               types/ (domain types per concern), state.rs (IbkrState — the shared root),
               mocks.rs (MockIbkrClient — the seam every test uses)
strategies/    StrategyDetector trait + MarketContext + SetupCandidate + DetectorRegistry,
               one detector per subdir (breakout / episodic_pivot / parabolic_short)
services/      Business logic. Each service is constructed in lib.rs and managed via
               app.manage(...) so Tauri commands can fetch them via State<T>.
middleware/    Cross-cutting: RateLimiter, HistoricalRateLimiter, logging
utils/         Calendar (RTH/holidays), shared helpers
lib.rs         Tauri setup. Wires Db → IbkrState → services → schedulers and registers
               every #[tauri::command] handler.
```

The wiring graph in `lib.rs::run` is the source of truth for how services compose. Read it before adding a new service — most additions are: define the service in `services/`, construct it in `run()`, `app.manage(...)` it, then add a Tauri command in `ibkr/commands/` that pulls it via `State<Arc<MyService>>`.

### `IbkrState` (the shared root)

`ibkr/state.rs` holds `Arc<IbkrClient>`, `Arc<EventEmitter>`, the SQLite `Arc<Db>`, the `TrackerService` / `TrackerStateMachine` / `LlmService`, and stream handles (`daily_pnl_handle`, `scanner_handle`, `eod_handle`, `intraday_handle`). All long-running streams follow the same start/stop pattern: `*_handle: Arc<RwLock<Option<StreamHandle>>>`, with start methods that stop-then-replace. Mirror this pattern when adding new streams.

### Tracker subsystem (Phases 1–20)

Watchlist → detectors → LLM enrichment → alerts pipeline. The runtime path:

1. **Schedulers** (`services/eod_scheduler`, `services/intraday_scheduler`) tick on a calendar-aware schedule and call `TrackerRunner`.
2. **TrackerRunner** (`services/tracker_runner`) fetches bars (`HistoricalDataService`) and news (`FinancialDataService`), builds `MarketContext`, runs the `DetectorRegistry`, persists `SetupCandidate` rows, drives the state machine, and emits `SetupDetected`.
3. **LLM enrichment** (`services/thesis_generator`, `services/decay_watcher`) calls `LlmService` (`services/llm_service`) which enforces a daily USD budget against the `llm_calls` ledger and re-emits enriched events.
4. **State machine** (`services/tracker_state_machine`) owns `watching → in_play → cool_down` transitions per ticker.

SQLite tables (see `src-tauri/src/storage/schema.sql`): `tracked_tickers`, `setups`, `alerts`, `bars_cache`, `news_cache`, `llm_calls`. The pre-existing file-based `cache_service.rs` (JSON, 7-day TTL for fundamentals/projections) is intentionally **not** migrated to SQLite.

### Frontend (`src/`)

Feature-based React 19 + TypeScript + Tailwind 4. Each `src/features/<area>/` is self-contained: `components/`, `hooks/`, `types/`. Cross-feature code lives in `src/shared/` (`api/` for Tauri command wrappers, `components/ui/` for shadcn-style primitives). Path alias `@/* → src/*` is configured in `tsconfig.json` and `vite.config.ts`.

All backend access goes through `src/shared/api/*.ts` — never call `invoke()` directly from a component.

## TDD discipline

Every phase in `impl/` follows red → green → refactor:

1. Write the listed tests first; confirm they fail.
2. Implement until green.
3. Run clippy + fmt; tick the phase checkboxes; commit.

The IBKR adapter has a trait-based seam: `IbkrClientTrait` in `ibkr/mocks.rs` plus `MockIbkrClient`. **All service-layer tests use `MockIbkrClient`** rather than touching live TWS — required, not optional. The same pattern applies to `BarsFetcher` / `NewsFetcher` / `HistoricalDataFetcher` / `AnthropicHttp` traits — each external dependency has a narrow trait so tests can inject canned data.

Don't start a phase whose dependencies (listed in each `impl/phase-*.md`) are unchecked.

## Configuration & secrets

Settings persist as JSON via `config::AppConfig` to the OS app-data dir (`~/.config/quantum-kapital/settings.json` on Linux; see `SETTINGS_GUIDE.md` for other platforms and the full schema). Secrets are read from `src-tauri/.env` via `dotenv`:

- `ALPHA_VANTAGE_API_KEY` — fundamental + news data (free tier: 25 calls/day; service falls back gracefully when absent).
- `ANTHROPIC_API_KEY` — Claude API for thesis / decay-watcher / news-interpreter / ranker. `LlmService` enforces `daily_llm_budget_usd` from settings; over-budget calls return `LlmError::BudgetExhausted` rather than billing.

Default IBKR connection is `127.0.0.1:4004` (live) with client ID 100. Use `7497` for paper trading.

## Conventions that affect code shape

See `CONTRIBUTING.md` for the full file-size policy. Summary: Rust soft cap **500 lines** / hard cap **800**; TS/TSX soft cap **300** / hard cap **500**. Files that exceed the hard cap need a top-of-file `// allow-large-file: <reason>` comment with a follow-up issue. The pre-Phase-25 offenders listed in `CONTRIBUTING.md` are exempt for unrelated changes; new files always follow the cap.

## Reference docs

- `impl.md` + `impl/phase-*.md` — phased plan with dependency graph; the source of truth for what to build next.
- `impl/scratch/` — cross-phase notes (schema decisions, detector calibration, prompt versions, backtest results) that don't belong in code.
- `IBKR_API_INTERFACES.md` — IBKR API surface used by `ibkr/client.rs`.
- `ALPHA_VANTAGE_SETUP.md`, `FUNDAMENTAL_DATA_API.md` — Alpha Vantage integration.
- `SETTINGS_GUIDE.md` — settings JSON schema, frontend/backend access patterns.
- `CONTRIBUTING.md` — file-size limits and escalation rules.
