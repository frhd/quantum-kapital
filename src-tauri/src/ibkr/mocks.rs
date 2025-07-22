use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::ibkr::types::*;
use crate::ibkr::error::{IbkrError, Result};

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
}