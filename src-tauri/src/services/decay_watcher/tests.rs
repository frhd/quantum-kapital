use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use serde_json::{json, Value};
use tempfile::NamedTempFile;
use tokio::sync::Mutex as AsyncMutex;

use crate::ibkr::error::Result as IbkrResult;
use crate::ibkr::types::historical::{BarSize, HistoricalBar};
use crate::ibkr::types::tracker::{Setup, SetupStatus};
use crate::services::historical_data_service::Lookback;
use crate::services::llm_service::{
    AnthropicHttp, AnthropicHttpError, LlmKind, LlmService, ToolChoice,
};
use crate::services::tracker_runner::BarsFetcher;
use crate::storage::Db;
use crate::strategies::{Direction, TargetLevel};

use super::{
    DecayClock, DecayContext, DecayOutcome, DecayWatcher, LlmDecayWatcher, FRESHNESS_GRACE, MODEL,
    TOOL_NAME,
};

// ---------------- helpers ----------------

#[derive(Default)]
struct MockHttp {
    canned: Mutex<VecDeque<Result<Value, AnthropicHttpError>>>,
    calls: Mutex<usize>,
}

impl MockHttp {
    fn new() -> Self {
        Self::default()
    }
    fn enqueue_ok(&self, value: Value) {
        self.canned.lock().unwrap().push_back(Ok(value));
    }
    fn call_count(&self) -> usize {
        *self.calls.lock().unwrap()
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
        *self.calls.lock().unwrap() += 1;
        self.canned
            .lock()
            .unwrap()
            .pop_front()
            .expect("MockHttp queue exhausted")
    }
}

#[derive(Default)]
struct StubBars {
    bars: AsyncMutex<Vec<HistoricalBar>>,
    calls: AsyncMutex<usize>,
}

impl StubBars {
    fn new(bars: Vec<HistoricalBar>) -> Self {
        Self {
            bars: AsyncMutex::new(bars),
            calls: AsyncMutex::new(0),
        }
    }
    async fn calls(&self) -> usize {
        *self.calls.lock().await
    }
}

#[async_trait]
impl BarsFetcher for StubBars {
    async fn fetch(
        &self,
        _symbol: &str,
        _bar_size: BarSize,
        _lookback: Lookback,
    ) -> IbkrResult<Vec<HistoricalBar>> {
        *self.calls.lock().await += 1;
        Ok(self.bars.lock().await.clone())
    }
}

#[derive(Clone)]
struct FixedClock(DateTime<Utc>);
impl DecayClock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }
}

fn make_db() -> (NamedTempFile, Arc<Db>) {
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Arc::new(Db::open(tmp.path()).expect("open db"));
    (tmp, db)
}

fn detected_at() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 4, 29, 13, 30, 0).unwrap()
}

fn now_after_grace() -> DateTime<Utc> {
    detected_at() + chrono::Duration::minutes(35)
}

fn bars(n: usize) -> Vec<HistoricalBar> {
    (0..n)
        .map(|i| HistoricalBar {
            time: format!("20260429 13:{:02}:00", 30 + i * 15),
            open: 100.0 + i as f64,
            high: 101.0 + i as f64,
            low: 99.0 + i as f64,
            close: 100.5 + i as f64,
            volume: 200_000 + i as i64 * 10_000,
            wap: 100.5 + i as f64,
            count: 0,
        })
        .collect()
}

fn sample_setup() -> Setup {
    Setup {
        id: 7,
        symbol: "AAPL".to_string(),
        strategy: "breakout".to_string(),
        direction: Direction::Long,
        detected_at: detected_at(),
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
        raw_signals: json!({"volume_multiple": 1.85}),
        thesis: Some("AAPL broke above 20d high on 1.85x volume.".to_string()),
        thesis_json: Some(json!({
            "thesis_md": "AAPL broke above 20d high on 1.85x volume.",
            "conviction": "B",
            "invalidation_levels": [
                { "label": "swing_low", "price": 100.0, "reason": "below 10d swing low" },
                { "label": "atr_stop", "price": 99.0, "reason": "1xATR(14) below trigger" }
            ],
            "risk_notes": "No earnings event scheduled."
        })),
        status: SetupStatus::Active,
        invalidated_at: None,
        invalidation_reason: None,
        archived_at: None,
        sizing: None,
        skipped_reason: None,
        skip_window_json: None,
        gate_warning: None,
        param_vintage_id: None,
    }
}

fn tool_use_response(input: Value) -> Value {
    json!({
        "content": [{
            "type": "tool_use",
            "id": "tu_1",
            "name": TOOL_NAME,
            "input": input,
        }],
        "usage": {
            "input_tokens": 80,
            "output_tokens": 20,
            "cache_read_input_tokens": 0,
            "cache_creation_input_tokens": 0
        }
    })
}

fn build_watcher(
    db: Arc<Db>,
    http: Arc<MockHttp>,
    bars_stub: Arc<StubBars>,
    now: DateTime<Utc>,
    budget_usd: f64,
) -> LlmDecayWatcher {
    let llm = Arc::new(
        LlmService::new("test-key".to_string(), Arc::clone(&db), budget_usd)
            .with_http(http as Arc<dyn AnthropicHttp>),
    );
    LlmDecayWatcher::new(llm, bars_stub as Arc<dyn BarsFetcher>)
        .with_clock(Arc::new(FixedClock(now)))
}

// ---------------- 1: builds_request_with_thesis_and_recent_bars ----------------

#[test]
fn builds_request_with_thesis_and_recent_bars() {
    let setup = sample_setup();
    let recent = bars(12);
    let ctx = DecayContext {
        recent_bars: &recent,
        current_quote: Some(recent.last().unwrap().close),
    };
    let req = LlmDecayWatcher::build_request(&setup, &ctx);

    assert!(matches!(req.kind, LlmKind::Decay));
    assert_eq!(req.model, MODEL);
    assert_eq!(req.setup_id, Some(setup.id));

    // Two system blocks: persona + per-setup thesis context.
    assert_eq!(req.system.len(), 2);
    let persona = &req.system[0].text;
    assert!(persona.contains("decide if it is still valid"));
    let thesis_block = &req.system[1].text;
    assert!(
        thesis_block.contains("AAPL broke above 20d high"),
        "thesis block must carry the thesis_md verbatim"
    );
    assert!(
        thesis_block.contains("invalidation_levels"),
        "thesis block must surface invalidation_levels"
    );
    assert!(
        thesis_block.contains("swing_low"),
        "thesis block must enumerate the named invalidation levels"
    );

    // User message embeds last 12 bars + current quote.
    assert_eq!(req.messages.len(), 1);
    let user_msg: Value =
        serde_json::from_str(&req.messages[0].content).expect("user message is JSON");
    let recent_bars = user_msg["recent_bars"]
        .as_array()
        .expect("recent_bars array");
    assert_eq!(recent_bars.len(), 12);
    assert_eq!(
        user_msg["current_quote"].as_f64().unwrap(),
        recent.last().unwrap().close
    );
}

// ---------------- 2: forces_emit_decay_tool_use ----------------

#[test]
fn forces_emit_decay_tool_use() {
    let setup = sample_setup();
    let recent = bars(3);
    let ctx = DecayContext {
        recent_bars: &recent,
        current_quote: Some(recent.last().unwrap().close),
    };
    let req = LlmDecayWatcher::build_request(&setup, &ctx);

    let tools = req.tools.as_ref().expect("tools");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, TOOL_NAME);
    let required = tools[0].input_schema["required"]
        .as_array()
        .expect("required");
    let names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
    for f in ["still_valid", "outcome", "reason"] {
        assert!(names.contains(&f), "tool schema must require `{f}`");
    }

    match req.tool_choice.as_ref().expect("tool_choice") {
        ToolChoice::ForceTool(name) => assert_eq!(name, TOOL_NAME),
        other => panic!("expected ForceTool, got {other:?}"),
    }
}

// ---------------- 3: parses_still_valid_true ----------------

#[tokio::test]
async fn parses_still_valid_true() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    http.enqueue_ok(tool_use_response(json!({
        "still_valid": true,
        "outcome": "still_valid",
        "reason": "structure intact above trigger",
    })));
    let stub_bars = Arc::new(StubBars::new(bars(5)));
    let watcher = build_watcher(
        Arc::clone(&db),
        Arc::clone(&http),
        Arc::clone(&stub_bars),
        now_after_grace(),
        100.0,
    );

    let decision = watcher.check(&sample_setup()).await;
    assert!(decision.still_valid);
    assert_eq!(decision.outcome, DecayOutcome::StillValid);
    assert_eq!(
        decision.reason.as_deref(),
        Some("structure intact above trigger")
    );
    assert_eq!(http.call_count(), 1);
    assert_eq!(stub_bars.calls().await, 1);
}

// ---------------- 4: parses_still_valid_false_triggers_invalidation ----------------

#[tokio::test]
async fn parses_still_valid_false_triggers_invalidation() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    http.enqueue_ok(tool_use_response(json!({
        "still_valid": false,
        "outcome": "invalidated",
        "reason": "broke below stop at 100",
    })));
    let stub_bars = Arc::new(StubBars::new(bars(5)));
    let watcher = build_watcher(
        Arc::clone(&db),
        Arc::clone(&http),
        Arc::clone(&stub_bars),
        now_after_grace(),
        100.0,
    );

    let decision = watcher.check(&sample_setup()).await;
    assert!(!decision.still_valid);
    assert_eq!(decision.outcome, DecayOutcome::Invalidated);
    assert_eq!(decision.reason.as_deref(), Some("broke below stop at 100"));
}

// ---------------- 5: parses_target_hit_completes_setup ----------------

#[tokio::test]
async fn parses_target_hit_completes_setup() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    http.enqueue_ok(tool_use_response(json!({
        "still_valid": false,
        "outcome": "target_hit",
        "reason": "2R target reached at 115",
        "suggested_action": "scale_out",
    })));
    let stub_bars = Arc::new(StubBars::new(bars(5)));
    let watcher = build_watcher(
        Arc::clone(&db),
        Arc::clone(&http),
        Arc::clone(&stub_bars),
        now_after_grace(),
        100.0,
    );

    let decision = watcher.check(&sample_setup()).await;
    assert!(!decision.still_valid);
    assert_eq!(decision.outcome, DecayOutcome::TargetHit);
    assert_eq!(decision.suggested_action.as_deref(), Some("scale_out"));
}

// ---------------- 6: respects_budget_kill_switch ----------------

#[tokio::test]
async fn respects_budget_kill_switch() {
    let (_tmp, db) = make_db();

    // Pre-populate llm_calls so the day's spend exceeds the budget.
    let now = now_after_grace();
    let day_start = (now.timestamp() / 86_400) * 86_400;
    db.with_conn(move |conn| {
        conn.execute(
            "INSERT INTO llm_calls (kind, model, input_tokens, output_tokens, \
             cache_read_tokens, cost_usd, called_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                "decay",
                "claude-haiku-4-5",
                0i64,
                0i64,
                0i64,
                10.0f64,
                day_start
            ],
        )?;
        Ok(())
    })
    .await
    .unwrap();

    let http = Arc::new(MockHttp::new()); // must not be called.
    let stub_bars = Arc::new(StubBars::new(bars(5)));
    let watcher = build_watcher(
        Arc::clone(&db),
        Arc::clone(&http),
        Arc::clone(&stub_bars),
        now,
        1.0, // tiny budget — already blown.
    );

    let decision = watcher.check(&sample_setup()).await;
    assert_eq!(
        decision.outcome,
        DecayOutcome::Skipped,
        "budget kill-switch must surface as Skipped"
    );
    assert!(decision.still_valid, "Skipped must not flip the setup");
    assert_eq!(http.call_count(), 0, "no HTTP call when budget exhausted");
}

// ---------------- 7: does_not_call_when_setup_too_fresh ----------------

#[tokio::test]
async fn does_not_call_when_setup_too_fresh() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new()); // must not be called.
    let stub_bars = Arc::new(StubBars::new(bars(5))); // must not be fetched.

    // Setup detected 10 minutes ago — well inside the 30-minute grace.
    let now = detected_at() + chrono::Duration::minutes(10);
    assert!(now.signed_duration_since(detected_at()) < FRESHNESS_GRACE);

    let watcher = build_watcher(
        Arc::clone(&db),
        Arc::clone(&http),
        Arc::clone(&stub_bars),
        now,
        100.0,
    );

    let decision = watcher.check(&sample_setup()).await;
    assert_eq!(decision.outcome, DecayOutcome::Skipped);
    assert!(decision.still_valid);
    assert_eq!(http.call_count(), 0);
    assert_eq!(stub_bars.calls().await, 0, "no bars fetch on fresh setup");
}

// ---------------- 8: caches_thesis_block_per_setup ----------------

#[test]
fn caches_thesis_block_per_setup() {
    let setup = sample_setup();
    let recent = bars(3);
    let ctx = DecayContext {
        recent_bars: &recent,
        current_quote: Some(recent.last().unwrap().close),
    };
    let req = LlmDecayWatcher::build_request(&setup, &ctx);
    let body = crate::services::llm_service::build_request_body(&req);

    // System block 0 = persona, system block 1 = per-setup thesis. Both
    // request ephemeral cache so successive calls within the cache TTL
    // amortize the prompt cost.
    assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
    assert_eq!(
        body["system"][1]["cache_control"]["type"], "ephemeral",
        "thesis system block must request cache_control"
    );
    let thesis_block_text = body["system"][1]["text"]
        .as_str()
        .expect("system block text");
    assert!(
        thesis_block_text.contains(&format!("\"setup_id\":{}", setup.id)),
        "thesis block must embed setup_id so the cache is keyed per-setup"
    );
}
