// allow-large-file: covers thesis prompt construction, tool-choice forcing,
// JSON parsing, persistence to setups, and budget-error fallback paths. The
// MockHttp queue scaffolding and DB seeding are shared across all cases.
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use tempfile::NamedTempFile;

use crate::events::{AppEvent, EventEmitter};
use crate::ibkr::types::historical::{BarSize, HistoricalBar};
use crate::ibkr::types::news::NewsItem;
use crate::ibkr::types::tracker::{Setup, SetupStatus};
use crate::services::llm_service::{
    AnthropicHttp, AnthropicHttpError, LlmKind, LlmService, ToolChoice,
};
use crate::services::tracker_service::TrackerService;
use crate::storage::Db;
use crate::strategies::{Direction, SetupCandidate, TargetLevel};

use super::{InvalidationLevel, ThesisContext, ThesisGenerator, MODEL, TOOL_NAME};

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
    fn enqueue_err(&self, err: AnthropicHttpError) {
        self.canned.lock().unwrap().push_back(Err(err));
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

fn sample_candidate() -> SetupCandidate {
    SetupCandidate {
        strategy: "breakout",
        tag: crate::ibkr::types::tracker::StrategyTag::Breakout,
        direction: Direction::Long,
        conviction_signal: 0.75,
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
        raw_signals: json!({"volume_multiple": 1.85, "lookback_high": 104.5}),
        timeframe: BarSize::Day1,
        detected_at: Utc::now(),
    }
}

fn sample_setup() -> Setup {
    let candidate = sample_candidate();
    Setup {
        id: 42,
        symbol: "AAPL".to_string(),
        strategy: candidate.strategy.to_string(),
        direction: candidate.direction,
        detected_at: candidate.detected_at,
        trigger_price: candidate.trigger_price,
        stop_price: candidate.stop_price,
        targets: candidate.targets,
        raw_signals: candidate.raw_signals,
        thesis: None,
        thesis_json: None,
        status: SetupStatus::Active,
        invalidated_at: None,
        invalidation_reason: None,
        archived_at: None,
    }
}

fn sample_bars(n: usize) -> Vec<HistoricalBar> {
    (0..n)
        .map(|i| HistoricalBar {
            time: format!("2026010{}", i + 1),
            open: 100.0 + i as f64,
            high: 101.0 + i as f64,
            low: 99.0 + i as f64,
            close: 100.5 + i as f64,
            volume: 1_000_000 + i as i64 * 50_000,
            wap: 100.5 + i as f64,
            count: 0,
        })
        .collect()
}

fn sample_news() -> Vec<NewsItem> {
    vec![NewsItem {
        time_published: Utc::now(),
        title: "AAPL beats Q3 expectations".to_string(),
        summary: "Earnings report blew past estimates...".to_string(),
        source: "Reuters".to_string(),
        url: "https://example.com/x".to_string(),
        overall_sentiment_score: Some(0.4),
        overall_sentiment_label: Some("Bullish".to_string()),
        ticker_sentiment: vec![],
    }]
}

fn well_formed_tool_input() -> Value {
    json!({
        "thesis_md": "AAPL breakout above 20d high backed by 1.85× volume. Trigger 105, stop 100, 2R 115.",
        "conviction": "B",
        "invalidation_levels": [
            { "label": "swing_low", "price": 100.0, "reason": "below 10d swing low" },
            { "label": "atr_stop", "price": 99.0, "reason": "1×ATR(14) below trigger" }
        ],
        "risk_notes": "No earnings event scheduled this week."
    })
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
            "input_tokens": 100,
            "output_tokens": 50,
            "cache_read_input_tokens": 0,
            "cache_creation_input_tokens": 0
        }
    })
}

fn build_generator(
    db: Arc<Db>,
    http: Arc<MockHttp>,
) -> (Arc<TrackerService>, Arc<EventEmitter>, ThesisGenerator) {
    let llm = Arc::new(
        LlmService::new("test-key".to_string(), Arc::clone(&db), 100.0)
            .with_http(http as Arc<dyn AnthropicHttp>),
    );
    let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));
    let emitter = Arc::new(EventEmitter::for_capture());
    let generator =
        ThesisGenerator::new(Arc::clone(&llm), Arc::clone(&tracker), Arc::clone(&emitter));
    (tracker, emitter, generator)
}

async fn add_ticker(tracker: &TrackerService, symbol: &str) {
    tracker
        .add(
            symbol,
            crate::ibkr::types::tracker::TrackerSource::Manual,
            None,
            vec![],
            None,
        )
        .await
        .expect("add ticker");
}

// ---------------- 1: builds_request_with_setup_data_and_context ----------------

#[test]
fn builds_request_with_setup_data_and_context() {
    let setup = sample_setup();
    let bars = sample_bars(5);
    let news = sample_news();
    let ctx = ThesisContext {
        daily_bars: &bars,
        recent_news: &news,
    };
    let req = ThesisGenerator::build_request(&setup, &ctx);

    assert!(matches!(req.kind, LlmKind::Thesis));
    assert_eq!(req.model, MODEL);
    assert_eq!(req.setup_id, Some(42));

    // System prompt is present and asks for cache control.
    assert_eq!(req.system.len(), 1);
    assert!(
        req.system[0].text.contains("sober swing trader"),
        "system prompt should describe analyst persona, got: {}",
        req.system[0].text
    );
    assert!(req.system[0].cache, "system block should request cache");

    // Tool schema: one tool, named `emit_thesis`, with the right top-level required fields.
    let tools = req.tools.as_ref().expect("tools must be set");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, TOOL_NAME);
    let required = tools[0].input_schema["required"]
        .as_array()
        .expect("required array");
    let names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
    for f in [
        "thesis_md",
        "conviction",
        "invalidation_levels",
        "risk_notes",
    ] {
        assert!(names.contains(&f), "tool schema must require `{f}`");
    }

    // Forced tool use.
    match req.tool_choice.as_ref().expect("tool_choice set") {
        ToolChoice::ForceTool(name) => assert_eq!(name, TOOL_NAME),
        other => panic!("expected ForceTool, got {other:?}"),
    }

    // User message embeds setup + bars summary + news.
    assert_eq!(req.messages.len(), 1);
    let msg: Value = serde_json::from_str(&req.messages[0].content).expect("user msg is JSON");
    assert_eq!(msg["setup"]["symbol"], "AAPL");
    assert_eq!(msg["setup"]["strategy"], "breakout");
    let bars_summary = msg["bars_summary"].as_array().expect("bars_summary array");
    assert_eq!(bars_summary.len(), 5);
    let news_summary = msg["recent_news"].as_array().expect("recent_news array");
    assert_eq!(news_summary.len(), 1);
    assert_eq!(
        news_summary[0]["title"], "AAPL beats Q3 expectations",
        "news summary keeps title"
    );
}

// ---------------- 2: parses_tool_response_into_typed_thesis ----------------

#[test]
fn parses_tool_response_into_typed_thesis() {
    let input = well_formed_tool_input();
    let thesis = ThesisGenerator::parse_thesis(&input).expect("parse");
    assert_eq!(
        thesis.thesis_md,
        "AAPL breakout above 20d high backed by 1.85× volume. Trigger 105, stop 100, 2R 115."
    );
    assert_eq!(thesis.conviction, 'B');
    assert_eq!(thesis.invalidation_levels.len(), 2);
    assert_eq!(
        thesis.invalidation_levels[0],
        InvalidationLevel {
            label: "swing_low".to_string(),
            price: 100.0,
            reason: "below 10d swing low".to_string(),
        }
    );
    assert_eq!(thesis.risk_notes, "No earnings event scheduled this week.");
}

// ---------------- 3: persists_thesis_to_setup_row ----------------

#[tokio::test]
async fn persists_thesis_to_setup_row() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    http.enqueue_ok(tool_use_response(well_formed_tool_input()));

    let (tracker, _emitter, generator) = build_generator(Arc::clone(&db), Arc::clone(&http));
    add_ticker(&tracker, "AAPL").await;
    let setup = tracker
        .insert_setup("AAPL", &sample_candidate())
        .await
        .expect("insert");
    let bars = sample_bars(5);
    let news = sample_news();
    let ctx = ThesisContext {
        daily_bars: &bars,
        recent_news: &news,
    };

    let result = generator.generate(&setup, &ctx).await.expect("generate");
    let thesis = result.expect("Some(Thesis) on success");
    assert_eq!(thesis.conviction, 'B');

    // Round-trip: row now has thesis (markdown) + thesis_json (full struct).
    let refreshed = tracker
        .get_setup(setup.id)
        .await
        .expect("get_setup")
        .expect("present");
    assert_eq!(refreshed.thesis.as_deref(), Some(thesis.thesis_md.as_str()));
    let stored_json = refreshed.thesis_json.expect("thesis_json populated");
    assert_eq!(stored_json["conviction"], "B");
    let levels = stored_json["invalidation_levels"]
        .as_array()
        .expect("invalidation_levels array");
    assert_eq!(levels.len(), 2);
}

// ---------------- 4: emits_setup_detected_with_thesis_after_generation ----------------

#[tokio::test]
async fn emits_setup_detected_with_thesis_after_generation() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    http.enqueue_ok(tool_use_response(well_formed_tool_input()));

    let (tracker, emitter, generator) = build_generator(Arc::clone(&db), Arc::clone(&http));
    add_ticker(&tracker, "AAPL").await;
    let setup = tracker
        .insert_setup("AAPL", &sample_candidate())
        .await
        .expect("insert");
    let bars = sample_bars(3);
    let news: Vec<NewsItem> = vec![];
    let ctx = ThesisContext {
        daily_bars: &bars,
        recent_news: &news,
    };

    generator.generate(&setup, &ctx).await.expect("generate");

    let events = emitter.captured().await;
    let detected: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AppEvent::SetupDetected { setup, thesis } => Some((setup.clone(), thesis.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(detected.len(), 1, "exactly one SetupDetected emitted");
    let (emitted_setup, thesis) = &detected[0];
    assert_eq!(emitted_setup.id, setup.id);
    assert_eq!(emitted_setup.symbol, "AAPL");
    assert!(
        thesis.as_deref().unwrap_or("").contains("breakout"),
        "thesis text should be present and reference the strategy"
    );
    assert!(
        emitted_setup.thesis.is_some(),
        "the emitted setup row carries the persisted thesis markdown"
    );
}

// ---------------- 5: falls_back_gracefully_on_llm_error ----------------

#[tokio::test]
async fn falls_back_gracefully_on_llm_error() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    http.enqueue_err(AnthropicHttpError::Upstream {
        status: 500,
        body: "boom".to_string(),
    });

    let (tracker, emitter, generator) = build_generator(Arc::clone(&db), Arc::clone(&http));
    add_ticker(&tracker, "AAPL").await;
    let setup = tracker
        .insert_setup("AAPL", &sample_candidate())
        .await
        .expect("insert");
    let bars = sample_bars(3);
    let ctx = ThesisContext {
        daily_bars: &bars,
        recent_news: &[],
    };

    let result = generator.generate(&setup, &ctx).await.expect("generate");
    assert!(result.is_none(), "Upstream 5xx should yield Ok(None)");

    // Row remains thesis-less.
    let refreshed = tracker.get_setup(setup.id).await.unwrap().unwrap();
    assert!(refreshed.thesis.is_none());
    assert!(refreshed.thesis_json.is_none());

    // No SetupDetected event emitted by the generator on graceful fallback.
    let detected = emitter
        .captured()
        .await
        .into_iter()
        .filter(|e| matches!(e, AppEvent::SetupDetected { .. }))
        .count();
    assert_eq!(
        detected, 0,
        "generator must not re-emit SetupDetected on graceful LLM failure"
    );
}

// ---------------- 6: falls_back_on_budget_exhausted ----------------

#[tokio::test]
async fn falls_back_on_budget_exhausted() {
    // Use a tiny budget and pre-load a row in `llm_calls` to push us over it.
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new()); // no canned responses — must not be called

    let now = chrono::Utc::now().timestamp();
    let day_start = (now / 86_400) * 86_400;
    db.with_conn(move |conn| {
        conn.execute(
            "INSERT INTO llm_calls (kind, model, input_tokens, output_tokens, \
             cache_read_tokens, cost_usd, called_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                "thesis",
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

    // Wire a generator with a budget below today's spend.
    let llm = Arc::new(
        LlmService::new("test-key".to_string(), Arc::clone(&db), 1.0)
            .with_http(Arc::clone(&http) as Arc<dyn AnthropicHttp>),
    );
    let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));
    let emitter = Arc::new(EventEmitter::for_capture());
    let generator = ThesisGenerator::new(llm, Arc::clone(&tracker), Arc::clone(&emitter));

    add_ticker(&tracker, "AAPL").await;
    let setup = tracker
        .insert_setup("AAPL", &sample_candidate())
        .await
        .expect("insert");
    let bars = sample_bars(3);
    let ctx = ThesisContext {
        daily_bars: &bars,
        recent_news: &[],
    };

    let result = generator.generate(&setup, &ctx).await.expect("generate");
    assert!(result.is_none(), "BudgetExhausted must surface as Ok(None)");
    assert_eq!(http.call_count(), 0, "no HTTP call when budget exhausted");

    let refreshed = tracker.get_setup(setup.id).await.unwrap().unwrap();
    assert!(refreshed.thesis.is_none());
}

// ---------------- 7: skips_when_thesis_already_present ----------------

#[tokio::test]
async fn skips_when_thesis_already_present() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new()); // must not be called

    let (tracker, emitter, generator) = build_generator(Arc::clone(&db), Arc::clone(&http));
    add_ticker(&tracker, "AAPL").await;
    let row = tracker
        .insert_setup("AAPL", &sample_candidate())
        .await
        .expect("insert");
    // Pre-populate a thesis on the row.
    tracker
        .update_setup_thesis(
            row.id,
            "existing thesis".to_string(),
            json!({"conviction": "A"}),
        )
        .await
        .unwrap();
    let refreshed = tracker.get_setup(row.id).await.unwrap().unwrap();

    let bars = sample_bars(3);
    let ctx = ThesisContext {
        daily_bars: &bars,
        recent_news: &[],
    };

    let result = generator
        .generate(&refreshed, &ctx)
        .await
        .expect("generate");
    assert!(result.is_none(), "should skip when thesis already present");
    assert_eq!(http.call_count(), 0, "no LLM call on idempotent skip");

    let post = tracker.get_setup(row.id).await.unwrap().unwrap();
    assert_eq!(post.thesis.as_deref(), Some("existing thesis"));

    let detected = emitter
        .captured()
        .await
        .into_iter()
        .filter(|e| matches!(e, AppEvent::SetupDetected { .. }))
        .count();
    assert_eq!(detected, 0, "no SetupDetected re-emit on idempotent skip");
}

// ---------------- 8: system_prompt_uses_cache_control ----------------

#[test]
fn system_prompt_uses_cache_control() {
    let setup = sample_setup();
    let bars = sample_bars(2);
    let ctx = ThesisContext {
        daily_bars: &bars,
        recent_news: &[],
    };
    let req = ThesisGenerator::build_request(&setup, &ctx);
    let body = crate::services::llm_service::build_request_body(&req);
    assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
}
