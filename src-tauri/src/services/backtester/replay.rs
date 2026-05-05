//! Phase 6 — bar-replay engine.
//!
//! Walks daily bars in time order; on each bar boundary, builds a
//! `MarketContext` from bars `<= current_bar` and runs the registered
//! detectors. A detector hit schedules an entry on the *next bar's
//! open* with slippage from the fill model. Active trades check
//! intra-bar [low, high] for stop/target hits — when both fall in the
//! same bar, the stop hits first (worst-case-for-trader). After
//! `max_hold_bars` without a hit, the trade closes at the horizon
//! bar's close as a `TimeStop`.
//!
//! Strict point-in-time: the detector's context only sees bars whose
//! `bar_time <= replay_bar_time`. Attempted look-ahead trips an
//! assertion in tests.

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, TimeZone, Utc};

use crate::ibkr::types::historical::{BarSize, HistoricalBar};
use crate::ibkr::types::DataTier;
use crate::services::backtester::bars_reader::{bar_time_utc, BarsReader};
use crate::services::backtester::fill_model::{FillModel, FillSide};
use crate::services::backtester::results::{BacktestTrade, ExitReason};
use crate::services::backtester::spec::{BacktestSpec, PositionSizingMode};
use crate::services::event_calendar::EventCalendarService;
use crate::services::risk_engine::compute_sizing;
use crate::services::risk_engine::{ConvictionGrade, EquitySnapshot, EquitySource, RiskConfig};
use crate::strategies::{
    DetectorRegistry, DetectorsConfig, Direction, MarketContext, SetupCandidate,
};

/// Per-symbol replay output.
#[derive(Debug, Clone, Default)]
pub struct ReplayDiagnostics {
    pub n_setups_fired: usize,
    pub n_setups_blackout_skipped: usize,
    pub n_setups_unsizable: usize,
}

/// Replay one symbol's daily bars over `[date_from, date_to_inclusive]`.
///
/// `seq_start` is the running per-trade sequence counter; the caller
/// passes the next free index and the fn returns the count of trades
/// emitted so the caller can advance for subsequent symbols.
#[allow(clippy::too_many_arguments)]
pub async fn replay_symbol(
    symbol: &str,
    bars: &[HistoricalBar],
    spec: &BacktestSpec,
    detectors_cfg: &DetectorsConfig,
    registry: &DetectorRegistry,
    fill_model: &mut dyn FillModel,
    event_calendar: Option<&Arc<EventCalendarService>>,
    seq_start: u32,
    out: &mut Vec<BacktestTrade>,
) -> ReplayDiagnostics {
    let mut diag = ReplayDiagnostics::default();
    if bars.len() < 2 {
        return diag;
    }
    let mut seq = seq_start;
    let bar_count = bars.len();
    // Active trades for THIS symbol. We allow one open trade per
    // symbol-strategy pair; a second hit on the same strategy while
    // still open is dropped (logged as a fired-but-unsized).
    let mut open: Vec<OpenTrade> = Vec::new();

    for i in 0..bar_count {
        let bar = &bars[i];
        let bar_time = match bar_time_utc(bar) {
            Some(t) => t,
            None => continue,
        };
        let bar_date = bar_time.date_naive();
        if bar_date < spec.date_from || bar_date > spec.date_to_inclusive {
            continue;
        }

        // 1. Mark-and-exit: any open trades hit by this bar's range.
        let mut still_open: Vec<OpenTrade> = Vec::with_capacity(open.len());
        for mut tr in open.drain(..) {
            tr.bars_held += 1;
            if let Some(exit) = decide_exit(&tr, bar, fill_model, spec.max_hold_bars) {
                let realized_pnl = realized_pnl_for(&tr, exit.price) - spec.commission_usd;
                let realized_r = realized_r_for(&tr, exit.price);
                out.push(BacktestTrade {
                    seq,
                    symbol: symbol.to_string(),
                    strategy: tr.strategy.clone(),
                    direction: tr.direction,
                    entry_time: tr.entry_time,
                    entry_price: tr.entry_price,
                    exit_time: bar_time,
                    exit_price: exit.price,
                    qty: tr.qty,
                    realized_r,
                    realized_pnl,
                    exit_reason: exit.reason,
                    conviction: tr.conviction.clone(),
                });
                seq += 1;
            } else {
                still_open.push(tr);
            }
        }
        open = still_open;

        // 2. Detector eval over the prefix-context. Daily-bar slice is
        //    `&bars[..=i]` — strict point-in-time.
        let ctx = MarketContext {
            symbol,
            daily_bars: &bars[..=i],
            intraday_bars: None,
            fundamentals: None,
            recent_news: &[],
            news_verdict: None,
            current_quote: None,
            data_tier: DataTier::Unknown,
            now: bar_time,
        };
        let outcomes = registry.evaluate_all(&ctx).await;

        // 3. For each fired candidate, gate (blackout) → size → schedule
        //    entry on next bar open. Skip if no next bar (last bar of
        //    history).
        if i + 1 >= bar_count {
            continue;
        }
        let next_bar = &bars[i + 1];
        let next_bar_time = match bar_time_utc(next_bar) {
            Some(t) => t,
            None => continue,
        };
        for outcome in outcomes {
            let candidate = match outcome.result {
                Ok(Some(c)) => c,
                _ => continue,
            };
            // Filter: skip if this strategy isn't in the spec's allow-list.
            if !strategy_allowed(&candidate, spec) {
                continue;
            }
            diag.n_setups_fired += 1;

            // Blackout gate: re-use P5 calendar with the per-detector
            // policy from the spec's snapshot of `DetectorsConfig`.
            if spec.event_blackouts_enabled {
                if let Some(cal) = event_calendar {
                    let policy = detectors_cfg.blackout_policy_for(candidate.strategy);
                    if let Ok(Some(_)) = cal.is_blackout(symbol, bar_time, &policy).await {
                        diag.n_setups_blackout_skipped += 1;
                        continue;
                    }
                }
            }

            // Skip duplicate same-strategy open trades.
            if open.iter().any(|o| o.strategy == candidate.strategy) {
                continue;
            }

            let qty = compute_qty(&candidate, spec);
            if qty == 0 {
                diag.n_setups_unsizable += 1;
                continue;
            }
            let entry_price = fill_model.fill_price(
                candidate.strategy,
                candidate.direction,
                FillSide::Entry,
                next_bar.open,
            );
            // Sanity: trigger-stop violation already filtered by sizing
            // (returns 0). Defensive recheck for zero R.
            let r = (candidate.trigger_price - candidate.stop_price).abs();
            if r <= 0.0 || !r.is_finite() {
                diag.n_setups_unsizable += 1;
                continue;
            }
            // Targets: use the FIRST positive-R target as the take-profit.
            // Detectors emit a `targets` Vec but the v1 backtester collapses
            // to a single 2R-equivalent by default.
            let target_price = first_target_price(&candidate);
            let conviction = ConvictionGrade::from_signal(candidate.conviction_signal);
            open.push(OpenTrade {
                strategy: candidate.strategy.to_string(),
                direction: candidate.direction,
                entry_time: next_bar_time,
                entry_price,
                stop_price: candidate.stop_price,
                target_price,
                qty,
                bars_held: 0,
                r_per_share: r,
                conviction: Some(conviction.as_str().to_string()),
            });
        }
    }
    // 4. Force-close any still-open trades at the last bar's close as
    //    TimeStop. Important for finite-window backtests so the result
    //    aggregator sees every trade.
    if let Some(last) = bars.last() {
        if let Some(last_time) = bar_time_utc(last) {
            for tr in open.drain(..) {
                let exit_price =
                    fill_model.fill_price(&tr.strategy, tr.direction, FillSide::Exit, last.close);
                let realized_pnl = realized_pnl_for(&tr, exit_price) - spec.commission_usd;
                let realized_r = realized_r_for(&tr, exit_price);
                out.push(BacktestTrade {
                    seq,
                    symbol: symbol.to_string(),
                    strategy: tr.strategy.clone(),
                    direction: tr.direction,
                    entry_time: tr.entry_time,
                    entry_price: tr.entry_price,
                    exit_time: last_time,
                    exit_price,
                    qty: tr.qty,
                    realized_r,
                    realized_pnl,
                    exit_reason: ExitReason::TimeStop,
                    conviction: tr.conviction.clone(),
                });
                seq += 1;
            }
        }
    }
    diag
}

/// Per-symbol open-trade ledger.
struct OpenTrade {
    strategy: String,
    direction: Direction,
    entry_time: DateTime<Utc>,
    entry_price: f64,
    stop_price: f64,
    target_price: f64,
    qty: u32,
    bars_held: u32,
    /// R-per-share at entry — pinned so realized R is invariant under
    /// future detector tweaks.
    r_per_share: f64,
    conviction: Option<String>,
}

#[derive(Debug)]
struct ExitDecision {
    price: f64,
    reason: ExitReason,
}

fn decide_exit(
    tr: &OpenTrade,
    bar: &HistoricalBar,
    fill_model: &mut dyn FillModel,
    max_hold_bars: u32,
) -> Option<ExitDecision> {
    // Worst-case-for-trader convention: when both stop and target lie
    // inside the bar's range, the stop wins.
    match tr.direction {
        Direction::Long => {
            if bar.low <= tr.stop_price {
                let p = fill_model.fill_price(
                    &tr.strategy,
                    tr.direction,
                    FillSide::Exit,
                    tr.stop_price,
                );
                return Some(ExitDecision {
                    price: p,
                    reason: ExitReason::Stop,
                });
            }
            if bar.high >= tr.target_price {
                let p = fill_model.fill_price(
                    &tr.strategy,
                    tr.direction,
                    FillSide::Exit,
                    tr.target_price,
                );
                return Some(ExitDecision {
                    price: p,
                    reason: ExitReason::Target,
                });
            }
        }
        Direction::Short => {
            if bar.high >= tr.stop_price {
                let p = fill_model.fill_price(
                    &tr.strategy,
                    tr.direction,
                    FillSide::Exit,
                    tr.stop_price,
                );
                return Some(ExitDecision {
                    price: p,
                    reason: ExitReason::Stop,
                });
            }
            if bar.low <= tr.target_price {
                let p = fill_model.fill_price(
                    &tr.strategy,
                    tr.direction,
                    FillSide::Exit,
                    tr.target_price,
                );
                return Some(ExitDecision {
                    price: p,
                    reason: ExitReason::Target,
                });
            }
        }
    }
    if tr.bars_held >= max_hold_bars {
        let p = fill_model.fill_price(&tr.strategy, tr.direction, FillSide::Exit, bar.close);
        return Some(ExitDecision {
            price: p,
            reason: ExitReason::TimeStop,
        });
    }
    None
}

fn realized_pnl_for(tr: &OpenTrade, exit_price: f64) -> f64 {
    let qty = tr.qty as f64;
    match tr.direction {
        Direction::Long => qty * (exit_price - tr.entry_price),
        Direction::Short => qty * (tr.entry_price - exit_price),
    }
}

fn realized_r_for(tr: &OpenTrade, exit_price: f64) -> f64 {
    if tr.r_per_share <= 0.0 {
        return 0.0;
    }
    match tr.direction {
        Direction::Long => (exit_price - tr.entry_price) / tr.r_per_share,
        Direction::Short => (tr.entry_price - exit_price) / tr.r_per_share,
    }
}

fn first_target_price(c: &SetupCandidate) -> f64 {
    if let Some(t) = c.targets.first() {
        return t.price;
    }
    // Synthetic 2R target if the detector emitted no targets.
    let r = (c.trigger_price - c.stop_price).abs();
    match c.direction {
        Direction::Long => c.trigger_price + 2.0 * r,
        Direction::Short => c.trigger_price - 2.0 * r,
    }
}

fn strategy_allowed(c: &SetupCandidate, spec: &BacktestSpec) -> bool {
    if spec.detector_tags.is_empty() {
        return true;
    }
    spec.detector_tags.iter().any(|t| t == &c.tag)
}

fn compute_qty(c: &SetupCandidate, spec: &BacktestSpec) -> u32 {
    match spec.position_sizing {
        PositionSizingMode::NoSizing => 1,
        PositionSizingMode::FixedR => {
            // Risk a $1k notional R; qty = 1000 / r_per_share floored.
            let r = (c.trigger_price - c.stop_price).abs();
            if r <= 0.0 {
                return 0;
            }
            ((1000.0 / r).floor() as i64).max(0).min(u32::MAX as i64) as u32
        }
        PositionSizingMode::ConvictionScaledR => {
            let snap = synthetic_snapshot(spec.starting_equity_usd);
            let cfg = RiskConfig::default();
            let s = compute_sizing(c, &snap, &cfg);
            s.qty
        }
    }
}

fn synthetic_snapshot(equity_usd: f64) -> EquitySnapshot {
    EquitySnapshot {
        account: "BACKTEST".to_string(),
        as_of_date: "2025-01-01".to_string(),
        nlv_cents: (equity_usd * 100.0).round() as i64,
        source: EquitySource::Manual,
        fetched_at: Utc::now(),
    }
}

/// Convenience: pull all daily bars for `(symbol, [from, to_inclusive])`
/// from a `BarsReader`. The caller is the per-symbol replay driver in
/// `mod.rs`; pulled out so unit tests can substitute a stub reader.
pub async fn read_daily_bars(
    reader: &dyn BarsReader,
    symbol: &str,
    from: NaiveDate,
    to_inclusive: NaiveDate,
) -> Vec<HistoricalBar> {
    let start = Utc
        .from_utc_datetime(&from.and_hms_opt(0, 0, 0).expect("midnight valid"))
        .timestamp();
    let end = Utc
        .from_utc_datetime(&to_inclusive.and_hms_opt(23, 59, 59).expect("eod valid"))
        .timestamp();
    reader
        .read_window(symbol, BarSize::Day1, start, end)
        .await
        .unwrap_or_default()
}
