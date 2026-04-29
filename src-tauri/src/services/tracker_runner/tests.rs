use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::json;
use tempfile::NamedTempFile;
use tokio::sync::Mutex;

use crate::events::{AppEvent, EventEmitter};
use crate::ibkr::error::{IbkrError, Result as IbkrResult};
use crate::ibkr::types::historical::{BarSize, HistoricalBar};
use crate::ibkr::types::news::NewsItem;
use crate::ibkr::types::tracker::{StrategyTag, TrackerSource, TrackerStatus};
use crate::services::historical_data_service::Lookback;
use crate::services::tracker_service::TrackerService;
use crate::services::tracker_state_machine::TrackerStateMachine;
use crate::storage::Db;
use crate::strategies::{
    DetectorError, DetectorRegistry, Direction, MarketContext, SetupCandidate, StrategyDetector,
    TargetLevel,
};

use super::{BarsFetcher, NewsFetcher, RunResult, TrackerRunner};

// ---------------- mocks ----------------

struct MockBars {
    /// Keyed by `(symbol, bar_size)`. Missing keys → empty.
    bars: HashMap<(String, BarSize), Vec<HistoricalBar>>,
    /// Symbols whose Day1 fetch should error.
    fail_daily: std::collections::HashSet<String>,
}

impl MockBars {
    fn new() -> Self {
        Self {
            bars: HashMap::new(),
            fail_daily: Default::default(),
        }
    }

    fn with_daily(mut self, symbol: &str, bars: Vec<HistoricalBar>) -> Self {
        self.bars
            .insert((symbol.to_uppercase(), BarSize::Day1), bars);
        self
    }

    fn with_intraday(mut self, symbol: &str, bar_size: BarSize, bars: Vec<HistoricalBar>) -> Self {
        self.bars.insert((symbol.to_uppercase(), bar_size), bars);
        self
    }

    fn fail_daily_for(mut self, symbol: &str) -> Self {
        self.fail_daily.insert(symbol.to_uppercase());
        self
    }
}

#[async_trait]
impl BarsFetcher for MockBars {
    async fn fetch(
        &self,
        symbol: &str,
        bar_size: BarSize,
        _lookback: Lookback,
    ) -> IbkrResult<Vec<HistoricalBar>> {
        let key = (symbol.to_uppercase(), bar_size);
        if matches!(bar_size, BarSize::Day1) && self.fail_daily.contains(&key.0) {
            return Err(IbkrError::RequestFailed(format!(
                "synthetic daily fetch failure for {symbol}"
            )));
        }
        Ok(self.bars.get(&key).cloned().unwrap_or_default())
    }
}

struct MockNews {
    items: HashMap<String, Vec<NewsItem>>,
}

impl MockNews {
    fn new() -> Self {
        Self {
            items: HashMap::new(),
        }
    }
}

#[async_trait]
impl NewsFetcher for MockNews {
    async fn fetch(&self, symbol: &str, _lookback_hours: u32) -> Vec<NewsItem> {
        self.items
            .get(&symbol.to_uppercase())
            .cloned()
            .unwrap_or_default()
    }
}

/// Detector that records every invocation and returns a configurable
/// fixed result. Lets tests verify (a) the registry actually called
/// it, and (b) which candidate was forwarded for persistence.
struct StubDetector {
    name: &'static str,
    tag: StrategyTag,
    result: Result<Option<SetupCandidate>, DetectorError>,
    calls: Arc<Mutex<usize>>,
}

impl StubDetector {
    fn new_hit(name: &'static str, tag: StrategyTag, candidate: SetupCandidate) -> Self {
        Self {
            name,
            tag,
            result: Ok(Some(candidate)),
            calls: Arc::new(Mutex::new(0)),
        }
    }

    fn new_miss(name: &'static str, tag: StrategyTag) -> Self {
        Self {
            name,
            tag,
            result: Ok(None),
            calls: Arc::new(Mutex::new(0)),
        }
    }

    fn calls(&self) -> Arc<Mutex<usize>> {
        Arc::clone(&self.calls)
    }
}

#[async_trait]
impl StrategyDetector for StubDetector {
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
        1
    }
    async fn evaluate(
        &self,
        _ctx: &MarketContext<'_>,
    ) -> Result<Option<SetupCandidate>, DetectorError> {
        let mut n = self.calls.lock().await;
        *n += 1;
        // Clone the result manually since DetectorError doesn't impl Clone.
        match &self.result {
            Ok(opt) => Ok(opt.clone()),
            Err(_) => unreachable!("StubDetector configured for hits/misses only"),
        }
    }
}

// ---------------- helpers ----------------

fn sample_candidate(strategy: &'static str, direction: Direction) -> SetupCandidate {
    SetupCandidate {
        strategy,
        tag: StrategyTag::Breakout,
        direction,
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
        raw_signals: json!({"volume_multiple": 1.8}),
        timeframe: BarSize::Day1,
        detected_at: Utc::now(),
    }
}

fn make_db() -> (NamedTempFile, Arc<Db>) {
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Arc::new(Db::open(tmp.path()).expect("open db"));
    (tmp, db)
}

fn fixture_daily_bars() -> Vec<HistoricalBar> {
    (0..5)
        .map(|i| HistoricalBar {
            time: format!("2026010{}", i + 1),
            open: 100.0 + i as f64,
            high: 101.0 + i as f64,
            low: 99.0 + i as f64,
            close: 100.5 + i as f64,
            volume: 1_000_000,
            wap: 100.5 + i as f64,
            count: 0,
        })
        .collect()
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

fn build_runner(
    db: Arc<Db>,
    bars: Arc<dyn BarsFetcher>,
    news: Arc<dyn NewsFetcher>,
    registry: DetectorRegistry,
) -> (Arc<TrackerService>, Arc<EventEmitter>, TrackerRunner) {
    let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));
    let emitter = Arc::new(EventEmitter::for_capture());
    let state_machine = Arc::new(TrackerStateMachine::new(
        Arc::clone(&db),
        Arc::clone(&tracker),
        Arc::clone(&emitter),
    ));
    let runner = TrackerRunner::new(
        Arc::clone(&db),
        Arc::clone(&tracker),
        state_machine,
        Arc::clone(&emitter),
        bars,
        news,
        Arc::new(registry),
    );
    (tracker, emitter, runner)
}

// ---------------- tests ----------------

#[tokio::test]
async fn gathers_context_for_symbol_with_daily_bars_only() {
    let (_tmp, db) = make_db();
    let bars = Arc::new(MockBars::new().with_daily("AAPL", fixture_daily_bars()));
    let news = Arc::new(MockNews::new());
    let (_tracker, _emitter, runner) = build_runner(db, bars, news, DetectorRegistry::new());

    let ctx = runner.context_for("aapl").await.expect("context");
    assert_eq!(ctx.symbol, "AAPL");
    assert_eq!(ctx.daily_bars.len(), 5);
    assert!(ctx.intraday_bars.is_none());
    assert!(ctx.recent_news.is_empty());
}

#[tokio::test]
async fn context_includes_intraday_when_provided() {
    let (_tmp, db) = make_db();
    let bars = Arc::new(
        MockBars::new()
            .with_daily("AAPL", fixture_daily_bars())
            .with_intraday(
                "AAPL",
                BarSize::Min15,
                vec![HistoricalBar {
                    time: "20260101 09:30:00".to_string(),
                    open: 100.0,
                    high: 101.0,
                    low: 99.5,
                    close: 100.5,
                    volume: 500_000,
                    wap: 100.5,
                    count: 0,
                }],
            ),
    );
    let news = Arc::new(MockNews::new());
    let (_tracker, _emitter, runner) = build_runner(db, bars, news, DetectorRegistry::new());

    let ctx = runner.context_for("AAPL").await.expect("context");
    let intraday = ctx.intraday_bars.expect("intraday present");
    assert_eq!(intraday.len(), 1);
}

#[tokio::test]
async fn runs_all_detectors_and_aggregates_outcomes() {
    let (_tmp, db) = make_db();
    let bars = Arc::new(MockBars::new().with_daily("AAPL", fixture_daily_bars()));
    let news = Arc::new(MockNews::new());

    let hit = StubDetector::new_hit(
        "stub_hit",
        StrategyTag::Breakout,
        sample_candidate("stub_hit", Direction::Long),
    );
    let miss = StubDetector::new_miss("stub_miss", StrategyTag::EpisodicPivot);
    let hit_calls = hit.calls();
    let miss_calls = miss.calls();

    let mut registry = DetectorRegistry::new();
    registry.register(Arc::new(hit));
    registry.register(Arc::new(miss));

    let (tracker, _emitter, runner) = build_runner(db, bars, news, registry);
    add_ticker(&tracker, "AAPL", TrackerStatus::Watching).await;

    let setups = runner.run_for("AAPL").await.expect("run_for");
    assert_eq!(setups.len(), 1, "only the hit detector should persist");
    assert_eq!(setups[0].strategy, "stub_hit");
    assert_eq!(*hit_calls.lock().await, 1);
    assert_eq!(*miss_calls.lock().await, 1);
}

#[tokio::test]
async fn persists_hit_to_setups_table() {
    let (_tmp, db) = make_db();
    let bars = Arc::new(MockBars::new().with_daily("AAPL", fixture_daily_bars()));
    let news = Arc::new(MockNews::new());

    let candidate = sample_candidate("breakout", Direction::Long);
    let mut registry = DetectorRegistry::new();
    registry.register(Arc::new(StubDetector::new_hit(
        "breakout",
        StrategyTag::Breakout,
        candidate.clone(),
    )));

    let (tracker, _emitter, runner) = build_runner(db, bars, news, registry);
    add_ticker(&tracker, "AAPL", TrackerStatus::Watching).await;

    let setups = runner.run_for("AAPL").await.expect("run_for");
    assert_eq!(setups.len(), 1);
    let row = &setups[0];
    assert_eq!(row.symbol, "AAPL");
    assert_eq!(row.strategy, "breakout");
    assert_eq!(row.direction, Direction::Long);
    assert_eq!(row.trigger_price, candidate.trigger_price);
    assert_eq!(row.stop_price, candidate.stop_price);
    assert_eq!(row.targets, candidate.targets);
    assert_eq!(row.raw_signals, candidate.raw_signals);

    // Round-trip via TrackerService::list_setups.
    let listed = tracker
        .list_setups(Some("AAPL"), None)
        .await
        .expect("list_setups");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, row.id);
}

#[tokio::test]
async fn does_not_persist_misses() {
    let (_tmp, db) = make_db();
    let bars = Arc::new(MockBars::new().with_daily("AAPL", fixture_daily_bars()));
    let news = Arc::new(MockNews::new());

    let mut registry = DetectorRegistry::new();
    registry.register(Arc::new(StubDetector::new_miss(
        "miss",
        StrategyTag::Breakout,
    )));

    let (tracker, _emitter, runner) = build_runner(db, bars, news, registry);
    add_ticker(&tracker, "AAPL", TrackerStatus::Watching).await;

    let setups = runner.run_for("AAPL").await.expect("run_for");
    assert!(setups.is_empty());
    assert!(tracker
        .list_setups(None, None)
        .await
        .expect("list")
        .is_empty());
}

#[tokio::test]
async fn dedups_recent_duplicates_within_window() {
    let (_tmp, db) = make_db();
    let bars = Arc::new(MockBars::new().with_daily("AAPL", fixture_daily_bars()));
    let news = Arc::new(MockNews::new());

    let candidate = sample_candidate("breakout", Direction::Long);
    let mut registry = DetectorRegistry::new();
    registry.register(Arc::new(StubDetector::new_hit(
        "breakout",
        StrategyTag::Breakout,
        candidate.clone(),
    )));

    let (tracker, _emitter, runner) = build_runner(db, bars, news, registry);
    add_ticker(&tracker, "AAPL", TrackerStatus::Watching).await;

    let first = runner.run_for("AAPL").await.expect("first run");
    assert_eq!(first.len(), 1);

    let second = runner.run_for("AAPL").await.expect("second run");
    assert!(
        second.is_empty(),
        "duplicate within window should not re-insert"
    );
    let stored = tracker.list_setups(None, None).await.unwrap();
    assert_eq!(stored.len(), 1, "still exactly one row in the table");
}

#[tokio::test]
async fn dedup_is_keyed_on_strategy_and_direction() {
    let (_tmp, db) = make_db();
    let bars = Arc::new(MockBars::new().with_daily("AAPL", fixture_daily_bars()));
    let news = Arc::new(MockNews::new());

    let mut registry = DetectorRegistry::new();
    registry.register(Arc::new(StubDetector::new_hit(
        "breakout",
        StrategyTag::Breakout,
        sample_candidate("breakout", Direction::Long),
    )));
    registry.register(Arc::new(StubDetector::new_hit(
        "parabolic_short",
        StrategyTag::ParabolicShort,
        sample_candidate("parabolic_short", Direction::Short),
    )));

    let (tracker, _emitter, runner) = build_runner(db, bars, news, registry);
    add_ticker(&tracker, "AAPL", TrackerStatus::Watching).await;

    // Two distinct (strategy, direction) pairs both persist on first pass.
    let first = runner.run_for("AAPL").await.expect("first run");
    assert_eq!(first.len(), 2);
    let stored = tracker.list_setups(None, None).await.unwrap();
    assert_eq!(stored.len(), 2);
}

#[tokio::test]
async fn run_all_iterates_watchlist_excluding_cool_down() {
    let (_tmp, db) = make_db();
    let bars = Arc::new(
        MockBars::new()
            .with_daily("AAPL", fixture_daily_bars())
            .with_daily("MSFT", fixture_daily_bars())
            .with_daily("NVDA", fixture_daily_bars()),
    );
    let news = Arc::new(MockNews::new());

    let mut registry = DetectorRegistry::new();
    registry.register(Arc::new(StubDetector::new_hit(
        "breakout",
        StrategyTag::Breakout,
        sample_candidate("breakout", Direction::Long),
    )));

    let (tracker, _emitter, runner) = build_runner(db, bars, news, registry);
    add_ticker(&tracker, "AAPL", TrackerStatus::Watching).await;
    add_ticker(&tracker, "MSFT", TrackerStatus::InPlay).await;
    add_ticker(&tracker, "NVDA", TrackerStatus::CoolDown).await;

    let results: Vec<RunResult> = runner.run_all().await.expect("run_all");
    let by_symbol: HashMap<String, &RunResult> =
        results.iter().map(|r| (r.symbol.clone(), r)).collect();

    assert_eq!(by_symbol.len(), 2, "cool_down rows skipped");
    assert!(by_symbol.contains_key("AAPL"));
    assert!(by_symbol.contains_key("MSFT"));
    assert!(!by_symbol.contains_key("NVDA"));
    assert_eq!(by_symbol["AAPL"].setups.len(), 1);
    assert_eq!(by_symbol["MSFT"].setups.len(), 1);
}

#[tokio::test]
async fn errors_in_one_symbol_dont_block_others() {
    let (_tmp, db) = make_db();
    let bars = Arc::new(
        MockBars::new()
            .with_daily("AAPL", fixture_daily_bars())
            .fail_daily_for("MSFT")
            .with_daily("NVDA", fixture_daily_bars()),
    );
    let news = Arc::new(MockNews::new());

    let mut registry = DetectorRegistry::new();
    registry.register(Arc::new(StubDetector::new_hit(
        "breakout",
        StrategyTag::Breakout,
        sample_candidate("breakout", Direction::Long),
    )));

    let (tracker, _emitter, runner) = build_runner(db, bars, news, registry);
    add_ticker(&tracker, "AAPL", TrackerStatus::Watching).await;
    add_ticker(&tracker, "MSFT", TrackerStatus::Watching).await;
    add_ticker(&tracker, "NVDA", TrackerStatus::Watching).await;

    let results = runner.run_all().await.expect("run_all");
    assert_eq!(results.len(), 3);
    let by_symbol: HashMap<String, &RunResult> =
        results.iter().map(|r| (r.symbol.clone(), r)).collect();

    assert!(by_symbol["AAPL"].error.is_none());
    assert_eq!(by_symbol["AAPL"].setups.len(), 1);
    assert!(
        by_symbol["MSFT"].error.is_some(),
        "MSFT should have surfaced its bars-fetch failure"
    );
    assert!(by_symbol["MSFT"].setups.is_empty());
    assert!(by_symbol["NVDA"].error.is_none());
    assert_eq!(by_symbol["NVDA"].setups.len(), 1);
}

#[tokio::test]
async fn run_for_touches_last_checked_on_success() {
    let (_tmp, db) = make_db();
    let bars = Arc::new(MockBars::new().with_daily("AAPL", fixture_daily_bars()));
    let news = Arc::new(MockNews::new());
    let registry = DetectorRegistry::new();

    let (tracker, _emitter, runner) = build_runner(db, bars, news, registry);
    add_ticker(&tracker, "AAPL", TrackerStatus::Watching).await;
    assert!(tracker
        .get("AAPL")
        .await
        .unwrap()
        .unwrap()
        .last_checked_at
        .is_none());

    let _ = runner.run_for("AAPL").await.expect("run");
    let after = tracker.get("AAPL").await.unwrap().unwrap();
    assert!(after.last_checked_at.is_some());
}

// ---------------- Phase 15: event emission ----------------

#[tokio::test]
async fn setup_detected_event_emitted_on_runner_persist() {
    let (_tmp, db) = make_db();
    let bars = Arc::new(MockBars::new().with_daily("AAPL", fixture_daily_bars()));
    let news = Arc::new(MockNews::new());

    let candidate = sample_candidate("breakout", Direction::Long);
    let mut registry = DetectorRegistry::new();
    registry.register(Arc::new(StubDetector::new_hit(
        "breakout",
        StrategyTag::Breakout,
        candidate.clone(),
    )));

    let (tracker, emitter, runner) = build_runner(db, bars, news, registry);
    add_ticker(&tracker, "AAPL", TrackerStatus::Watching).await;

    let setups = runner.run_for("AAPL").await.expect("run_for");
    assert_eq!(setups.len(), 1);

    let events = emitter.captured().await;
    let detected: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AppEvent::SetupDetected { setup, thesis } => Some((setup.clone(), thesis.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(detected.len(), 1, "exactly one SetupDetected emitted");
    let (setup, thesis) = &detected[0];
    assert_eq!(setup.symbol, "AAPL");
    assert_eq!(setup.strategy, "breakout");
    assert_eq!(setup.direction, Direction::Long);
    assert_eq!(setup.id, setups[0].id);
    assert!(thesis.is_none(), "thesis is None until Phase 17");
}

#[tokio::test]
async fn setup_detected_not_emitted_on_dedup_skip() {
    let (_tmp, db) = make_db();
    let bars = Arc::new(MockBars::new().with_daily("AAPL", fixture_daily_bars()));
    let news = Arc::new(MockNews::new());

    let candidate = sample_candidate("breakout", Direction::Long);
    let mut registry = DetectorRegistry::new();
    registry.register(Arc::new(StubDetector::new_hit(
        "breakout",
        StrategyTag::Breakout,
        candidate,
    )));

    let (tracker, emitter, runner) = build_runner(db, bars, news, registry);
    add_ticker(&tracker, "AAPL", TrackerStatus::Watching).await;

    runner.run_for("AAPL").await.expect("first run");
    runner.run_for("AAPL").await.expect("second run");

    let detected_count = emitter
        .captured()
        .await
        .iter()
        .filter(|e| matches!(e, AppEvent::SetupDetected { .. }))
        .count();
    assert_eq!(
        detected_count, 1,
        "duplicate within window must not re-emit"
    );
}

#[tokio::test]
async fn setup_detected_event_serializes_with_expected_fields() {
    // The frontend listener relies on the `setup` payload carrying
    // every field of the `Setup` row (snake_case to match other event
    // payloads on the wire).
    let (_tmp, db) = make_db();
    let bars = Arc::new(MockBars::new().with_daily("AAPL", fixture_daily_bars()));
    let news = Arc::new(MockNews::new());

    let candidate = sample_candidate("breakout", Direction::Long);
    let mut registry = DetectorRegistry::new();
    registry.register(Arc::new(StubDetector::new_hit(
        "breakout",
        StrategyTag::Breakout,
        candidate,
    )));

    let (tracker, emitter, runner) = build_runner(db, bars, news, registry);
    add_ticker(&tracker, "AAPL", TrackerStatus::Watching).await;
    runner.run_for("AAPL").await.expect("run_for");

    let detected = emitter
        .captured()
        .await
        .into_iter()
        .find(|e| matches!(e, AppEvent::SetupDetected { .. }))
        .expect("at least one SetupDetected");

    let json = serde_json::to_value(&detected).unwrap();
    assert_eq!(json["type"], "SetupDetected");
    let data = &json["data"];
    let setup = &data["setup"];
    for field in [
        "id",
        "symbol",
        "strategy",
        "direction",
        "trigger_price",
        "stop_price",
        "targets",
        "detected_at",
    ] {
        assert!(
            setup.get(field).is_some(),
            "expected setup.{field} in event payload, got: {setup}"
        );
    }
    assert_eq!(setup["symbol"], "AAPL");
    assert_eq!(setup["direction"], "long");
    // `thesis` is present (Some(_)) once Phase 17 lands; for now it's
    // serialized as `null`, which the frontend treats as absent.
    assert!(data.get("thesis").is_some());
}

// ---------------- Phase 17: thesis generator integration ----------------

#[tokio::test]
async fn run_for_with_thesis_generator_emits_thesis_populated_event_once() {
    use std::collections::VecDeque;
    use std::sync::Mutex as StdMutex;

    use serde_json::Value;

    use crate::services::llm_service::{AnthropicHttp, AnthropicHttpError, LlmService};
    use crate::services::thesis_generator::ThesisGenerator;

    // Lightweight HTTP mock — single canned tool_use response.
    struct MockHttp {
        canned: StdMutex<VecDeque<Result<Value, AnthropicHttpError>>>,
    }
    impl MockHttp {
        fn new() -> Self {
            Self {
                canned: StdMutex::new(VecDeque::new()),
            }
        }
        fn enqueue_ok(&self, value: Value) {
            self.canned.lock().unwrap().push_back(Ok(value));
        }
    }
    #[async_trait]
    impl AnthropicHttp for MockHttp {
        async fn send_messages(
            &self,
            _api_key: &str,
            _anthropic_version: &str,
            _body: &Value,
        ) -> Result<Value, AnthropicHttpError> {
            self.canned.lock().unwrap().pop_front().expect("queue")
        }
    }

    let (_tmp, db) = make_db();
    let bars = Arc::new(MockBars::new().with_daily("AAPL", fixture_daily_bars()));
    let news = Arc::new(MockNews::new());

    let candidate = sample_candidate("breakout", Direction::Long);
    let mut registry = DetectorRegistry::new();
    registry.register(Arc::new(StubDetector::new_hit(
        "breakout",
        StrategyTag::Breakout,
        candidate.clone(),
    )));

    let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));
    let emitter = Arc::new(EventEmitter::for_capture());
    let state_machine = Arc::new(TrackerStateMachine::new(
        Arc::clone(&db),
        Arc::clone(&tracker),
        Arc::clone(&emitter),
    ));

    let http = Arc::new(MockHttp::new());
    http.enqueue_ok(json!({
        "content": [{
            "type": "tool_use",
            "id": "tu_1",
            "name": "emit_thesis",
            "input": {
                "thesis_md": "AAPL breakout looks clean: 1.85× volume confirms.",
                "conviction": "B",
                "invalidation_levels": [
                    { "label": "stop", "price": 100.0, "reason": "below 10d swing low" }
                ],
                "risk_notes": "Earnings clear next 7 days."
            }
        }],
        "usage": {
            "input_tokens": 100, "output_tokens": 60,
            "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0
        }
    }));
    let llm = Arc::new(
        LlmService::new("test-key".to_string(), Arc::clone(&db), 100.0)
            .with_http(http as Arc<dyn AnthropicHttp>),
    );
    let thesis_generator = Arc::new(ThesisGenerator::new(
        Arc::clone(&llm),
        Arc::clone(&tracker),
        Arc::clone(&emitter),
    ));

    let runner = TrackerRunner::new(
        Arc::clone(&db),
        Arc::clone(&tracker),
        Arc::clone(&state_machine),
        Arc::clone(&emitter),
        bars,
        news,
        Arc::new(registry),
    )
    .with_thesis_generator(Arc::clone(&thesis_generator));

    add_ticker(&tracker, "AAPL", TrackerStatus::Watching).await;
    let setups = runner.run_for("AAPL").await.expect("run_for");
    assert_eq!(setups.len(), 1);

    let detected: Vec<_> = emitter
        .captured()
        .await
        .into_iter()
        .filter_map(|e| match e {
            AppEvent::SetupDetected { setup, thesis } => Some((setup, thesis)),
            _ => None,
        })
        .collect();
    assert_eq!(
        detected.len(),
        1,
        "exactly one SetupDetected — generator owns the emit"
    );
    let (emitted, thesis) = &detected[0];
    assert!(thesis.is_some(), "thesis populated by generator");
    assert!(thesis.as_deref().unwrap().contains("breakout"));
    assert!(
        emitted.thesis.is_some(),
        "row carries persisted thesis markdown"
    );

    // Round-trip check — DB row is updated.
    let stored = tracker
        .get_setup(setups[0].id)
        .await
        .unwrap()
        .expect("stored");
    assert!(stored.thesis.is_some());
    assert!(stored.thesis_json.is_some());
}

// Bind unused imports for compile cleanliness; helpers live in scope
// only when tests reference them.
#[allow(dead_code)]
fn _bind_unused(_: DateTime<Utc>, _: ChronoDuration) {}
