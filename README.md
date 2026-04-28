# Quantum Kapital

A professional cross-platform algorithmic trading application built with Tauri and React, providing seamless integration with Interactive Brokers (IBKR) API for real-time portfolio management and automated trading.

## Features

- **Real-time Portfolio Dashboard**: Monitor your IBKR account with live updates
- **Position Management**: Track all your positions with real-time P&L calculations
- **Account Summary**: Comprehensive view of account metrics including equity, buying power, and available funds
- **Market Data Streaming**: Subscribe to real-time market data for your holdings
- **Order Execution**: Place market and limit orders directly from the application
- **Forward Analysis & Projections**: Multi-year financial projections with Bear/Base/Bull scenarios
- **Fundamental Data Integration**: Real fundamental data via Alpha Vantage API (revenue, EPS, analyst estimates)
- **Test-Driven Development**: Comprehensive test suite with MockIbkrClient for reliable development
- **Cross-Platform**: Runs natively on Windows, macOS, and Linux
- **Secure**: All sensitive data is handled securely through Tauri's IPC bridge

## Architecture

### Frontend (React + TypeScript)
- **UI Framework**: React 18 with TypeScript
- **Component Library**: Custom shadcn/ui components with Tailwind CSS
- **State Management**: React hooks with async Tauri command invocation
- **Icons**: Lucide React for consistent iconography
- **Styling**: Tailwind CSS with custom gradient themes

### Backend (Tauri + Rust)
- **Framework**: Tauri v2 for secure desktop application development
- **IBKR Integration**: Using `ibapi` Rust crate for TWS/Gateway communication
- **Async Runtime**: Tokio for handling concurrent operations
- **Error Handling**: Custom error types with thiserror
- **Logging**: Structured logging with tracing

## Project Structure

```
quantum-kapital/
├── src/                           # React frontend (feature-based architecture)
│   ├── app/                      # Main application entry
│   │   └── App.tsx              # Root component with routing
│   ├── features/                # Feature-based modules
│   │   ├── analysis/            # Forward projections & fundamental analysis
│   │   │   ├── components/      # TickerAnalysis, ProjectionSummary, etc.
│   │   │   ├── hooks/           # useProjections, useTickerSearch
│   │   │   └── types/           # Analysis type definitions
│   │   ├── connection/          # IBKR connection management
│   │   │   ├── components/      # ConnectionSettings, ConnectionStatus
│   │   │   └── hooks/           # useConnection
│   │   ├── portfolio/           # Account & position management
│   │   │   ├── components/      # AccountSummary, StockPositions, etc.
│   │   │   └── hooks/           # useAccountData
│   │   ├── market-data/         # Real-time market data streaming
│   │   └── trading/             # Order placement & execution
│   ├── shared/                  # Shared utilities & components
│   │   ├── api/                 # API layer (ibkr.ts, settings.ts)
│   │   ├── components/          # Reusable UI components
│   │   │   ├── ui/             # 50+ shadcn/ui components
│   │   │   └── layout/         # Layout components
│   │   ├── hooks/              # Shared custom hooks
│   │   ├── lib/                # Utility functions
│   │   └── types/              # Shared type definitions
│   └── main.tsx                # Application entry point
├── src-tauri/                   # Rust backend (layered architecture)
│   ├── src/
│   │   ├── ibkr/               # IBKR API integration layer
│   │   │   ├── client.rs       # IBKR TWS/Gateway client
│   │   │   ├── commands/       # Modular command handlers
│   │   │   │   ├── connection.rs  # Connection management
│   │   │   │   ├── accounts.rs    # Account operations
│   │   │   │   ├── market_data.rs # Market data subscriptions
│   │   │   │   ├── trading.rs     # Order placement
│   │   │   │   └── analysis.rs    # Fundamental data & projections
│   │   │   ├── types/          # Domain-specific types
│   │   │   │   ├── account.rs     # Account types
│   │   │   │   ├── positions.rs   # Position types
│   │   │   │   ├── market_data.rs # Market data types
│   │   │   │   └── fundamentals.rs # Analysis types
│   │   │   ├── state.rs        # Application state management
│   │   │   ├── error.rs        # Custom error types
│   │   │   ├── mocks.rs        # MockIbkrClient for testing
│   │   │   └── tests/          # Comprehensive test suite
│   │   ├── services/           # Business logic layer
│   │   │   ├── account_service.rs      # Account operations
│   │   │   ├── market_service.rs       # Market data operations
│   │   │   ├── trading_service.rs      # Trading operations
│   │   │   ├── financial_data_service.rs # Alpha Vantage integration
│   │   │   └── projection_service.rs   # Financial projections
│   │   ├── middleware/         # Cross-cutting concerns
│   │   │   ├── logging.rs      # Structured logging
│   │   │   └── rate_limit.rs   # API rate limiting
│   │   ├── events/             # Event system for real-time updates
│   │   │   └── emitter.rs      # Event emitter
│   │   ├── config/             # Application configuration
│   │   │   └── settings.rs     # Configuration management
│   │   ├── utils/              # Shared utilities
│   │   ├── lib.rs              # Tauri setup & command registration
│   │   └── main.rs             # Application entry point
│   └── Cargo.toml              # Rust dependencies
└── package.json                # Node.js dependencies
```

## Prerequisites

- **Node.js** (v18 or higher)
- **Rust** (latest stable)
- **pnpm** (recommended) or npm
- **Interactive Brokers TWS** or **IB Gateway** running locally
- IBKR account with API access enabled

## Installation

1. Clone the repository:
```bash
git clone https://github.com/yourusername/quantum-kapital.git
cd quantum-kapital
```

2. Install dependencies:
```bash
pnpm install
```

3. Configure IBKR connection:
   - Ensure TWS or IB Gateway is running
   - Default connection settings: `127.0.0.1:4004`
   - Client ID: 100 (configurable in the app)

## Development

### Frontend Development
```bash
# Install dependencies
pnpm install

# Run development server (Vite + Tauri)
pnpm tauri dev

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

### Development Workflow
Running `pnpm tauri dev` will:
- Start the Vite dev server for the React frontend
- Build and run the Tauri application
- Enable hot module replacement for the frontend
- Watch for changes in the Rust code

## 📱 Building for Production

Create a production build:

```bash
pnpm tauri build
```

This will create optimized binaries for your platform in `src-tauri/target/release/`.

## IBKR API Configuration

### TWS Configuration
1. Enable API connections in TWS:
   - File → Global Configuration → API → Settings
   - Enable "Enable ActiveX and Socket Clients"
   - Configure "Socket port" (default: 4004)
   - Add "127.0.0.1" to "Trusted IPs"

### IB Gateway Configuration
1. Similar settings available in IB Gateway
2. Recommended for production use (more stable for long-running connections)

## Fundamental Data API Setup ✅ CONFIGURED

The application uses Alpha Vantage API for fetching real fundamental data. This is **optional** - the app works with mock data if not configured.

### ✅ Already Configured for You!

Your API key is already set up and tested:
- API key added to `src-tauri/.env`
- All endpoints verified and working
- Real data ready for 8+ ticker lookups per day

Just run `pnpm tauri dev` and select a ticker to see real fundamental data!

### Manual Setup (If Needed)

1. Get a free API key from [Alpha Vantage](https://www.alphavantage.co/support/#api-key)
2. Create `.env` file in `src-tauri/`:
   ```bash
   cd src-tauri
   cp .env.example .env
   ```
3. Add your API key to `.env`:
   ```
   ALPHA_VANTAGE_API_KEY=your_api_key_here
   ```
4. Restart the application

**Free tier includes 25 API calls per day** (~8 ticker lookups with real fundamental data).

For detailed documentation, see [ALPHA_VANTAGE_SETUP.md](ALPHA_VANTAGE_SETUP.md).

## Available Commands

The application exposes the following Tauri commands:

### Connection & Account Management
- `ibkr_connect`: Establish connection to IBKR
- `ibkr_disconnect`: Close the connection
- `ibkr_get_connection_status`: Check connection status
- `ibkr_get_accounts`: Retrieve account list
- `ibkr_get_account_summary`: Get detailed account metrics
- `ibkr_get_positions`: Fetch current positions

### Market Data & Trading
- `ibkr_subscribe_market_data`: Subscribe to real-time quotes
- `ibkr_place_order`: Submit orders to IBKR

### Analysis & Projections
- `ibkr_get_fundamental_data`: Fetch fundamental data (real via Alpha Vantage or mock)
- `ibkr_generate_projections`: Generate Bear/Base/Bull scenario projections

## UI Components

The application uses a comprehensive component library:

- **Cards**: Display account metrics and positions
- **Tabs**: Navigate between positions and account details
- **Buttons**: Connect/disconnect and action buttons
- **Input fields**: Configure connection settings
- **Badges**: Status indicators
- **Tables**: Position listings (extensible)

## Security Considerations

- All IBKR API communications happen through the Rust backend
- No sensitive data is exposed to the web context
- Connection settings are managed securely through Tauri's state management
- Consider using environment variables for production deployments

## Code Quality

The project uses pre-commit hooks to ensure code quality:

```bash
# Install pre-commit (if not already installed)
brew install pre-commit

# Install hooks in the repository
pre-commit install

# Run hooks manually on all files
pre-commit run --all-files
```

### Configured Hooks
- **cargo fmt --check**: Ensures Rust code formatting
- **cargo clippy**: Runs Rust linter with strict warnings
- **trailing-whitespace**: Removes trailing whitespace
- **end-of-file-fixer**: Ensures files end with newline
- **check-merge-conflict**: Prevents committing merge conflicts

## Technologies Used

### Frontend
- React 18.3
- TypeScript 5
- Tailwind CSS 3.4
- Vite 6
- shadcn/ui-style components (Alert, Badge, Button, Card, Input, Label, Skeleton, Table, Tabs)
- Lucide React icons

### Backend
- Tauri 2.0
- Rust (stable)
- ibapi 1.2 (IBKR integration)
- Tokio (async runtime)
- Serde (serialization)
- Tracing (structured logging)
- Thiserror (error handling)
- Alpha Vantage API (fundamental data)

## Resources

- [Tauri Documentation](https://tauri.app)
- [IBKR API Documentation](https://interactivebrokers.github.io)
- [Rust IBAPI Crate](https://github.com/wboayue/rust-ibapi)
- [React Documentation](https://react.dev)
- [shadcn/ui](https://ui.shadcn.com)

## Contributing

Contributions are welcome! Please read our contributing guidelines before submitting PRs.

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Disclaimer

This software is for educational and informational purposes only. Trading financial instruments carries risk. Always perform your own research and consider consulting with a qualified financial advisor before making investment decisions.
