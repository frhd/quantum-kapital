use std::sync::Arc;

use crate::ibkr::types::StrategyTag;

use super::candidate::SetupCandidate;
use super::context::MarketContext;
use super::trait_def::{DetectorError, StrategyDetector};

#[derive(Debug)]
pub struct DetectorOutcome {
    pub detector: &'static str,
    pub result: Result<Option<SetupCandidate>, DetectorError>,
}

#[derive(Default)]
pub struct DetectorRegistry {
    detectors: Vec<Arc<dyn StrategyDetector>>,
}

impl DetectorRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, detector: Arc<dyn StrategyDetector>) {
        self.detectors.push(detector);
    }

    pub async fn evaluate_all(&self, ctx: &MarketContext<'_>) -> Vec<DetectorOutcome> {
        let mut out = Vec::with_capacity(self.detectors.len());
        for d in &self.detectors {
            let name = d.name();
            let result = d.evaluate(ctx).await;
            out.push(DetectorOutcome {
                detector: name,
                result,
            });
        }
        out
    }

    pub async fn evaluate_for_tags(
        &self,
        ctx: &MarketContext<'_>,
        tags: &[StrategyTag],
    ) -> Vec<DetectorOutcome> {
        let mut out = Vec::new();
        for d in &self.detectors {
            if !tags.iter().any(|t| t == &d.tag()) {
                continue;
            }
            let name = d.name();
            let result = d.evaluate(ctx).await;
            out.push(DetectorOutcome {
                detector: name,
                result,
            });
        }
        out
    }
}
