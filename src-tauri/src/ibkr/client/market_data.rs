use std::time::Duration;

use ibapi::contracts::tick_types::TickType;
use ibapi::contracts::Contract;
use ibapi::market_data::realtime::TickTypes;

use crate::ibkr::error::{IbkrError, Result};
use crate::ibkr::types::MarketDataSnapshot;

use super::IbkrClient;

#[allow(dead_code)] // removed in Task 6 when QuoteFetcher for IbkrClient lands
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(5);

impl IbkrClient {
    /// Existing best-effort subscription. Kept because
    /// `ibkr_subscribe_market_data` Tauri command depends on it.
    pub async fn subscribe_market_data(&self, symbol: &str) -> Result<()> {
        let client_clone = self.ibapi_client().await?;

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

    /// One-shot snapshot of level-1 market data for `symbol`.
    ///
    /// Uses ibapi's `snapshot=true` mode so the server pushes a fixed
    /// burst of ticks then sends `SnapshotEnd` — we drain those ticks
    /// and return as soon as `SnapshotEnd` arrives or `SNAPSHOT_TIMEOUT`
    /// elapses (whichever comes first).
    ///
    /// Errors:
    /// - `IbkrError::NotConnected` if there is no live ibapi client.
    /// - `IbkrError::MarketDataPermissionDenied` if TWS replies with
    ///   error code 354 ("Requested market data is not subscribed").
    /// - `IbkrError::Timeout` if no `SnapshotEnd` arrives within
    ///   `SNAPSHOT_TIMEOUT`.
    /// - `IbkrError::ApiError` for any other ibapi error.
    #[allow(dead_code)] // removed in Task 6 when QuoteFetcher for IbkrClient lands
    pub async fn get_market_data_snapshot(&self, symbol: &str) -> Result<MarketDataSnapshot> {
        let client_clone = self.ibapi_client().await?;
        let symbol_owned = symbol.to_string();

        tokio::task::spawn_blocking(move || -> Result<MarketDataSnapshot> {
            let contract = Contract::stock(&symbol_owned).build();
            let generic_ticks: Vec<&str> = Vec::new();

            let subscription = client_clone
                .market_data(&contract)
                .generic_ticks(&generic_ticks)
                .snapshot()
                .subscribe()
                .map_err(IbkrError::from)?;

            let mut snapshot = MarketDataSnapshot {
                symbol: symbol_owned.clone(),
                bid_price: None,
                bid_size: None,
                ask_price: None,
                ask_size: None,
                last_price: None,
                last_size: None,
                high: None,
                low: None,
                volume: None,
                close: None,
                open: None,
                timestamp: chrono::Utc::now().timestamp(),
            };

            let deadline = std::time::Instant::now() + SNAPSHOT_TIMEOUT;

            loop {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    return Err(IbkrError::Timeout(SNAPSHOT_TIMEOUT.as_millis() as u64));
                }

                match subscription.next_timeout(remaining) {
                    Some(TickTypes::Price(tick)) => {
                        apply_price(&mut snapshot, &tick);
                    }
                    Some(TickTypes::Size(tick)) => {
                        apply_size(&mut snapshot, &tick);
                    }
                    Some(TickTypes::PriceSize(tick)) => {
                        apply_price_size(&mut snapshot, &tick);
                    }
                    Some(TickTypes::SnapshotEnd) => {
                        snapshot.timestamp = chrono::Utc::now().timestamp();
                        return Ok(snapshot);
                    }
                    Some(TickTypes::Notice(notice)) => {
                        // ibapi delivers TWS error codes through Notice.
                        // 354 = "Requested market data is not subscribed".
                        if notice.code == 354 {
                            return Err(IbkrError::MarketDataPermissionDenied);
                        }
                        // Other notices (e.g. farm connection messages)
                        // are informational; keep looping.
                    }
                    Some(_) => {
                        // Other tick types (Generic, String, EFP,
                        // RequestParameters, etc.) aren't projected
                        // into MarketDataSnapshot — ignore.
                    }
                    None => {
                        if let Some(err) = subscription.error() {
                            return Err(IbkrError::from(err));
                        }
                        return Err(IbkrError::Timeout(SNAPSHOT_TIMEOUT.as_millis() as u64));
                    }
                }
            }
        })
        .await
        .map_err(|e| IbkrError::Unknown(e.to_string()))?
    }
}

#[allow(dead_code)] // removed in Task 6 when QuoteFetcher for IbkrClient lands
fn apply_price(snapshot: &mut MarketDataSnapshot, tick: &ibapi::market_data::realtime::TickPrice) {
    match tick.tick_type {
        TickType::Bid | TickType::DelayedBid => snapshot.bid_price = Some(tick.price),
        TickType::Ask | TickType::DelayedAsk => snapshot.ask_price = Some(tick.price),
        TickType::Last | TickType::DelayedLast => snapshot.last_price = Some(tick.price),
        TickType::High | TickType::DelayedHigh => snapshot.high = Some(tick.price),
        TickType::Low | TickType::DelayedLow => snapshot.low = Some(tick.price),
        TickType::Close | TickType::DelayedClose => snapshot.close = Some(tick.price),
        TickType::Open | TickType::DelayedOpen => snapshot.open = Some(tick.price),
        _ => {}
    }
}

#[allow(dead_code)] // removed in Task 6 when QuoteFetcher for IbkrClient lands
fn apply_size(snapshot: &mut MarketDataSnapshot, tick: &ibapi::market_data::realtime::TickSize) {
    match tick.tick_type {
        TickType::BidSize | TickType::DelayedBidSize => {
            snapshot.bid_size = Some(tick.size as i32);
        }
        TickType::AskSize | TickType::DelayedAskSize => {
            snapshot.ask_size = Some(tick.size as i32);
        }
        TickType::LastSize | TickType::DelayedLastSize => {
            snapshot.last_size = Some(tick.size as i32);
        }
        TickType::Volume | TickType::DelayedVolume => {
            snapshot.volume = Some(tick.size as i64);
        }
        _ => {}
    }
}

#[allow(dead_code)] // removed in Task 6 when QuoteFetcher for IbkrClient lands
fn apply_price_size(
    snapshot: &mut MarketDataSnapshot,
    tick: &ibapi::market_data::realtime::TickPriceSize,
) {
    match tick.price_tick_type {
        TickType::Bid | TickType::DelayedBid => snapshot.bid_price = Some(tick.price),
        TickType::Ask | TickType::DelayedAsk => snapshot.ask_price = Some(tick.price),
        TickType::Last | TickType::DelayedLast => snapshot.last_price = Some(tick.price),
        _ => {}
    }
    match tick.size_tick_type {
        TickType::BidSize | TickType::DelayedBidSize => snapshot.bid_size = Some(tick.size as i32),
        TickType::AskSize | TickType::DelayedAskSize => snapshot.ask_size = Some(tick.size as i32),
        TickType::LastSize | TickType::DelayedLastSize => {
            snapshot.last_size = Some(tick.size as i32);
        }
        _ => {}
    }
}
