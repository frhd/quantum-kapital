use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, NaiveDate, Utc};
use serde_json::{json, Value};
use tempfile::NamedTempFile;

use crate::events::{AppEvent, EventEmitter};
use crate::ibkr::types::historical::BarSize;
use crate::ibkr::types::tracker::{Setup, StrategyTag, TrackerSource};
use crate::services::llm_service::{
    AnthropicHttp, AnthropicHttpError, LlmKind, LlmService, ToolChoice,
};
use crate::services::tracker_service::TrackerService;
use crate::storage::Db;
use crate::strategies::{Direction, SetupCandidate, TargetLevel};

use super::{DailyRanker, MODEL, TOOL_NAME};

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

fn make_db() -> (NamedTempFile, Arc<Db>) {
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Arc::new(Db::open(tmp.path()).expect("open db"));
    (tmp, db)
}

fn build_ranker(
    db: Arc<Db>,
    http: Arc<MockHttp>,
    budget_usd: f64,
) -> (Arc<TrackerService>, Arc<EventEmitter>, DailyRanker) {
    let llm = Arc::new(
        LlmService::new("test-key".to_string(), Arc::clone(&db), budget_usd)
            .with_http(http as Arc<dyn AnthropicHttp>),
    );
    let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));
    let emitter = Arc::new(EventEmitter::for_capture());
    let ranker = DailyRanker::new(
        Arc::clone(&llm),
        Arc::clone(&tracker),
        Arc::clone(&db),
        Arc::clone(&emitter),
    );
    (tracker, emitter, ranker)
}

async fn add_ticker(tracker: &TrackerService, symbol: &str) {
    tracker
        .add(symbol, TrackerSource::Manual, None, vec![], None)
        .await
        .expect("add ticker");
}

fn candidate(conviction_signal: f64) -> SetupCandidate {
    SetupCandidate {
        strategy: "breakout",
        tag: StrategyTag::Breakout,
        direction: Direction::Long,
        conviction_signal,
        trigger_price: 105.0,
        stop_price: 100.0,
        targets: vec![TargetLevel {
            label: "2R".to_string(),
            price: 115.0,
        }],
        raw_signals: json!({"volume_multiple": 1.85, "conviction_signal": conviction_signal}),
        timeframe: BarSize::Day1,
        detected_at: Utc::now(),
    }
}

async fn insert_setup(tracker: &TrackerService, symbol: &str, conviction_signal: f64) -> Setup {
    add_ticker(tracker, symbol).await;
    tracker
        .insert_setup(symbol, &candidate(conviction_signal))
        .await
        .expect("insert setup")
}

fn tool_use_response(ranked: Value) -> Value {
    json!({
        "content": [{
            "type": "tool_use",
            "id": "tu_1",
            "name": TOOL_NAME,
            "input": { "ranked": ranked },
        }],
        "usage": {
            "input_tokens": 200,
            "output_tokens": 80,
            "cache_read_input_tokens": 0,
            "cache_creation_input_tokens": 0
        }
    })
}

fn et_today() -> NaiveDate {
    let et_offset = chrono::FixedOffset::west_opt(5 * 3600).unwrap();
    Utc::now().with_timezone(&et_offset).date_naive()
}

// ---------------- 1: builds_request_with_all_todays_setups ----------------

#[tokio::test]
async fn builds_request_with_all_todays_setups() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    let (tracker, _emitter, _ranker) = build_ranker(Arc::clone(&db), Arc::clone(&http), 100.0);

    // 12 active setups today, with mixed conviction signals.
    let mut setups = Vec::with_capacity(12);
    for i in 0..12 {
        let sym = format!("TKR{i}");
        setups.push(insert_setup(&tracker, &sym, 0.5 + (i as f64) * 0.01).await);
    }

    let req = DailyRanker::build_request(&setups, 5);
    assert!(matches!(req.kind, LlmKind::Ranker));
    assert_eq!(req.model, MODEL);
    assert_eq!(req.system.len(), 1);
    assert!(req.system[0].cache, "system block must be cached");

    let msg: Value = serde_json::from_str(&req.messages[0].content).expect("user msg JSON");
    assert_eq!(msg["top_n"], 5);
    let arr = msg["setups"].as_array().expect("setups array");
    assert_eq!(arr.len(), 12, "all 12 active setups must be in payload");
    // Spot-check first setup carries id, symbol, strategy, raw_signals.
    assert_eq!(arr[0]["setup_id"], setups[0].id);
    assert_eq!(arr[0]["symbol"], setups[0].symbol);
    assert_eq!(arr[0]["strategy"], "breakout");
    assert!(arr[0]["raw_signals"]["volume_multiple"].is_number());
}

// ---------------- 2: forces_emit_morning_pack_tool_use ----------------

#[tokio::test]
async fn forces_emit_morning_pack_tool_use() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    let (tracker, _emitter, _ranker) = build_ranker(Arc::clone(&db), Arc::clone(&http), 100.0);
    let s = insert_setup(&tracker, "AAPL", 0.6).await;
    let req = DailyRanker::build_request(&[s], 5);

    match req.tool_choice.as_ref().expect("tool_choice set") {
        ToolChoice::ForceTool(name) => assert_eq!(name, TOOL_NAME),
        other => panic!("expected ForceTool, got {other:?}"),
    }
    let tools = req.tools.as_ref().expect("tools set");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, TOOL_NAME);
}

// ---------------- 3: parses_ranked_top_n ----------------

#[tokio::test]
async fn parses_ranked_top_n() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());

    // Three setups; LLM ranks them and returns extra entry beyond top_n cap.
    let (tracker, _emitter, ranker) = build_ranker(Arc::clone(&db), Arc::clone(&http), 100.0);
    let a = insert_setup(&tracker, "AAA", 0.8).await;
    let b = insert_setup(&tracker, "BBB", 0.7).await;
    let c = insert_setup(&tracker, "CCC", 0.6).await;

    http.enqueue_ok(tool_use_response(json!([
        {"setup_id": a.id, "rank": 1, "why_top_pick": "Strongest conviction A"},
        {"setup_id": b.id, "rank": 2, "why_top_pick": "Clean breakout structure"},
        {"setup_id": c.id, "rank": 3, "why_top_pick": "Volume confirmation"},
    ])));

    let pack = ranker.rank_today(et_today(), 2).await.expect("rank");
    assert_eq!(pack.ranked.len(), 2, "top_n cap of 2 enforced");
    assert_eq!(pack.ranked[0].setup_id, a.id);
    assert_eq!(pack.ranked[0].rank, 1);
    assert!(pack.ranked[0].why_top_pick.contains("conviction"));
    assert_eq!(pack.ranked[1].setup_id, b.id);
}

// ---------------- 4: persists_morning_pack_to_db ----------------

#[tokio::test]
async fn persists_morning_pack_to_db() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    let (tracker, _emitter, ranker) = build_ranker(Arc::clone(&db), Arc::clone(&http), 100.0);
    let a = insert_setup(&tracker, "AAA", 0.8).await;

    http.enqueue_ok(tool_use_response(json!([
        {"setup_id": a.id, "rank": 1, "why_top_pick": "Strongest"},
    ])));

    let date = et_today();
    let pack = ranker.rank_today(date, 5).await.expect("rank");
    assert_eq!(pack.date, date);

    // Round-trip through `get_pack`.
    let fetched = ranker.get_pack(date).await.expect("get").expect("present");
    assert_eq!(fetched.date, date);
    assert_eq!(fetched.ranked.len(), 1);
    assert_eq!(fetched.ranked[0].setup_id, a.id);
}

// ---------------- 5: dedup_per_date ----------------

#[tokio::test]
async fn dedup_per_date() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    let (tracker, emitter, ranker) = build_ranker(Arc::clone(&db), Arc::clone(&http), 100.0);
    let a = insert_setup(&tracker, "AAA", 0.8).await;
    let b = insert_setup(&tracker, "BBB", 0.7).await;

    let date = et_today();

    http.enqueue_ok(tool_use_response(json!([
        {"setup_id": a.id, "rank": 1, "why_top_pick": "first run pick"},
    ])));
    let first = ranker.rank_today(date, 5).await.expect("rank 1");
    assert_eq!(first.ranked[0].setup_id, a.id);

    http.enqueue_ok(tool_use_response(json!([
        {"setup_id": b.id, "rank": 1, "why_top_pick": "second run pick"},
    ])));
    let second = ranker.rank_today(date, 5).await.expect("rank 2");
    assert_eq!(
        second.ranked[0].setup_id, b.id,
        "second call overrides first"
    );

    let stored = ranker.get_pack(date).await.unwrap().unwrap();
    assert_eq!(stored.ranked[0].setup_id, b.id);

    // MorningPackReady emitted twice (once per call).
    let events = emitter.captured().await;
    let pack_events: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, AppEvent::MorningPackReady { .. }))
        .collect();
    assert_eq!(pack_events.len(), 2, "MorningPackReady emitted per call");
}

// ---------------- 6: respects_budget_kill_switch ----------------

#[tokio::test]
async fn respects_budget_kill_switch() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new()); // no canned responses — must not be called

    // Pre-load llm_calls so today's spend exceeds the tiny budget.
    let now = chrono::Utc::now().timestamp();
    let day_start = (now / 86_400) * 86_400;
    db.with_conn(move |conn| {
        conn.execute(
            "INSERT INTO llm_calls (kind, model, input_tokens, output_tokens, \
             cache_read_tokens, cost_usd, called_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                "ranker",
                "claude-sonnet-4-6",
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

    let (tracker, _emitter, ranker) = build_ranker(Arc::clone(&db), Arc::clone(&http), 1.0);
    let a = insert_setup(&tracker, "AAA", 0.9).await;
    let b = insert_setup(&tracker, "BBB", 0.5).await;

    let date = et_today();
    let pack = ranker
        .rank_today(date, 5)
        .await
        .expect("rank under budget exhaustion");
    assert_eq!(http.call_count(), 0, "no HTTP call when budget exhausted");
    assert_eq!(pack.ranked.len(), 2, "naive top-N still returned");
    assert_eq!(
        pack.ranked[0].setup_id, a.id,
        "naive ranker orders by conviction_signal desc"
    );
    assert_eq!(pack.ranked[1].setup_id, b.id);
    for r in &pack.ranked {
        assert!(
            r.why_top_pick.to_lowercase().contains("fallback"),
            "fallback rationale should mention fallback, got: {}",
            r.why_top_pick
        );
    }
}

// ---------------- 7: empty_setups_today_skips_call ----------------

#[tokio::test]
async fn empty_setups_today_skips_call() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new()); // must not be called
    let (_tracker, emitter, ranker) = build_ranker(Arc::clone(&db), Arc::clone(&http), 100.0);

    let date = et_today();
    let pack = ranker.rank_today(date, 5).await.expect("rank");
    assert_eq!(http.call_count(), 0, "no LLM call when no setups today");
    assert!(pack.ranked.is_empty(), "empty ranked list");
    assert_eq!(pack.date, date);

    let events = emitter.captured().await;
    let count = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                AppEvent::MorningPackReady {
                    ranked_count: 0,
                    ..
                }
            )
        })
        .count();
    assert_eq!(count, 1, "still emit MorningPackReady with 0 candidates");
}

// ---------------- 8: older_setups_excluded ----------------

#[tokio::test]
async fn older_setups_excluded() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    let (tracker, _emitter, ranker) = build_ranker(Arc::clone(&db), Arc::clone(&http), 100.0);

    // Today's setup.
    let today = insert_setup(&tracker, "TODAY", 0.7).await;
    // Old setup — backdate detected_at to 2 days ago.
    add_ticker(&tracker, "OLD").await;
    let old = tracker
        .insert_setup("OLD", &candidate(0.9))
        .await
        .expect("insert");
    let old_ts = (Utc::now() - ChronoDuration::days(2)).timestamp();
    db.with_conn(move |conn| {
        conn.execute(
            "UPDATE setups SET detected_at = ?1 WHERE id = ?2",
            rusqlite::params![old_ts, old.id],
        )?;
        Ok(())
    })
    .await
    .unwrap();

    http.enqueue_ok(tool_use_response(json!([
        {"setup_id": today.id, "rank": 1, "why_top_pick": "only today"},
    ])));

    let pack = ranker.rank_today(et_today(), 5).await.expect("rank");
    assert_eq!(pack.ranked.len(), 1);
    assert_eq!(
        pack.ranked[0].setup_id, today.id,
        "old setup must be excluded from today's pack"
    );
}
