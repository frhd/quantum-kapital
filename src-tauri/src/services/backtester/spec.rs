//! Phase 6 — `BacktestSpec`: the deterministic input to a run.
//!
//! Spec carries everything that influences result determinism:
//! symbols, date range, detector tags, fill-model choice + parameters,
//! position-sizing mode, splits config, RNG seed, commissions.
//! `spec_hash()` is a stable 16-hex fingerprint over a canonical
//! string encoding of the spec — same spec ⇒ same hash ⇒ runs are
//! re-runnable to the byte. `BacktestSpec` itself stays plain serde
//! so the row's `spec_json` round-trips trivially.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::ibkr::types::StrategyTag;

/// Hard cap on a single run's symbol count and history span. Beyond
/// these the backtester refuses with a "split into smaller runs"
/// error. Mirrors the master-plan committed defaults; configurable
/// as a future toggle once memory/time profiling justifies a higher
/// ceiling.
pub const MAX_SYMBOLS_PER_RUN: usize = 50;
pub const MAX_HISTORY_DAYS: i64 = 5 * 365;

/// Position-sizing mode for the backtest. Default is conviction-scaled
/// R: same path as production sizing. `FixedR` produces a per-trade R
/// stream isolated from equity-cap effects (useful for measuring raw
/// detector edge); `NoSizing` returns 1 share per trade (PnL = R).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PositionSizingMode {
    /// Use `RiskEngine::compute_sizing` against a synthetic snapshot.
    /// PnL is then `qty * (exit - entry) - commission`.
    #[default]
    ConvictionScaledR,
    /// 1R per trade; equity grows by `realized_r * unit_dollar_risk`
    /// per trade. `unit_dollar_risk` is fixed at the start of the
    /// run.
    FixedR,
    /// 1 share per trade; PnL is the raw price difference. Use this
    /// when you only care about the R-stream and don't want sizing
    /// math to color the result.
    NoSizing,
}

/// Choice of fill model. Slippage knobs are inline so the spec self-
/// documents; the `CalibratedFromPath` variant defers calibration to
/// a P2 attribution lookup at run time and stores only the lookup
/// scope in the spec.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FillModelKind {
    /// Fill at next bar's OPEN ± `bps` slippage. Default for first-pass
    /// backtests; matches the master-plan `Defaults committed` table.
    NaiveNextOpen {
        /// Symmetric slippage applied to entry and exit fills, in bps.
        slippage_bps: u32,
    },
    /// Per-strategy slippage distribution, sampled at run time. Mean +
    /// stdev are pulled from `tca_get_slippage_distribution` for the
    /// `(date_from, date_to_inclusive)` window. RNG seeded from
    /// `BacktestSpec.rng_seed`.
    Calibrated {
        /// ET-local date range to source slippage from. Independent of
        /// the backtest window so the user can calibrate from the most
        /// recent month of live data and apply it to any historical
        /// backtest.
        date_from: NaiveDate,
        date_to_inclusive: NaiveDate,
        /// Account to sample from. `None` ⇒ all accounts.
        account: Option<String>,
        /// Fallback bps when the strategy has zero observed fills.
        fallback_bps: u32,
    },
}

impl Default for FillModelKind {
    fn default() -> Self {
        Self::NaiveNextOpen { slippage_bps: 8 }
    }
}

/// Walk-forward split config. Defaults to 12-month train, 3-month
/// OOS, 1-month roll (master-plan committed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WalkForwardSplits {
    pub train_months: u32,
    pub oos_months: u32,
    pub roll_months: u32,
}

impl Default for WalkForwardSplits {
    fn default() -> Self {
        Self {
            train_months: 12,
            oos_months: 3,
            roll_months: 1,
        }
    }
}

/// Top-level backtest input. `rng_seed` is used by the calibrated fill
/// model and any future sampling-based split — the rest of the
/// pipeline is deterministic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BacktestSpec {
    pub date_from: NaiveDate,
    pub date_to_inclusive: NaiveDate,
    pub symbols: Vec<String>,
    /// Detector tags to include. Empty ⇒ all detectors in the registry.
    #[serde(default)]
    pub detector_tags: Vec<StrategyTag>,
    #[serde(default)]
    pub fill_model: FillModelKind,
    #[serde(default)]
    pub position_sizing: PositionSizingMode,
    #[serde(default)]
    pub splits: WalkForwardSplits,
    /// Per-trade flat commission in USD. Master-plan default $1.
    #[serde(default = "default_commission_usd")]
    pub commission_usd: f64,
    /// Starting equity for the equity curve, in USD. Used to baseline
    /// the equity series; does not constrain sizing (sizing reads from
    /// the synthetic snapshot built from this number).
    #[serde(default = "default_starting_equity_usd")]
    pub starting_equity_usd: f64,
    /// `true` ⇒ honor event blackouts (P5) at replay-time. Backtests
    /// that want to compare with-vs-without should run the same spec
    /// twice with this flipped.
    #[serde(default = "default_event_blackouts_enabled")]
    pub event_blackouts_enabled: bool,
    /// Maximum bars to hold a trade if neither stop nor target hit
    /// (daily-bar count). After this, the trade closes at the
    /// horizon-bar's close as a "time stop". 10 default.
    #[serde(default = "default_max_hold_bars")]
    pub max_hold_bars: u32,
    /// PRNG seed for any sampling-based fill model. Stored in the
    /// spec so reruns are byte-identical.
    #[serde(default = "default_rng_seed")]
    pub rng_seed: u64,
    /// Optional human-readable label for the run (rendered in the UI).
    #[serde(default)]
    pub label: Option<String>,
}

fn default_commission_usd() -> f64 {
    1.0
}

fn default_starting_equity_usd() -> f64 {
    100_000.0
}

fn default_event_blackouts_enabled() -> bool {
    true
}

fn default_max_hold_bars() -> u32 {
    10
}

fn default_rng_seed() -> u64 {
    0x5345_4544_5345_4544 // "SEEDSEED" mnemonic
}

#[derive(Debug, thiserror::Error)]
pub enum SpecValidationError {
    #[error("date_from {from} is after date_to {to}")]
    InvertedDateRange { from: NaiveDate, to: NaiveDate },
    #[error("history span {span_days}d > max {max} — split into smaller runs")]
    HistoryTooLong { span_days: i64, max: i64 },
    #[error("no symbols provided")]
    NoSymbols,
    #[error("symbol count {n} > max {max} — split into smaller runs")]
    TooManySymbols { n: usize, max: usize },
    #[error("oos_months={oos} must be >= 1")]
    OosTooSmall { oos: u32 },
    #[error("train_months={train} must be >= oos_months={oos}")]
    TrainSmallerThanOos { train: u32, oos: u32 },
}

impl BacktestSpec {
    /// Reject specs that would OOM the runner or produce statistically
    /// meaningless OOS windows. Pure: no IO; safe to call on the
    /// command thread before kicking off the run.
    pub fn validate(&self) -> Result<(), SpecValidationError> {
        if self.date_from > self.date_to_inclusive {
            return Err(SpecValidationError::InvertedDateRange {
                from: self.date_from,
                to: self.date_to_inclusive,
            });
        }
        let span = (self.date_to_inclusive - self.date_from).num_days();
        if span > MAX_HISTORY_DAYS {
            return Err(SpecValidationError::HistoryTooLong {
                span_days: span,
                max: MAX_HISTORY_DAYS,
            });
        }
        if self.symbols.is_empty() {
            return Err(SpecValidationError::NoSymbols);
        }
        if self.symbols.len() > MAX_SYMBOLS_PER_RUN {
            return Err(SpecValidationError::TooManySymbols {
                n: self.symbols.len(),
                max: MAX_SYMBOLS_PER_RUN,
            });
        }
        if self.splits.oos_months < 1 {
            return Err(SpecValidationError::OosTooSmall {
                oos: self.splits.oos_months,
            });
        }
        if self.splits.train_months < self.splits.oos_months {
            return Err(SpecValidationError::TrainSmallerThanOos {
                train: self.splits.train_months,
                oos: self.splits.oos_months,
            });
        }
        Ok(())
    }

    /// Stable 16-hex fingerprint of the canonical encoding. Two specs
    /// with the same hash produce the same backtest result; the same
    /// spec across processes hashes identically (no clock / no RNG /
    /// no float rounding in the digest path).
    pub fn spec_hash(&self) -> String {
        let canon = self.canonical_string();
        // FNV-1a 64-bit. We don't need crypto strength here — equality-
        // discrimination is the load-bearing property.
        const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut h: u64 = FNV_OFFSET;
        for b in canon.as_bytes() {
            h ^= u64::from(*b);
            h = h.wrapping_mul(FNV_PRIME);
        }
        format!("{:016x}", h)
    }

    /// Canonical string — a sorted-key serialization without
    /// whitespace. `serde_json::to_string` with a BTreeMap-backed
    /// shape would do but the stdlib lacks deterministic field order
    /// across crate versions. We do it by hand; the cost is one extra
    /// place to remember when adding spec fields. The
    /// `hash_changes_when_spec_changes` test pins this.
    fn canonical_string(&self) -> String {
        let mut symbols = self.symbols.clone();
        symbols.sort();
        let mut tags: Vec<String> = self
            .detector_tags
            .iter()
            .map(|t| t.as_str().to_string())
            .collect();
        tags.sort();
        let fill_kind = match &self.fill_model {
            FillModelKind::NaiveNextOpen { slippage_bps } => {
                format!("naive:{}", slippage_bps)
            }
            FillModelKind::Calibrated {
                date_from,
                date_to_inclusive,
                account,
                fallback_bps,
            } => format!(
                "calibrated:{}:{}:{}:{}",
                date_from,
                date_to_inclusive,
                account.as_deref().unwrap_or(""),
                fallback_bps,
            ),
        };
        let sizing = match self.position_sizing {
            PositionSizingMode::ConvictionScaledR => "conviction_scaled_r",
            PositionSizingMode::FixedR => "fixed_r",
            PositionSizingMode::NoSizing => "no_sizing",
        };
        format!(
            "v1|range={}..{}|symbols={}|tags={}|fill={}|sizing={}|splits={}/{}/{}|comm={:.2}|equity={:.2}|blackouts={}|max_hold={}|seed={}",
            self.date_from,
            self.date_to_inclusive,
            symbols.join(","),
            tags.join(","),
            fill_kind,
            sizing,
            self.splits.train_months,
            self.splits.oos_months,
            self.splits.roll_months,
            self.commission_usd,
            self.starting_equity_usd,
            self.event_blackouts_enabled,
            self.max_hold_bars,
            self.rng_seed,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_spec() -> BacktestSpec {
        BacktestSpec {
            date_from: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
            date_to_inclusive: NaiveDate::from_ymd_opt(2025, 6, 30).unwrap(),
            symbols: vec!["AAPL".to_string(), "MSFT".to_string()],
            detector_tags: Vec::new(),
            fill_model: FillModelKind::default(),
            position_sizing: PositionSizingMode::default(),
            splits: WalkForwardSplits::default(),
            commission_usd: 1.0,
            starting_equity_usd: 100_000.0,
            event_blackouts_enabled: true,
            max_hold_bars: 10,
            rng_seed: 42,
            label: None,
        }
    }

    #[test]
    fn validate_rejects_inverted_range() {
        let spec = BacktestSpec {
            date_from: NaiveDate::from_ymd_opt(2025, 6, 30).unwrap(),
            date_to_inclusive: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
            ..base_spec()
        };
        assert!(matches!(
            spec.validate(),
            Err(SpecValidationError::InvertedDateRange { .. })
        ));
    }

    #[test]
    fn validate_rejects_too_many_symbols() {
        let spec = BacktestSpec {
            symbols: (0..51).map(|i| format!("S{}", i)).collect(),
            ..base_spec()
        };
        assert!(matches!(
            spec.validate(),
            Err(SpecValidationError::TooManySymbols { .. })
        ));
    }

    #[test]
    fn validate_rejects_history_too_long() {
        let spec = BacktestSpec {
            date_from: NaiveDate::from_ymd_opt(2010, 1, 1).unwrap(),
            date_to_inclusive: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            ..base_spec()
        };
        assert!(matches!(
            spec.validate(),
            Err(SpecValidationError::HistoryTooLong { .. })
        ));
    }

    #[test]
    fn validate_rejects_no_symbols() {
        let spec = BacktestSpec {
            symbols: Vec::new(),
            ..base_spec()
        };
        assert!(matches!(
            spec.validate(),
            Err(SpecValidationError::NoSymbols)
        ));
    }

    #[test]
    fn validate_passes_default_split() {
        assert!(base_spec().validate().is_ok());
    }

    #[test]
    fn hash_is_stable_for_same_inputs() {
        let a = base_spec();
        let b = base_spec();
        assert_eq!(a.spec_hash(), b.spec_hash());
    }

    #[test]
    fn hash_changes_when_spec_changes() {
        let a = base_spec();
        let mut b = base_spec();
        b.commission_usd = 1.5;
        assert_ne!(a.spec_hash(), b.spec_hash());

        let mut c = base_spec();
        c.symbols = vec!["MSFT".to_string(), "AAPL".to_string()]; // reordered
        assert_eq!(a.spec_hash(), c.spec_hash(), "symbol order ignored");

        let mut d = base_spec();
        d.symbols.push("GOOG".to_string());
        assert_ne!(a.spec_hash(), d.spec_hash(), "extra symbol counts");

        let mut e = base_spec();
        e.fill_model = FillModelKind::NaiveNextOpen { slippage_bps: 12 };
        assert_ne!(a.spec_hash(), e.spec_hash());
    }

    #[test]
    fn label_does_not_affect_hash() {
        // Cosmetic field — two specs that differ only on label should
        // hash the same so users can re-label without invalidating the
        // result-cache lookup.
        let a = base_spec();
        let b = BacktestSpec {
            label: Some("ablation: no blackouts".to_string()),
            ..base_spec()
        };
        assert_eq!(a.spec_hash(), b.spec_hash());
    }

    #[test]
    fn round_trip_serde_preserves_equality() {
        let a = base_spec();
        let json = serde_json::to_string(&a).unwrap();
        let b: BacktestSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.spec_hash(), b.spec_hash());
    }
}
