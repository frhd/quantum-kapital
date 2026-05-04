use std::sync::Arc;
use std::time::Duration;

use ibapi::client::blocking::Client;
use ibapi::contracts::tick_types::TickType;
use ibapi::contracts::Contract;
use ibapi::market_data::realtime::TickTypes;
use tracing::{debug, info};

use crate::ibkr::error::{IbkrError, Result};
use crate::ibkr::types::{DataTier, MarketDataSnapshot, MarketDataType};

use super::IbkrClient;

pub(super) const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(5);

/// Quiet-window after the last received tick before `streaming_drain_blocking`
/// considers the burst complete and exits early. IBKR delivers an initial
/// burst of ticks (Bid/Ask/Last/Close/Volume…) within ~500–800ms on streaming
/// market data; longer than that and we're almost certainly idle.
const STREAMING_QUIET_WINDOW: Duration = Duration::from_millis(750);

/// Dispatch decision for `get_market_data_snapshot`.
///
/// IBKR's `reqMktData(snapshot=True)` requires *real-time* subscription
/// rights even when the connection is in `Delayed*` mode — TWS replies
/// with error 354 ("not subscribed") otherwise. Streaming subscriptions
/// honor `Delayed*`, so the delayed paths take a different code path
/// that drains ticks for a short window and then exits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SnapshotMode {
    /// `reqMktData(snapshot=true)` — fast, ends on `SnapshotEnd`. Needs
    /// a real-time market-data subscription on the account.
    OneShot,
    /// `reqMktData(snapshot=false)` — drain the first burst of ticks,
    /// then exit on quiet-window or hard timeout. Works with delayed /
    /// delayed-frozen data.
    StreamingDrain,
}

impl SnapshotMode {
    pub(super) fn for_market_data_type(mdt: MarketDataType) -> Self {
        match mdt {
            MarketDataType::Live | MarketDataType::Frozen => Self::OneShot,
            MarketDataType::Delayed | MarketDataType::DelayedFrozen => Self::StreamingDrain,
        }
    }
}

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

    /// One-shot fetch of level-1 market data for `symbol`.
    ///
    /// Dispatches by the configured `MarketDataType` (see [`SnapshotMode`]):
    /// - `Live` / `Frozen` → ibapi `snapshot=true` (server pushes a fixed
    ///   burst, ends on `SnapshotEnd`).
    /// - `Delayed` / `DelayedFrozen` → streaming subscription drained for
    ///   `STREAMING_QUIET_WINDOW` after the last tick. `snapshot=true`
    ///   does **not** honor delayed-data permissions; TWS would respond
    ///   with error 354.
    ///
    /// Errors:
    /// - `IbkrError::NotConnected` if there is no live ibapi client.
    /// - `IbkrError::MarketDataPermissionDenied` on TWS error 354.
    /// - `IbkrError::Timeout` if no usable ticks arrive in
    ///   `SNAPSHOT_TIMEOUT`.
    /// - `IbkrError::ApiError` for any other ibapi error.
    pub async fn get_market_data_snapshot(&self, symbol: &str) -> Result<MarketDataSnapshot> {
        debug!("get_market_data_snapshot: enter symbol={}", symbol);
        let client_clone = self.ibapi_client().await?;
        let market_data_type = self.config.read().await.market_data_type;
        let mode = SnapshotMode::for_market_data_type(market_data_type);
        let symbol_owned = symbol.to_string();

        tokio::task::spawn_blocking(move || -> Result<MarketDataSnapshot> {
            match mode {
                SnapshotMode::OneShot => snapshot_blocking(client_clone, symbol_owned),
                SnapshotMode::StreamingDrain => {
                    streaming_drain_blocking(client_clone, symbol_owned)
                }
            }
        })
        .await
        .map_err(|e| IbkrError::Unknown(e.to_string()))?
    }
}

fn empty_snapshot(symbol: &str) -> MarketDataSnapshot {
    MarketDataSnapshot {
        symbol: symbol.to_string(),
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
    }
}

/// True if `snapshot` carries any usable price (last, close, or open).
/// Used by the streaming path to decide whether a quiet-window or
/// channel-close should return `Ok(snapshot)` or `Err(Timeout)`.
fn snapshot_has_price(snapshot: &MarketDataSnapshot) -> bool {
    snapshot.last_price.is_some() || snapshot.close.is_some() || snapshot.open.is_some()
}

/// `reqMktData(snapshot=true)` path. Original behavior — kept verbatim
/// for accounts on `Live` or `Frozen` market data.
fn snapshot_blocking(client: Arc<Client>, symbol: String) -> Result<MarketDataSnapshot> {
    let contract = Contract::stock(&symbol).build();
    let generic_ticks: Vec<&str> = Vec::new();

    let subscription = client
        .market_data(&contract)
        .generic_ticks(&generic_ticks)
        .snapshot()
        .subscribe()
        .map_err(|e| {
            info!(
                "get_market_data_snapshot({}): subscribe failed: {}",
                symbol, e
            );
            IbkrError::from(e)
        })?;

    let mut snapshot = empty_snapshot(&symbol);
    let deadline = std::time::Instant::now() + SNAPSHOT_TIMEOUT;

    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            info!(
                "get_market_data_snapshot({}): -> Timeout ({}ms, deadline reached)",
                symbol,
                SNAPSHOT_TIMEOUT.as_millis()
            );
            return Err(IbkrError::Timeout(SNAPSHOT_TIMEOUT.as_millis() as u64));
        }

        match subscription.next_timeout(remaining) {
            Some(TickTypes::Price(tick)) => apply_price(&mut snapshot, &tick),
            Some(TickTypes::Size(tick)) => apply_size(&mut snapshot, &tick),
            Some(TickTypes::PriceSize(tick)) => apply_price_size(&mut snapshot, &tick),
            Some(TickTypes::SnapshotEnd) => {
                snapshot.timestamp = chrono::Utc::now().timestamp();
                debug!(
                    "get_market_data_snapshot({}): SnapshotEnd last_price={:?} close={:?}",
                    symbol, snapshot.last_price, snapshot.close
                );
                return Ok(snapshot);
            }
            Some(TickTypes::Notice(notice)) => {
                debug!(
                    "get_market_data_snapshot({}): notice code={} message={:?}",
                    symbol, notice.code, notice.message
                );
                if notice.code == 354 {
                    info!(
                        "get_market_data_snapshot({}): -> MarketDataPermissionDenied (code 354)",
                        symbol
                    );
                    return Err(IbkrError::MarketDataPermissionDenied);
                }
            }
            Some(_) => {}
            None => {
                if let Some(err) = subscription.error() {
                    info!(
                        "get_market_data_snapshot({}): subscription closed with error: {}",
                        symbol, err
                    );
                    return Err(IbkrError::from(err));
                }
                info!(
                    "get_market_data_snapshot({}): -> Timeout ({}ms, channel closed without error)",
                    symbol,
                    SNAPSHOT_TIMEOUT.as_millis()
                );
                return Err(IbkrError::Timeout(SNAPSHOT_TIMEOUT.as_millis() as u64));
            }
        }
    }
}

/// Streaming `reqMktData(snapshot=false)` path. Drains the initial
/// burst of ticks until quiet for `STREAMING_QUIET_WINDOW` or the hard
/// `SNAPSHOT_TIMEOUT` deadline elapses.
///
/// Compared to the snapshot path:
/// - There is no `SnapshotEnd` in streaming mode, so we exit on quiet
///   window instead.
/// - A `next_timeout` slice that returns `None` may be either a slice
///   timeout *or* a closed channel — we disambiguate via
///   `subscription.error()`. Slice timeouts re-enter the loop.
/// - On exit, the held `Subscription` drops and ibapi sends
///   `cancelMktData` automatically.
fn streaming_drain_blocking(client: Arc<Client>, symbol: String) -> Result<MarketDataSnapshot> {
    let contract = Contract::stock(&symbol).build();
    let generic_ticks: Vec<&str> = Vec::new();

    let subscription = client
        .market_data(&contract)
        .generic_ticks(&generic_ticks)
        .subscribe()
        .map_err(|e| {
            info!(
                "get_market_data_snapshot({}, streaming): subscribe failed: {}",
                symbol, e
            );
            IbkrError::from(e)
        })?;

    let mut snapshot = empty_snapshot(&symbol);
    let now = std::time::Instant::now();
    let hard_deadline = now + SNAPSHOT_TIMEOUT;
    let mut last_tick_at: Option<std::time::Instant> = None;

    loop {
        let now = std::time::Instant::now();

        // Hard cap: out of time.
        if now >= hard_deadline {
            if snapshot_has_price(&snapshot) {
                snapshot.timestamp = chrono::Utc::now().timestamp();
                debug!(
                    "get_market_data_snapshot({}, streaming): -> hard deadline, returning partial last_price={:?} close={:?}",
                    symbol, snapshot.last_price, snapshot.close
                );
                return Ok(snapshot);
            }
            info!(
                "get_market_data_snapshot({}, streaming): -> Timeout ({}ms, no ticks)",
                symbol,
                SNAPSHOT_TIMEOUT.as_millis()
            );
            return Err(IbkrError::Timeout(SNAPSHOT_TIMEOUT.as_millis() as u64));
        }

        // Wait at most until the quiet-window expires (if we've seen a
        // tick) or until the hard deadline (if we haven't). Whichever
        // is sooner caps the slice — a slice timeout means we re-check.
        let quiet_deadline = last_tick_at
            .map(|t| t + STREAMING_QUIET_WINDOW)
            .unwrap_or(hard_deadline);
        let slice_until = quiet_deadline.min(hard_deadline);
        let slice = slice_until.saturating_duration_since(now);
        if slice.is_zero() {
            // Quiet-window expired with at least one prior tick → exit.
            if snapshot_has_price(&snapshot) {
                snapshot.timestamp = chrono::Utc::now().timestamp();
                debug!(
                    "get_market_data_snapshot({}, streaming): -> quiet window, last_price={:?} close={:?}",
                    symbol, snapshot.last_price, snapshot.close
                );
                return Ok(snapshot);
            }
            // Quiet without price (only sizes / notices) — keep waiting
            // until the hard deadline.
            last_tick_at = None;
            continue;
        }

        match subscription.next_timeout(slice) {
            Some(TickTypes::Price(tick)) => {
                apply_price(&mut snapshot, &tick);
                last_tick_at = Some(std::time::Instant::now());
            }
            Some(TickTypes::Size(tick)) => {
                apply_size(&mut snapshot, &tick);
                last_tick_at = Some(std::time::Instant::now());
            }
            Some(TickTypes::PriceSize(tick)) => {
                apply_price_size(&mut snapshot, &tick);
                last_tick_at = Some(std::time::Instant::now());
            }
            Some(TickTypes::SnapshotEnd) => {
                // Some servers still emit SnapshotEnd on streaming reqs — treat as exit signal.
                snapshot.timestamp = chrono::Utc::now().timestamp();
                debug!(
                    "get_market_data_snapshot({}, streaming): SnapshotEnd last_price={:?} close={:?}",
                    symbol, snapshot.last_price, snapshot.close
                );
                return Ok(snapshot);
            }
            Some(TickTypes::Notice(notice)) => {
                debug!(
                    "get_market_data_snapshot({}, streaming): notice code={} message={:?}",
                    symbol, notice.code, notice.message
                );
                if notice.code == 354 {
                    info!(
                        "get_market_data_snapshot({}, streaming): -> MarketDataPermissionDenied (code 354)",
                        symbol
                    );
                    return Err(IbkrError::MarketDataPermissionDenied);
                }
            }
            Some(_) => {}
            None => {
                // Either slice timeout or channel close — disambiguate.
                if let Some(err) = subscription.error() {
                    info!(
                        "get_market_data_snapshot({}, streaming): subscription closed with error: {}",
                        symbol, err
                    );
                    return Err(IbkrError::from(err));
                }
                // Slice timeout. If this slice was the quiet-window
                // (we had a tick and it elapsed), the next loop iter
                // will compute slice == 0 and exit. Otherwise we keep
                // waiting toward the hard deadline.
            }
        }
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

    #[test]
    fn snapshot_mode_routes_real_time_to_one_shot() {
        assert_eq!(
            SnapshotMode::for_market_data_type(MarketDataType::Live),
            SnapshotMode::OneShot
        );
        assert_eq!(
            SnapshotMode::for_market_data_type(MarketDataType::Frozen),
            SnapshotMode::OneShot
        );
    }

    #[test]
    fn snapshot_mode_routes_delayed_to_streaming_drain() {
        // Snapshot=true requests don't honor delayed/delayed-frozen
        // permissions — TWS returns error 354. Streaming subscriptions
        // do, so the delayed paths must use the streaming drain.
        assert_eq!(
            SnapshotMode::for_market_data_type(MarketDataType::Delayed),
            SnapshotMode::StreamingDrain
        );
        assert_eq!(
            SnapshotMode::for_market_data_type(MarketDataType::DelayedFrozen),
            SnapshotMode::StreamingDrain
        );
    }

    #[test]
    fn snapshot_has_price_detects_any_filled_price_field() {
        let mut snap = empty_snapshot("AMD");
        assert!(!snapshot_has_price(&snap));

        snap.bid_price = Some(100.0);
        snap.ask_price = Some(100.5);
        assert!(
            !snapshot_has_price(&snap),
            "bid/ask alone shouldn't count — UI needs last/close/open"
        );

        snap.last_price = Some(100.25);
        assert!(snapshot_has_price(&snap));

        let mut snap = empty_snapshot("AMD");
        snap.close = Some(99.0);
        assert!(snapshot_has_price(&snap));

        let mut snap = empty_snapshot("AMD");
        snap.open = Some(98.5);
        assert!(snapshot_has_price(&snap));
    }

    #[test]
    fn empty_snapshot_carries_symbol_and_clears_optionals() {
        let snap = empty_snapshot("AMD");
        assert_eq!(snap.symbol, "AMD");
        assert!(snap.bid_price.is_none());
        assert!(snap.ask_price.is_none());
        assert!(snap.last_price.is_none());
        assert!(snap.close.is_none());
        assert!(snap.open.is_none());
        assert!(snap.high.is_none());
        assert!(snap.low.is_none());
        assert!(snap.volume.is_none());
        assert!(snap.bid_size.is_none());
        assert!(snap.ask_size.is_none());
        assert!(snap.last_size.is_none());
    }
}
