use serde::{Deserialize, Serialize};

/// Empirically detected market-data capability for the active IBKR
/// connection. Probed at connect time by `IbkrClient::probe_data_tier`
/// and surfaced through `IbkrState.data_tier` + `AppEvent::DataTierDetected`.
///
/// Distinct from `MarketDataType` (the *configured* mode the client
/// requests): a paper account configured as `Live` may still only get
/// `Delayed` ticks, and that fact only shows up in the data stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DataTier {
    /// Probe has not run, has not finished, or has been reset by a
    /// disconnect. Consumers should treat this as "don't know yet" and
    /// suspend tier-gated work rather than guessing.
    #[default]
    Unknown,
    /// Connection delivers 15-minute-delayed ticks (`DelayedLast`,
    /// `DelayedBid`, `DelayedAsk`, ...). The free IBKR tier and most
    /// paper accounts land here.
    Delayed,
    /// Connection delivers real-time ticks (`Last`, `Bid`, `Ask`, ...).
    /// Implies the account has the matching market-data subscription.
    RealTime,
}
