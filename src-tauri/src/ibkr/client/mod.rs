mod historical;
mod market_data;
mod orders;
mod streams;

pub use self::streams::StreamHandle;

use ibapi::accounts::types::AccountId;
use ibapi::client::blocking::Client;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{error, info};

use crate::ibkr::error::{IbkrError, Result};
use crate::ibkr::types::{
    AccountSummary, ConnectionConfig, ConnectionStatus, MarketDataType, Position,
};

pub struct IbkrClient {
    pub(super) client: Arc<RwLock<Option<Arc<Client>>>>,
    pub(super) config: Arc<RwLock<ConnectionConfig>>,
    // Serializes calls into ibapi's shared `account_updates` channel. The
    // crossbeam receiver behind RequestAccountData is MPMC: two concurrent
    // readers will split incoming PortfolioValue/AccountValue messages
    // between them and the first to see AccountDownloadEnd will break out
    // before the other has consumed its share. See client.rs:get_positions.
    pub(super) account_updates_lock: Arc<Mutex<()>>,
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
            let account_id = AccountId(account.clone());
            match client_clone.account_updates(&account_id) {
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
            let account_id = AccountId(account.clone());
            match client_clone.account_updates(&account_id) {
                Ok(stream) => {
                    for update in stream {
                        match update {
                            ibapi::accounts::AccountUpdate::PortfolioValue(portfolio) => {
                                tracing::info!("Portfolio position: symbol={}, position={}, market_price={}, market_value={}, unrealized_pnl={}",
                                    portfolio.contract.symbol, portfolio.position, portfolio.market_price,
                                    portfolio.market_value, portfolio.unrealized_pnl);
                                positions.push(Position {
                                    account: portfolio.account.clone().unwrap_or_else(|| account.clone()),
                                    symbol: portfolio.contract.symbol.0.clone(),
                                    position: portfolio.position,
                                    average_cost: portfolio.average_cost,
                                    market_price: portfolio.market_price,
                                    market_value: portfolio.market_value,
                                    unrealized_pnl: portfolio.unrealized_pnl,
                                    realized_pnl: portfolio.realized_pnl,
                                    contract_type: portfolio.contract.security_type.clone().to_string(),
                                    currency: portfolio.contract.currency.0.clone(),
                                    exchange: portfolio.contract.exchange.0.clone(),
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
}
