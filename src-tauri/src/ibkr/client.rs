use ibapi::contracts::Contract;
use ibapi::orders::Order;
use ibapi::Client;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::events::{AppEvent, EventEmitter};
use crate::ibkr::error::{IbkrError, Result};
use crate::ibkr::types::{
    AccountSummary, ConnectionConfig, ConnectionStatus, MarketDataType, OrderAction, OrderRequest,
    OrderType, Position,
};

pub struct DailyPnLHandle {
    shutdown: Arc<AtomicBool>,
    join: JoinHandle<()>,
}

impl DailyPnLHandle {
    pub async fn stop(self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Err(e) = self.join.await {
            warn!("Daily PnL task join error: {e}");
        }
    }
}

pub struct IbkrClient {
    client: Arc<RwLock<Option<Arc<Client>>>>,
    config: Arc<RwLock<ConnectionConfig>>,
    // Serializes calls into ibapi's shared `account_updates` channel. The
    // crossbeam receiver behind RequestAccountData is MPMC: two concurrent
    // readers will split incoming PortfolioValue/AccountValue messages
    // between them and the first to see AccountDownloadEnd will break out
    // before the other has consumed its share. See client.rs:get_positions.
    account_updates_lock: Arc<Mutex<()>>,
}

impl IbkrClient {
    #[allow(dead_code)]
    pub fn new(config: ConnectionConfig) -> Self {
        Self {
            client: Arc::new(RwLock::new(None)),
            config: Arc::new(RwLock::new(config)),
            account_updates_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn with_shared_config(config: Arc<RwLock<ConnectionConfig>>) -> Self {
        Self {
            client: Arc::new(RwLock::new(None)),
            config,
            account_updates_lock: Arc::new(Mutex::new(())),
        }
    }

    pub async fn connect(&self) -> Result<()> {
        // Check if already connected and verify the connection is still active
        {
            let client_lock = self.client.read().await;
            if let Some(ref client) = *client_lock {
                let client_clone = Arc::clone(client);
                drop(client_lock);

                // Verify the existing connection is still active with a timeout
                let check_result = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    tokio::task::spawn_blocking(move || client_clone.server_time().is_ok()),
                )
                .await;

                let is_active = match check_result {
                    Ok(Ok(result)) => result,
                    _ => {
                        info!("Connection check timed out or failed, treating as dead");
                        false
                    }
                };

                if is_active {
                    info!("Reusing existing active connection to IBKR API Gateway");
                    return Ok(());
                } else {
                    info!("Existing connection is dead, reconnecting");
                    self.disconnect().await?;
                }
            }
        }

        let config = self.config.read().await;
        let connection_url = format!("{}:{}", config.host, config.port);
        let client_id = config.client_id;
        let market_data_type = config.market_data_type;
        drop(config);

        info!(
            "🟢 CONNECTING TO IBKR API GATEWAY AT {} WITH CLIENT ID {}",
            connection_url, client_id
        );

        // Run the synchronous connect in a blocking task
        let connect_result =
            tokio::task::spawn_blocking(move || Client::connect(&connection_url, client_id))
                .await
                .map_err(|e| IbkrError::Unknown(e.to_string()))?;

        match connect_result {
            Ok(client) => {
                info!(
                    "🟢 SUCCESSFULLY CONNECTED TO IBKR API GATEWAY WITH CLIENT ID {}",
                    client_id
                );

                let client = Arc::new(client);

                // Apply the configured market data type so accounts without
                // real-time subscriptions (paper trading, etc.) still get ticks
                // instead of error 354. Failures here are non-fatal — the
                // connection is still usable, just with the server's default.
                Self::apply_market_data_type(&client, market_data_type).await;

                let mut client_lock = self.client.write().await;
                *client_lock = Some(client);
                Ok(())
            }
            Err(e) => {
                error!(
                    "🟢 FAILED TO CONNECT TO IBKR API GATEWAY WITH CLIENT ID {}: {}",
                    client_id, e
                );
                Err(IbkrError::ConnectionFailed(e.to_string()))
            }
        }
    }

    async fn apply_market_data_type(client: &Arc<Client>, market_data_type: MarketDataType) {
        let client_clone = Arc::clone(client);
        let mapped = market_data_type.into();
        let result =
            tokio::task::spawn_blocking(move || client_clone.switch_market_data_type(mapped)).await;

        match result {
            Ok(Ok(())) => {
                info!(
                    "🟢 MARKET DATA TYPE SET TO {:?} FOR THIS CONNECTION",
                    market_data_type
                );
            }
            Ok(Err(e)) => {
                error!(
                    "Failed to switch market data type to {:?}: {}",
                    market_data_type, e
                );
            }
            Err(e) => {
                error!(
                    "Join error while switching market data type to {:?}: {}",
                    market_data_type, e
                );
            }
        }
    }

    pub async fn disconnect(&self) -> Result<()> {
        info!("🔴 CLIENT DISCONNECT METHOD CALLED");
        let mut client_lock = self.client.write().await;
        info!("🔴 CLIENT WRITE LOCK ACQUIRED");

        if let Some(client) = client_lock.take() {
            info!("🔴 CLIENT EXISTS - DISCONNECTING FROM IBKR API GATEWAY");

            // Drop the client in a blocking task with a timeout
            // Wait for it to complete to ensure TWS/Gateway releases the client ID
            let drop_result = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                tokio::task::spawn_blocking(move || {
                    info!("🔴 BLOCKING TASK - DROPPING CLIENT");
                    drop(client);
                    info!("🔴 IBKR CLIENT DROPPED SUCCESSFULLY");
                }),
            )
            .await;

            match drop_result {
                Ok(Ok(_)) => {
                    info!("🔴 CLIENT DROP COMPLETED");
                }
                Ok(Err(e)) => {
                    error!("🔴 CLIENT DROP TASK ERROR: {}", e);
                }
                Err(_) => {
                    info!("🔴 CLIENT DROP TIMED OUT (but connection removed from state)");
                }
            }

            // Add a small delay to ensure TWS/Gateway has fully released the client ID
            info!("🔴 WAITING 1 SECOND FOR TWS/GATEWAY TO RELEASE CLIENT ID");
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            info!("🔴 DISCONNECTED FROM IBKR API GATEWAY");

            Ok(())
        } else {
            info!("🔴 NO CLIENT EXISTS - ALREADY DISCONNECTED FROM IBKR API GATEWAY");
            Ok(())
        }
    }

    #[allow(dead_code)]
    pub async fn is_connected(&self) -> bool {
        let client = self.client.read().await;
        client.is_some()
    }

    pub async fn get_connection_status(&self) -> Result<ConnectionStatus> {
        let client = self.client.read().await;
        let config = self.config.read().await;

        if let Some(ref client) = *client {
            let client_clone = Arc::clone(client);

            // Check server time with a timeout to prevent hanging
            let time_check = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                tokio::task::spawn_blocking(move || client_clone.server_time()),
            )
            .await;

            match time_check {
                Ok(Ok(Ok(time))) => {
                    // Successfully got server time
                    Ok(ConnectionStatus {
                        connected: true,
                        server_time: Some(time.to_string()),
                        client_id: config.client_id,
                    })
                }
                _ => {
                    // Timeout, spawn error, or server_time error - treat as disconnected
                    info!("Connection status check failed or timed out");
                    Ok(ConnectionStatus {
                        connected: false,
                        server_time: None,
                        client_id: config.client_id,
                    })
                }
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
        let client_clone = {
            let client = self.client.read().await;
            let client = client.as_ref().ok_or(IbkrError::NotConnected)?;
            Arc::clone(client)
        }; // Lock is dropped here!

        let accounts = tokio::task::spawn_blocking(move || client_clone.managed_accounts())
            .await
            .map_err(|e| IbkrError::Unknown(e.to_string()))?;

        match accounts {
            Ok(accounts) => {
                tracing::info!("Retrieved accounts: {:?}", accounts);
                Ok(accounts)
            }
            Err(e) => Err(IbkrError::from(e)),
        }
    }

    pub async fn get_account_summary(&self, account: &str) -> Result<Vec<AccountSummary>> {
        let client_clone = {
            let client = self.client.read().await;
            let client = client.as_ref().ok_or(IbkrError::NotConnected)?;
            Arc::clone(client)
        }; // Lock is dropped here!

        let account = account.to_string();
        let updates_guard = self.account_updates_lock.clone().lock_owned().await;

        let summaries = tokio::task::spawn_blocking(move || {
            let _updates_guard = updates_guard;
            let mut summaries = Vec::new();

            // Use account_updates to get all account values
            match client_clone.account_updates(&account) {
                Ok(stream) => {
                    for update in stream {
                        match update {
                            ibapi::accounts::AccountUpdate::AccountValue(value) => {
                                tracing::info!(
                                    "Account value: key={}, value={}, currency={}",
                                    value.key,
                                    value.value,
                                    value.currency
                                );
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
                Err(e) => Err(IbkrError::from(e)),
            }
        })
        .await
        .map_err(|e| IbkrError::Unknown(e.to_string()))?;

        summaries
    }

    pub async fn get_positions(&self) -> Result<Vec<Position>> {
        // First get the accounts (before acquiring any locks)
        let accounts = self.get_accounts().await?;
        if accounts.is_empty() {
            return Ok(Vec::new());
        }

        let account = accounts[0].clone(); // Use first account

        let client_clone = {
            let client = self.client.read().await;
            let client = client.as_ref().ok_or(IbkrError::NotConnected)?;
            Arc::clone(client)
        }; // Lock is dropped here!

        let updates_guard = self.account_updates_lock.clone().lock_owned().await;

        let positions = tokio::task::spawn_blocking(move || {
            let _updates_guard = updates_guard;
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
        let client_clone = {
            let client = self.client.read().await;
            let client = client.as_ref().ok_or(IbkrError::NotConnected)?;
            Arc::clone(client)
        }; // Lock is dropped here!

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
                Err(e) => Err(IbkrError::from(e)),
            }
        })
        .await
        .map_err(|e| IbkrError::Unknown(e.to_string()))?
    }

    pub async fn start_daily_pnl_stream(
        &self,
        account: &str,
        emitter: Arc<EventEmitter>,
    ) -> Result<DailyPnLHandle> {
        let client_clone = {
            let client = self.client.read().await;
            let client = client.as_ref().ok_or(IbkrError::NotConnected)?;
            Arc::clone(client)
        };

        let account = account.to_string();
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_task = Arc::clone(&shutdown);

        let join = tokio::task::spawn_blocking(move || {
            let subscription = match client_clone.pnl(&account, None) {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to subscribe to daily PnL for {account}: {e}");
                    return;
                }
            };

            info!("Daily PnL subscription started for {account}");
            while !shutdown_task.load(Ordering::Relaxed) {
                if let Some(pnl) = subscription.next_timeout(Duration::from_millis(500)) {
                    let event = AppEvent::DailyPnLUpdate {
                        account: account.clone(),
                        daily_pnl: pnl.daily_pnl,
                        unrealized_pnl: pnl.unrealized_pnl,
                        realized_pnl: pnl.realized_pnl,
                    };
                    let emitter = Arc::clone(&emitter);
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = emitter.emit(event).await {
                            warn!("Failed to emit daily-pnl-update: {e}");
                        }
                    });
                }
            }

            subscription.cancel();
            info!("Daily PnL subscription stopped for {account}");
        });

        Ok(DailyPnLHandle { shutdown, join })
    }

    pub async fn place_order(&self, order_request: OrderRequest) -> Result<i32> {
        let client_clone = {
            let client = self.client.read().await;
            let client = client.as_ref().ok_or(IbkrError::NotConnected)?;
            Arc::clone(client)
        }; // Lock is dropped here!

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
                _ => {
                    return Err(IbkrError::RequestFailed(
                        "Order type not implemented".to_string(),
                    ))
                }
            };

            match client_clone.place_order(order_id, &contract, &order) {
                Ok(_subscription) => {
                    // TODO: Handle order status updates
                    Ok(order_id)
                }
                Err(e) => Err(IbkrError::from(e)),
            }
        })
        .await
        .map_err(|e| IbkrError::Unknown(e.to_string()))?;

        order_id
    }
}
