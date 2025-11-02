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
- **Component Library**: shadcn/ui implementation (50+ components in `src/shared/components/ui/`)
- **State Management**: React hooks with Tauri command invocation via `@tauri-apps/api`
- **Styling**: Tailwind CSS with custom gradient themes
- **TypeScript**: Strict mode with path mapping (`@/*` → `./src/*`)
- **Form Handling**: react-hook-form with zod validation
- **Icons**: Lucide React, recharts for data visualization
- **Structure**:
  - `app/`: Main application entry and layout
  - `features/`: Feature-based modules (connection, portfolio, market-data, trading)
  - `shared/`: Reusable components, utilities, hooks, types, and API layer

### Backend Architecture
The Rust backend (`/src-tauri/src`) follows a layered architecture:
- **Core Modules**:
  - `ibkr/`: IBKR API integration layer
    - `client.rs`: IBKR TWS/Gateway connection using `ibapi` crate
    - `commands/`: Tauri command handlers modularized by domain
      - `connection.rs`: Connection management commands
      - `accounts.rs`: Account-related commands
      - `market_data.rs`: Market data subscription commands
      - `trading.rs`: Order placement commands
      - `analysis.rs`: Fundamental data and projection commands
    - `types/`: Type definitions modularized by domain (account, connection, historical, market_data, orders, positions, scanner)
    - `state.rs`: Application state management with Tokio async runtime
    - `error.rs`: Custom error types with thiserror
    - `mocks.rs`: MockIbkrClient for test-driven development
    - `tests/`: Comprehensive test modules (api_interface, client, command, integration)
  - `services/`: Business logic layer
    - `account_service.rs`: Account management operations
    - `market_service.rs`: Market data operations
    - `trading_service.rs`: Trading operations
  - `middleware/`: Cross-cutting concerns
    - `logging.rs`: Structured logging with tracing
    - `rate_limit.rs`: API rate limiting
  - `events/`: Event system
    - `emitter.rs`: Event emitter for frontend notifications
  - `config/`: Application configuration
    - `settings.rs`: Configuration management
  - `utils/`: Shared utilities
- **Entry Points**:
  - `main.rs`: Application entry
  - `lib.rs`: Tauri setup, command registration, and state initialization

### Key Integration Points

1. **Tauri Commands**: All IBKR functionality exposed through these commands (registered in `lib.rs:39-50`):
   - `ibkr_connect`: Establish connection to TWS/Gateway
   - `ibkr_disconnect`: Close connection
   - `ibkr_get_connection_status`: Check connection state
   - `ibkr_get_accounts`: Retrieve account list
   - `ibkr_get_account_summary`: Get account metrics
   - `ibkr_get_positions`: Fetch current positions
   - `ibkr_subscribe_market_data`: Real-time quotes
   - `ibkr_place_order`: Submit orders
   - `ibkr_get_fundamental_data`: Fetch fundamental data (via Alpha Vantage or mock)
   - `ibkr_generate_projections`: Generate forward-looking scenario projections

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
- For React: Component testing setup needs to be added (no test framework currently configured)
- Integration testing: Test Tauri commands with mock IBKR responses

## Code Quality and Pre-commit Hooks

### Pre-commit Setup
The project uses pre-commit hooks to ensure code quality before commits:

```bash
# Install pre-commit (if not already installed)
brew install pre-commit

# Install hooks in the repository
pre-commit install

# Run hooks manually on all files
pre-commit run --all-files
```

### Configured Hooks
- **cargo fmt --check**: Ensures Rust code formatting compliance
- **cargo clippy**: Runs Rust linter with warnings as errors (-D warnings)
- **trailing-whitespace**: Removes trailing whitespace
- **end-of-file-fixer**: Ensures files end with newline
- **check-merge-conflict**: Prevents committing merge conflict markers
- **check-yaml**: Validates YAML syntax
- **check-toml**: Validates TOML syntax

### Development Workflow
The pre-commit hooks will automatically run when you commit, preventing commits that don't meet quality standards. If hooks fail:
1. Fix the reported issues
2. Stage the fixes with `git add`
3. Commit again

Common issues and fixes:
- **Formatting**: Run `cargo fmt --manifest-path src-tauri/Cargo.toml`
- **Clippy warnings**: Fix the specific warnings reported
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
- `market-data/`: Real-time market data streaming and display
- `trading/`: Order placement and execution
- `analysis/`: Fundamental data analysis and forward projections (integrated with Alpha Vantage API)
Each feature contains its own components, hooks, and types.

When creating new features:
- Place shared components in `src/shared/components/ui/`
- Feature-specific components go in their feature directory
- Use the `Table` component from `shared/components/ui/table.tsx` for data tables
- Use `Form` components with react-hook-form for forms
- Use `Skeleton` component for loading states
- Use `Alert` component for error/success messages
- API calls to Tauri commands should be placed in `src/shared/api/` (see `src/shared/api/ibkr.ts` for existing IBKR API wrapper)
