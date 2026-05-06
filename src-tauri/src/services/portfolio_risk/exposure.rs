//! Phase 8 — exposure math. Pure: takes positions + bracket stops +
//! sector/factor lookups and produces a `PortfolioRisk` snapshot.
//! Has no IO, no async, no IBKR dependency — the gate's test
//! coverage hinges on this being trivially callable from unit tests.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ibkr::types::positions::Position;

use super::factors::{FactorBuckets, FactorInputs, FactorMembership};
use super::sector_map::SectorMap;

/// Default fallback stop distance when no bracket stop is recorded
/// for a position. Master decision: "5% below entry; surface
/// 'stop estimated' annotation". 0.05 = 5%.
pub const DEFAULT_STOP_FALLBACK_PCT: f64 = 0.05;

/// One open position joined with its inferred stop. Cents are
/// rounded down at the boundary to keep SQLite integers honest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenPosition {
    pub symbol: String,
    pub qty: i64,
    /// `+1` long / `-1` short. Multi-leg / option positions out of
    /// scope for P8.
    pub direction: i8,
    pub avg_cost_cents: i64,
    pub stop_cents: i64,
    /// `true` when `stop_cents` was inferred from
    /// `DEFAULT_STOP_FALLBACK_PCT` rather than read from
    /// `bracket_groups`. UI shows a "stop estimated" annotation.
    pub stop_estimated: bool,
    /// Per-position dollar-risk in cents. Always positive: the
    /// notional loss if the stop fills.
    pub dollar_risk_cents: i64,
    pub sector: String,
    pub factors: PositionFactors,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PositionFactors {
    pub momentum: String,
    pub value: String,
    pub size: String,
}

impl From<FactorMembership> for PositionFactors {
    fn from(m: FactorMembership) -> Self {
        Self {
            momentum: m.momentum.to_string(),
            value: m.value.to_string(),
            size: m.size.to_string(),
        }
    }
}

/// Aggregate exposure per sector. The frontend's RiskSnapshot bar
/// reads this directly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectorBucket {
    pub label: String,
    pub dollar_risk_cents: i64,
    pub position_count: usize,
}

/// Aggregate count per factor bucket. Coarse: the heatmap doesn't
/// need dollar-risk on factors, just headcount.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactorBucket {
    pub label: String,
    pub count: usize,
}

/// One slice of a snapshot: per-sector or per-factor aggregate
/// view. The snapshot writes both kinds at once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExposureSlice {
    pub by_sector: Vec<SectorBucket>,
    pub by_factor: Vec<FactorBucket>,
}

/// The headline portfolio-risk view. Returned from
/// `PortfolioRiskService::snapshot` and persisted as a single row in
/// `portfolio_snapshots` (with `exposures_json` carrying the by-*
/// slices). `snapshot_id` lets the caller re-fetch the same row
/// later for replay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortfolioRisk {
    pub snapshot_id: i64,
    pub account: String,
    pub at: DateTime<Utc>,
    pub nlv_cents: i64,
    /// Sum of every open-position `dollar_risk_cents`. Documented
    /// caveat: assumes independence — overstates loss in correlated
    /// markets, understates in mean-reverting. The number is still
    /// the right anchor for "am I about to add a 5th high-risk
    /// position".
    pub total_dollar_risk_cents: i64,
    pub open_positions: Vec<OpenPosition>,
    pub by_sector: Vec<SectorBucket>,
    pub by_factor: Vec<FactorBucket>,
}

impl PortfolioRisk {
    /// Convenience: total dollar-risk as a fraction of NLV. `0.0`
    /// when NLV is zero (defensive — `current()` should not return
    /// zero NLV but a 0/0 here would be `NaN` and propagate badly).
    pub fn total_risk_pct_nlv(&self) -> f64 {
        if self.nlv_cents <= 0 {
            return 0.0;
        }
        self.total_dollar_risk_cents as f64 / self.nlv_cents as f64
    }

    /// Convenience: per-symbol dollar-risk lookup.
    pub fn risk_for(&self, symbol: &str) -> i64 {
        let upper = symbol.to_uppercase();
        self.open_positions
            .iter()
            .find(|p| p.symbol == upper)
            .map(|p| p.dollar_risk_cents)
            .unwrap_or(0)
    }

    /// Convenience: aggregate dollar-risk in `sector` (cents).
    pub fn sector_risk(&self, sector: &str) -> i64 {
        self.by_sector
            .iter()
            .find(|s| s.label == sector)
            .map(|s| s.dollar_risk_cents)
            .unwrap_or(0)
    }

    /// Convenience: position count in a factor bucket.
    pub fn factor_count(&self, factor: &str) -> usize {
        self.by_factor
            .iter()
            .find(|f| f.label == factor)
            .map(|f| f.count)
            .unwrap_or(0)
    }
}

/// The compute path. Pure — given the inputs, returns the same
/// snapshot every time. The caller (`PortfolioRiskService::snapshot`)
/// glues IBKR + DB into the inputs and persists the output.
///
/// `bracket_stops` is a `(symbol, stop_cents)` map; missing entries
/// fall back to `avg_cost * (1 - DEFAULT_STOP_FALLBACK_PCT)` for
/// longs and `avg_cost * (1 + DEFAULT_STOP_FALLBACK_PCT)` for shorts.
#[allow(clippy::too_many_arguments)]
pub fn compute(
    account: &str,
    at: DateTime<Utc>,
    nlv_cents: i64,
    raw_positions: &[Position],
    bracket_stops: &std::collections::HashMap<String, i64>,
    sector_map: &SectorMap,
    factors: &FactorBuckets,
) -> PortfolioRisk {
    let mut open_positions = Vec::with_capacity(raw_positions.len());
    let mut by_sector_acc: BTreeMap<String, (i64, usize)> = BTreeMap::new();
    let mut by_factor_acc: BTreeMap<String, usize> = BTreeMap::new();
    let mut total_dollar_risk_cents: i64 = 0;

    for raw in raw_positions {
        if raw.position.abs() < 0.5 {
            continue; // closed / fractional residue.
        }
        let direction: i8 = if raw.position > 0.0 { 1 } else { -1 };
        let qty: i64 = raw.position.round().abs() as i64;
        let avg_cost_cents = (raw.average_cost * 100.0).round() as i64;
        let upper = raw.symbol.to_uppercase();

        let (stop_cents, stop_estimated) = match bracket_stops.get(&upper) {
            Some(s) => (*s, false),
            None => {
                let fallback = if direction == 1 {
                    raw.average_cost * (1.0 - DEFAULT_STOP_FALLBACK_PCT)
                } else {
                    raw.average_cost * (1.0 + DEFAULT_STOP_FALLBACK_PCT)
                };
                ((fallback * 100.0).round() as i64, true)
            }
        };

        let per_share_risk_cents = (avg_cost_cents - stop_cents).abs();
        let dollar_risk_cents = per_share_risk_cents.saturating_mul(qty);
        total_dollar_risk_cents = total_dollar_risk_cents.saturating_add(dollar_risk_cents);

        let sector = SectorMap::lookup_static(&upper)
            .unwrap_or("unknown")
            .to_string();
        // Pure path defaults factor membership to `unknown`. The async
        // snapshot route can pre-warm the factors cache via
        // `lookup_or_compute` so future passes get richer buckets, but
        // `compute` itself stays deterministic and IO-free. The
        // `factors` and `sector_map` params are kept on the signature
        // so the wiring point doesn't change when the cache lookup is
        // inlined here in a future pass.
        let factor_membership = FactorBuckets::compute_from(&FactorInputs::default());
        let _ = factors;
        let _ = sector_map;

        let pf: PositionFactors = factor_membership.into();
        *by_sector_acc.entry(sector.clone()).or_insert((0, 0)) =
            apply_sector_acc(by_sector_acc.get(&sector).copied(), dollar_risk_cents);
        *by_factor_acc.entry(pf.momentum.clone()).or_insert(0) += 1;
        // Skip adding momentum=value=size unknown to the factor
        // bucket multiple times: only momentum carries forward as
        // the gate's `factor_concurrent` axis (master committed the
        // gate cuts on 4-concurrent, not on a 12-cell heatmap).
        // Surface size + value buckets in the heatmap but don't gate
        // on them yet.
        if pf.size != "unknown" {
            *by_factor_acc.entry(pf.size.clone()).or_insert(0) += 1;
        }
        if pf.value != "unknown" {
            *by_factor_acc.entry(pf.value.clone()).or_insert(0) += 1;
        }

        open_positions.push(OpenPosition {
            symbol: upper,
            qty,
            direction,
            avg_cost_cents,
            stop_cents,
            stop_estimated,
            dollar_risk_cents,
            sector,
            factors: pf,
        });
    }

    let by_sector = by_sector_acc
        .into_iter()
        .map(
            |(label, (dollar_risk_cents, position_count))| SectorBucket {
                label,
                dollar_risk_cents,
                position_count,
            },
        )
        .collect();
    let by_factor = by_factor_acc
        .into_iter()
        .map(|(label, count)| FactorBucket { label, count })
        .collect();

    PortfolioRisk {
        snapshot_id: 0, // populated by the persistence layer
        account: account.to_string(),
        at,
        nlv_cents,
        total_dollar_risk_cents,
        open_positions,
        by_sector,
        by_factor,
    }
}

/// Apply a new position's dollar-risk to a sector accumulator.
fn apply_sector_acc(prior: Option<(i64, usize)>, add_risk: i64) -> (i64, usize) {
    let (existing_risk, existing_count) = prior.unwrap_or((0, 0));
    (existing_risk.saturating_add(add_risk), existing_count + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::collections::HashMap;

    fn pos(symbol: &str, qty: f64, avg: f64) -> Position {
        Position {
            symbol: symbol.to_string(),
            position: qty,
            average_cost: avg,
            ..Default::default()
        }
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 6, 14, 30, 0).unwrap()
    }

    #[test]
    fn empty_portfolio_zero_risk() {
        let map = SectorMap::new();
        let factors = FactorBuckets::new();
        let r = compute(
            "DU1",
            now(),
            10_000_000,
            &[],
            &HashMap::new(),
            &map,
            &factors,
        );
        assert_eq!(r.total_dollar_risk_cents, 0);
        assert!(r.open_positions.is_empty());
        assert!(r.by_sector.is_empty());
    }

    #[test]
    fn single_long_with_recorded_stop() {
        let map = SectorMap::new();
        let factors = FactorBuckets::new();
        let mut stops = HashMap::new();
        stops.insert("NVDA".to_string(), 9500); // $95.00
        let positions = vec![pos("NVDA", 100.0, 100.0)]; // entry $100, 100 sh long
        let r = compute(
            "DU1",
            now(),
            100_000_000,
            &positions,
            &stops,
            &map,
            &factors,
        );
        assert_eq!(r.open_positions.len(), 1);
        let p = &r.open_positions[0];
        assert_eq!(p.qty, 100);
        assert_eq!(p.direction, 1);
        assert_eq!(p.avg_cost_cents, 10_000);
        assert_eq!(p.stop_cents, 9_500);
        assert!(!p.stop_estimated);
        // 100 sh × $5 risk = $500 = 50_000 cents
        assert_eq!(p.dollar_risk_cents, 50_000);
        assert_eq!(r.total_dollar_risk_cents, 50_000);
        assert_eq!(p.sector, "semis");
        let semis = r.by_sector.iter().find(|s| s.label == "semis").unwrap();
        assert_eq!(semis.dollar_risk_cents, 50_000);
        assert_eq!(semis.position_count, 1);
    }

    #[test]
    fn missing_stop_falls_back_to_5pct() {
        let map = SectorMap::new();
        let factors = FactorBuckets::new();
        let positions = vec![pos("NVDA", 100.0, 100.0)];
        let r = compute(
            "DU1",
            now(),
            100_000_000,
            &positions,
            &HashMap::new(),
            &map,
            &factors,
        );
        let p = &r.open_positions[0];
        assert!(p.stop_estimated);
        // 5% of $100 = $5 → stop $95 → risk $5/sh × 100 = $500
        assert_eq!(p.stop_cents, 9_500);
        assert_eq!(p.dollar_risk_cents, 50_000);
    }

    #[test]
    fn short_position_inverts_stop_direction() {
        let map = SectorMap::new();
        let factors = FactorBuckets::new();
        // Short 50 sh at $100 → fallback stop $105 → risk $5 × 50 = $250
        let positions = vec![pos("AAPL", -50.0, 100.0)];
        let r = compute(
            "DU1",
            now(),
            100_000_000,
            &positions,
            &HashMap::new(),
            &map,
            &factors,
        );
        let p = &r.open_positions[0];
        assert_eq!(p.direction, -1);
        assert_eq!(p.qty, 50);
        assert_eq!(p.stop_cents, 10_500);
        assert!(p.stop_estimated);
        assert_eq!(p.dollar_risk_cents, 25_000);
    }

    #[test]
    fn multi_position_sector_aggregation() {
        let map = SectorMap::new();
        let factors = FactorBuckets::new();
        let mut stops = HashMap::new();
        stops.insert("NVDA".to_string(), 9_500);
        stops.insert("AMD".to_string(), 14_000);
        let positions = vec![
            pos("NVDA", 100.0, 100.0), // +500
            pos("AMD", 50.0, 150.0),   // +500 (50 × $10)
            pos("JPM", 25.0, 200.0),   // financials, fallback stop = $190 → $10/sh × 25 = $250
        ];
        let r = compute(
            "DU1",
            now(),
            100_000_000,
            &positions,
            &stops,
            &map,
            &factors,
        );
        // Two semis names, one financials.
        assert_eq!(r.open_positions.len(), 3);
        let semis = r.by_sector.iter().find(|s| s.label == "semis").unwrap();
        assert_eq!(semis.position_count, 2);
        assert_eq!(semis.dollar_risk_cents, 100_000);
        let fins = r
            .by_sector
            .iter()
            .find(|s| s.label == "financials")
            .unwrap();
        assert_eq!(fins.position_count, 1);
        assert_eq!(fins.dollar_risk_cents, 25_000);
        assert_eq!(r.total_dollar_risk_cents, 125_000);
    }

    #[test]
    fn unknown_sector_does_not_panic() {
        let map = SectorMap::new();
        let factors = FactorBuckets::new();
        let positions = vec![pos("ZZZZZ", 10.0, 50.0)];
        let r = compute(
            "DU1",
            now(),
            100_000_000,
            &positions,
            &HashMap::new(),
            &map,
            &factors,
        );
        assert_eq!(r.open_positions[0].sector, "unknown");
        let unknown = r.by_sector.iter().find(|s| s.label == "unknown").unwrap();
        assert_eq!(unknown.position_count, 1);
    }

    #[test]
    fn zero_qty_residue_filtered() {
        let map = SectorMap::new();
        let factors = FactorBuckets::new();
        let positions = vec![pos("NVDA", 0.0, 100.0)];
        let r = compute(
            "DU1",
            now(),
            100_000_000,
            &positions,
            &HashMap::new(),
            &map,
            &factors,
        );
        assert!(r.open_positions.is_empty());
    }

    #[test]
    fn total_risk_pct_nlv_computes_fraction() {
        let r = PortfolioRisk {
            snapshot_id: 0,
            account: "DU1".to_string(),
            at: now(),
            nlv_cents: 10_000_000,            // $100k
            total_dollar_risk_cents: 500_000, // $5k
            open_positions: vec![],
            by_sector: vec![],
            by_factor: vec![],
        };
        assert!((r.total_risk_pct_nlv() - 0.05).abs() < 1e-9);
    }
}
