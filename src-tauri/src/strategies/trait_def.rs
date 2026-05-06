use async_trait::async_trait;
use thiserror::Error;

use crate::ibkr::types::{BarSize, StrategyTag};
use crate::services::regime::RegimeFilter;

use super::candidate::SetupCandidate;
use super::context::MarketContext;

#[derive(Debug, Error)]
pub enum DetectorError {
    #[error("insufficient bars: need at least {needed}, got {available}")]
    InsufficientBars { needed: usize, available: usize },
    #[error("intraday bars required but not provided")]
    IntradayBarsRequired,
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("internal detector error: {0}")]
    Internal(String),
}

#[async_trait]
pub trait StrategyDetector: Send + Sync {
    fn name(&self) -> &'static str;
    fn tag(&self) -> StrategyTag;
    fn timeframe(&self) -> BarSize;
    fn min_lookback_days(&self) -> u32;
    /// Phase 9 — declared regime preferences. Default returns
    /// [`RegimeFilter::default`] (no constraints), so detectors that
    /// don't override stay regime-agnostic. The runtime gate reads
    /// from `RegimeConfig.per_detector` first; this trait method is
    /// the operator-overridable fallback.
    fn preferred_regimes(&self) -> RegimeFilter {
        RegimeFilter::default()
    }
    async fn evaluate(
        &self,
        ctx: &MarketContext<'_>,
    ) -> Result<Option<SetupCandidate>, DetectorError>;
}
