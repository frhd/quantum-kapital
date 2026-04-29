use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::events::{AppEvent, EventEmitter};
use crate::ibkr::error::{IbkrError, Result};
use crate::ibkr::types::{ContractDetails, ScannerData, ScannerSubscription, SecurityType};

use super::IbkrClient;

pub struct StreamHandle {
    name: &'static str,
    shutdown: Arc<AtomicBool>,
    join: JoinHandle<()>,
}

impl StreamHandle {
    pub fn new(name: &'static str, shutdown: Arc<AtomicBool>, join: JoinHandle<()>) -> Self {
        Self {
            name,
            shutdown,
            join,
        }
    }

    pub async fn stop(self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Err(e) = self.join.await {
            warn!("{} task join error: {e}", self.name);
        }
    }
}

impl IbkrClient {
    pub async fn start_daily_pnl_stream(
        &self,
        account: &str,
        emitter: Arc<EventEmitter>,
    ) -> Result<StreamHandle> {
        use ibapi::accounts::types::AccountId;

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
