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
      - `tracker.rs`: Tracker subsystem commands (Phase 02 added `tracker_fetch_bars`; Phase 03 added `tracker_get_news`; Phase 04 added watchlist CRUD: `tracker_add` / `tracker_remove` / `tracker_list` / `tracker_get` / `tracker_set_tags` / `tracker_set_status`)
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
    - `tracker_service/`: Watchlist persistence over the `tracked_tickers` table (added Phase 04). `TrackerService::new(db: Arc<Db>)`. CRUD surface: `add` (returns `TrackerError::AlreadyTracked` on PK conflict), `remove` (idempotent), `list(status_filter)`, `get`, `set_tags`, `set_status(in_play_until)`, `touch_last_checked`. Symbols are normalized to uppercase. `tags` and `source_meta` round-trip as JSON columns; status is stored as a snake_case string but transitions are NOT enforced here — Phase 12 owns the state machine.
    - `historical_data_service/`: Historical bars fetcher with SQLite cache (added Phase 02)
      - `mod.rs`: `HistoricalDataService` with cache-first reads, write-through, in-flight dedup via `tokio::sync::Mutex<HashMap<key, Arc<Mutex<()>>>>`, partial-range gap fetch for daily bars, intraday cache invalidation at session rollover. Exposes `HistoricalDataFetcher` trait (blanket-impl'd by `IbkrClient`) + injectable `Clock` for tests.
      - `tests.rs`: 9 unit tests covering cache hit/miss, partial-range fetch, daily-vs-intraday TTL, rate-limiter accounting, dedup, and bit-equal SQLite round-trip
      - `Lookback` enum: `Days(u32)` for daily bars, `TradingDay(NaiveDate)` for intraday
  - `middleware/`: Cross-cutting concerns
    - `rate_limit.rs`: API rate limiting (default 50 req/sec; tracing is initialized in `lib.rs::run`)
    - `historical_rate_limit.rs`: Sliding 60-second window for IBKR historical-data calls (default 6 req/min); separate from the 50 req/sec general limiter
  - `events/`: Event system
    - `emitter.rs`: Event emitter for frontend notifications
  - `config/`: Application configuration
    - `settings.rs`: Configuration management
  - `storage/`: SQLite layer for the Tracker subsystem (added Phase 01)
    - `mod.rs`: `Db` (r2d2 pool wrapper) + async `with_conn` helper around `tokio::task::spawn_blocking`
    - `schema.sql`: Embedded baseline schema (`tracked_tickers`, `setups`, `alerts`, `bars_cache`, `news_cache`, `llm_calls` + `idx_setups_symbol`, `idx_setups_status_detected`)
    - `migrations.rs`: Idempotent `CREATE TABLE IF NOT EXISTS` runner invoked at startup
    - `error.rs`: `StorageError` (`Sqlite`, `Pool`, `Migration`, `Serde`, `Join`)
    - PRAGMAs (`journal_mode=WAL`, `foreign_keys=ON`, `synchronous=NORMAL`) applied per pooled connection via `SqliteConnectionManager::with_init`
    - DB lives at `app_local_data_dir()/tracker.sqlite`; `Arc<Db>` is both `app.manage`d in `lib.rs::run` and held on `IbkrState` (Phase 04 wired `IbkrState::db` + `IbkrState::tracker: Arc<TrackerService>`)
    - `bars_cache` (Phase 02) is read/written exclusively through `HistoricalDataService` — composite PK `(symbol, bar_size, bar_time)` is the only index; writes use `INSERT OR REPLACE` for idempotency
  - `utils/`: Shared utilities
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
   - `tracker_set_tags(symbol, tags)` / `tracker_set_status(symbol, status, inPlayUntil?)`: Update tags or status; both return the refreshed row, error `NotFound` if missing (Phase 04)
   - `get_settings` / `update_settings` / `get_settings_path`: Configuration management (in `config::commands`)

   Streaming commands (daily P&L, scanner) follow a "replace any existing subscription" pattern: starting a new stream stops the previous one. See `IbkrState::start_*` / `stop_*` in `ibkr/state.rs`.

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
- `scanner/`: Market scanner UI consuming the streaming scanner backend; selecting a row deep-links to the analysis tab via the `pendingSymbol` prop on `TickerAnalysis`

Each feature contains its own components, hooks, and (where relevant) types. Real-time market data and order placement are exposed as backend Tauri commands but do not have dedicated feature directories yet.

When creating new features:
- Place shared components in `src/shared/components/ui/`
- Feature-specific components go in their feature directory
- Use the `Table` component from `shared/components/ui/table.tsx` for data tables
- Use `Skeleton` component for loading states
- Use `Alert` component for error/success messages
- API calls to Tauri commands should be placed in `src/shared/api/` (see `src/shared/api/ibkr.ts` for existing IBKR API wrapper)
