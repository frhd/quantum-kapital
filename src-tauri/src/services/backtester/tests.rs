//! Integration tests for the backtester orchestrator.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{NaiveDate, TimeZone, Utc};
use tempfile::tempdir;

use crate::ibkr::types::historical::{BarSize, HistoricalBar};
use crate::services::backtester::bars_reader::{insert_bar, BarsReader, DbBarsReader};
use crate::services::backtester::fill_model::{FillModel, FillSide, NaiveNextOpenFill};
use crate::services::backtester::replay::{read_daily_bars, replay_symbol};
use crate::services::backtester::results::ExitReason;
use crate::services::backtester::spec::{
    BacktestSpec, FillModelKind, PositionSizingMode, WalkForwardSplits,
};
use crate::services::backtester::Backtester;
use crate::storage::Db;
use crate::strategies::{registry_from_config, DetectorsConfig, Direction};

fn open_temp_db() -> (tempfile::TempDir, Arc<Db>) {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("bt.sqlite");
    let db = Arc::new(Db::open(&path).expect("open db"));
    (dir, db)
}

/// Build a synthetic 30-day daily bar series. Day index 0 is
/// `start_date`; each bar opens at `base + i*step`, closes higher,
/// with a tight low/high range. Bars are emitted at midnight UTC.
fn synthetic_bars(start_date: NaiveDate, n_days: u32, base: f64, step: f64) -> Vec<HistoricalBar> {
    (0..n_days)
        .map(|i| {
            let d = start_date + chrono::Duration::days(i as i64);
            let o = base + step * i as f64;
            HistoricalBar {
                time: d.format("%Y%m%d").to_string(),
                open: o,
                high: o + 1.0,
                low: o - 1.0,
                close: o + 0.5,
                volume: 1_000_000,
                wap: o,
                count: 0,
            }
        })
        .collect()
}

/// Build a daily-bar series with enough data for the breakout detector
/// to fire. The breakout detector needs ~`lookback_days + atr_period`
/// bars of history with one final bar at a 20-day high on volume.
fn breakout_bars(start_date: NaiveDate) -> Vec<HistoricalBar> {
    let mut bars = Vec::new();
    // 30 baseline bars in a tight range.
    for i in 0..30 {
        let d = start_date + chrono::Duration::days(i);
        bars.push(HistoricalBar {
            time: d.format("%Y%m%d").to_string(),
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1_000_000,
            wap: 100.0,
            count: 0,
        });
    }
    // Day 31: a breakout bar.
    let d = start_date + chrono::Duration::days(30);
    bars.push(HistoricalBar {
        time: d.format("%Y%m%d").to_string(),
        open: 102.0,
        high: 110.0,
        low: 101.5,
        close: 109.0,
        volume: 5_000_000,
        wap: 105.0,
        count: 0,
    });
    // Day 32+: continuation. The replay loop needs at least one bar
    // after a fire to fill an entry.
    for i in 31..40 {
        let d = start_date + chrono::Duration::days(i);
        let drift = 109.0 + (i as f64 - 30.0) * 1.0;
        bars.push(HistoricalBar {
            time: d.format("%Y%m%d").to_string(),
            open: drift,
            high: drift + 2.0,
            low: drift - 1.5,
            close: drift + 1.0,
            volume: 1_500_000,
            wap: drift,
            count: 0,
        });
    }
    bars
}

#[tokio::test]
async fn db_bars_reader_round_trips_a_window() {
    let (_dir, db) = open_temp_db();
    let reader = DbBarsReader::new(Arc::clone(&db));
    let date = NaiveDate::from_ymd_opt(2025, 5, 1).unwrap();
    let bars = synthetic_bars(date, 5, 100.0, 0.5);
    for b in &bars {
        insert_bar(&db, "AAPL", BarSize::Day1, b)
            .await
            .expect("insert");
    }
    let start = Utc
        .from_utc_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
        .timestamp();
    let end = Utc
        .from_utc_datetime(
            &(date + chrono::Duration::days(10))
                .and_hms_opt(23, 59, 59)
                .unwrap(),
        )
        .timestamp();
    let read = reader
        .read_window("AAPL", BarSize::Day1, start, end)
        .await
        .expect("read");
    assert_eq!(read.len(), 5);
    // Sorted ascending.
    for w in read.windows(2) {
        assert!(w[0].time <= w[1].time);
    }
}

#[tokio::test]
async fn replay_yields_no_trades_on_flat_bars() {
    // Detectors should not fire on a flat 100/100/100 series. The
    // replay loop must emit zero trades — this is the "harness baseline"
    // sanity check.
    let (_dir, db) = open_temp_db();
    let reader: Arc<dyn BarsReader> = Arc::new(DbBarsReader::new(Arc::clone(&db)));
    let date = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
    let bars = synthetic_bars(date, 30, 100.0, 0.0);
    for b in &bars {
        insert_bar(&db, "AAPL", BarSize::Day1, b).await.unwrap();
    }
    let detectors_cfg = DetectorsConfig::default();
    let registry = Arc::new(registry_from_config(&detectors_cfg));
    let bt = Backtester::new(
        Arc::clone(&db),
        reader,
        Arc::clone(&registry),
        Arc::new(detectors_cfg),
    );
    let spec = BacktestSpec {
        date_from: date,
        date_to_inclusive: date + chrono::Duration::days(29),
        symbols: vec!["AAPL".to_string()],
        detector_tags: Vec::new(),
        fill_model: FillModelKind::NaiveNextOpen { slippage_bps: 0 },
        position_sizing: PositionSizingMode::NoSizing,
        splits: WalkForwardSplits::default(),
        commission_usd: 0.0,
        starting_equity_usd: 100_000.0,
        event_blackouts_enabled: false,
        max_hold_bars: 5,
        rng_seed: 0,
        label: None,
    };
    let result = bt.run(spec).await.expect("run");
    assert_eq!(result.trades.len(), 0, "flat bars should fire no detectors");
}

#[tokio::test]
async fn rerun_same_spec_yields_identical_trade_count_and_pnl() {
    // Determinism contract: same spec, same bars ⇒ same headline.
    let (_dir, db) = open_temp_db();
    let reader: Arc<dyn BarsReader> = Arc::new(DbBarsReader::new(Arc::clone(&db)));
    let date = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
    let bars = breakout_bars(date);
    for b in &bars {
        insert_bar(&db, "AAPL", BarSize::Day1, b).await.unwrap();
    }
    let detectors_cfg = DetectorsConfig::default();
    let registry = Arc::new(registry_from_config(&detectors_cfg));
    let bt = Backtester::new(
        Arc::clone(&db),
        reader,
        Arc::clone(&registry),
        Arc::new(detectors_cfg),
    );
    let spec = BacktestSpec {
        date_from: date,
        date_to_inclusive: date + chrono::Duration::days(60),
        symbols: vec!["AAPL".to_string()],
        detector_tags: Vec::new(),
        fill_model: FillModelKind::NaiveNextOpen { slippage_bps: 0 },
        position_sizing: PositionSizingMode::NoSizing,
        splits: WalkForwardSplits::default(),
        commission_usd: 0.0,
        starting_equity_usd: 100_000.0,
        event_blackouts_enabled: false,
        max_hold_bars: 5,
        rng_seed: 0,
        label: None,
    };
    let r1 = bt.run(spec.clone()).await.expect("r1");
    let r2 = bt.run(spec.clone()).await.expect("r2");
    assert_eq!(r1.trades.len(), r2.trades.len());
    assert_eq!(r1.headline.n_trades, r2.headline.n_trades);
    assert!((r1.headline.expectancy_r - r2.headline.expectancy_r).abs() < 1e-9);
}

#[tokio::test]
async fn list_runs_returns_most_recent_first() {
    let (_dir, db) = open_temp_db();
    let reader: Arc<dyn BarsReader> = Arc::new(DbBarsReader::new(Arc::clone(&db)));
    let detectors_cfg = DetectorsConfig::default();
    let registry = Arc::new(registry_from_config(&detectors_cfg));
    let bt = Backtester::new(Arc::clone(&db), reader, registry, Arc::new(detectors_cfg));
    let date = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
    let spec = BacktestSpec {
        date_from: date,
        date_to_inclusive: date + chrono::Duration::days(5),
        symbols: vec!["FAKE".to_string()],
        detector_tags: Vec::new(),
        fill_model: FillModelKind::default(),
        position_sizing: PositionSizingMode::default(),
        splits: WalkForwardSplits::default(),
        commission_usd: 1.0,
        starting_equity_usd: 100_000.0,
        event_blackouts_enabled: false,
        max_hold_bars: 5,
        rng_seed: 0,
        label: Some("first".to_string()),
    };
    bt.run(spec.clone()).await.expect("first");
    // Sleep 2ms so the run-id timestamp differs.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let mut spec2 = spec.clone();
    spec2.label = Some("second".to_string());
    bt.run(spec2).await.expect("second");
    let runs = bt.list_runs(10).await.expect("list");
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].label.as_deref(), Some("second"));
}

#[tokio::test]
async fn get_run_hydrates_trades() {
    let (_dir, db) = open_temp_db();
    let reader: Arc<dyn BarsReader> = Arc::new(DbBarsReader::new(Arc::clone(&db)));
    let detectors_cfg = DetectorsConfig::default();
    let registry = Arc::new(registry_from_config(&detectors_cfg));
    let bt = Backtester::new(Arc::clone(&db), reader, registry, Arc::new(detectors_cfg));
    let date = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
    let bars = breakout_bars(date);
    for b in &bars {
        insert_bar(&db, "AAPL", BarSize::Day1, b).await.unwrap();
    }
    let spec = BacktestSpec {
        date_from: date,
        date_to_inclusive: date + chrono::Duration::days(60),
        symbols: vec!["AAPL".to_string()],
        detector_tags: Vec::new(),
        fill_model: FillModelKind::NaiveNextOpen { slippage_bps: 0 },
        position_sizing: PositionSizingMode::NoSizing,
        splits: WalkForwardSplits::default(),
        commission_usd: 0.0,
        starting_equity_usd: 100_000.0,
        event_blackouts_enabled: false,
        max_hold_bars: 5,
        rng_seed: 0,
        label: None,
    };
    let live = bt.run(spec).await.expect("run");
    let reloaded = bt.get_run(&live.run_id).await.expect("get").expect("some");
    assert_eq!(reloaded.trades.len(), live.trades.len());
    if !live.trades.is_empty() {
        assert_eq!(reloaded.trades[0].seq, live.trades[0].seq);
        assert!((reloaded.trades[0].realized_pnl - live.trades[0].realized_pnl).abs() < 1e-6);
    }
}

#[tokio::test]
async fn missing_bars_does_not_panic() {
    // Symbol with zero cached bars should be tolerated — emits 0 trades
    // for that symbol but the run completes.
    let (_dir, db) = open_temp_db();
    let reader: Arc<dyn BarsReader> = Arc::new(DbBarsReader::new(Arc::clone(&db)));
    let detectors_cfg = DetectorsConfig::default();
    let registry = Arc::new(registry_from_config(&detectors_cfg));
    let bt = Backtester::new(Arc::clone(&db), reader, registry, Arc::new(detectors_cfg));
    let date = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
    let spec = BacktestSpec {
        date_from: date,
        date_to_inclusive: date + chrono::Duration::days(5),
        symbols: vec!["MISSING".to_string()],
        detector_tags: Vec::new(),
        fill_model: FillModelKind::default(),
        position_sizing: PositionSizingMode::default(),
        splits: WalkForwardSplits::default(),
        commission_usd: 1.0,
        starting_equity_usd: 100_000.0,
        event_blackouts_enabled: false,
        max_hold_bars: 5,
        rng_seed: 0,
        label: None,
    };
    let result = bt.run(spec).await.expect("run despite missing bars");
    assert_eq!(result.trades.len(), 0);
}

/// Look-ahead sanity check. Build a stub detector that asserts the
/// context only carries bars `<= now`. If the harness ever sliced
/// ahead, this would panic.
#[tokio::test]
async fn replay_never_passes_future_bars_to_detector() {
    use crate::ibkr::types::{BarSize, StrategyTag};
    use crate::strategies::{
        DetectorError, DetectorRegistry, MarketContext, SetupCandidate, StrategyDetector,
    };
    use async_trait::async_trait;

    struct PointInTimeAuditor;

    #[async_trait]
    impl StrategyDetector for PointInTimeAuditor {
        fn name(&self) -> &'static str {
            "pit_audit"
        }
        fn tag(&self) -> StrategyTag {
            StrategyTag::Custom("pit_audit".to_string())
        }
        fn timeframe(&self) -> BarSize {
            BarSize::Day1
        }
        fn min_lookback_days(&self) -> u32 {
            0
        }
        async fn evaluate(
            &self,
            ctx: &MarketContext<'_>,
        ) -> Result<Option<SetupCandidate>, DetectorError> {
            // Last bar's time must be <= ctx.now.
            if let Some(last) = ctx.daily_bars.last() {
                let t = crate::services::backtester::bars_reader::bar_time_utc(last)
                    .expect("bar time parses");
                assert!(
                    t <= ctx.now,
                    "look-ahead: last_bar_time={t} > now={}",
                    ctx.now
                );
            }
            Ok(None)
        }
    }

    let (_dir, db) = open_temp_db();
    let reader: Arc<dyn BarsReader> = Arc::new(DbBarsReader::new(Arc::clone(&db)));
    let date = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
    let bars = synthetic_bars(date, 40, 100.0, 0.5);
    for b in &bars {
        insert_bar(&db, "AAPL", BarSize::Day1, b).await.unwrap();
    }
    let mut registry = DetectorRegistry::new();
    registry.register(Arc::new(PointInTimeAuditor));
    let registry = Arc::new(registry);
    let detectors_cfg = Arc::new(DetectorsConfig::default());
    let bt = Backtester::new(Arc::clone(&db), reader, registry, detectors_cfg);
    let spec = BacktestSpec {
        date_from: date,
        date_to_inclusive: date + chrono::Duration::days(39),
        symbols: vec!["AAPL".to_string()],
        detector_tags: Vec::new(),
        fill_model: FillModelKind::NaiveNextOpen { slippage_bps: 0 },
        position_sizing: PositionSizingMode::NoSizing,
        splits: WalkForwardSplits::default(),
        commission_usd: 0.0,
        starting_equity_usd: 100_000.0,
        event_blackouts_enabled: false,
        max_hold_bars: 5,
        rng_seed: 0,
        label: None,
    };
    bt.run(spec).await.expect("run");
}

/// Stub `BarsReader` that lies — returns bars in non-monotone time
/// order. Lets us pin the replay-loop's time-monotone assumption.
struct ShuffledBarsReader {
    inner: Vec<HistoricalBar>,
}

#[async_trait]
impl BarsReader for ShuffledBarsReader {
    async fn read_window(
        &self,
        _symbol: &str,
        _bar_size: BarSize,
        _start_unix: i64,
        _end_unix_inclusive: i64,
    ) -> Result<Vec<HistoricalBar>, crate::storage::StorageError> {
        Ok(self.inner.clone())
    }
}

#[tokio::test]
async fn replay_handles_single_symbol_multi_strategy_no_overlap_violation() {
    // Verify the `one open trade per symbol-strategy pair` invariant:
    // two consecutive breakout-firing bars on the same symbol shouldn't
    // produce two simultaneously-open trades for the same strategy.
    let (_dir, db) = open_temp_db();
    let reader: Arc<dyn BarsReader> = Arc::new(DbBarsReader::new(Arc::clone(&db)));
    let detectors_cfg = DetectorsConfig::default();
    let registry = Arc::new(registry_from_config(&detectors_cfg));
    let bt = Backtester::new(Arc::clone(&db), reader, registry, Arc::new(detectors_cfg));
    let date = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
    let bars = breakout_bars(date);
    for b in &bars {
        insert_bar(&db, "AAPL", BarSize::Day1, b).await.unwrap();
    }
    let spec = BacktestSpec {
        date_from: date,
        date_to_inclusive: date + chrono::Duration::days(60),
        symbols: vec!["AAPL".to_string()],
        detector_tags: Vec::new(),
        fill_model: FillModelKind::NaiveNextOpen { slippage_bps: 0 },
        position_sizing: PositionSizingMode::NoSizing,
        splits: WalkForwardSplits::default(),
        commission_usd: 0.0,
        starting_equity_usd: 100_000.0,
        event_blackouts_enabled: false,
        max_hold_bars: 5,
        rng_seed: 0,
        label: None,
    };
    let result = bt.run(spec).await.expect("run");
    // We don't pin a specific count — that depends on the breakout
    // detector's internals. We DO pin: trades have non-overlapping
    // (entry_time, exit_time) ranges per (symbol, strategy).
    use std::collections::BTreeMap;
    let mut by_strat: BTreeMap<String, Vec<&_>> = BTreeMap::new();
    for t in &result.trades {
        by_strat.entry(t.strategy.clone()).or_default().push(t);
    }
    for (_, trades) in by_strat {
        for w in trades.windows(2) {
            assert!(
                w[0].exit_time <= w[1].entry_time,
                "overlap detected: prev exit {} > next entry {}",
                w[0].exit_time,
                w[1].entry_time,
            );
        }
    }
}

#[tokio::test]
async fn read_daily_bars_returns_empty_for_unknown_symbol() {
    let (_dir, db) = open_temp_db();
    let reader = DbBarsReader::new(Arc::clone(&db));
    let from = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
    let to = NaiveDate::from_ymd_opt(2025, 6, 30).unwrap();
    let bars = read_daily_bars(&reader, "UNKNOWN", from, to).await;
    assert!(bars.is_empty());
}

#[tokio::test]
async fn fill_model_box_dyn_works_through_replay() {
    // Spot-check that boxed FillModel survives the trait-object call
    // path inside replay_symbol. (Compile-time check disguised as a
    // runtime test.)
    use crate::services::backtester::replay::ReplayDiagnostics;
    let (_dir, db) = open_temp_db();
    let _reader = DbBarsReader::new(Arc::clone(&db));
    let date = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
    let bars = synthetic_bars(date, 5, 100.0, 0.0);
    let spec = BacktestSpec {
        date_from: date,
        date_to_inclusive: date + chrono::Duration::days(29),
        symbols: vec!["AAPL".to_string()],
        detector_tags: Vec::new(),
        fill_model: FillModelKind::NaiveNextOpen { slippage_bps: 0 },
        position_sizing: PositionSizingMode::NoSizing,
        splits: WalkForwardSplits::default(),
        commission_usd: 0.0,
        starting_equity_usd: 100_000.0,
        event_blackouts_enabled: false,
        max_hold_bars: 5,
        rng_seed: 0,
        label: None,
    };
    let detectors_cfg = DetectorsConfig::default();
    let registry = registry_from_config(&detectors_cfg);
    let mut model: Box<dyn FillModel> = Box::new(NaiveNextOpenFill { bps: 0 });
    let mut trades = Vec::new();
    let _diag: ReplayDiagnostics = replay_symbol(
        "AAPL",
        &bars,
        &spec,
        &detectors_cfg,
        &registry,
        model.as_mut(),
        None,
        0,
        &mut trades,
    )
    .await;
    // Smoke: should not panic, may emit zero or more trades.
    assert!(trades.is_empty() || trades.iter().all(|t| t.qty >= 1));
    let _ = Direction::Long;
    let _ = ExitReason::Stop;
    let _ = FillSide::Entry;
}
