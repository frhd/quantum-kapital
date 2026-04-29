// allow-large-file: LLM-service tests cover request body shape, response parsing,
// budget enforcement, retry/cost ledger integration, and error mapping. All cases
// share one MockHttp fixture; splitting forks the queue scaffolding.
use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{json, Value};
use tempfile::NamedTempFile;

use crate::storage::Db;

use super::{
    build_request_body, utc_day_start_unix, AnthropicHttp, AnthropicHttpError, LlmClock, LlmError,
    LlmKind, LlmRequest, LlmService, Message, Role, SystemBlock, ToolChoice, ToolSchema,
};
use crate::services::llm_service::prices;

// ---------------- helpers ----------------

fn make_db() -> (NamedTempFile, Arc<Db>) {
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Db::open(tmp.path()).expect("open db");
    (tmp, Arc::new(db))
}

#[derive(Default)]
struct MockHttp {
    canned: Mutex<VecDeque<Result<Value, AnthropicHttpError>>>,
    calls: Mutex<Vec<(String, String, Value)>>,
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
    fn last_call(&self) -> Option<(String, String, Value)> {
        self.calls.lock().unwrap().last().cloned()
    }
    fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

#[async_trait]
impl AnthropicHttp for MockHttp {
    async fn send_messages(
        &self,
        api_key: &str,
        anthropic_version: &str,
        body: &Value,
    ) -> Result<Value, AnthropicHttpError> {
        self.calls.lock().unwrap().push((
            api_key.to_string(),
            anthropic_version.to_string(),
            body.clone(),
        ));
        self.canned
            .lock()
            .unwrap()
            .pop_front()
            .expect("MockHttp queue exhausted")
    }
}

struct FixedClock(AtomicI64);

impl FixedClock {
    fn new(now: i64) -> Self {
        Self(AtomicI64::new(now))
    }
}

impl LlmClock for FixedClock {
    fn now_unix(&self) -> i64 {
        self.0.load(Ordering::Relaxed)
    }
}

fn build_service(
    db: Arc<Db>,
    http: Arc<MockHttp>,
    clock: Arc<FixedClock>,
    budget: f64,
) -> LlmService {
    LlmService::new("test-key".to_string(), db, budget)
        .with_http(http as Arc<dyn AnthropicHttp>)
        .with_clock(clock as Arc<dyn LlmClock>)
}

fn text_response(text: &str, in_tokens: u32, out_tokens: u32) -> Value {
    json!({
        "content": [{"type": "text", "text": text}],
        "usage": {
            "input_tokens": in_tokens,
            "output_tokens": out_tokens,
            "cache_read_input_tokens": 0,
            "cache_creation_input_tokens": 0
        }
    })
}

fn tool_use_response(name: &str, input: Value) -> Value {
    json!({
        "content": [{"type": "tool_use", "id": "tu_1", "name": name, "input": input}],
        "usage": {
            "input_tokens": 100,
            "output_tokens": 50,
            "cache_read_input_tokens": 0,
            "cache_creation_input_tokens": 0
        }
    })
}

fn simple_request(model: &'static str) -> LlmRequest {
    LlmRequest {
        kind: LlmKind::Thesis,
        model,
        max_tokens: 1024,
        system: vec![],
        messages: vec![Message {
            role: Role::User,
            content: "hello".to_string(),
        }],
        tools: None,
        tool_choice: None,
        setup_id: None,
    }
}

// ---------------- 1: sends correct headers ----------------

#[tokio::test]
async fn sends_correct_headers() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    let clock = Arc::new(FixedClock::new(1_700_000_000));
    http.enqueue_ok(text_response("ok", 10, 20));

    let svc = build_service(db, Arc::clone(&http), clock, 10.0);
    svc.message(simple_request("claude-sonnet-4-6"))
        .await
        .unwrap();

    let (api_key, version, _body) = http.last_call().unwrap();
    assert_eq!(api_key, "test-key");
    assert_eq!(version, "2023-06-01");
}

// ---------------- 2: serializes messages correctly ----------------

#[tokio::test]
async fn serializes_messages_correctly() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    let clock = Arc::new(FixedClock::new(1_700_000_000));
    http.enqueue_ok(text_response("ok", 10, 20));

    let req = LlmRequest {
        kind: LlmKind::Thesis,
        model: "claude-sonnet-4-6",
        max_tokens: 2048,
        system: vec![SystemBlock {
            text: "sys".to_string(),
            cache: false,
        }],
        messages: vec![Message {
            role: Role::User,
            content: "hello".to_string(),
        }],
        tools: Some(vec![ToolSchema {
            name: "emit_thesis".to_string(),
            description: "d".to_string(),
            input_schema: json!({"type": "object"}),
        }]),
        tool_choice: Some(ToolChoice::ForceTool("emit_thesis".to_string())),
        setup_id: None,
    };

    let svc = build_service(db, Arc::clone(&http), clock, 10.0);
    svc.message(req).await.unwrap();

    let (_key, _ver, body) = http.last_call().unwrap();
    assert_eq!(body["model"], "claude-sonnet-4-6");
    assert_eq!(body["max_tokens"], 2048);
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["messages"][0]["content"], "hello");
    assert_eq!(body["system"][0]["type"], "text");
    assert_eq!(body["system"][0]["text"], "sys");
    // No cache_control key when cache=false
    assert!(body["system"][0].get("cache_control").is_none());
    assert_eq!(body["tools"][0]["name"], "emit_thesis");
    assert_eq!(body["tool_choice"]["type"], "tool");
    assert_eq!(body["tool_choice"]["name"], "emit_thesis");
}

// ---------------- 3: parses text response ----------------

#[tokio::test]
async fn parses_text_response() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    let clock = Arc::new(FixedClock::new(1_700_000_000));
    http.enqueue_ok(text_response("hello world", 10, 20));

    let svc = build_service(db, Arc::clone(&http), clock, 10.0);
    let resp = svc
        .message(simple_request("claude-sonnet-4-6"))
        .await
        .unwrap();

    assert_eq!(resp.text, Some("hello world".to_string()));
    assert!(resp.tool_calls.is_empty());
    assert_eq!(resp.usage.input_tokens, 10);
    assert_eq!(resp.usage.output_tokens, 20);
}

// ---------------- 4: parses tool_use response ----------------

#[tokio::test]
async fn parses_tool_use_response() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    let clock = Arc::new(FixedClock::new(1_700_000_000));
    http.enqueue_ok(tool_use_response("emit_thesis", json!({"thesis": "..."})));

    let svc = build_service(db, Arc::clone(&http), clock, 10.0);
    let resp = svc
        .message(simple_request("claude-sonnet-4-6"))
        .await
        .unwrap();

    assert_eq!(resp.tool_calls.len(), 1);
    assert_eq!(resp.tool_calls[0].name, "emit_thesis");
    assert_eq!(resp.tool_calls[0].input["thesis"], "...");
}

// ---------------- 5: forced tool use returns typed args ----------------

#[tokio::test]
async fn forced_tool_use_returns_typed_args() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    let clock = Arc::new(FixedClock::new(1_700_000_000));
    http.enqueue_ok(tool_use_response("emit_thesis", json!({"conviction": "A"})));

    let req = LlmRequest {
        kind: LlmKind::Thesis,
        model: "claude-sonnet-4-6",
        max_tokens: 1024,
        system: vec![],
        messages: vec![Message {
            role: Role::User,
            content: "analyze".to_string(),
        }],
        tools: Some(vec![ToolSchema {
            name: "emit_thesis".to_string(),
            description: "d".to_string(),
            input_schema: json!({"type": "object"}),
        }]),
        tool_choice: Some(ToolChoice::ForceTool("emit_thesis".to_string())),
        setup_id: None,
    };

    let svc = build_service(db, Arc::clone(&http), clock, 10.0);
    let resp = svc.message(req).await.unwrap();

    assert_eq!(resp.tool_calls[0].input["conviction"], "A");
}

// ---------------- 6: records call in DB with cost ----------------

#[tokio::test]
async fn records_call_in_db_with_cost() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    let clock = Arc::new(FixedClock::new(1_700_000_000));
    http.enqueue_ok(json!({
        "content": [{"type": "text", "text": "done"}],
        "usage": {
            "input_tokens": 1000,
            "output_tokens": 500,
            "cache_read_input_tokens": 0,
            "cache_creation_input_tokens": 0
        }
    }));

    let req = LlmRequest {
        kind: LlmKind::Thesis,
        model: "claude-sonnet-4-6",
        max_tokens: 1024,
        system: vec![],
        messages: vec![Message {
            role: Role::User,
            content: "go".to_string(),
        }],
        tools: None,
        tool_choice: None,
        setup_id: Some(42),
    };

    let svc = build_service(Arc::clone(&db), Arc::clone(&http), clock, 10.0);
    svc.message(req).await.unwrap();

    let (kind, setup_id, model, input_tokens, output_tokens, cache_read_tokens, cost_usd) = db
        .with_conn(|conn| {
            conn.query_row(
                "SELECT kind, setup_id, model, input_tokens, output_tokens, \
                 cache_read_tokens, cost_usd FROM llm_calls",
                [],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<i64>>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, i64>(4)?,
                        r.get::<_, i64>(5)?,
                        r.get::<_, f64>(6)?,
                    ))
                },
            )
            .map_err(crate::storage::StorageError::from)
        })
        .await
        .unwrap();

    assert_eq!(kind, "thesis");
    assert_eq!(setup_id, Some(42));
    assert_eq!(model, "claude-sonnet-4-6");
    assert_eq!(input_tokens, 1000);
    assert_eq!(output_tokens, 500);
    assert_eq!(cache_read_tokens, 0);
    // sonnet: 1000*3/M + 500*15/M = 0.003 + 0.0075 = 0.0105
    assert!((cost_usd - 0.0105).abs() < 1e-9, "cost_usd={cost_usd}");
}

// ---------------- 7: cost calculator handles each supported model ----------------

#[test]
fn cost_calculator_handles_each_supported_model() {
    // sonnet: 1M*3 + 1M*15 + 1M*0.30 = 18.30
    let sonnet = prices::cost_usd("claude-sonnet-4-6", 1_000_000, 1_000_000, 1_000_000).unwrap();
    assert!((sonnet - 18.30).abs() < 1e-6, "sonnet={sonnet}");

    // haiku: 1M*1 + 1M*5 + 1M*0.10 = 6.10
    let haiku = prices::cost_usd("claude-haiku-4-5", 1_000_000, 1_000_000, 1_000_000).unwrap();
    assert!((haiku - 6.10).abs() < 1e-6, "haiku={haiku}");

    assert!(prices::cost_usd("unknown-model", 1, 1, 1).is_none());
}

// ---------------- 8: prompt cache block serializes with cache_control ----------------

#[tokio::test]
async fn prompt_cache_block_serializes_with_cache_control() {
    let req = LlmRequest {
        kind: LlmKind::Thesis,
        model: "claude-sonnet-4-6",
        max_tokens: 1024,
        system: vec![SystemBlock {
            text: "cached".to_string(),
            cache: true,
        }],
        messages: vec![Message {
            role: Role::User,
            content: "hello".to_string(),
        }],
        tools: None,
        tool_choice: None,
        setup_id: None,
    };

    let body = build_request_body(&req);
    let sys_block = &body["system"][0];
    assert_eq!(sys_block["type"], "text");
    assert_eq!(sys_block["text"], "cached");
    assert_eq!(sys_block["cache_control"]["type"], "ephemeral");
}

// ---------------- 9: daily budget kill switch blocks new calls ----------------

#[tokio::test]
async fn daily_budget_kill_switch_blocks_new_calls() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    let now = 1_700_000_000i64;
    let clock = Arc::new(FixedClock::new(now));

    // Pre-insert a row with cost_usd = 0.005 for today
    let day_start = utc_day_start_unix(now);
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
                0.005f64,
                day_start
            ],
        )?;
        Ok(())
    })
    .await
    .unwrap();

    // Budget is 0.001 — less than the 0.005 already spent
    let svc = build_service(db, Arc::clone(&http), clock, 0.001);
    let err = svc
        .message(simple_request("claude-sonnet-4-6"))
        .await
        .unwrap_err();

    assert!(
        matches!(err, LlmError::BudgetExhausted),
        "expected BudgetExhausted, got {err:?}"
    );
    assert_eq!(
        http.call_count(),
        0,
        "HTTP must not be called when budget exhausted"
    );
}

// ---------------- 10: kill switch resets at midnight UTC ----------------

#[tokio::test]
async fn kill_switch_resets_at_midnight_utc() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    // now = 2023-11-14 22:13:20 UTC; midnight = 1_699_920_000
    let now = 1_700_000_000i64;
    let clock = Arc::new(FixedClock::new(now));

    // Insert a row with called_at = 1_699_910_000 (yesterday 23:53:20 UTC — before today's midnight)
    let yesterday_called_at = 1_699_910_000i64;
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
                100.0f64,
                yesterday_called_at
            ],
        )?;
        Ok(())
    })
    .await
    .unwrap();

    http.enqueue_ok(text_response("ok", 10, 20));
    // budget = 1.0; yesterday's 100.0 should NOT count
    let svc = build_service(db, Arc::clone(&http), clock, 1.0);
    svc.message(simple_request("claude-sonnet-4-6"))
        .await
        .expect("should succeed — yesterday's cost is before today's midnight");

    assert_eq!(http.call_count(), 1);
}

// ---------------- 11: propagates 4xx errors ----------------

#[tokio::test]
async fn propagates_4xx_errors() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    let clock = Arc::new(FixedClock::new(1_700_000_000));
    http.enqueue_err(AnthropicHttpError::Auth);

    let svc = build_service(db, Arc::clone(&http), clock, 10.0);
    let err = svc
        .message(simple_request("claude-sonnet-4-6"))
        .await
        .unwrap_err();

    assert!(matches!(err, LlmError::Auth), "expected Auth, got {err:?}");
}

// ---------------- 12: propagates 5xx with retry disabled ----------------

#[tokio::test]
async fn propagates_5xx_with_retry_disabled() {
    let (_tmp, db) = make_db();
    let http = Arc::new(MockHttp::new());
    let clock = Arc::new(FixedClock::new(1_700_000_000));
    http.enqueue_err(AnthropicHttpError::Upstream {
        status: 500,
        body: "boom".to_string(),
    });

    let svc = build_service(db, Arc::clone(&http), clock, 10.0);
    let err = svc
        .message(simple_request("claude-sonnet-4-6"))
        .await
        .unwrap_err();

    assert!(
        matches!(err, LlmError::Upstream { status: 500, .. }),
        "expected Upstream 500, got {err:?}"
    );
    assert_eq!(http.call_count(), 1, "no retries expected");
}
