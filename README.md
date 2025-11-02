# Quantum Kapital

A professional cross-platform algorithmic trading application built with Tauri and React, providing seamless integration with Interactive Brokers (IBKR) API for real-time portfolio management and automated trading.

## Features

- **Real-time Portfolio Dashboard**: Monitor your IBKR account with live updates
- **Position Management**: Track all your positions with real-time P&L calculations
- **Account Summary**: Comprehensive view of account metrics including equity, buying power, and available funds
- **Market Data Streaming**: Subscribe to real-time market data for your holdings
- **Order Execution**: Place market and limit orders directly from the application
- **Forward Analysis & Projections**: Multi-year financial projections with Bear/Base/Bull scenarios
- **Fundamental Data Integration**: Real fundamental data via Alpha Vantage API (configured and working!)
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
├── src/                    # React frontend
│   ├── App.tsx            # Main application component with IBKR integration
│   ├── components/        # Reusable UI components
│   │   ├── ui/           # shadcn/ui component library
│   │   └── theme-provider.tsx
│   ├── lib/              # Utility functions
│   └── main.tsx          # Application entry point
├── src-tauri/            # Rust backend
│   ├── src/
│   │   ├── ibkr/        # IBKR API integration modules
│   │   │   ├── client.rs     # IBKR client implementation
│   │   │   ├── commands.rs   # Tauri command handlers
│   │   │   ├── types.rs      # Shared type definitions
│   │   │   ├── state.rs      # Application state management
│   │   │   └── error.rs      # Error handling
│   │   ├── lib.rs       # Library configuration
│   │   └── main.rs      # Application entry point
│   └── Cargo.toml       # Rust dependencies
└── package.json         # Node.js dependencies
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

Run the application in development mode:

```bash
pnpm tauri dev
```

This will:
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
- `ibkr_get_fundamental_data`: Fetch fundamental data (real or mock)
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

## Technologies Used

### Frontend
- React 18.3
- TypeScript 5
- Tailwind CSS 3.4
- Vite 6
- shadcn/ui components
- Lucide React icons

### Backend
- Tauri 2.0
- Rust (stable)
- ibapi 1.2
- Tokio (async runtime)
- Serde (serialization)
- Tracing (logging)

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