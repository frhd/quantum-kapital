// allow-large-file: integration tests for the 5-minute intraday scheduler covering
// calendar-aware ticking, in-play subscription churn, and concurrency. The mock
// scaffolding is shared across cases; splitting forks the harness.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveTime, TimeZone, Utc};
use tempfile::NamedTempFile;
use tokio::sync::Mutex;

use crate::ibkr::error::{IbkrError, Result as IbkrResult};
use crate::ibkr::types::historical::{BarSize, HistoricalBar};
use crate::ibkr::types::news::NewsItem;
use crate::ibkr::types::tracker::{Setup, StrategyTag, TrackerSource, TrackerStatus};
use crate::services::decay_watcher::{DecayDecision, DecayWatcher, DecayWatcherStub};
use crate::services::historical_data_service::Lookback;
use crate::services::tracker_runner::{BarsFetcher, NewsFetcher, TrackerRunner};
use crate::services::tracker_service::TrackerService;
use crate::services::tracker_state_machine::TrackerStateMachine;
use crate::storage::Db;
use crate::strategies::{
    DetectorRegistry, Direction, SetupCandidate, StrategyDetector, TargetLevel,
};

use super::{Clock, IntradayScheduler, DEFAULT_TICK_INTERVAL};

// ---------------- helpers ----------------

fn et_offset() -> FixedOffset {
    FixedOffset::west_opt(5 * 3600).unwrap()
}

fn et_dt(date: NaiveDate, h: u32, m: u32) -> DateTime<Utc> {
    let naive = date.and_time(NaiveTime::from_hms_opt(h, m, 0).unwrap());
    et_offset()
        .from_local_datetime(&naive)
        .unwrap()
        .with_timezone(&Utc)
}

/// Tuesday 2026-05-26 — first trading day after Memorial Day weekend.
fn tuesday() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 5, 26).unwrap()
}

fn saturday() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 5, 2).unwrap()
}

/// 2026-07-03 — Independence Day observed (NYSE full close).
fn holiday() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 7, 3).unwrap()
}

// ---------------- test doubles ----------------

#[derive(Default)]
struct RecordingBars {
    daily: HashMap<String, Vec<HistoricalBar>>,
    fail_daily: HashSet<String>,
    calls: Mutex<Vec<String>>,
}

impl RecordingBars {
    fn new() -> Self {
        Self::default()
    }
    fn with_daily(mut self, symbol: &str) -> Self {
        self.daily.insert(
            symbol.to_uppercase(),
            vec![HistoricalBar {
                time: "20260101".to_string(),
                open: 100.0,
                high: 101.0,
                low: 99.0,
                close: 100.5,
                volume: 1_000_000,
                wap: 100.5,
                count: 0,
            }],
        );
        self
    }
    fn fail_daily_for(mut self, symbol: &str) -> Self {
        self.fail_daily.insert(symbol.to_uppercase());
        self
    }
    async fn calls_snapshot(&self) -> Vec<String> {
        self.calls.lock().await.clone()
    }
}

#[async_trait]
impl BarsFetcher for RecordingBars {
    async fn fetch(
        &self,
        symbol: &str,
        bar_size: BarSize,
        _lookback: Lookback,
    ) -> IbkrResult<Vec<HistoricalBar>> {
        let upper = symbol.to_uppercase();
        if matches!(bar_size, BarSize::Day1) {
            self.calls.lock().await.push(upper.clone());
            if self.fail_daily.contains(&upper) {
                return Err(IbkrError::RequestFailed(format!(
                    "synthetic daily fetch failure for {symbol}"
                )));
            }
            return Ok(self.daily.get(&upper).cloned().unwrap_or_default());
        }
        Ok(Vec::new())
    }
}

struct EmptyNews;

#[async_trait]
impl NewsFetcher for EmptyNews {
    async fn fetch(&self, _symbol: &str, _lookback_hours: u32) -> Vec<NewsItem> {
        Vec::new()
    }
}

#[derive(Default)]
struct RecordingDecayWatcher {
    /// Per-setup-id decision. Setups without an entry get `still_valid = true`.
    decisions: HashMap<i64, DecayDecision>,
    calls: Mutex<Vec<i64>>,
}

impl RecordingDecayWatcher {
    fn new() -> Self {
        Self::default()
    }
    fn with_decision(mut self, setup_id: i64, decision: DecayDecision) -> Self {
        self.decisions.insert(setup_id, decision);
        self
    }
    async fn calls_snapshot(&self) -> Vec<i64> {
        self.calls.lock().await.clone()
    }
}

#[async_trait]
impl DecayWatcher for RecordingDecayWatcher {
    async fn check(&self, setup: &Setup) -> DecayDecision {
        self.calls.lock().await.push(setup.id);
        self.decisions
            .get(&setup.id)
            .cloned()
            .unwrap_or_else(DecayDecision::still_valid)
    }
}

/// Stub detector that always emits a single hit. Used to seed `setups`
/// rows without spinning up a real detector.
struct AlwaysHit {
    candidate: SetupCandidate,
}

#[async_trait]
impl StrategyDetector for AlwaysHit {
    fn name(&self) -> &'static str {
        "always_hit"
    }
    fn tag(&self) -> StrategyTag {
        StrategyTag::Breakout
    }
    fn timeframe(&self) -> BarSize {
        BarSize::Day1
    }
    fn min_lookback_days(&self) -> u32 {
        1
    }
    async fn evaluate(
        &self,
        _ctx: &crate::strategies::MarketContext<'_>,
    ) -> Result<Option<SetupCandidate>, crate::strategies::DetectorError> {
        Ok(Some(self.candidate.clone()))
    }
}

fn sample_candidate() -> SetupCandidate {
    SetupCandidate {
        strategy: "always_hit",
        tag: StrategyTag::Breakout,
        direction: Direction::Long,
        conviction_signal: 0.7,
        trigger_price: 105.0,
        stop_price: 100.0,
        targets: vec![
            TargetLevel {
                label: "2R".to_string(),
                price: 115.0,
            },
            TargetLevel {
                label: "3R".to_string(),
                price: 120.0,
            },
        ],
        raw_signals: serde_json::json!({}),
        timeframe: BarSize::Day1,
        detected_at: Utc::now(),
    }
}

// ---------------- factory ----------------

struct Harness {
    _tmp: NamedTempFile,
    db: Arc<Db>,
    tracker: Arc<TrackerService>,
    state_machine: Arc<TrackerStateMachine>,
    runner: Arc<TrackerRunner>,
    bars: Arc<RecordingBars>,
}

fn build_harness(
    bars: Arc<RecordingBars>,
    registry: DetectorRegistry,
    now: DateTime<Utc>,
) -> Harness {
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Arc::new(Db::open(tmp.path()).expect("open db"));
    let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));
    let emitter = Arc::new(crate::events::EventEmitter::for_capture());
    let state_machine = Arc::new(TrackerStateMachine::with_clock(
        Arc::clone(&db),
        Arc::clone(&tracker),
        Arc::clone(&emitter),
        crate::services::tracker_state_machine::Clock::Fixed(now),
    ));
    let news: Arc<dyn NewsFetcher> = Arc::new(EmptyNews);
    let bars_dyn: Arc<dyn BarsFetcher> = Arc::clone(&bars) as Arc<dyn BarsFetcher>;
    let runner = Arc::new(TrackerRunner::new(
        Arc::clone(&db),
        Arc::clone(&tracker),
        Arc::clone(&state_machine),
        Arc::clone(&emitter),
        bars_dyn,
        news,
        Arc::new(registry),
    ));
    Harness {
        _tmp: tmp,
        db,
        tracker,
        state_machine,
        runner,
        bars,
    }
}

async fn add_ticker(svc: &TrackerService, symbol: &str, status: TrackerStatus) {
    svc.add(symbol, TrackerSource::Manual, None, vec![], None)
        .await
        .expect("add");
    if !matches!(status, TrackerStatus::Watching) {
        svc.set_status(symbol, status, None, None)
            .await
            .expect("status");
    }
}

// ---------------- tests ----------------

#[tokio::test]
async fn does_not_run_outside_rth() {
    let now = et_dt(tuesday(), 18, 0);
    let bars = Arc::new(RecordingBars::new());
    let h = build_harness(bars, DetectorRegistry::new(), now);

    let scheduler = IntradayScheduler::with_clock(
        Arc::clone(&h.runner),
        Arc::clone(&h.state_machine),
        Arc::clone(&h.tracker),
        Arc::new(DecayWatcherStub),
        DEFAULT_TICK_INTERVAL,
        Clock::Fixed(now),
    );

    let outcome = scheduler.tick().await.expect("tick");
    assert!(outcome.is_none(), "tick at 18:00 ET should be a no-op");
    assert!(h.bars.calls_snapshot().await.is_empty());
    assert!(scheduler.last_tick_at().await.is_none());
}

#[tokio::test]
async fn does_not_run_on_holiday() {
    let now = et_dt(holiday(), 12, 0);
    let bars = Arc::new(RecordingBars::new());
    let h = build_harness(bars, DetectorRegistry::new(), now);

    let scheduler = IntradayScheduler::with_clock(
        Arc::clone(&h.runner),
        Arc::clone(&h.state_machine),
        Arc::clone(&h.tracker),
        Arc::new(DecayWatcherStub),
        DEFAULT_TICK_INTERVAL,
        Clock::Fixed(now),
    );
    add_ticker(&h.tracker, "AAPL", TrackerStatus::InPlay).await;

    let outcome = scheduler.tick().await.expect("tick");
    assert!(outcome.is_none(), "holiday should never run");
    assert!(h.bars.calls_snapshot().await.is_empty());
}

#[tokio::test]
async fn does_not_run_on_weekend() {
    let now = et_dt(saturday(), 12, 0);
    let bars = Arc::new(RecordingBars::new());
    let h = build_harness(bars, DetectorRegistry::new(), now);

    let scheduler = IntradayScheduler::with_clock(
        Arc::clone(&h.runner),
        Arc::clone(&h.state_machine),
        Arc::clone(&h.tracker),
        Arc::new(DecayWatcherStub),
        DEFAULT_TICK_INTERVAL,
        Clock::Fixed(now),
    );
    add_ticker(&h.tracker, "AAPL", TrackerStatus::InPlay).await;

    let outcome = scheduler.tick().await.expect("tick");
    assert!(outcome.is_none());
}

#[tokio::test]
async fn runs_only_for_in_play_symbols() {
    let now = et_dt(tuesday(), 12, 0);
    let bars = Arc::new(
        RecordingBars::new()
            .with_daily("AAPL")
            .with_daily("MSFT")
            .with_daily("NVDA")
            .with_daily("GOOG")
            .with_daily("AMZN")
            .with_daily("TSLA")
            .with_daily("META")
            .with_daily("PLTR"),
    );
    let h = build_harness(Arc::clone(&bars), DetectorRegistry::new(), now);

    // 5 watching, 2 in-play, 1 setup-active
    for sym in ["AAPL", "MSFT", "NVDA", "GOOG", "AMZN"] {
        add_ticker(&h.tracker, sym, TrackerStatus::Watching).await;
    }
    add_ticker(&h.tracker, "TSLA", TrackerStatus::InPlay).await;
    add_ticker(&h.tracker, "META", TrackerStatus::InPlay).await;
    add_ticker(&h.tracker, "PLTR", TrackerStatus::SetupActive).await;

    let scheduler = IntradayScheduler::with_clock(
        Arc::clone(&h.runner),
        Arc::clone(&h.state_machine),
        Arc::clone(&h.tracker),
        Arc::new(DecayWatcherStub),
        DEFAULT_TICK_INTERVAL,
        Clock::Fixed(now),
    );

    let outcome = scheduler.tick().await.expect("tick").expect("ran");
    let processed: HashSet<String> = outcome.processed_symbols.iter().cloned().collect();
    assert_eq!(
        processed,
        ["TSLA", "META", "PLTR"]
            .into_iter()
            .map(String::from)
            .collect::<HashSet<_>>()
    );

    let calls = bars.calls_snapshot().await;
    let calls_set: HashSet<String> = calls.into_iter().collect();
    assert!(calls_set.contains("TSLA"));
    assert!(calls_set.contains("META"));
    assert!(calls_set.contains("PLTR"));
    assert!(
        !calls_set.contains("AAPL"),
        "watching tickers must not get bars fetched"
    );
}

#[tokio::test]
async fn runs_decay_watcher_for_active_setups() {
    let now = et_dt(tuesday(), 12, 0);
    let bars = Arc::new(RecordingBars::new().with_daily("AAPL"));
    let mut registry = DetectorRegistry::new();
    registry.register(Arc::new(AlwaysHit {
        candidate: sample_candidate(),
    }));
    let h = build_harness(Arc::clone(&bars), registry, now);

    add_ticker(&h.tracker, "AAPL", TrackerStatus::Watching).await;
    // Seed an active setup row for AAPL by running detectors once
    // through the runner. This will also flip AAPL into SetupActive
    // via the state machine.
    let setups = h.runner.run_for("AAPL").await.expect("seed run");
    assert_eq!(setups.len(), 1);
    let setup_id = setups[0].id;
    let row_after_seed = h.tracker.get("AAPL").await.unwrap().unwrap();
    assert_eq!(row_after_seed.status, TrackerStatus::SetupActive);

    let watcher = Arc::new(RecordingDecayWatcher::new());
    let scheduler = IntradayScheduler::with_clock(
        Arc::clone(&h.runner),
        Arc::clone(&h.state_machine),
        Arc::clone(&h.tracker),
        Arc::clone(&watcher) as Arc<dyn DecayWatcher>,
        DEFAULT_TICK_INTERVAL,
        Clock::Fixed(now),
    );

    let outcome = scheduler.tick().await.expect("tick").expect("ran");
    assert_eq!(outcome.processed_symbols, vec!["AAPL".to_string()]);
    assert!(outcome.invalidated_setup_ids.is_empty());

    let calls = watcher.calls_snapshot().await;
    assert_eq!(
        calls,
        vec![setup_id],
        "decay watcher should be called once with the active setup"
    );
}

#[tokio::test]
async fn decay_watcher_invalidation_flips_state() {
    let now = et_dt(tuesday(), 12, 0);
    let bars = Arc::new(RecordingBars::new().with_daily("AAPL"));
    let mut registry = DetectorRegistry::new();
    registry.register(Arc::new(AlwaysHit {
        candidate: sample_candidate(),
    }));
    let h = build_harness(Arc::clone(&bars), registry, now);

    add_ticker(&h.tracker, "AAPL", TrackerStatus::Watching).await;
    let setups = h.runner.run_for("AAPL").await.expect("seed run");
    let setup_id = setups[0].id;

    let watcher = Arc::new(
        RecordingDecayWatcher::new().with_decision(setup_id, DecayDecision::invalidate("stop hit")),
    );
    let scheduler = IntradayScheduler::with_clock(
        Arc::clone(&h.runner),
        Arc::clone(&h.state_machine),
        Arc::clone(&h.tracker),
        Arc::clone(&watcher) as Arc<dyn DecayWatcher>,
        DEFAULT_TICK_INTERVAL,
        Clock::Fixed(now),
    );

    let outcome = scheduler.tick().await.expect("tick").expect("ran");
    assert_eq!(outcome.invalidated_setup_ids, vec![setup_id]);

    // The setup row should now be `Invalidated` with the reason set.
    let setup = h.tracker.get_setup(setup_id).await.unwrap().unwrap();
    assert_eq!(
        setup.status,
        crate::ibkr::types::tracker::SetupStatus::Invalidated
    );
    assert_eq!(setup.invalidation_reason.as_deref(), Some("stop hit"));

    // With no other active setups, the ticker flips to CoolDown.
    let row = h.tracker.get("AAPL").await.unwrap().unwrap();
    assert_eq!(row.status, TrackerStatus::CoolDown);
}

#[tokio::test]
async fn tick_interval_is_5_minutes() {
    let first = et_dt(tuesday(), 12, 0);
    let bars = Arc::new(RecordingBars::new().with_daily("AAPL"));
    let h = build_harness(Arc::clone(&bars), DetectorRegistry::new(), first);
    add_ticker(&h.tracker, "AAPL", TrackerStatus::InPlay).await;

    let scheduler = IntradayScheduler::with_clock(
        Arc::clone(&h.runner),
        Arc::clone(&h.state_machine),
        Arc::clone(&h.tracker),
        Arc::new(DecayWatcherStub),
        Duration::from_secs(300),
        Clock::Fixed(first),
    );

    let outcome = scheduler.tick().await.expect("tick");
    assert!(outcome.is_some(), "first tick should run");

    // Advance 4:30 — should be skipped.
    let mid = first + chrono::Duration::seconds(270);
    scheduler.set_clock(Clock::Fixed(mid)).await;
    let mid_outcome = scheduler.tick().await.expect("mid tick");
    assert!(
        mid_outcome.is_none(),
        "second tick within 5min should be a no-op"
    );

    // Advance to 5:00 — should run again.
    let later = first + chrono::Duration::seconds(300);
    scheduler.set_clock(Clock::Fixed(later)).await;
    let later_outcome = scheduler.tick().await.expect("later tick");
    assert!(
        later_outcome.is_some(),
        "tick at exactly 5min should fire again"
    );
}

#[tokio::test]
async fn errors_in_one_symbol_dont_block_others() {
    let now = et_dt(tuesday(), 12, 0);
    let bars = Arc::new(
        RecordingBars::new()
            .with_daily("AAPL")
            .fail_daily_for("MSFT")
            .with_daily("NVDA"),
    );
    let h = build_harness(Arc::clone(&bars), DetectorRegistry::new(), now);
    for sym in ["AAPL", "MSFT", "NVDA"] {
        add_ticker(&h.tracker, sym, TrackerStatus::InPlay).await;
    }

    let scheduler = IntradayScheduler::with_clock(
        Arc::clone(&h.runner),
        Arc::clone(&h.state_machine),
        Arc::clone(&h.tracker),
        Arc::new(DecayWatcherStub),
        DEFAULT_TICK_INTERVAL,
        Clock::Fixed(now),
    );

    let outcome = scheduler.tick().await.expect("tick").expect("ran");
    assert_eq!(outcome.run_results.len(), 3);

    let by_symbol: HashMap<String, &super::RunResult> = outcome
        .run_results
        .iter()
        .map(|r| (r.symbol.clone(), r))
        .collect();
    assert!(by_symbol["AAPL"].error.is_none());
    assert!(
        by_symbol["MSFT"].error.is_some(),
        "MSFT should have surfaced its bars-fetch failure"
    );
    assert!(by_symbol["NVDA"].error.is_none());

    // The cadence cursor should still advance even when individual
    // symbols error so we don't churn on the next tick.
    assert!(scheduler.last_tick_at().await.is_some());
}

// ---- IbkrState integration: handle replacement / drop ----

#[tokio::test]
async fn start_replaces_existing_handle() {
    use crate::config::AppConfig;
    use crate::ibkr::IbkrState;

    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Arc::new(Db::open(tmp.path()).expect("open db"));
    let cfg = AppConfig::default().ibkr.into();
    let llm = Arc::new(crate::services::llm_service::LlmService::new(
        String::new(),
        Arc::clone(&db),
        0.0,
    ));
    let state = IbkrState::new(cfg, Arc::clone(&db), llm);

    let bars: Arc<dyn BarsFetcher> = Arc::new(RecordingBars::new());
    let news: Arc<dyn NewsFetcher> = Arc::new(EmptyNews);
    let runner = Arc::new(TrackerRunner::new(
        Arc::clone(&db),
        Arc::clone(&state.tracker),
        Arc::clone(&state.state_machine),
        Arc::clone(&state.event_emitter),
        bars,
        news,
        Arc::new(DetectorRegistry::new()),
    ));

    // Pin the clock outside RTH so the spawned loop never actually
    // fires within the test's lifetime.
    let scheduler = Arc::new(IntradayScheduler::with_clock(
        runner,
        Arc::clone(&state.state_machine),
        Arc::clone(&state.tracker),
        Arc::new(DecayWatcherStub),
        DEFAULT_TICK_INTERVAL,
        Clock::Fixed(et_dt(tuesday(), 18, 0)),
    ));

    state
        .start_intraday_scheduler(Arc::clone(&scheduler))
        .await
        .expect("first start");
    assert!(state.intraday_handle.read().await.is_some());

    state
        .start_intraday_scheduler(Arc::clone(&scheduler))
        .await
        .expect("second start");
    assert!(state.intraday_handle.read().await.is_some());

    state.stop_intraday_scheduler().await;
}

#[tokio::test]
async fn stop_drops_handle() {
    use crate::config::AppConfig;
    use crate::ibkr::IbkrState;

    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Arc::new(Db::open(tmp.path()).expect("open db"));
    let cfg = AppConfig::default().ibkr.into();
    let llm = Arc::new(crate::services::llm_service::LlmService::new(
        String::new(),
        Arc::clone(&db),
        0.0,
    ));
    let state = IbkrState::new(cfg, Arc::clone(&db), llm);

    let bars: Arc<dyn BarsFetcher> = Arc::new(RecordingBars::new());
    let news: Arc<dyn NewsFetcher> = Arc::new(EmptyNews);
    let runner = Arc::new(TrackerRunner::new(
        Arc::clone(&db),
        Arc::clone(&state.tracker),
        Arc::clone(&state.state_machine),
        Arc::clone(&state.event_emitter),
        bars,
        news,
        Arc::new(DetectorRegistry::new()),
    ));

    let scheduler = Arc::new(IntradayScheduler::with_clock(
        runner,
        Arc::clone(&state.state_machine),
        Arc::clone(&state.tracker),
        Arc::new(DecayWatcherStub),
        DEFAULT_TICK_INTERVAL,
        Clock::Fixed(et_dt(tuesday(), 18, 0)),
    ));

    state
        .start_intraday_scheduler(Arc::clone(&scheduler))
        .await
        .expect("start");
    assert!(state.intraday_handle.read().await.is_some());

    state.stop_intraday_scheduler().await;
    assert!(state.intraday_handle.read().await.is_none());
}
