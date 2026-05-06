//! Phase 8 — `ConcentrationGate`. Pure decision: given a portfolio
//! snapshot + a candidate's projected dollar-risk + a config, return
//! `pass | warn | block` and the breach descriptors. Does NOT
//! persist anything; the caller (`TrackerRunner`) writes the row
//! and the audit.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::exposure::PortfolioRisk;
use super::sector_map::SectorMap;
use super::types::{ConcentrationKind, GateLimitBreach};

/// Concentration limits. Defaults match the master plan's "Decisions
/// to make in this phase":
///   - 5% NLV per sector
///   - 1.5% NLV per name
///   - 10% NLV total open
///   - 4 concurrent positions per factor bucket
///
/// Severity ladder (also master): 80% of limit = `warn` (banner,
/// no override), 100% = `block` (override required, audited).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConcentrationConfig {
    pub max_total_pct_nlv: f64,
    pub max_sector_pct_nlv: f64,
    pub max_name_pct_nlv: f64,
    pub max_factor_concurrent: u32,
    /// Fraction at which a `warn` fires. Must be < 1.0; defaults to
    /// 0.80. Below this fraction the gate is `pass`.
    pub warn_threshold: f64,
}

impl Default for ConcentrationConfig {
    fn default() -> Self {
        Self {
            max_total_pct_nlv: 0.10,
            max_sector_pct_nlv: 0.05,
            max_name_pct_nlv: 0.015,
            max_factor_concurrent: 4,
            warn_threshold: 0.80,
        }
    }
}

/// What the gate decides for a given candidate. `breaches` is empty
/// for `Pass`; for `Warn` and `Block` it carries one entry per
/// limit that fired (a single setup can hit multiple at once).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateSeverity {
    Pass,
    Warn,
    Block,
}

impl GateSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            GateSeverity::Pass => "pass",
            GateSeverity::Warn => "warn",
            GateSeverity::Block => "block",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateResult {
    pub severity: GateSeverity,
    pub breaches: Vec<GateLimitBreach>,
}

impl GateResult {
    pub fn pass() -> Self {
        Self {
            severity: GateSeverity::Pass,
            breaches: Vec::new(),
        }
    }

    /// Compose the headline `gate_warning` annotation tag for a
    /// `warn` outcome — "{kind}_80pct" of the worst breach. `None`
    /// when severity isn't `Warn`.
    pub fn warning_tag(&self) -> Option<String> {
        if !matches!(self.severity, GateSeverity::Warn) {
            return None;
        }
        let worst = self.breaches.iter().max_by_key(|b| {
            // Order by (severity, projected/limit ratio) so the
            // banner reflects whichever limit binds tightest.
            let ratio_bp = if b.limit > 0 {
                (b.projected as f64 / b.limit as f64 * 10_000.0) as i64
            } else {
                0
            };
            (b.severity as i64, ratio_bp)
        })?;
        Some(format!("{}_80pct", worst.kind.as_str()))
    }
}

/// The gate. Holds a snapshot + config snapshot at construction
/// time so a `check` is cheap (no IO). Cheap to clone.
#[derive(Clone)]
pub struct ConcentrationGate {
    snapshot: Arc<PortfolioRisk>,
    config: ConcentrationConfig,
    sector_map: Arc<SectorMap>,
}

/// Pre-trade payload describing what the candidate would add. Kept
/// as a flat struct rather than using `SetupCandidate` directly so
/// the gate stays decoupled from the strategies module + can be
/// unit-tested without a full candidate.
#[derive(Debug, Clone)]
pub struct GateInput<'a> {
    pub symbol: &'a str,
    /// Dollar-risk of the proposed setup, in cents. Output of the
    /// risk-engine sizing for the candidate. `0` means the engine
    /// refused to size (skipped) — the gate also passes those
    /// since they don't actually consume risk.
    pub projected_dollar_risk_cents: i64,
    /// Strategy tag — used only to surface in breach messaging
    /// today; the gate doesn't yet weight by strategy.
    pub strategy: &'a str,
    /// Optional explicit factor membership for the candidate (when
    /// the caller already has it). When None, the gate doesn't
    /// project the factor-concurrent breach.
    pub momentum_bucket: Option<&'a str>,
}

impl ConcentrationGate {
    pub fn new(
        snapshot: PortfolioRisk,
        config: ConcentrationConfig,
        sector_map: Arc<SectorMap>,
    ) -> Self {
        Self {
            snapshot: Arc::new(snapshot),
            config,
            sector_map,
        }
    }

    /// Resolve the candidate's sector via the live `SectorMap`
    /// async lookup. Tests that don't care can pass `None` to
    /// `check_with_sector` and the gate will skip the sector
    /// breach evaluation.
    pub async fn check(&self, input: &GateInput<'_>) -> GateResult {
        let sector = self.sector_map.lookup(input.symbol).await;
        self.check_with_sector(input, sector)
    }

    /// Pure synchronous variant. The caller hands in the resolved
    /// sector (or `None` to skip the sector check). Used by the
    /// snapshot path that already has the answer cached and by
    /// tests.
    pub fn check_with_sector(&self, input: &GateInput<'_>, sector: Option<&str>) -> GateResult {
        if input.projected_dollar_risk_cents <= 0 {
            return GateResult::pass();
        }

        let mut breaches = Vec::new();
        let nlv = self.snapshot.nlv_cents;
        let warn_t = self.config.warn_threshold;

        // 1. Total open dollar-risk.
        if nlv > 0 {
            let limit = (nlv as f64 * self.config.max_total_pct_nlv).round() as i64;
            let current = self.snapshot.total_dollar_risk_cents;
            let projected = current.saturating_add(input.projected_dollar_risk_cents);
            if let Some(b) = bucket_breach(
                ConcentrationKind::TotalRisk,
                "".to_string(),
                current,
                projected,
                limit,
                warn_t,
            ) {
                breaches.push(b);
            }
        }

        // 2. Single-name dollar-risk.
        if nlv > 0 {
            let limit = (nlv as f64 * self.config.max_name_pct_nlv).round() as i64;
            let current = self.snapshot.risk_for(input.symbol);
            let projected = current.saturating_add(input.projected_dollar_risk_cents);
            if let Some(b) = bucket_breach(
                ConcentrationKind::SingleName,
                input.symbol.to_uppercase(),
                current,
                projected,
                limit,
                warn_t,
            ) {
                breaches.push(b);
            }
        }

        // 3. Single-sector dollar-risk. Per master gotcha "do not
        //    block setup just because sector unknown" — if the
        //    candidate's sector resolves to None, the gate skips
        //    this leg entirely. If the resolved sector is the
        //    synthetic `"unknown"` bucket the gate also passes
        //    leniently (downgrades any block to warn).
        if nlv > 0 {
            if let Some(sec) = sector {
                let limit = (nlv as f64 * self.config.max_sector_pct_nlv).round() as i64;
                let current = self.snapshot.sector_risk(sec);
                let projected = current.saturating_add(input.projected_dollar_risk_cents);
                if let Some(mut b) = bucket_breach(
                    ConcentrationKind::SingleSector,
                    sec.to_string(),
                    current,
                    projected,
                    limit,
                    warn_t,
                ) {
                    if sec == "unknown" && b.severity == 2 {
                        // Downgrade: blocks on `unknown` are forbidden
                        // per master gotcha — convert to warn so the
                        // trader still sees the banner.
                        b.severity = 1;
                    }
                    breaches.push(b);
                }
            }
        }

        // 4. Factor-concurrent (master cuts on momentum bucket
        //    only — keep the other factor cells for the heatmap
        //    but don't gate on them yet).
        if let Some(mom) = input.momentum_bucket {
            if mom != "unknown" {
                let limit = self.config.max_factor_concurrent as i64;
                let current = self.snapshot.factor_count(mom) as i64;
                let projected = current + 1;
                if let Some(b) = bucket_breach(
                    ConcentrationKind::FactorConcurrent,
                    mom.to_string(),
                    current,
                    projected,
                    limit,
                    warn_t,
                ) {
                    breaches.push(b);
                }
            }
        }

        let severity = breaches
            .iter()
            .map(|b| b.severity)
            .max()
            .map(|s| match s {
                0 => GateSeverity::Pass,
                1 => GateSeverity::Warn,
                _ => GateSeverity::Block,
            })
            .unwrap_or(GateSeverity::Pass);

        GateResult { severity, breaches }
    }
}

/// Severity ladder evaluation for one limit. Returns `None` when
/// `projected < warn_threshold * limit` (a clean pass — no breach
/// to record). `Some` carries the breach with severity 1 (warn) or
/// 2 (block).
fn bucket_breach(
    kind: ConcentrationKind,
    label: String,
    current: i64,
    projected: i64,
    limit: i64,
    warn_threshold: f64,
) -> Option<GateLimitBreach> {
    if limit <= 0 {
        return None;
    }
    let ratio = projected as f64 / limit as f64;
    let severity: u8 = if ratio >= 1.0 {
        2
    } else if ratio >= warn_threshold {
        1
    } else {
        return None;
    };
    Some(GateLimitBreach {
        kind,
        label,
        current,
        projected,
        limit,
        severity,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::portfolio_risk::exposure::{OpenPosition, PositionFactors, SectorBucket};
    use chrono::{TimeZone, Utc};

    fn snap_with_total(nlv_cents: i64, total_risk_cents: i64) -> PortfolioRisk {
        PortfolioRisk {
            snapshot_id: 0,
            account: "DU1".to_string(),
            at: Utc.with_ymd_and_hms(2026, 5, 6, 14, 30, 0).unwrap(),
            nlv_cents,
            total_dollar_risk_cents: total_risk_cents,
            open_positions: vec![],
            by_sector: vec![],
            by_factor: vec![],
        }
    }

    fn input(symbol: &str, risk_cents: i64) -> GateInput<'static> {
        GateInput {
            symbol: Box::leak(symbol.to_string().into_boxed_str()),
            projected_dollar_risk_cents: risk_cents,
            strategy: "breakout",
            momentum_bucket: None,
        }
    }

    #[test]
    fn empty_portfolio_passes_under_limits() {
        let snap = snap_with_total(10_000_000, 0);
        let gate = ConcentrationGate::new(snap, ConcentrationConfig::default(), SectorMap::arc());
        let r = gate.check_with_sector(&input("NVDA", 5_000), Some("semis"));
        // $50 risk on $100k NLV — well under all limits.
        assert_eq!(r.severity, GateSeverity::Pass);
    }

    #[test]
    fn total_risk_block_at_100pct() {
        // NLV $100k, max_total 10% = $10k limit. Current $9.5k +
        // candidate $600 = $10.1k → block.
        let snap = snap_with_total(10_000_000, 950_000);
        let gate = ConcentrationGate::new(snap, ConcentrationConfig::default(), SectorMap::arc());
        let r = gate.check_with_sector(&input("NVDA", 60_000), Some("semis"));
        assert_eq!(r.severity, GateSeverity::Block);
        assert!(r
            .breaches
            .iter()
            .any(|b| b.kind == ConcentrationKind::TotalRisk));
    }

    #[test]
    fn total_risk_warn_at_80pct() {
        // NLV $100k, limit $10k. Current $7k + $1.2k = $8.2k → 82%
        // → warn.
        let snap = snap_with_total(10_000_000, 700_000);
        let gate = ConcentrationGate::new(snap, ConcentrationConfig::default(), SectorMap::arc());
        let r = gate.check_with_sector(&input("NVDA", 120_000), Some("semis"));
        assert_eq!(r.severity, GateSeverity::Warn);
    }

    #[test]
    fn single_name_block_when_existing_position_already_at_limit() {
        // NLV $100k, name limit 1.5% = $1.5k. Existing NVDA = $1.4k.
        // Adding $200 lands at $1.6k → over 100% → block.
        let mut snap = snap_with_total(10_000_000, 140_000);
        snap.open_positions.push(OpenPosition {
            symbol: "NVDA".to_string(),
            qty: 100,
            direction: 1,
            avg_cost_cents: 10_000,
            stop_cents: 9_860,
            stop_estimated: false,
            dollar_risk_cents: 140_000,
            sector: "semis".to_string(),
            factors: PositionFactors {
                momentum: "momentum_high".to_string(),
                value: "unknown".to_string(),
                size: "size_mega".to_string(),
            },
        });
        let gate = ConcentrationGate::new(snap, ConcentrationConfig::default(), SectorMap::arc());
        let r = gate.check_with_sector(&input("NVDA", 20_000), Some("semis"));
        assert_eq!(r.severity, GateSeverity::Block);
    }

    #[test]
    fn unknown_sector_downgrades_block_to_warn() {
        // Sector limit $5k. Existing unknown = $4.9k. Adding $200 →
        // $5.1k → would be block, but unknown is leniency-bucketed →
        // warn.
        let mut snap = snap_with_total(10_000_000, 490_000);
        snap.by_sector.push(SectorBucket {
            label: "unknown".to_string(),
            dollar_risk_cents: 490_000,
            position_count: 1,
        });
        let gate = ConcentrationGate::new(snap, ConcentrationConfig::default(), SectorMap::arc());
        let r = gate.check_with_sector(&input("ZZZ", 20_000), Some("unknown"));
        assert_eq!(r.severity, GateSeverity::Warn);
    }

    #[test]
    fn sector_unknown_when_none_skips_sector_check() {
        let snap = snap_with_total(10_000_000, 0);
        let gate = ConcentrationGate::new(snap, ConcentrationConfig::default(), SectorMap::arc());
        // Even an absurdly large risk is allowed when sector lookup
        // returns None — only total / name fire.
        let r = gate.check_with_sector(&input("ZZZ", 5_000_000), None);
        // $50k risk on $100k NLV: total = 500% of limit → block,
        // single-name = 33x limit → block. Sector unchecked.
        assert!(r
            .breaches
            .iter()
            .all(|b| b.kind != ConcentrationKind::SingleSector));
    }

    #[test]
    fn factor_concurrent_block_at_5_high_momentum() {
        let mut snap = snap_with_total(10_000_000, 0);
        snap.by_factor
            .push(crate::services::portfolio_risk::exposure::FactorBucket {
                label: "momentum_high".to_string(),
                count: 4,
            });
        let gate = ConcentrationGate::new(snap, ConcentrationConfig::default(), SectorMap::arc());
        let mut i = input("NVDA", 5_000);
        i.momentum_bucket = Some("momentum_high");
        let r = gate.check_with_sector(&i, Some("semis"));
        assert_eq!(r.severity, GateSeverity::Block);
        assert!(r
            .breaches
            .iter()
            .any(|b| b.kind == ConcentrationKind::FactorConcurrent));
    }

    #[test]
    fn warning_tag_picks_worst_breach() {
        let snap = snap_with_total(10_000_000, 700_000);
        let gate = ConcentrationGate::new(snap, ConcentrationConfig::default(), SectorMap::arc());
        let r = gate.check_with_sector(&input("NVDA", 120_000), Some("semis"));
        assert_eq!(r.severity, GateSeverity::Warn);
        let tag = r.warning_tag().unwrap();
        assert!(tag.ends_with("_80pct"));
    }

    #[test]
    fn zero_dollar_risk_input_is_a_pass() {
        let snap = snap_with_total(10_000_000, 990_000);
        let gate = ConcentrationGate::new(snap, ConcentrationConfig::default(), SectorMap::arc());
        let r = gate.check_with_sector(&input("NVDA", 0), Some("semis"));
        assert_eq!(r.severity, GateSeverity::Pass);
    }
}
