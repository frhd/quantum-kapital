use ibapi::accounts::types::AccountId;
use ibapi::client::blocking::Client;
use ibapi::contracts::Contract;
use ibapi::orders::Order;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::events::{AppEvent, EventEmitter};
use crate::ibkr::error::{IbkrError, Result};
use crate::ibkr::types::historical::{
    BarSize as OurBarSize, HistoricalBar, HistoricalDataRequest, WhatToShow as OurWhatToShow,
};
use crate::ibkr::types::{
    AccountSummary, ConnectionConfig, ConnectionStatus, ContractDetails, MarketDataType,
    OrderAction, OrderRequest, OrderType, Position, ScannerData, ScannerSubscription, SecurityType,
};

pub struct StreamHandle {
    name: &'static str,
    shutdown: Arc<AtomicBool>,
    join: JoinHandle<()>,
}

impl StreamHandle {
    pub async fn stop(self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Err(e) = self.join.await {
            warn!("{} task join error: {e}", self.name);
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

    pub async fn subscribe_market_data(&self, symbol: &str) -> Result<()> {
        let client_clone = {
            let client = self.client.read().await;
            let client = client.as_ref().ok_or(IbkrError::NotConnected)?;
            Arc::clone(client)
        }; // Lock is dropped here!

        let symbol = symbol.to_string();

        tokio::task::spawn_blocking(move || {
            let contract = Contract::stock(&symbol).build();
            // For now, we'll request basic tick types
            let tick_types = &["233"]; // RTVolume
            match client_clone
                .market_data(&contract)
                .generic_ticks(tick_types)
                .subscribe()
            {
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
    ) -> Result<StreamHandle> {
        let client_clone = {
            let client = self.client.read().await;
            let client = client.as_ref().ok_or(IbkrError::NotConnected)?;
            Arc::clone(client)
        };

        let account = account.to_string();
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_task = Arc::clone(&shutdown);

        let join = tokio::task::spawn_blocking(move || {
            let account_id = AccountId(account.clone());
            let subscription = match client_clone.pnl(&account_id, None) {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to subscribe to daily PnL for {account}: {e}");
                    return;
                }
            };

            info!("Daily PnL subscription started for {account}");
            while !shutdown_task.load(Ordering::Relaxed) {
                if let Some(pnl) = subscription.next_timeout(Duration::from_secs(1)) {
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

        Ok(StreamHandle {
            name: "Daily PnL",
            shutdown,
            join,
        })
    }

    pub async fn start_scanner_stream(
        &self,
        opts: ScannerSubscription,
        emitter: Arc<EventEmitter>,
    ) -> Result<StreamHandle> {
        let client_clone = {
            let client = self.client.read().await;
            let client = client.as_ref().ok_or(IbkrError::NotConnected)?;
            Arc::clone(client)
        };

        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_task = Arc::clone(&shutdown);

        let scan_code = opts.scan_code.clone();
        let location_code = opts.location_code.clone();

        let join = tokio::task::spawn_blocking(move || {
            let ib_sub = to_ibapi_scanner_subscription(&opts);
            let filter: Vec<ibapi::orders::TagValue> = Vec::new();

            let subscription = match client_clone.scanner_subscription(&ib_sub, &filter) {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to start scanner subscription: {e}");
                    return;
                }
            };

            info!("Scanner subscription started: scan_code={scan_code}, location={location_code}");

            while !shutdown_task.load(Ordering::Relaxed) {
                if let Some(rows) = subscription.next_timeout(Duration::from_secs(1)) {
                    let results: Vec<ScannerData> =
                        rows.iter().map(from_ibapi_scanner_data).collect();
                    let event = AppEvent::ScannerUpdate { results };
                    let emitter = Arc::clone(&emitter);
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = emitter.emit(event).await {
                            warn!("Failed to emit scanner-update: {e}");
                        }
                    });
                }
            }

            subscription.cancel();
            info!("Scanner subscription stopped");
        });

        Ok(StreamHandle {
            name: "Scanner",
            shutdown,
            join,
        })
    }

    /// Fetches historical bars from IBKR. Real-IBKR live integration is
    /// not exercised by unit tests in Phase 02 — service tests use a
    /// mock fetcher implementing `HistoricalDataFetcher`. The body here
    /// translates our domain types to ibapi 2's enums and runs the
    /// blocking call on `spawn_blocking` like the rest of this module.
    pub async fn get_historical_data(
        &self,
        request: HistoricalDataRequest,
    ) -> Result<Vec<HistoricalBar>> {
        use ibapi::market_data::historical::{
            BarSize as IbBarSize, Duration as IbDuration, WhatToShow as IbWhatToShow,
        };
        use ibapi::market_data::TradingHours;

        let client_clone = {
            let client = self.client.read().await;
            let client = client.as_ref().ok_or(IbkrError::NotConnected)?;
            Arc::clone(client)
        };

        let bars = tokio::task::spawn_blocking(move || -> Result<Vec<HistoricalBar>> {
            let contract = Contract::stock(&request.symbol).build();
            let ib_bar = match request.bar_size {
                OurBarSize::Sec1 => IbBarSize::Sec,
                OurBarSize::Sec5 => IbBarSize::Sec5,
                OurBarSize::Sec15 => IbBarSize::Sec15,
                OurBarSize::Sec30 => IbBarSize::Sec30,
                OurBarSize::Min1 => IbBarSize::Min,
                OurBarSize::Min2 => IbBarSize::Min2,
                OurBarSize::Min3 => IbBarSize::Min3,
                OurBarSize::Min5 => IbBarSize::Min5,
                OurBarSize::Min15 => IbBarSize::Min15,
                OurBarSize::Min20 => IbBarSize::Min20,
                OurBarSize::Min30 => IbBarSize::Min30,
                OurBarSize::Hour1 => IbBarSize::Hour,
                OurBarSize::Day1 => IbBarSize::Day,
            };
            let ib_what = match request.what_to_show {
                OurWhatToShow::Trades => IbWhatToShow::Trades,
                OurWhatToShow::Midpoint => IbWhatToShow::MidPoint,
                OurWhatToShow::Bid => IbWhatToShow::Bid,
                OurWhatToShow::Ask => IbWhatToShow::Ask,
                OurWhatToShow::BidAsk => IbWhatToShow::BidAsk,
                OurWhatToShow::HistoricalVolatility => IbWhatToShow::HistoricalVolatility,
                OurWhatToShow::OptionImpliedVolatility => IbWhatToShow::OptionImpliedVolatility,
            };

            // Parse our "{N} {UNIT}" duration string back into ibapi's Duration.
            // We only emit "{N} D" from the service so this is the common path.
            let ib_duration: IbDuration = request.duration.parse().map_err(|e| {
                IbkrError::RequestFailed(format!(
                    "invalid duration string '{}': {e}",
                    request.duration
                ))
            })?;

            let trading_hours = if request.use_rth {
                TradingHours::Regular
            } else {
                TradingHours::Extended
            };

            // We pass `None` for end_date_time and let IBKR default to "now".
            // The end_date_time string in our request type is informational
            // for now; a future revision can route it through OffsetDateTime.
            let data = client_clone
                .historical_data(&contract, None, ib_duration, ib_bar, ib_what, trading_hours)
                .map_err(IbkrError::from)?;

            Ok(data
                .bars
                .into_iter()
                .map(|b| {
                    let ts = b.date.unix_timestamp();
                    let chrono_dt =
                        chrono::DateTime::from_timestamp(ts, 0).unwrap_or_else(chrono::Utc::now);
                    let formatted = if request.bar_size == OurBarSize::Day1 {
                        chrono_dt.format("%Y%m%d").to_string()
                    } else {
                        chrono_dt.format("%Y%m%d %H:%M:%S").to_string()
                    };
                    HistoricalBar {
                        time: formatted,
                        open: b.open,
                        high: b.high,
                        low: b.low,
                        close: b.close,
                        volume: b.volume as i64,
                        wap: b.wap,
                        count: b.count,
                    }
                })
                .collect())
        })
        .await
        .map_err(|e| IbkrError::Unknown(e.to_string()))??;

        Ok(bars)
    }

    pub async fn place_order(&self, order_request: OrderRequest) -> Result<i32> {
        let client_clone = {
            let client = self.client.read().await;
            let client = client.as_ref().ok_or(IbkrError::NotConnected)?;
            Arc::clone(client)
        }; // Lock is dropped here!

        let order_id = tokio::task::spawn_blocking(move || {
            let contract = Contract::stock(&order_request.symbol).build();
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

fn to_ibapi_scanner_subscription(s: &ScannerSubscription) -> ibapi::scanner::ScannerSubscription {
    ibapi::scanner::ScannerSubscription {
        number_of_rows: s.number_of_rows,
        instrument: Some(s.instrument.clone()),
        location_code: Some(s.location_code.clone()),
        scan_code: Some(s.scan_code.clone()),
        above_price: s.above_price,
        below_price: s.below_price,
        above_volume: s.above_volume,
        market_cap_above: s.market_cap_above,
        market_cap_below: s.market_cap_below,
        ..Default::default()
    }
}

fn from_ibapi_scanner_data(d: &ibapi::scanner::ScannerData) -> ScannerData {
    ScannerData {
        rank: d.rank,
        contract: from_ibapi_contract_details(&d.contract_details),
        leg: d.leg.clone(),
    }
}

fn from_ibapi_contract_details(cd: &ibapi::contracts::ContractDetails) -> ContractDetails {
    ContractDetails {
        symbol: cd.contract.symbol.0.clone(),
        sec_type: from_ibapi_security_type(&cd.contract.security_type),
        exchange: cd.contract.exchange.0.clone(),
        primary_exchange: cd.contract.primary_exchange.0.clone(),
        currency: cd.contract.currency.0.clone(),
        local_symbol: cd.contract.local_symbol.clone(),
        trading_class: cd.contract.trading_class.clone(),
        contract_id: cd.contract.contract_id,
        min_tick: cd.min_tick,
        multiplier: cd.contract.multiplier.clone(),
        price_magnifier: cd.price_magnifier,
    }
}

fn from_ibapi_security_type(t: &ibapi::contracts::SecurityType) -> SecurityType {
    use ibapi::contracts::SecurityType as Ib;
    match t {
        Ib::Stock => SecurityType::Stock,
        Ib::Option => SecurityType::Option,
        Ib::Future | Ib::FuturesOption => SecurityType::Future,
        Ib::ForexPair => SecurityType::Forex,
        Ib::Spread => SecurityType::Combo,
        Ib::Warrant => SecurityType::Warrant,
        Ib::Bond => SecurityType::Bond,
        Ib::Commodity => SecurityType::Commodity,
        Ib::News => SecurityType::News,
        Ib::MutualFund => SecurityType::Fund,
        // Index, Crypto, CFD, Other(_) — fall back to Stock for the v1 STK-only scanner.
        _ => SecurityType::Stock,
    }
}
