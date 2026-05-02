use std::sync::Arc;
use std::time::Duration;

use ibapi::client::blocking::Client;
use ibapi::contracts::tick_types::TickType;
use ibapi::contracts::Contract;
use ibapi::market_data::realtime::TickTypes;
use tracing::{debug, info};

use crate::ibkr::error::{IbkrError, Result};
use crate::ibkr::types::{DataTier, MarketDataSnapshot};

use super::IbkrClient;

pub(super) const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(5);

/// Classify a single `TickType` into the data tier it implies.
///
/// Returns `None` for tick types that don't carry a tier signal (e.g.
/// `Halted`, `RtVolume`, options-specific ticks). The probe loop keeps
/// reading until it gets a `Some(_)` or hits its timeout / snapshot
/// end, so unclassified ticks just don't move the answer.
fn classify_tick_type(tick_type: TickType) -> Option<DataTier> {
    match tick_type {
        TickType::Bid
        | TickType::Ask
        | TickType::Last
        | TickType::High
        | TickType::Low
        | TickType::Close
        | TickType::Open
        | TickType::Volume
        | TickType::BidSize
        | TickType::AskSize
        | TickType::LastSize => Some(DataTier::RealTime),
        TickType::DelayedBid
        | TickType::DelayedAsk
        | TickType::DelayedLast
        | TickType::DelayedHigh
        | TickType::DelayedLow
        | TickType::DelayedClose
        | TickType::DelayedOpen
        | TickType::DelayedVolume
        | TickType::DelayedBidSize
        | TickType::DelayedAskSize
        | TickType::DelayedLastSize => Some(DataTier::Delayed),
        _ => None,
    }
}

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
    pub async fn get_market_data_snapshot(&self, symbol: &str) -> Result<MarketDataSnapshot> {
        debug!("get_market_data_snapshot: enter symbol={}", symbol);
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
                .map_err(|e| {
                    info!(
                        "get_market_data_snapshot({}): subscribe failed: {}",
                        symbol_owned, e
                    );
                    IbkrError::from(e)
                })?;

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
                    info!(
                        "get_market_data_snapshot({}): -> Timeout ({}ms, deadline reached)",
                        symbol_owned,
                        SNAPSHOT_TIMEOUT.as_millis()
                    );
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
                        debug!(
                            "get_market_data_snapshot({}): SnapshotEnd last_price={:?} close={:?}",
                            symbol_owned, snapshot.last_price, snapshot.close
                        );
                        return Ok(snapshot);
                    }
                    Some(TickTypes::Notice(notice)) => {
                        debug!(
                            "get_market_data_snapshot({}): notice code={} message={:?}",
                            symbol_owned, notice.code, notice.message
                        );
                        // ibapi delivers TWS error codes through Notice.
                        // 354 = "Requested market data is not subscribed".
                        if notice.code == 354 {
                            info!(
                                "get_market_data_snapshot({}): -> MarketDataPermissionDenied (code 354)",
                                symbol_owned
                            );
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
                            info!(
                                "get_market_data_snapshot({}): subscription closed with error: {}",
                                symbol_owned, err
                            );
                            return Err(IbkrError::from(err));
                        }
                        info!(
                            "get_market_data_snapshot({}): -> Timeout ({}ms, channel closed without error)",
                            symbol_owned,
                            SNAPSHOT_TIMEOUT.as_millis()
                        );
                        return Err(IbkrError::Timeout(SNAPSHOT_TIMEOUT.as_millis() as u64));
                    }
                }
            }
        })
        .await
        .map_err(|e| IbkrError::Unknown(e.to_string()))?
    }
}

impl IbkrClient {
    /// Probe the active connection's market-data tier by snapshotting
    /// `symbol` and inspecting which tick variants come back. Returns
    /// `Ok(DataTier::RealTime)` on the first non-delayed price/size
    /// tick, `Ok(DataTier::Delayed)` on the first delayed variant, or
    /// `Ok(DataTier::Unknown)` on `MarketDataPermissionDenied`,
    /// timeout, or a `SnapshotEnd` that arrived before any
    /// classifiable tick. Other ibapi errors propagate as `Err`.
    ///
    /// Mirrors `get_market_data_snapshot` (same `spawn_blocking` +
    /// `next_timeout` loop, same `SNAPSHOT_TIMEOUT`) but reads a
    /// classifier instead of populating a snapshot struct.
    ///
    /// Production usage runs through `connect()` (which calls the
    /// `run_probe` helper directly with a fresh client handle); this
    /// inherent method is kept as a manual debugging surface and
    /// mirrors the trait method on `IbkrClientTrait`.
    #[allow(dead_code)]
    pub async fn probe_data_tier(&self, symbol: &str) -> Result<DataTier> {
        let client_clone = self.ibapi_client().await?;
        run_probe(client_clone, symbol).await
    }
}

/// Probe `symbol` against an already-cloned ibapi client. Public to
/// the parent module so `connect()` can spawn the probe without
/// going back through `IbkrClient`.
pub(super) async fn run_probe(client: Arc<Client>, symbol: &str) -> Result<DataTier> {
    let symbol_owned = symbol.to_string();

    tokio::task::spawn_blocking(move || -> Result<DataTier> {
        let contract = Contract::stock(&symbol_owned).build();
        let generic_ticks: Vec<&str> = Vec::new();

        let subscription = client
            .market_data(&contract)
            .generic_ticks(&generic_ticks)
            .snapshot()
            .subscribe()
            .map_err(IbkrError::from)?;

        let deadline = std::time::Instant::now() + SNAPSHOT_TIMEOUT;

        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Ok(DataTier::Unknown);
            }

            match subscription.next_timeout(remaining) {
                Some(TickTypes::Price(tick)) => {
                    if let Some(tier) = classify_tick_type(tick.tick_type) {
                        return Ok(tier);
                    }
                }
                Some(TickTypes::Size(tick)) => {
                    if let Some(tier) = classify_tick_type(tick.tick_type) {
                        return Ok(tier);
                    }
                }
                Some(TickTypes::PriceSize(tick)) => {
                    if let Some(tier) = classify_tick_type(tick.price_tick_type) {
                        return Ok(tier);
                    }
                    if let Some(tier) = classify_tick_type(tick.size_tick_type) {
                        return Ok(tier);
                    }
                }
                Some(TickTypes::SnapshotEnd) => {
                    return Ok(DataTier::Unknown);
                }
                Some(TickTypes::Notice(notice)) => {
                    if notice.code == 354 {
                        return Ok(DataTier::Unknown);
                    }
                }
                Some(_) => {}
                None => {
                    if let Some(err) = subscription.error() {
                        return Err(IbkrError::from(err));
                    }
                    return Ok(DataTier::Unknown);
                }
            }
        }
    })
    .await
    .map_err(|e| IbkrError::Unknown(e.to_string()))?
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_real_time_price_ticks() {
        assert_eq!(classify_tick_type(TickType::Last), Some(DataTier::RealTime));
        assert_eq!(classify_tick_type(TickType::Bid), Some(DataTier::RealTime));
        assert_eq!(classify_tick_type(TickType::Ask), Some(DataTier::RealTime));
        assert_eq!(classify_tick_type(TickType::High), Some(DataTier::RealTime));
        assert_eq!(classify_tick_type(TickType::Low), Some(DataTier::RealTime));
        assert_eq!(
            classify_tick_type(TickType::Close),
            Some(DataTier::RealTime)
        );
        assert_eq!(classify_tick_type(TickType::Open), Some(DataTier::RealTime));
        assert_eq!(
            classify_tick_type(TickType::Volume),
            Some(DataTier::RealTime)
        );
    }

    #[test]
    fn classify_real_time_size_ticks() {
        assert_eq!(
            classify_tick_type(TickType::BidSize),
            Some(DataTier::RealTime)
        );
        assert_eq!(
            classify_tick_type(TickType::AskSize),
            Some(DataTier::RealTime)
        );
        assert_eq!(
            classify_tick_type(TickType::LastSize),
            Some(DataTier::RealTime)
        );
    }

    #[test]
    fn classify_delayed_price_ticks() {
        assert_eq!(
            classify_tick_type(TickType::DelayedLast),
            Some(DataTier::Delayed)
        );
        assert_eq!(
            classify_tick_type(TickType::DelayedBid),
            Some(DataTier::Delayed)
        );
        assert_eq!(
            classify_tick_type(TickType::DelayedAsk),
            Some(DataTier::Delayed)
        );
        assert_eq!(
            classify_tick_type(TickType::DelayedHigh),
            Some(DataTier::Delayed)
        );
        assert_eq!(
            classify_tick_type(TickType::DelayedLow),
            Some(DataTier::Delayed)
        );
        assert_eq!(
            classify_tick_type(TickType::DelayedClose),
            Some(DataTier::Delayed)
        );
        assert_eq!(
            classify_tick_type(TickType::DelayedOpen),
            Some(DataTier::Delayed)
        );
        assert_eq!(
            classify_tick_type(TickType::DelayedVolume),
            Some(DataTier::Delayed)
        );
    }

    #[test]
    fn classify_delayed_size_ticks() {
        assert_eq!(
            classify_tick_type(TickType::DelayedBidSize),
            Some(DataTier::Delayed)
        );
        assert_eq!(
            classify_tick_type(TickType::DelayedAskSize),
            Some(DataTier::Delayed)
        );
        assert_eq!(
            classify_tick_type(TickType::DelayedLastSize),
            Some(DataTier::Delayed)
        );
    }

    #[test]
    fn classify_unrelated_ticks_returns_none() {
        assert_eq!(classify_tick_type(TickType::Halted), None);
        assert_eq!(classify_tick_type(TickType::Unknown), None);
        assert_eq!(classify_tick_type(TickType::RtVolume), None);
        assert_eq!(classify_tick_type(TickType::MarkPrice), None);
    }
}
