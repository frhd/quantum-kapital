mod executions_merge;
mod historical;
mod market_data;
mod news;
mod orders;
mod streams;

pub use self::streams::StreamHandle;

use ibapi::accounts::types::AccountId;
use ibapi::client::blocking::Client;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{Mutex, RwLock};
use tracing::{error, info};

use crate::events::{AppEvent, EventEmitter};
use crate::ibkr::error::{IbkrError, Result};
use crate::ibkr::types::{
    AccountSummary, ConnectionConfig, ConnectionStatus, DataTier, MarketDataType, Position,
};

/// Sink wired in by `IbkrState::new` so the probe-on-connect task and
/// the disconnect path can publish a tier without `IbkrClient` knowing
/// about state. Stays `None` in tests that construct an `IbkrClient`
/// directly.
#[derive(Clone)]
pub(super) struct TierSink {
    pub tier: Arc<RwLock<DataTier>>,
    pub emitter: Arc<EventEmitter>,
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
    tier_sink: Arc<StdMutex<Option<TierSink>>>,
}

impl IbkrClient {
    /// Snapshot the underlying `ibapi::Client` handle. Returns `NotConnected`
    /// when no connection is live; otherwise clones the `Arc` so the
    /// caller can run `spawn_blocking` work without holding the read lock.
    pub(super) async fn ibapi_client(&self) -> Result<Arc<Client>> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(IbkrError::NotConnected)?;
        Ok(Arc::clone(client))
    }
}

impl IbkrClient {
    #[allow(dead_code)]
    pub fn new(config: ConnectionConfig) -> Self {
        Self {
            client: Arc::new(RwLock::new(None)),
            config: Arc::new(RwLock::new(config)),
            account_updates_lock: Arc::new(Mutex::new(())),
            tier_sink: Arc::new(StdMutex::new(None)),
        }
    }

    pub fn with_shared_config(config: Arc<RwLock<ConnectionConfig>>) -> Self {
        Self {
            client: Arc::new(RwLock::new(None)),
            config,
            account_updates_lock: Arc::new(Mutex::new(())),
            tier_sink: Arc::new(StdMutex::new(None)),
        }
    }

    /// Wire the data-tier sink. `IbkrState::new` calls this once after
    /// constructing both the client and the emitter, so the
    /// probe-on-connect task and `disconnect()` can publish without
    /// going back through `IbkrState`. Sync because `IbkrState::new`
    /// is sync and the `std::sync::Mutex` is uncontended at that
    /// point.
    pub fn set_tier_sink(&self, tier: Arc<RwLock<DataTier>>, emitter: Arc<EventEmitter>) {
        let mut sink = self.tier_sink.lock().expect("tier_sink poisoned");
        *sink = Some(TierSink { tier, emitter });
    }

    /// Snapshot of the wired sink (cloned so callers don't hold the
    /// lock across awaits).
    fn tier_sink_snapshot(&self) -> Option<TierSink> {
        self.tier_sink.lock().expect("tier_sink poisoned").clone()
    }

    async fn publish_tier(sink: &TierSink, tier: DataTier) {
        *sink.tier.write().await = tier;
        if let Err(e) = sink.emitter.emit(AppEvent::DataTierDetected { tier }).await {
            // Pre-AppHandle attachment (early startup) returns this — not fatal.
            tracing::debug!("DataTierDetected emit skipped: {}", e);
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
                *client_lock = Some(Arc::clone(&client));
                drop(client_lock);

                // Probe the actual delivered tier off the live connection.
                // Async + non-blocking so the connect button stays snappy;
                // the sink defaults to `Unknown` until the probe completes.
                if let Some(sink) = self.tier_sink_snapshot() {
                    let probe_client = Arc::clone(&client);
                    tokio::spawn(async move {
                        Self::apply_market_data_type(&probe_client, MarketDataType::Live).await;
                        let tier = market_data::run_probe(Arc::clone(&probe_client), "SPY")
                            .await
                            .unwrap_or_else(|e| {
                                tracing::warn!("data-tier probe failed: {}", e);
                                DataTier::Unknown
                            });
                        // Restore so live operation matches user intent.
                        Self::apply_market_data_type(&probe_client, market_data_type).await;
                        info!("🟢 DATA TIER PROBE → {:?}", tier);
                        Self::publish_tier(&sink, tier).await;
                    });
                }

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
        // Reset the tier first so consumers see `Unknown` immediately
        // (before the blocking client drop holds the write lock).
        if let Some(sink) = self.tier_sink_snapshot() {
            Self::publish_tier(&sink, DataTier::Unknown).await;
        }

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
        let client_clone = self.ibapi_client().await?;

        let accounts = tokio::task::spawn_blocking(move || client_clone.managed_accounts())
            .await
            .map_err(|e| IbkrError::Unknown(e.to_string()))?;

        match accounts {
            Ok(accounts) => {
                tracing::debug!("Retrieved accounts: {:?}", accounts);
                Ok(accounts)
            }
            Err(e) => Err(IbkrError::from(e)),
        }
    }

    pub async fn get_account_summary(&self, account: &str) -> Result<Vec<AccountSummary>> {
        let client_clone = self.ibapi_client().await?;

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

    pub async fn get_positions(&self, account: &str) -> Result<Vec<Position>> {
        let client_clone = self.ibapi_client().await?;

        let updates_guard = self.account_updates_lock.clone().lock_owned().await;
        let account = account.to_string();

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
                                tracing::debug!("Portfolio position: symbol={}, position={}, market_price={}, market_value={}, unrealized_pnl={}",
                                    portfolio.contract.symbol, portfolio.position, portfolio.market_price,
                                    portfolio.market_value, portfolio.unrealized_pnl);
                                let contract_type =
                                    portfolio.contract.security_type.clone().to_string();
                                // Stock contracts come back with empty
                                // strings / 0.0 in the option-only fields;
                                // surface them as `None` so the JSON omits
                                // them entirely (see `Position` serde
                                // `skip_serializing_if`).
                                let is_option_like =
                                    matches!(contract_type.as_str(), "OPT" | "FOP" | "FUT" | "WAR");
                                let opt_string = |s: String| {
                                    if is_option_like && !s.is_empty() {
                                        Some(s)
                                    } else {
                                        None
                                    }
                                };
                                let opt_f64 = |v: f64| {
                                    if is_option_like && v != 0.0 {
                                        Some(v)
                                    } else {
                                        None
                                    }
                                };
                                positions.push(Position {
                                    account: portfolio.account.clone().unwrap_or_else(|| account.clone()),
                                    symbol: portfolio.contract.symbol.0.clone(),
                                    position: portfolio.position,
                                    average_cost: portfolio.average_cost,
                                    market_price: portfolio.market_price,
                                    market_value: portfolio.market_value,
                                    unrealized_pnl: portfolio.unrealized_pnl,
                                    realized_pnl: portfolio.realized_pnl,
                                    contract_type,
                                    currency: portfolio.contract.currency.0.clone(),
                                    exchange: portfolio.contract.exchange.0.clone(),
                                    local_symbol: portfolio.contract.local_symbol.clone(),
                                    expiry: opt_string(
                                        portfolio.contract.last_trade_date_or_contract_month.clone(),
                                    ),
                                    strike: opt_f64(portfolio.contract.strike),
                                    right: opt_string(portfolio.contract.right.clone()),
                                    multiplier: opt_string(portfolio.contract.multiplier.clone()),
                                });
                            }
                            ibapi::accounts::AccountUpdate::End => {
                                tracing::debug!("All portfolio positions received");
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
