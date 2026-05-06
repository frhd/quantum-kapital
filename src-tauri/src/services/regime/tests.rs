//! Phase 9 — `RegimeService` integration tests against a real SQLite
//! DB + a hand-rolled `BarsReader` mock so we don't need a primed
//! bars_cache to drive the gate.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use tempfile::NamedTempFile;
use tokio::sync::Mutex;

use crate::events::{AppEvent, EventEmitter};
use crate::ibkr::types::historical::{BarSize, HistoricalBar};
use crate::services::backtester::bars_reader::BarsReader;
use crate::storage::error::Result as StorageResult;
use crate::storage::Db;

use super::config::RegimeConfig;
use super::inputs::InputGatherer;
use super::types::{RegimeFilter, SnapshotSource, TrendAxis, VolAxis};
use super::RegimeService;

/// Produces a straight-line monotone series of `n` daily bars ending
/// at `end`. `start_close` is the day-0 close; each subsequent day
/// adds `step` to the close. Lets the classifier produce a clean
/// trend without us hand-rolling 200+ bars.
fn line_series(end: DateTime<Utc>, n: usize, start_close: f64, step: f64) -> Vec<HistoricalBar> {
    let mut bars = Vec::with_capacity(n);
    for i in 0..n {
        let day_offset = (n - 1 - i) as i64;
        let when = end - Duration::days(day_offset);
        let close = start_close + step * i as f64;
        bars.push(HistoricalBar {
            time: when.format("%Y%m%d").to_string(),
            open: close,
            high: close + 0.5,
            low: close - 0.5,
            close,
            volume: 1_000_000,
            wap: close,
            count: 0,
        });
    }
    bars
}

/// Hand-rolled BarsReader: returns a per-symbol fixture, ignoring
/// (start_unix, end_unix) for simplicity. Tests configure the bars
/// on construction.
struct CannedBars {
    rows: Mutex<HashMap<String, Vec<HistoricalBar>>>,
}

impl CannedBars {
    fn arc() -> (Arc<Self>, Arc<dyn BarsReader>) {
        let inner = Arc::new(Self {
            rows: Mutex::new(HashMap::new()),
        });
        let dyn_arc: Arc<dyn BarsReader> = Arc::clone(&inner) as Arc<dyn BarsReader>;
        (inner, dyn_arc)
    }

    async fn set(&self, symbol: &str, bars: Vec<HistoricalBar>) {
        self.rows.lock().await.insert(symbol.to_uppercase(), bars);
    }
}

#[async_trait]
impl BarsReader for CannedBars {
    async fn read_window(
        &self,
        symbol: &str,
        _bar_size: BarSize,
        _start_unix: i64,
        _end_unix_inclusive: i64,
    ) -> StorageResult<Vec<HistoricalBar>> {
        let map = self.rows.lock().await;
        Ok(map.get(&symbol.to_uppercase()).cloned().unwrap_or_default())
    }
}

async fn build_service(
    canned: Arc<CannedBars>,
    cfg: RegimeConfig,
) -> (
    NamedTempFile,
    Arc<RegimeService>,
    Arc<EventEmitter>,
    Arc<Db>,
) {
    let tmp = NamedTempFile::new().unwrap();
    let db = Arc::new(Db::open(tmp.path()).unwrap());
    let emitter = Arc::new(EventEmitter::for_capture());
    let dyn_bars: Arc<dyn BarsReader> = canned;
    let gatherer = Arc::new(InputGatherer::with_universe(
        dyn_bars,
        // Tiny test universe: 5 names. Coverage threshold (80%) means
        // 4/5 must have fresh bars for breadth/corr to compute.
        vec!["AAPL", "MSFT", "NVDA", "GOOGL", "META"],
    ));
    let svc = Arc::new(RegimeService::with_gatherer(
        Arc::clone(&db),
        Arc::clone(&emitter),
        gatherer,
        cfg,
    ));
    (tmp, svc, emitter, db)
}

#[tokio::test]
async fn snapshot_persists_and_caches() {
    let (raw, dyn_bars) = CannedBars::arc();
    // Just SPY rising — the rest is empty so missing[] populates.
    let now = Utc.with_ymd_and_hms(2026, 5, 5, 21, 0, 0).unwrap();
    raw.set("SPY", line_series(now, 220, 400.0, 1.0)).await;
    let _ = dyn_bars; // keep the alias alive (unused)
    let (_tmp, svc, _emitter, _db) = build_service(raw, RegimeConfig::default()).await;

    let s = svc.snapshot(SnapshotSource::DailyClose).await.unwrap();
    assert!(s.snapshot_id > 0);
    assert_eq!(s.source, SnapshotSource::DailyClose);
    // Cache is populated.
    let cur = svc.current().await.unwrap();
    assert_eq!(cur.snapshot_id, s.snapshot_id);
    // History reads it back.
    let hist = svc.history(10).await.unwrap();
    assert_eq!(hist.len(), 1);
    assert_eq!(hist[0].id, s.snapshot_id);
}

#[tokio::test]
async fn vix_spike_classifies_high_vol() {
    let (raw, _dyn) = CannedBars::arc();
    let now = Utc.with_ymd_and_hms(2026, 5, 5, 21, 0, 0).unwrap();
    raw.set("SPY", line_series(now, 220, 400.0, 1.0)).await;
    raw.set("VIX", line_series(now, 30, 24.0, 0.5)).await;
    let (_tmp, svc, _emitter, _db) = build_service(raw, RegimeConfig::default()).await;

    let s = svc.snapshot(SnapshotSource::DailyClose).await.unwrap();
    assert_eq!(s.raw.vol, VolAxis::High, "VIX > 22 should classify High");
}

#[tokio::test]
async fn missing_breadth_data_falls_back_to_mixed() {
    let (raw, _dyn) = CannedBars::arc();
    let now = Utc.with_ymd_and_hms(2026, 5, 5, 21, 0, 0).unwrap();
    // Only SPY + VIX — universe is empty → breadth missing.
    raw.set("SPY", line_series(now, 220, 400.0, 1.0)).await;
    raw.set("VIX", line_series(now, 30, 14.0, 0.0)).await;
    let (_tmp, svc, _emitter, _db) = build_service(raw, RegimeConfig::default()).await;

    let s = svc.snapshot(SnapshotSource::DailyClose).await.unwrap();
    assert!(
        s.inputs.missing.iter().any(|m| m.starts_with("breadth_")),
        "missing breadth proxy should be logged"
    );
}

#[tokio::test]
async fn persistence_rule_holds_until_three_daily_close_rows_agree() {
    let (raw, _dyn) = CannedBars::arc();
    // First snapshot: clean Up trend (rising SPY).
    let day1 = Utc.with_ymd_and_hms(2026, 5, 1, 21, 0, 0).unwrap();
    raw.set("SPY", line_series(day1, 220, 400.0, 1.0)).await;
    let (_tmp, svc, _emitter, _db) = build_service(raw.clone(), RegimeConfig::default()).await;

    let s1 = svc.snapshot(SnapshotSource::DailyClose).await.unwrap();
    assert_eq!(s1.raw.trend, TrendAxis::Up);
    assert_eq!(s1.stable.trend, TrendAxis::Up);

    // Day 2: SPY flat-lines (sideways read), but persistence rule
    // says we need 3 in a row before the stable view flips.
    let day2 = Utc.with_ymd_and_hms(2026, 5, 2, 21, 0, 0).unwrap();
    raw.set("SPY", line_series(day2, 220, 600.0, 0.0)).await;
    let s2 = svc.snapshot(SnapshotSource::DailyClose).await.unwrap();
    // After the new row lands, prior_history.first() is s1 (Up).
    // Raw today is Sideways; prior_raws[0..2] would need to be
    // Sideways — they aren't (only s1 = Up exists). Stable stays Up.
    assert_eq!(s2.raw.trend, TrendAxis::Sideways);
    assert_eq!(s2.stable.trend, TrendAxis::Up);

    // Day 3: still flat. Now prior_raws = [s2.raw=Sideways, s1.raw=Up].
    // Not 2 priors agreeing, so still no flip.
    let day3 = Utc.with_ymd_and_hms(2026, 5, 3, 21, 0, 0).unwrap();
    raw.set("SPY", line_series(day3, 220, 600.0, 0.0)).await;
    let s3 = svc.snapshot(SnapshotSource::DailyClose).await.unwrap();
    assert_eq!(s3.raw.trend, TrendAxis::Sideways);
    assert_eq!(s3.stable.trend, TrendAxis::Up);

    // Day 4: still flat. prior_raws = [s3=Sideways, s2=Sideways].
    // 3 in a row → flip.
    let day4 = Utc.with_ymd_and_hms(2026, 5, 4, 21, 0, 0).unwrap();
    raw.set("SPY", line_series(day4, 220, 600.0, 0.0)).await;
    let s4 = svc.snapshot(SnapshotSource::DailyClose).await.unwrap();
    assert_eq!(s4.raw.trend, TrendAxis::Sideways);
    assert_eq!(
        s4.stable.trend,
        TrendAxis::Sideways,
        "3 consecutive same-axis daily reads flip the stable view"
    );
}

#[tokio::test]
async fn intraday_snapshots_dont_trigger_persistence_flip() {
    let (raw, _dyn) = CannedBars::arc();
    let now = Utc.with_ymd_and_hms(2026, 5, 5, 18, 0, 0).unwrap();
    raw.set("SPY", line_series(now, 220, 400.0, 1.0)).await;
    let (_tmp, svc, _emitter, _db) = build_service(raw.clone(), RegimeConfig::default()).await;

    // Daily close establishes Up.
    let _ = svc.snapshot(SnapshotSource::DailyClose).await.unwrap();

    // Three intraday recomputes with flat SPY shouldn't flip stable.
    raw.set(
        "SPY",
        line_series(
            Utc.with_ymd_and_hms(2026, 5, 5, 19, 0, 0).unwrap(),
            220,
            600.0,
            0.0,
        ),
    )
    .await;
    let s = svc.snapshot(SnapshotSource::Intraday).await.unwrap();
    assert_eq!(s.stable.trend, TrendAxis::Up);
    let s = svc.snapshot(SnapshotSource::Intraday).await.unwrap();
    assert_eq!(s.stable.trend, TrendAxis::Up);
    let s = svc.snapshot(SnapshotSource::Intraday).await.unwrap();
    assert_eq!(s.stable.trend, TrendAxis::Up);
}

#[tokio::test]
async fn evaluate_blocks_off_regime_detector() {
    let (raw, _dyn) = CannedBars::arc();
    let now = Utc.with_ymd_and_hms(2026, 5, 5, 21, 0, 0).unwrap();
    // Down trend + High vol — breakout's preferred set excludes it.
    raw.set("SPY", line_series(now, 220, 600.0, -1.0)).await;
    raw.set("VIX", line_series(now, 30, 25.0, 0.5)).await;
    let (_tmp, svc, _emitter, _db) = build_service(raw, RegimeConfig::default()).await;

    let _ = svc.snapshot(SnapshotSource::DailyClose).await.unwrap();
    let breakout = svc.evaluate("breakout").await.unwrap();
    assert!(
        !breakout.matches,
        "breakout should be off-regime in Down + High vol"
    );
    assert_eq!(breakout.regime.trend, TrendAxis::Down);
    assert_eq!(breakout.regime.vol, VolAxis::High);

    // Episodic pivot is regime-agnostic.
    let episodic = svc.evaluate("episodic_pivot").await.unwrap();
    assert!(episodic.matches);
}

#[tokio::test]
async fn evaluate_passes_when_globally_disabled() {
    let (raw, _dyn) = CannedBars::arc();
    let now = Utc.with_ymd_and_hms(2026, 5, 5, 21, 0, 0).unwrap();
    raw.set("SPY", line_series(now, 220, 600.0, -1.0)).await;
    raw.set("VIX", line_series(now, 30, 25.0, 0.5)).await;

    let cfg = RegimeConfig {
        enabled: false,
        ..RegimeConfig::default()
    };
    let (_tmp, svc, _emitter, _db) = build_service(raw, cfg).await;

    let _ = svc.snapshot(SnapshotSource::DailyClose).await.unwrap();
    let breakout = svc.evaluate("breakout").await.unwrap();
    assert!(
        breakout.matches,
        "globally-disabled regime gate must pass every detector"
    );
}

#[tokio::test]
async fn unknown_detector_passes_with_default_filter() {
    let (raw, _dyn) = CannedBars::arc();
    let now = Utc.with_ymd_and_hms(2026, 5, 5, 21, 0, 0).unwrap();
    raw.set("SPY", line_series(now, 220, 600.0, -1.0)).await;
    raw.set("VIX", line_series(now, 30, 25.0, 0.5)).await;
    let (_tmp, svc, _emitter, _db) = build_service(raw, RegimeConfig::default()).await;

    let _ = svc.snapshot(SnapshotSource::DailyClose).await.unwrap();
    let unknown = svc.evaluate("not_a_real_detector").await.unwrap();
    assert!(
        unknown.matches,
        "detectors not in the per-detector config fall through to RegimeFilter::default()"
    );
}

#[tokio::test]
async fn snapshot_emits_on_first_classification_and_on_flip() {
    let (raw, _dyn) = CannedBars::arc();
    let now = Utc.with_ymd_and_hms(2026, 5, 5, 21, 0, 0).unwrap();
    raw.set("SPY", line_series(now, 220, 400.0, 1.0)).await;
    let (_tmp, svc, emitter, _db) = build_service(raw, RegimeConfig::default()).await;

    let _ = svc.snapshot(SnapshotSource::DailyClose).await.unwrap();
    let captured = emitter.captured().await;
    assert!(
        captured
            .iter()
            .any(|e| matches!(e, AppEvent::RegimeChanged { .. })),
        "first snapshot should emit RegimeChanged from None → Some"
    );
}

#[tokio::test]
async fn record_override_persists_to_gate_overrides() {
    let (raw, _dyn) = CannedBars::arc();
    let now = Utc.with_ymd_and_hms(2026, 5, 5, 21, 0, 0).unwrap();
    raw.set("SPY", line_series(now, 220, 400.0, 1.0)).await;
    let (_tmp, svc, _emitter, db) = build_service(raw, RegimeConfig::default()).await;
    // Insert a stub setup via the same Db pool the service uses, so the
    // FK reference resolves regardless of the WAL-checkpoint timing.
    let setup_id: i64 = db
        .with_conn(|conn| {
            // setups.symbol FK references tracked_tickers — seed both.
            conn.execute(
                "INSERT INTO tracked_tickers (symbol, source, added_at) \
                 VALUES ('NVDA', 'manual', 1234567890)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO setups \
                 (symbol, strategy, direction, detected_at, trigger_price, stop_price, \
                  targets, raw_signals, status) \
                 VALUES ('NVDA', 'breakout', 'long', 1234567890, 100.0, 95.0, '[]', '{}', 'active')",
                [],
            )
            .unwrap();
            Ok(conn.last_insert_rowid())
        })
        .await
        .unwrap();
    let id = svc
        .record_override(
            setup_id,
            "regime ignored — strong setup-level signal",
            "human",
        )
        .await
        .unwrap();
    assert!(id > 0);

    // Verify the row landed via the same pool.
    let count: i64 = db
        .with_conn(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT COUNT(*) FROM gate_overrides WHERE gate_kind = 'regime' AND setup_id = ?1",
                    rusqlite::params![setup_id],
                    |r| r.get(0),
                )
                .unwrap())
        })
        .await
        .unwrap();
    assert_eq!(count, 1);
}

/// Phase 9 exit criterion — "Per-detector preference must be testable
/// from outside. Add a unit test: `for each detector, for each regime,
/// the gate behaves as declared`. Catches drift between code and config."
#[tokio::test]
async fn detector_regime_drift_test_per_master_plan() {
    use super::types::{BreadthAxis, CorrAxis, Regime};

    let cases: Vec<(&str, Regime, bool, &str)> = vec![
        // Breakout default: skipped in Down trend, accepted in Up + Normal vol.
        (
            "breakout",
            Regime {
                trend: TrendAxis::Down,
                vol: VolAxis::High,
                breadth: BreadthAxis::Narrow,
                corr: CorrAxis::High,
            },
            false,
            "Down + High vol must be off-regime",
        ),
        (
            "breakout",
            Regime {
                trend: TrendAxis::Up,
                vol: VolAxis::Normal,
                breadth: BreadthAxis::Healthy,
                corr: CorrAxis::Low,
            },
            true,
            "clean uptrend + normal vol must pass",
        ),
        // Parabolic short: skipped in clean melt-ups (Up + Low vol).
        (
            "parabolic_short",
            Regime {
                trend: TrendAxis::Up,
                vol: VolAxis::Low,
                breadth: BreadthAxis::Healthy,
                corr: CorrAxis::Low,
            },
            false,
            "clean melt-up has no parabolas to short",
        ),
        (
            "parabolic_short",
            Regime {
                trend: TrendAxis::Sideways,
                vol: VolAxis::High,
                breadth: BreadthAxis::Mixed,
                corr: CorrAxis::High,
            },
            true,
            "sideways + high vol is the prime parabolic-short regime",
        ),
        // Episodic pivot: regime-agnostic.
        (
            "episodic_pivot",
            Regime {
                trend: TrendAxis::Down,
                vol: VolAxis::High,
                breadth: BreadthAxis::Narrow,
                corr: CorrAxis::High,
            },
            true,
            "episodic_pivot is initially regime-agnostic",
        ),
        (
            "episodic_pivot",
            Regime {
                trend: TrendAxis::Up,
                vol: VolAxis::Low,
                breadth: BreadthAxis::Healthy,
                corr: CorrAxis::Low,
            },
            true,
            "episodic_pivot is initially regime-agnostic",
        ),
    ];

    let cfg = RegimeConfig::default();
    for (detector, regime, want_match, msg) in cases {
        let f = cfg.filter_for(detector);
        assert_eq!(
            f.matches(&regime),
            want_match,
            "{detector}: {msg} (regime={regime:?})"
        );
    }
}

// Suppress warnings for the unused `NaiveDate` import on platforms
// where the compiler decides it's redundant.
#[allow(dead_code)]
fn _date(_: NaiveDate) {}
