use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{Duration, Utc};
use serde_json::{json, Value};
use tempfile::NamedTempFile;

use crate::ibkr::types::news::{NewsItem, NewsTone, NewsVerdict, TickerSentiment};
use crate::services::llm_service::{
    AnthropicHttp, AnthropicHttpError, LlmKind, LlmService, ToolChoice,
};
use crate::services::news_cache::read_cache_with_verdict;
use crate::storage::Db;

use super::{NewsInterpreter, MODEL, TOOL_NAME};

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

fn news_with(title: &str, summary: &str, score: f64, label: &str, symbol: &str) -> NewsItem {
    NewsItem {
        time_published: Utc::now() - Duration::hours(2),
        title: title.to_string(),
        summary: summary.to_string(),
        source: "Reuters".to_string(),
        url: "https://example.test/article".to_string(),
        overall_sentiment_score: Some(score),
        overall_sentiment_label: Some(label.to_string()),
        ticker_sentiment: vec![TickerSentiment {
            ticker: symbol.to_string(),
            relevance_score: 0.8,
            ticker_sentiment_score: score,
            ticker_sentiment_label: label.to_string(),
        }],
    }
}

fn tool_use_response(input: Value) -> Value {
    json!({
        "content": [{
            "type": "tool_use",
            "id": "tu_news_1",
            "name": TOOL_NAME,
            "input": input,
        }],
        "usage": {
            "input_tokens": 200,
            "output_tokens": 60,
            "cache_read_input_tokens": 0,
            "cache_creation_input_tokens": 0
        }
    })
}

fn build_interpreter(db: Arc<Db>, http: Arc<MockHttp>) -> NewsInterpreter {
    let llm = Arc::new(
        LlmService::new("test-key".to_string(), Arc::clone(&db), 100.0)
            .with_http(http as Arc<dyn AnthropicHttp>),
    );
    NewsInterpreter::new(llm, db)
}

async fn seed_news_row(db: &Db, symbol: &str, items: &[NewsItem]) {
    let symbol = symbol.to_string();
    let payload = serde_json::to_string(items).expect("serialize");
    let now = Utc::now().timestamp();
    db.with_conn(move |conn| {
        conn.execute(
            "INSERT OR REPLACE INTO news_cache (symbol, fetched_at, payload) VALUES (?1, ?2, ?3)",
            rusqlite::params![symbol, now, payload],
        )?;
        Ok(())
    })
    .await
    .expect("seed news_cache row");
}

async fn read_verdict_json(db: &Db, symbol: &str) -> Option<String> {
    let cached = read_cache_with_verdict(db, symbol)
        .await
        .expect("read_cache_with_verdict")
        .expect("row present");
    cached.verdict_json
}

// ---------------- 1: builds_request_with_news_block_and_cache ----------------

#[test]
fn builds_request_with_news_block_and_cache() {
    let items = vec![news_with(
        "AAPL beats Q3",
        "Earnings blew past estimates...",
        0.4,
        "Bullish",
        "AAPL",
    )];
    let req = NewsInterpreter::build_request("AAPL", &items);

    assert!(matches!(req.kind, LlmKind::News));
    assert_eq!(req.model, MODEL);
    assert_eq!(req.setup_id, None);

    assert_eq!(req.system.len(), 1);
    assert!(req.system[0].cache, "system block must be cache_control'd");

    let tools = req.tools.as_ref().expect("tools must be set");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, TOOL_NAME);

    match req.tool_choice.as_ref().expect("tool_choice set") {
        ToolChoice::ForceTool(name) => assert_eq!(name, TOOL_NAME),
        other => panic!("expected ForceTool, got {other:?}"),
    }

    let msg: Value = serde_json::from_str(&req.messages[0].content).expect("user msg JSON");
    assert_eq!(msg["symbol"], "AAPL");
    let news = msg["news"].as_array().expect("news array");
    assert_eq!(news.len(), 1);
    assert_eq!(news[0]["title"], "AAPL beats Q3");
}

// ---------------- 2: caches_news_block_per_symbol ----------------

#[test]
fn caches_news_block_per_symbol() {
    let items = vec![news_with("h", "s", 0.1, "Neutral", "AAPL")];
    let req = NewsInterpreter::build_request("AAPL", &items);
    let body = crate::services::llm_service::build_request_body(&req);
    assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
}

// ---------------- 3: parses_well_formed_tool_input ----------------

#[test]
fn parses_well_formed_tool_input() {
    let input = json!({
        "tone": "bullish",
        "ep_worthy": true,
        "parabolic_risk": false,
        "summary": "Earnings beat with raised guidance.",
    });
    let v = NewsInterpreter::parse_verdict(&input).expect("parse");
    assert_eq!(v.tone, NewsTone::Bullish);
    assert!(v.ep_worthy);
    assert!(!v.parabolic_risk);
    assert!(v.summary.contains("Earnings"));
}

// ---------------- 4: interprets_bullish_earnings_beat ----------------

#[tokio::test]
async fn interprets_bullish_earnings_beat() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    http.enqueue_ok(tool_use_response(json!({
        "tone": "bullish",
        "ep_worthy": true,
        "parabolic_risk": false,
        "summary": "Earnings beat by 12%; guidance raised.",
    })));

    let items = vec![news_with(
        "AAPL beats Q3 by 12%, raises guidance",
        "Strong iPhone demand drove the beat.",
        0.55,
        "Bullish",
        "AAPL",
    )];
    seed_news_row(&db, "AAPL", &items).await;

    let interpreter = build_interpreter(Arc::clone(&db), Arc::clone(&http));
    let v = interpreter.interpret("AAPL").await.expect("interpret");
    let v = v.expect("Some(verdict)");

    assert_eq!(v.tone, NewsTone::Bullish);
    assert!(v.ep_worthy);
    assert!(!v.parabolic_risk);
}

// ---------------- 5: interprets_bearish_guidance_cut ----------------

#[tokio::test]
async fn interprets_bearish_guidance_cut() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    http.enqueue_ok(tool_use_response(json!({
        "tone": "bearish",
        "ep_worthy": true,
        "parabolic_risk": false,
        "summary": "Q4 guidance cut by 8%; demand softening.",
    })));

    let items = vec![news_with(
        "AAPL slashes Q4 guidance",
        "Demand in China softened.",
        -0.45,
        "Bearish",
        "AAPL",
    )];
    seed_news_row(&db, "AAPL", &items).await;

    let interpreter = build_interpreter(Arc::clone(&db), Arc::clone(&http));
    let v = interpreter
        .interpret("AAPL")
        .await
        .expect("interpret")
        .expect("Some(verdict)");

    assert_eq!(v.tone, NewsTone::Bearish);
    assert!(v.ep_worthy);
    assert!(!v.parabolic_risk);
}

// ---------------- 6: interprets_neutral_routine_filing ----------------

#[tokio::test]
async fn interprets_neutral_routine_filing() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    http.enqueue_ok(tool_use_response(json!({
        "tone": "neutral",
        "ep_worthy": false,
        "parabolic_risk": false,
        "summary": "Routine 10-K filed; no new disclosures.",
    })));

    let items = vec![news_with(
        "AAPL files 10-K annual report",
        "No material changes from prior guidance.",
        0.05,
        "Neutral",
        "AAPL",
    )];
    seed_news_row(&db, "AAPL", &items).await;

    let interpreter = build_interpreter(Arc::clone(&db), Arc::clone(&http));
    let v = interpreter
        .interpret("AAPL")
        .await
        .expect("interpret")
        .expect("Some(verdict)");

    assert_eq!(v.tone, NewsTone::Neutral);
    assert!(!v.ep_worthy);
    assert!(!v.parabolic_risk);
}

// ---------------- 7: flags_parabolic_risk_on_short_squeeze_chatter ----------------

#[tokio::test]
async fn flags_parabolic_risk_on_short_squeeze_chatter() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    http.enqueue_ok(tool_use_response(json!({
        "tone": "bullish",
        "ep_worthy": false,
        "parabolic_risk": true,
        "summary": "Short squeeze chatter on retail boards; vertical move.",
    })));

    let items = vec![news_with(
        "GME short squeeze accelerates as retail piles in",
        "Vertical move on retail interest; halted twice.",
        0.7,
        "Bullish",
        "GME",
    )];
    seed_news_row(&db, "GME", &items).await;

    let interpreter = build_interpreter(Arc::clone(&db), Arc::clone(&http));
    let v = interpreter
        .interpret("GME")
        .await
        .expect("interpret")
        .expect("Some(verdict)");

    assert_eq!(v.tone, NewsTone::Bullish);
    assert!(
        v.parabolic_risk,
        "short-squeeze chatter must flag parabolic_risk"
    );
}

// ---------------- 8: persists_to_news_cache_payload ----------------

#[tokio::test]
async fn persists_to_news_cache_payload() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    http.enqueue_ok(tool_use_response(json!({
        "tone": "bullish",
        "ep_worthy": true,
        "parabolic_risk": false,
        "summary": "Strong earnings beat.",
    })));

    let items = vec![news_with(
        "AAPL crushes Q3",
        "summary",
        0.5,
        "Bullish",
        "AAPL",
    )];
    seed_news_row(&db, "AAPL", &items).await;

    let interpreter = build_interpreter(Arc::clone(&db), Arc::clone(&http));
    interpreter.interpret("AAPL").await.expect("interpret");

    let json = read_verdict_json(&db, "AAPL")
        .await
        .expect("verdict_json column populated");
    let stored: NewsVerdict = serde_json::from_str(&json).expect("verdict_json parses");
    assert_eq!(stored.tone, NewsTone::Bullish);
    assert!(stored.ep_worthy);
    assert!(!stored.parabolic_risk);
    assert!(stored.summary.contains("earnings"));
}

// ---------------- 9: does_not_call_llm_when_verdict_already_present ----------------

#[tokio::test]
async fn does_not_call_llm_when_verdict_already_present() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    http.enqueue_ok(tool_use_response(json!({
        "tone": "neutral",
        "ep_worthy": false,
        "parabolic_risk": false,
        "summary": "first pass",
    })));

    let items = vec![news_with("h", "s", 0.0, "Neutral", "AAPL")];
    seed_news_row(&db, "AAPL", &items).await;

    let interpreter = build_interpreter(Arc::clone(&db), Arc::clone(&http));
    interpreter
        .interpret("AAPL")
        .await
        .expect("first interpret");
    assert_eq!(http.call_count(), 1, "first call must hit the LLM");

    // Second invocation: verdict_json is already populated, so no new
    // payload arrived and we must short-circuit without calling the LLM.
    let result = interpreter
        .interpret("AAPL")
        .await
        .expect("second interpret");
    assert!(result.is_none(), "second call must skip");
    assert_eq!(
        http.call_count(),
        1,
        "no second LLM call when verdict cached"
    );
}

// ---------------- 10: respects_budget_kill_switch ----------------

#[tokio::test]
async fn respects_budget_kill_switch() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new()); // must not be called

    // Pre-load llm_calls with cost above today's budget.
    let now = Utc::now().timestamp();
    let day_start = (now / 86_400) * 86_400;
    db.with_conn(move |conn| {
        conn.execute(
            "INSERT INTO llm_calls (kind, model, input_tokens, output_tokens, \
             cache_read_tokens, cost_usd, called_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                "news",
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

    let llm = Arc::new(
        LlmService::new("test-key".to_string(), Arc::clone(&db), 1.0)
            .with_http(Arc::clone(&http) as Arc<dyn AnthropicHttp>),
    );
    let interpreter = NewsInterpreter::new(llm, Arc::clone(&db));

    let items = vec![news_with("h", "s", 0.4, "Bullish", "AAPL")];
    seed_news_row(&db, "AAPL", &items).await;

    let result = interpreter.interpret("AAPL").await.expect("interpret");
    assert!(result.is_none(), "BudgetExhausted must surface as Ok(None)");
    assert_eq!(http.call_count(), 0, "no HTTP call when budget exhausted");
    assert!(
        read_verdict_json(&db, "AAPL").await.is_none(),
        "no verdict written on budget kill-switch"
    );
}

// ---------------- 11: skips_when_no_cached_news ----------------

#[tokio::test]
async fn skips_when_no_cached_news() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new()); // must not be called

    let interpreter = build_interpreter(Arc::clone(&db), Arc::clone(&http));
    let result = interpreter.interpret("NOSUCH").await.expect("interpret");
    assert!(result.is_none());
    assert_eq!(http.call_count(), 0);
}

// ---------------- 12: skips_when_cached_news_is_empty ----------------

#[tokio::test]
async fn skips_when_cached_news_is_empty() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new()); // must not be called

    seed_news_row(&db, "AAPL", &[]).await;

    let interpreter = build_interpreter(Arc::clone(&db), Arc::clone(&http));
    let result = interpreter.interpret("AAPL").await.expect("interpret");
    assert!(result.is_none());
    assert_eq!(http.call_count(), 0);
}
