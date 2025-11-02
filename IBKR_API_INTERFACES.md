# IBKR API Interfaces Documentation

This document describes the IBKR API interfaces available in Quantum Kapital, their purposes, and usage examples.

## Core Interfaces

### 1. Market Data

#### Real-time Market Data Snapshot
```rust
async fn get_market_data_snapshot(&self, symbol: &str) -> Result<MarketDataSnapshot>
```
Retrieves current market data for a symbol including bid/ask, last trade, volume, and daily high/low.

**Response Structure:**
- `bid_price`, `bid_size`: Current best bid
- `ask_price`, `ask_size`: Current best ask
- `last_price`, `last_size`: Last trade information
- `high`, `low`, `open`, `close`: Daily price points
- `volume`: Daily volume
- `timestamp`: Unix timestamp

#### Historical Data
```rust
async fn get_historical_data(&self, request: HistoricalDataRequest) -> Result<Vec<HistoricalBar>>
```
Fetches historical price bars for technical analysis and charting.

**Request Parameters:**
- `symbol`: Security symbol
- `end_date_time`: End date/time for data (format: "YYYYMMDD HH:MM:SS")
- `duration`: Time span (e.g., "1 D", "1 W", "1 M")
- `bar_size`: Bar granularity (1sec to 1day)
- `what_to_show`: Data type (Trades, Midpoint, Bid, Ask)
- `use_rth`: Regular trading hours only

### 2. Contract Information

#### Contract Details
```rust
async fn get_contract_details(&self, symbol: &str) -> Result<ContractDetails>
```
Retrieves detailed contract specifications for a security.

**Response Includes:**
- Security type (Stock, Option, Future, etc.)
- Exchange listings
- Trading currency
- Contract ID
- Tick size and multiplier

### 3. Account Management

#### Account Values
```rust
async fn get_account_values(&self, account: &str) -> Result<Vec<AccountValue>>
```
Fetches all account values including balances, margins, and P&L.

**Common Values:**
- `NetLiquidation`: Total account value
- `TotalCashValue`: Cash balance
- `BuyingPower`: Available buying power
- `GrossPositionValue`: Total position value
- `UnrealizedPnL`: Open position P&L
- `RealizedPnL`: Closed position P&L

### 4. Trading & Execution

#### Execution Reports
```rust
async fn get_executions(&self, filter: Option<String>) -> Result<Vec<Execution>>
```
Retrieves trade execution reports with optional filtering.

**Execution Details:**
- Trade ID, time, and account
- Symbol and exchange
- Side (BOT/SLD)
- Quantity and price
- Order and client IDs

### 5. Market Scanning

#### Market Scanner
```rust
async fn scan_market(&self, subscription: ScannerSubscription) -> Result<Vec<ScannerData>>
```
Scans market for securities matching specified criteria.

**Scanner Parameters:**
- `scan_code`: Scan type (e.g., "TOP_PERC_GAIN", "HIGH_VOLUME")
- `instrument`: Security type filter
- `location_code`: Market location
- Price and volume filters
- Market cap constraints

## Usage Examples

### Example 1: Real-time Portfolio Monitoring
```rust
// Get current positions
let positions = client.get_positions("DU123456").await?;

// Fetch market data for each position
for position in positions {
    let market_data = client.get_market_data_snapshot(&position.symbol).await?;
    let current_value = position.position * market_data.last_price.unwrap_or(0.0);
    let unrealized_pnl = current_value - (position.position * position.average_cost);
    println!("{}: ${:.2} P&L: ${:.2}", position.symbol, current_value, unrealized_pnl);
}
```

### Example 2: Historical Analysis
```rust
let request = HistoricalDataRequest {
    symbol: "AAPL".to_string(),
    end_date_time: "20240115 16:00:00".to_string(),
    duration: "30 D".to_string(),
    bar_size: BarSize::Day1,
    what_to_show: WhatToShow::Trades,
    use_rth: true,
};

let bars = client.get_historical_data(request).await?;

// Calculate simple moving average
let sma_20 = bars.iter()
    .rev()
    .take(20)
    .map(|bar| bar.close)
    .sum::<f64>() / 20.0;
```

### Example 3: Market Scanning
```rust
let scanner = ScannerSubscription {
    number_of_rows: 25,
    instrument: "STK".to_string(),
    location_code: "STK.US.MAJOR".to_string(),
    scan_code: "HIGH_VOLUME_RATE".to_string(),
    above_price: Some(5.0),
    below_price: Some(100.0),
    above_volume: Some(1000000),
    market_cap_above: Some(1000000000.0),
    market_cap_below: None,
};

let results = client.scan_market(scanner).await?;
for result in results {
    println!("#{}: {} - {}", result.rank, result.contract.symbol, result.contract.primary_exchange);
}
```

## Data Types Reference

### Security Types
- `Stock`: Common stocks
- `Option`: Stock and index options
- `Future`: Futures contracts
- `Forex`: Currency pairs
- `Bond`: Fixed income
- `Commodity`: Physical commodities
- `Fund`: Mutual funds and ETFs

### Bar Sizes
- Second: `Sec1`, `Sec5`, `Sec15`, `Sec30`
- Minute: `Min1`, `Min2`, `Min3`, `Min5`, `Min15`, `Min20`, `Min30`
- Hour: `Hour1`
- Day: `Day1`

### Tick Types
- Price: `Bid`, `Ask`, `Last`, `High`, `Low`, `Close`, `Open`
- Size: `BidSize`, `AskSize`, `LastSize`, `Volume`
- Computed: `BidOptionComputation`, `AskOptionComputation`
- Status: `Halted`

## Error Handling

All API methods return `Result<T, IbkrError>` where errors include:
- `NotConnected`: Client not connected to TWS/Gateway
- `ConnectionFailed`: Connection attempt failed
- `ApiError`: IBKR API returned an error
- `RequestFailed`: Request could not be processed
- `SerializationError`: Data serialization failed

## Best Practices

1. **Connection Management**: Always check connection status before making API calls
2. **Rate Limiting**: IBKR has rate limits; implement appropriate delays between requests
3. **Error Recovery**: Implement retry logic for transient failures
4. **Data Validation**: Validate all data before processing
5. **Market Hours**: Be aware of market hours when requesting real-time data

## Testing

All interfaces have comprehensive mock implementations for testing:
```rust
let mock_client = MockIbkrClient::new();
mock_client.connect().await.unwrap();

// Set up test data
let snapshot = test_fixtures::sample_market_data_snapshot();
// ... test your logic
```

See `src-tauri/src/ibkr/tests/api_interface_tests.rs` for complete examples.
