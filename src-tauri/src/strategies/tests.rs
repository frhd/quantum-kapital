use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;

use crate::ibkr::types::{BarSize, DataTier, StrategyTag};

use super::candidate::{targets_for_risk_profile, Direction, SetupCandidate, TargetLevel};
use super::context::MarketContext;
use super::registry::DetectorRegistry;
use super::trait_def::{DetectorError, StrategyDetector};

struct MockDetector {
    name: &'static str,
    tag: StrategyTag,
    result: fn() -> Result<Option<SetupCandidate>, DetectorError>,
}

#[async_trait]
impl StrategyDetector for MockDetector {
    fn name(&self) -> &'static str {
        self.name
    }
    fn tag(&self) -> StrategyTag {
        self.tag.clone()
    }
    fn timeframe(&self) -> BarSize {
        BarSize::Day1
    }
    fn min_lookback_days(&self) -> u32 {
        20
    }
    async fn evaluate(
        &self,
        _ctx: &MarketContext<'_>,
    ) -> Result<Option<SetupCandidate>, DetectorError> {
        (self.result)()
    }
}

fn empty_ctx<'a>(symbol: &'a str) -> MarketContext<'a> {
    MarketContext {
        symbol,
        daily_bars: &[],
        intraday_bars: None,
        fundamentals: None,
        recent_news: &[],
        news_verdict: None,
        current_quote: None,
        data_tier: DataTier::Unknown,
        now: Utc::now(),
    }
}

fn ok_none() -> Result<Option<SetupCandidate>, DetectorError> {
    Ok(None)
}

fn ok_some_candidate() -> Result<Option<SetupCandidate>, DetectorError> {
    Ok(Some(SetupCandidate {
        strategy: "mock",
        tag: StrategyTag::Breakout,
        direction: Direction::Long,
        conviction_signal: 0.5,
        trigger_price: 100.0,
        stop_price: 98.0,
        targets: vec![],
        raw_signals: serde_json::json!({}),
        timeframe: BarSize::Day1,
        detected_at: Utc::now(),
    }))
}

fn err_internal() -> Result<Option<SetupCandidate>, DetectorError> {
    Err(DetectorError::Internal("boom".into()))
}

#[tokio::test]
async fn registry_evaluate_all_runs_each_detector_once() {
    let mut registry = DetectorRegistry::new();
    registry.register(Arc::new(MockDetector {
        name: "alpha",
        tag: StrategyTag::Breakout,
        result: ok_none,
    }));
    registry.register(Arc::new(MockDetector {
        name: "beta",
        tag: StrategyTag::EpisodicPivot,
        result: ok_some_candidate,
    }));
    registry.register(Arc::new(MockDetector {
        name: "gamma",
        tag: StrategyTag::ParabolicShort,
        result: ok_none,
    }));

    let ctx = empty_ctx("AAPL");
    let outcomes = registry.evaluate_all(&ctx).await;

    assert_eq!(outcomes.len(), 3);
    assert_eq!(outcomes[0].detector, "alpha");
    assert_eq!(outcomes[1].detector, "beta");
    assert_eq!(outcomes[2].detector, "gamma");
    assert!(outcomes[0].result.as_ref().unwrap().is_none());
    assert!(outcomes[1].result.as_ref().unwrap().is_some());
    assert!(outcomes[2].result.as_ref().unwrap().is_none());
}

#[tokio::test]
async fn registry_evaluate_filters_by_tag() {
    let mut registry = DetectorRegistry::new();
    registry.register(Arc::new(MockDetector {
        name: "alpha",
        tag: StrategyTag::Breakout,
        result: ok_some_candidate,
    }));
    registry.register(Arc::new(MockDetector {
        name: "beta",
        tag: StrategyTag::EpisodicPivot,
        result: ok_some_candidate,
    }));
    registry.register(Arc::new(MockDetector {
        name: "gamma",
        tag: StrategyTag::ParabolicShort,
        result: ok_some_candidate,
    }));

    let ctx = empty_ctx("AAPL");
    let outcomes = registry
        .evaluate_for_tags(&ctx, &[StrategyTag::Breakout])
        .await;

    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].detector, "alpha");
}

#[tokio::test]
async fn registry_collects_errors_without_short_circuiting() {
    let mut registry = DetectorRegistry::new();
    registry.register(Arc::new(MockDetector {
        name: "alpha",
        tag: StrategyTag::Breakout,
        result: ok_some_candidate,
    }));
    registry.register(Arc::new(MockDetector {
        name: "beta",
        tag: StrategyTag::EpisodicPivot,
        result: err_internal,
    }));
    registry.register(Arc::new(MockDetector {
        name: "gamma",
        tag: StrategyTag::ParabolicShort,
        result: ok_none,
    }));

    let ctx = empty_ctx("AAPL");
    let outcomes = registry.evaluate_all(&ctx).await;

    assert_eq!(outcomes.len(), 3);
    assert!(outcomes[0].result.is_ok());
    assert!(matches!(
        outcomes[1].result,
        Err(DetectorError::Internal(_))
    ));
    assert!(outcomes[2].result.is_ok());
}

#[test]
fn setup_candidate_targets_at_2r_3r_match_risk_profile() {
    let long = targets_for_risk_profile(Direction::Long, 100.0, 98.0).unwrap();
    assert_eq!(
        long,
        vec![
            TargetLevel {
                label: "2R".into(),
                price: 104.0,
            },
            TargetLevel {
                label: "3R".into(),
                price: 106.0,
            },
        ]
    );

    let short = targets_for_risk_profile(Direction::Short, 100.0, 102.0).unwrap();
    assert_eq!(
        short,
        vec![
            TargetLevel {
                label: "2R".into(),
                price: 96.0,
            },
            TargetLevel {
                label: "3R".into(),
                price: 94.0,
            },
        ]
    );
}

#[test]
fn targets_helper_handles_zero_risk_distance() {
    let err = targets_for_risk_profile(Direction::Long, 100.0, 100.0).unwrap_err();
    assert!(err.contains("zero"));
}
