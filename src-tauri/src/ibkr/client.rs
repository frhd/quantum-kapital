use ibapi::Client;
use ibapi::contracts::Contract;
use ibapi::orders::Order;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{info, error};

use crate::ibkr::types::*;
use crate::ibkr::error::{IbkrError, Result};

pub struct IbkrClient {
    client: Arc<RwLock<Option<Arc<Client>>>>,
    config: Arc<Mutex<ConnectionConfig>>,
}

impl IbkrClient {
    pub fn new(config: ConnectionConfig) -> Self {
        Self {
            client: Arc::new(RwLock::new(None)),
            config: Arc::new(Mutex::new(config)),
        }
    }

    pub async fn connect(&self) -> Result<()> {
        let config = self.config.lock().await;
        let connection_url = format!("{}:{}", config.host, config.port);
        let client_id = config.client_id;
        drop(config);

        info!("Connecting to IBKR API Gateway at {}", connection_url);

        // Run the synchronous connect in a blocking task
        let connect_result = tokio::task::spawn_blocking(move || {
            Client::connect(&connection_url, client_id)
        }).await.map_err(|e| IbkrError::Unknown(e.to_string()))?;

        match connect_result {
            Ok(client) => {
                info!("Successfully connected to IBKR API Gateway");
                let mut client_lock = self.client.write().await;
                *client_lock = Some(Arc::new(client));
                Ok(())
            }
            Err(e) => {
                error!("Failed to connect to IBKR API Gateway: {}", e);
                Err(IbkrError::ConnectionFailed(e.to_string()))
            }
        }
    }

    pub async fn disconnect(&self) -> Result<()> {
        let mut client_lock = self.client.write().await;
        if client_lock.is_some() {
            *client_lock = None;
            info!("Disconnected from IBKR API Gateway");
        }
        Ok(())
    }

    pub async fn is_connected(&self) -> bool {
        let client = self.client.read().await;
        client.is_some()
    }

    pub async fn get_connection_status(&self) -> Result<ConnectionStatus> {
        let client = self.client.read().await;
        let config = self.config.lock().await;
        
        if let Some(ref client) = *client {
            let client_clone = Arc::clone(client);
            let time_result = tokio::task::spawn_blocking(move || {
                client_clone.server_time()
            }).await.map_err(|e| IbkrError::Unknown(e.to_string()))?;

            match time_result {
                Ok(time) => Ok(ConnectionStatus {
                    connected: true,
                    server_time: Some(time.to_string()),
                    client_id: config.client_id,
                }),
                Err(_) => Ok(ConnectionStatus {
                    connected: false,
                    server_time: None,
                    client_id: config.client_id,
                })
            }
        } else {
            Ok(ConnectionStatus {
                connected: false,
                server_time: None,
                client_id: config.client_id,
            })
        }
    }

    pub async fn get_accounts(&self) -> Result<Vec<String>> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(IbkrError::NotConnected)?;
        let client_clone = Arc::clone(client);
        
        let accounts = tokio::task::spawn_blocking(move || {
            client_clone.managed_accounts()
        }).await.map_err(|e| IbkrError::Unknown(e.to_string()))?;
        
        match accounts {
            Ok(accounts) => {
                tracing::info!("Retrieved accounts: {:?}", accounts);
                Ok(accounts)
            }
            Err(e) => Err(IbkrError::from(e))
        }
    }

    pub async fn get_account_summary(&self, account: &str) -> Result<Vec<AccountSummary>> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(IbkrError::NotConnected)?;
        let client_clone = Arc::clone(client);
        let account = account.to_string();
        
        let summaries = tokio::task::spawn_blocking(move || {
            let mut summaries = Vec::new();
            
            // Use account_updates to get all account values
            match client_clone.account_updates(&account) {
                Ok(stream) => {
                    for update in stream {
                        match update {
                            ibapi::accounts::AccountUpdate::AccountValue(value) => {
                                tracing::info!("Account value: key={}, value={}, currency={}", 
                                    value.key, value.value, value.currency);
                                summaries.push(AccountSummary {
                                    account: account.clone(),
                                    tag: value.key,
                                    value: value.value,
                                    currency: value.currency,
                                });
                            }
                            ibapi::accounts::AccountUpdate::End => {
                                tracing::info!("Account updates end reached");
                                break;
                            }
                            _ => {} // Ignore other update types (PortfolioValue, UpdateTime)
                        }
                    }
                    Ok(summaries)
                }
                Err(e) => Err(IbkrError::from(e))
            }
        }).await.map_err(|e| IbkrError::Unknown(e.to_string()))?;
        
        summaries
    }

    pub async fn get_positions(&self) -> Result<Vec<Position>> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(IbkrError::NotConnected)?;
        let client_clone = Arc::clone(client);
        
        // First get the accounts
        let accounts = self.get_accounts().await?;
        if accounts.is_empty() {
            return Ok(Vec::new());
        }
        
        let account = accounts[0].clone(); // Use first account
        
        let positions = tokio::task::spawn_blocking(move || {
            let mut positions = Vec::new();
            
            // Use account_updates to get portfolio values with market data
            match client_clone.account_updates(&account) {
                Ok(stream) => {
                    for update in stream {
                        match update {
                            ibapi::accounts::AccountUpdate::PortfolioValue(portfolio) => {
                                tracing::info!("Portfolio position: symbol={}, position={}, market_price={}, market_value={}, unrealized_pnl={}", 
                                    portfolio.contract.symbol, portfolio.position, portfolio.market_price, 
                                    portfolio.market_value, portfolio.unrealized_pnl);
                                
                                positions.push(Position {
                                    account: portfolio.account.unwrap_or_else(|| account.clone()),
                                    symbol: portfolio.contract.symbol.clone(),
                                    position: portfolio.position,
                                    average_cost: portfolio.average_cost,
                                    market_price: portfolio.market_price,
                                    market_value: portfolio.market_value,
                                    unrealized_pnl: portfolio.unrealized_pnl,
                                    realized_pnl: portfolio.realized_pnl,
                                    contract_type: portfolio.contract.security_type.clone().to_string(),
                                    currency: portfolio.contract.currency.clone(),
                                    exchange: portfolio.contract.exchange.clone(),
                                    local_symbol: portfolio.contract.local_symbol.clone(),
                                });
                            }
                            ibapi::accounts::AccountUpdate::End => {
                                tracing::info!("All portfolio positions received");
                                break;
                            }
                            _ => {} // Ignore other update types
                        }
                    }
                    Ok(positions)
                }
                Err(e) => Err(IbkrError::from(e))
            }
        }).await.map_err(|e| IbkrError::Unknown(e.to_string()))?;
        
        positions
    }

    pub async fn subscribe_market_data(&self, symbol: &str) -> Result<()> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(IbkrError::NotConnected)?;
        let client_clone = Arc::clone(client);
        let symbol = symbol.to_string();
        
        tokio::task::spawn_blocking(move || {
            let contract = Contract::stock(&symbol);
            // For now, we'll request basic tick types
            let tick_types = &["233"]; // RTVolume
            match client_clone.market_data(&contract, tick_types, false, false) {
                Ok(_subscription) => {
                    // TODO: Store subscription and handle market data updates
                    Ok(())
                }
                Err(e) => Err(IbkrError::from(e))
            }
        }).await.map_err(|e| IbkrError::Unknown(e.to_string()))?
    }

    pub async fn place_order(&self, order_request: OrderRequest) -> Result<i32> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(IbkrError::NotConnected)?;
        let client_clone = Arc::clone(client);
        
        let order_id = tokio::task::spawn_blocking(move || {
            let contract = Contract::stock(&order_request.symbol);
            let order_id = client_clone.next_order_id();
            
            let mut order = Order::default();
            
            // Set action and order type using the ibapi types
            use ibapi::orders::Action;
            
            order.action = match order_request.action {
                OrderAction::Buy => Action::Buy,
                OrderAction::Sell => Action::Sell,
            };
            
            order.total_quantity = order_request.quantity;
            
            // Set order type - Type is likely a string in ibapi
            match order_request.order_type {
                OrderType::Market => {
                    order.order_type = "MKT".to_string();
                }
                OrderType::Limit => {
                    order.order_type = "LMT".to_string();
                    order.limit_price = order_request.price;
                }
                _ => return Err(IbkrError::RequestFailed("Order type not implemented".to_string())),
            };
            
            match client_clone.place_order(order_id, &contract, &order) {
                Ok(_subscription) => {
                    // TODO: Handle order status updates
                    Ok(order_id)
                }
                Err(e) => Err(IbkrError::from(e))
            }
        }).await.map_err(|e| IbkrError::Unknown(e.to_string()))?;
        
        order_id
    }
}