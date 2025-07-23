use crate::ibkr::error::{IbkrError, Result};
use crate::ibkr::types::*;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

#[async_trait]
pub trait IbkrClientTrait: Send + Sync {
    async fn connect(&self) -> Result<()>;
    async fn disconnect(&self) -> Result<()>;
    async fn is_connected(&self) -> bool;
    async fn get_accounts(&self) -> Result<Vec<String>>;
    async fn get_account_summary(&self, account: &str) -> Result<Vec<AccountSummary>>;
    async fn get_positions(&self, account: &str) -> Result<Vec<Position>>;
    async fn subscribe_market_data(&self, contract_id: i32, symbol: &str) -> Result<()>;
    async fn place_order(&self, order: OrderRequest) -> Result<OrderStatus>;

    // New interface methods
    async fn get_market_data_snapshot(&self, symbol: &str) -> Result<MarketDataSnapshot>;
    async fn get_historical_data(
        &self,
        request: HistoricalDataRequest,
    ) -> Result<Vec<HistoricalBar>>;
    async fn get_contract_details(&self, symbol: &str) -> Result<ContractDetails>;
    async fn get_executions(&self, filter: Option<String>) -> Result<Vec<Execution>>;
    async fn get_account_values(&self, account: &str) -> Result<Vec<AccountValue>>;
    async fn scan_market(&self, subscription: ScannerSubscription) -> Result<Vec<ScannerData>>;
}

#[derive(Clone)]
pub struct MockIbkrClient {
    connected: Arc<RwLock<bool>>,
    accounts: Arc<RwLock<Vec<String>>>,
    positions: Arc<RwLock<Vec<Position>>>,
    account_summary: Arc<RwLock<Vec<AccountSummary>>>,
    error_mode: Arc<RwLock<Option<IbkrError>>>,
}

impl MockIbkrClient {
    pub fn new() -> Self {
        Self {
            connected: Arc::new(RwLock::new(false)),
            accounts: Arc::new(RwLock::new(vec!["DU123456".to_string()])),
            positions: Arc::new(RwLock::new(Vec::new())),
            account_summary: Arc::new(RwLock::new(Vec::new())),
            error_mode: Arc::new(RwLock::new(None)),
        }
    }

    pub fn with_error(error: IbkrError) -> Self {
        let mut client = Self::new();
        client.error_mode = Arc::new(RwLock::new(Some(error)));
        client
    }

    pub async fn set_connected(&self, connected: bool) {
        *self.connected.write().await = connected;
    }

    pub async fn set_accounts(&self, accounts: Vec<String>) {
        *self.accounts.write().await = accounts;
    }

    pub async fn set_positions(&self, positions: Vec<Position>) {
        *self.positions.write().await = positions;
    }

    pub async fn set_account_summary(&self, summary: Vec<AccountSummary>) {
        *self.account_summary.write().await = summary;
    }

    pub async fn set_error(&self, error: Option<IbkrError>) {
        *self.error_mode.write().await = error;
    }

    async fn check_error(&self) -> Result<()> {
        if let Some(ref error) = *self.error_mode.read().await {
            return Err(error.clone());
        }
        Ok(())
    }
}

#[async_trait]
impl IbkrClientTrait for MockIbkrClient {
    async fn connect(&self) -> Result<()> {
        self.check_error().await?;
        self.set_connected(true).await;
        Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
        self.check_error().await?;
        self.set_connected(false).await;
        Ok(())
    }

    async fn is_connected(&self) -> bool {
        *self.connected.read().await
    }

    async fn get_accounts(&self) -> Result<Vec<String>> {
        self.check_error().await?;
        if !self.is_connected().await {
            return Err(IbkrError::NotConnected);
        }
        Ok(self.accounts.read().await.clone())
    }

    async fn get_account_summary(&self, _account: &str) -> Result<Vec<AccountSummary>> {
        self.check_error().await?;
        if !self.is_connected().await {
            return Err(IbkrError::NotConnected);
        }

        Ok(self.account_summary.read().await.clone())
    }

    async fn get_positions(&self, _account: &str) -> Result<Vec<Position>> {
        self.check_error().await?;
        if !self.is_connected().await {
            return Err(IbkrError::NotConnected);
        }
        Ok(self.positions.read().await.clone())
    }

    async fn subscribe_market_data(&self, _contract_id: i32, _symbol: &str) -> Result<()> {
        self.check_error().await?;
        if !self.is_connected().await {
            return Err(IbkrError::NotConnected);
        }
        Ok(())
    }

    async fn place_order(&self, order: OrderRequest) -> Result<OrderStatus> {
        self.check_error().await?;
        if !self.is_connected().await {
            return Err(IbkrError::NotConnected);
        }

        Ok(OrderStatus {
            order_id: 12345,
            status: "Submitted".to_string(),
            filled: 0.0,
            remaining: order.quantity,
            average_fill_price: None,
        })
    }

    async fn get_market_data_snapshot(&self, symbol: &str) -> Result<MarketDataSnapshot> {
        self.check_error().await?;
        if !self.is_connected().await {
            return Err(IbkrError::NotConnected);
        }

        Ok(MarketDataSnapshot {
            symbol: symbol.to_string(),
            bid_price: Some(150.25),
            bid_size: Some(100),
            ask_price: Some(150.50),
            ask_size: Some(200),
            last_price: Some(150.35),
            last_size: Some(50),
            high: Some(152.00),
            low: Some(148.50),
            volume: Some(1234567),
            close: Some(149.80),
            open: Some(149.00),
            timestamp: chrono::Utc::now().timestamp(),
        })
    }

    async fn get_historical_data(
        &self,
        _request: HistoricalDataRequest,
    ) -> Result<Vec<HistoricalBar>> {
        self.check_error().await?;
        if !self.is_connected().await {
            return Err(IbkrError::NotConnected);
        }

        // Return mock historical data
        Ok(vec![
            HistoricalBar {
                time: "20240115 09:30:00".to_string(),
                open: 149.00,
                high: 149.50,
                low: 148.80,
                close: 149.30,
                volume: 10000,
                wap: 149.20,
                count: 250,
            },
            HistoricalBar {
                time: "20240115 09:31:00".to_string(),
                open: 149.30,
                high: 150.00,
                low: 149.20,
                close: 149.80,
                volume: 12000,
                wap: 149.65,
                count: 300,
            },
        ])
    }

    async fn get_contract_details(&self, symbol: &str) -> Result<ContractDetails> {
        self.check_error().await?;
        if !self.is_connected().await {
            return Err(IbkrError::NotConnected);
        }

        Ok(ContractDetails {
            symbol: symbol.to_string(),
            sec_type: SecurityType::Stock,
            exchange: "SMART".to_string(),
            primary_exchange: "NASDAQ".to_string(),
            currency: "USD".to_string(),
            local_symbol: symbol.to_string(),
            trading_class: symbol.to_string(),
            contract_id: 265598,
            min_tick: 0.01,
            multiplier: "".to_string(),
            price_magnifier: 1,
        })
    }

    async fn get_executions(&self, _filter: Option<String>) -> Result<Vec<Execution>> {
        self.check_error().await?;
        if !self.is_connected().await {
            return Err(IbkrError::NotConnected);
        }

        Ok(vec![Execution {
            exec_id: "0001".to_string(),
            time: "20240115 10:30:00".to_string(),
            account: "DU123456".to_string(),
            exchange: "NASDAQ".to_string(),
            side: "BOT".to_string(),
            shares: 100.0,
            price: 150.25,
            perm_id: 123456,
            client_id: 100,
            order_id: 12345,
            liquidation: false,
            cum_qty: 100.0,
            avg_price: 150.25,
        }])
    }

    async fn get_account_values(&self, account: &str) -> Result<Vec<AccountValue>> {
        self.check_error().await?;
        if !self.is_connected().await {
            return Err(IbkrError::NotConnected);
        }

        Ok(vec![
            AccountValue {
                key: "NetLiquidation".to_string(),
                value: "100000.00".to_string(),
                currency: "USD".to_string(),
                account: account.to_string(),
            },
            AccountValue {
                key: "TotalCashValue".to_string(),
                value: "50000.00".to_string(),
                currency: "USD".to_string(),
                account: account.to_string(),
            },
            AccountValue {
                key: "BuyingPower".to_string(),
                value: "200000.00".to_string(),
                currency: "USD".to_string(),
                account: account.to_string(),
            },
        ])
    }

    async fn scan_market(&self, _subscription: ScannerSubscription) -> Result<Vec<ScannerData>> {
        self.check_error().await?;
        if !self.is_connected().await {
            return Err(IbkrError::NotConnected);
        }

        Ok(vec![ScannerData {
            rank: 1,
            contract: ContractDetails {
                symbol: "AAPL".to_string(),
                sec_type: SecurityType::Stock,
                exchange: "SMART".to_string(),
                primary_exchange: "NASDAQ".to_string(),
                currency: "USD".to_string(),
                local_symbol: "AAPL".to_string(),
                trading_class: "AAPL".to_string(),
                contract_id: 265598,
                min_tick: 0.01,
                multiplier: "".to_string(),
                price_magnifier: 1,
            },
            distance: "".to_string(),
            benchmark: "".to_string(),
            projection: "".to_string(),
            legs: "".to_string(),
        }])
    }
}

pub mod test_fixtures {
    use super::*;

    pub fn sample_position() -> Position {
        Position {
            symbol: "AAPL".to_string(),
            position: 100.0,
            market_price: 150.0,
            market_value: 15000.0,
            average_cost: 145.0,
            unrealized_pnl: 500.0,
            realized_pnl: 0.0,
            account: "DU123456".to_string(),
            contract_type: "STK".to_string(),
            currency: "USD".to_string(),
            exchange: "NASDAQ".to_string(),
            local_symbol: "AAPL".to_string(),
        }
    }

    pub fn sample_account_summary() -> Vec<AccountSummary> {
        vec![
            AccountSummary {
                account: "DU123456".to_string(),
                tag: "NetLiquidation".to_string(),
                value: "100000.0".to_string(),
                currency: "USD".to_string(),
            },
            AccountSummary {
                account: "DU123456".to_string(),
                tag: "TotalCashValue".to_string(),
                value: "50000.0".to_string(),
                currency: "USD".to_string(),
            },
            AccountSummary {
                account: "DU123456".to_string(),
                tag: "BuyingPower".to_string(),
                value: "200000.0".to_string(),
                currency: "USD".to_string(),
            },
        ]
    }

    pub fn sample_order_request() -> OrderRequest {
        OrderRequest {
            symbol: "AAPL".to_string(),
            action: OrderAction::Buy,
            quantity: 100.0,
            order_type: OrderType::Limit,
            price: Some(150.0),
        }
    }

    #[allow(dead_code)]
    pub fn sample_market_data_snapshot() -> MarketDataSnapshot {
        MarketDataSnapshot {
            symbol: "AAPL".to_string(),
            bid_price: Some(150.25),
            bid_size: Some(100),
            ask_price: Some(150.50),
            ask_size: Some(200),
            last_price: Some(150.35),
            last_size: Some(50),
            high: Some(152.00),
            low: Some(148.50),
            volume: Some(1234567),
            close: Some(149.80),
            open: Some(149.00),
            timestamp: 1704825600, // Fixed timestamp for testing
        }
    }

    pub fn sample_historical_data_request() -> HistoricalDataRequest {
        HistoricalDataRequest {
            symbol: "AAPL".to_string(),
            end_date_time: "20240115 16:00:00".to_string(),
            duration: "1 D".to_string(),
            bar_size: BarSize::Min5,
            what_to_show: WhatToShow::Trades,
            use_rth: true,
        }
    }

    #[allow(dead_code)]
    pub fn sample_contract_details() -> ContractDetails {
        ContractDetails {
            symbol: "AAPL".to_string(),
            sec_type: SecurityType::Stock,
            exchange: "SMART".to_string(),
            primary_exchange: "NASDAQ".to_string(),
            currency: "USD".to_string(),
            local_symbol: "AAPL".to_string(),
            trading_class: "AAPL".to_string(),
            contract_id: 265598,
            min_tick: 0.01,
            multiplier: "".to_string(),
            price_magnifier: 1,
        }
    }

    pub fn sample_scanner_subscription() -> ScannerSubscription {
        ScannerSubscription {
            number_of_rows: 50,
            instrument: "STK".to_string(),
            location_code: "STK.US.MAJOR".to_string(),
            scan_code: "TOP_PERC_GAIN".to_string(),
            above_price: Some(10.0),
            below_price: Some(1000.0),
            above_volume: Some(100000),
            market_cap_above: Some(1000000000.0),
            market_cap_below: None,
        }
    }
}
