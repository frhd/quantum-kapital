# Phase 5 — `TradeReviewGenerator::generate` orchestrator

> Part of [In-app trade-review generator](master.md). See master for invariants.

**Status:** todo

**Depends on:** Phase 1 (`LlmKind::Review`), Phase 2 (prompt), Phase 3 (tool schema + parser), Phase 4 (summary).

**Goal:** A struct + method that wires the four pure modules to the IBKR seam, the LLM service, and the persisted-review store. End state: calling `TradeReviewGenerator::new(...).generate(date, "U1234567")` writes a row to `day_reviews` and returns the populated `TradeReview`.

## Files

**Modify:**
- `src-tauri/src/services/trade_reviews/generator/mod.rs` — add the orchestrator struct + `generate` method + `#[cfg(test)] mod tests`.
- `src-tauri/src/services/trade_reviews/mod.rs` — re-export `TradeReviewGenerator`, `GenerateError`.

## Design

```text
generate(date, account)
  ├── reader.executions(account, date)        → Vec<ExecutionRow>           (Phase 1 of prior plan + AccountReader seam)
  ├── if empty → Err(GenerateError::NoFills)
  ├── match_legs(&fills)                       → Vec<TradeLeg>               (services::trade_legs)
  ├── summary::summarize(&legs)                → LegSummary                  (Phase 4)
  ├── prompt::format_prompt(date, &legs, &summary) → String                  (Phase 2)
  ├── LlmRequest { kind: Review, model, system, user, tools, force_tool }
  ├── llm.message(request)                     → LlmResponse                 (LlmService — budget-gated)
  ├── find tool_call where name == TOOL_NAME or → Err(NoToolCall)
  ├── tool::parse_tool_response(&input)        → ParsedReview                (Phase 3)
  ├── TradeReviewStore::write(WriteTradeReviewRequest {...})
  └── Ok(outcome.review)
```

**Constants:**

```rust
pub const MODEL: &str = "claude-sonnet-4-6";
pub const MAX_TOKENS: u32 = 2048;
pub const SYSTEM_PROMPT: &str = include_str!("system_prompt.md");
```

The system prompt is committed verbatim in a sibling `.md` file so it diffs cleanly. Source: `agent/prompts/trade_review.md`. Edit only when bumping `PROMPT_VERSION_RUST`.

**`llm_call_id`:** v1 leaves this `None`. `LlmService::message` does not currently surface the inserted `llm_calls.id`; threading that through is out-of-scope for this phase.

## Steps

- [ ] **Step 1: Copy the system prompt verbatim from the agent.**

```bash
cp agent/prompts/trade_review.md src-tauri/src/services/trade_reviews/generator/system_prompt.md
```

Verify it's there:
```bash
ls -la src-tauri/src/services/trade_reviews/generator/system_prompt.md
```

- [ ] **Step 2: Write the failing tests at the bottom of `generator/mod.rs`.**

Replace `src-tauri/src/services/trade_reviews/generator/mod.rs` with:

```rust
//! In-app trade-review generator.
//!
//! Pulls a day's fills, FIFO-matches them, computes the leg summary,
//! asks Claude to pick behavioral tags + write a narrative through a
//! forced `submit_trade_review` tool call, and persists via
//! `TradeReviewStore::write` (which computes the grade deterministically).
//!
//! No sidecar — this is the in-app counterpart to
//! `agent/eod_review.py`. The Python loop continues to serve cron- and
//! /eod-review-driven flows; both paths persist into the same
//! `day_reviews` table.

#![allow(dead_code)] // wiring lands in Phase 6.

pub mod prompt;
pub mod summary;
pub mod tool;

use std::sync::Arc;

use chrono::NaiveDate;
use thiserror::Error;
use tracing::warn;

use crate::ibkr::error::IbkrError;
use crate::mcp::ibkr_seam::AccountReader;
use crate::services::llm_service::{
    LlmError, LlmKind, LlmRequest, LlmService, Message, Role, SystemBlock, ToolChoice,
};
use crate::services::trade_legs::match_legs;
use crate::services::trade_reviews::store::{TradeReviewError, TradeReviewStore};
use crate::services::trade_reviews::types::{TradeReview, WriteTradeReviewRequest};
use crate::storage::Db;

pub const PROMPT_VERSION_RUST: i32 = 1;
pub const MODEL: &str = "claude-sonnet-4-6";
pub const MAX_TOKENS: u32 = 2048;
const SYSTEM_PROMPT: &str = include_str!("system_prompt.md");

#[derive(Error, Debug)]
pub enum GenerateError {
    #[error("no fills found for {date} on account {account}")]
    NoFills { date: NaiveDate, account: String },
    #[error("ibkr seam: {0}")]
    Reader(#[from] IbkrError),
    #[error("llm: {0}")]
    Llm(#[from] LlmError),
    #[error("LLM did not return a `submit_trade_review` tool call")]
    NoToolCall,
    #[error("parse: {0}")]
    Parse(#[from] tool::ParseError),
    #[error("storage: {0}")]
    Store(#[from] TradeReviewError),
}

#[derive(Clone)]
pub struct TradeReviewGenerator {
    llm: Arc<LlmService>,
    reader: Arc<dyn AccountReader>,
    db: Arc<Db>,
}

impl TradeReviewGenerator {
    pub fn new(llm: Arc<LlmService>, reader: Arc<dyn AccountReader>, db: Arc<Db>) -> Self {
        Self { llm, reader, db }
    }

    pub async fn generate(
        &self,
        date: NaiveDate,
        account: &str,
    ) -> Result<TradeReview, GenerateError> {
        // Implemented in Step 4.
        let _ = (date, account);
        Err(GenerateError::NoFills {
            date,
            account: account.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ibkr::error::Result as IbkrResult;
    use crate::ibkr::types::{AccountSummary, ExecutionSide, Position};
    use crate::mcp::tools::executions::ExecutionRow;
    use crate::services::llm_service::{
        AnthropicHttp, AnthropicHttpError, LlmClock, LlmService,
    };
    use async_trait::async_trait;
    use chrono::{DateTime, TimeZone, Utc};
    use serde_json::{json, Value};
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;

    // ---------- Fakes ----------

    struct FakeReader {
        rows: Mutex<Vec<ExecutionRow>>,
        err: Mutex<Option<IbkrError>>,
    }

    impl FakeReader {
        fn with_rows(rows: Vec<ExecutionRow>) -> Self {
            Self {
                rows: Mutex::new(rows),
                err: Mutex::new(None),
            }
        }
    }

    #[async_trait]
    impl AccountReader for FakeReader {
        async fn list_accounts(&self) -> IbkrResult<Vec<String>> {
            Ok(vec!["U1".into()])
        }
        async fn get_positions(&self, _account: &str) -> IbkrResult<Vec<Position>> {
            Ok(vec![])
        }
        async fn get_account_summary(
            &self,
            _account: &str,
        ) -> IbkrResult<Vec<AccountSummary>> {
            Ok(vec![])
        }
        async fn executions(
            &self,
            _account: &str,
            _date: NaiveDate,
        ) -> IbkrResult<Vec<ExecutionRow>> {
            if let Some(e) = self.err.lock().unwrap().take() {
                return Err(e);
            }
            Ok(self.rows.lock().unwrap().clone())
        }
    }

    #[derive(Default)]
    struct MockHttp {
        canned: Mutex<VecDeque<Result<Value, AnthropicHttpError>>>,
        last_body: Mutex<Option<Value>>,
    }
    impl MockHttp {
        fn enqueue_ok(&self, v: Value) {
            self.canned.lock().unwrap().push_back(Ok(v));
        }
        fn last_body(&self) -> Option<Value> {
            self.last_body.lock().unwrap().clone()
        }
    }
    #[async_trait]
    impl AnthropicHttp for MockHttp {
        async fn send_messages(
            &self,
            _api_key: &str,
            _anthropic_version: &str,
            body: &Value,
        ) -> Result<Value, AnthropicHttpError> {
            *self.last_body.lock().unwrap() = Some(body.clone());
            self.canned
                .lock()
                .unwrap()
                .pop_front()
                .expect("MockHttp queue exhausted")
        }
    }

    struct FixedClock(i64);
    impl LlmClock for FixedClock {
        fn now_unix(&self) -> i64 {
            self.0
        }
    }

    fn make_db() -> (NamedTempFile, Arc<Db>) {
        let tmp = NamedTempFile::new().expect("tempfile");
        let db = Arc::new(Db::open(tmp.path()).expect("open db"));
        (tmp, db)
    }

    fn dt(h: u32, m: u32) -> DateTime<Utc> {
        chrono::Utc.with_ymd_and_hms(2026, 5, 4, h, m, 0).unwrap()
    }

    fn row(
        id: &str,
        side: ExecutionSide,
        symbol: &str,
        qty: f64,
        price: f64,
        commission: Option<f64>,
        time_h: u32,
        time_m: u32,
        order_id: i32,
    ) -> ExecutionRow {
        ExecutionRow {
            exec_id: id.into(),
            account: "U1".into(),
            symbol: symbol.into(),
            contract_type: "STK".into(),
            expiry: None,
            strike: None,
            right: None,
            multiplier: None,
            side,
            qty,
            avg_price: price,
            currency: Some("USD".into()),
            time: dt(time_h, time_m),
            order_id,
            commission,
            realized_pnl: None,
            commission_currency: Some("USD".into()),
        }
    }

    fn anthropic_envelope_with_tool_call(tool_input: Value) -> Value {
        json!({
            "id": "msg_01",
            "type": "message",
            "role": "assistant",
            "model": MODEL,
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 100, "output_tokens": 50},
            "content": [
                {
                    "type": "tool_use",
                    "id": "toolu_01",
                    "name": tool::TOOL_NAME,
                    "input": tool_input,
                }
            ]
        })
    }

    fn anthropic_envelope_text_only() -> Value {
        json!({
            "id": "msg_02",
            "type": "message",
            "role": "assistant",
            "model": MODEL,
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 100, "output_tokens": 50},
            "content": [{"type": "text", "text": "I refuse to call the tool."}]
        })
    }

    fn build_generator(
        db: Arc<Db>,
        rows: Vec<ExecutionRow>,
        http: Arc<MockHttp>,
    ) -> TradeReviewGenerator {
        let llm = Arc::new(
            LlmService::new("k".to_string(), Arc::clone(&db), 5.0)
                .with_http(http as Arc<dyn AnthropicHttp>)
                .with_clock(Arc::new(FixedClock(1_700_000_000))),
        );
        let reader: Arc<dyn AccountReader> = Arc::new(FakeReader::with_rows(rows));
        TradeReviewGenerator::new(llm, reader, db)
    }

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 5, 4).unwrap()
    }

    // ---------- Tests ----------

    #[tokio::test]
    async fn empty_day_returns_no_fills_error() {
        let (_tmp, db) = make_db();
        let http = Arc::new(MockHttp::default()); // must not be called
        let gen = build_generator(db, vec![], http);
        let err = gen.generate(date(), "U1").await.expect_err("no fills");
        match err {
            GenerateError::NoFills { account, .. } => assert_eq!(account, "U1"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn happy_path_writes_row_and_returns_populated_review() {
        let (_tmp, db) = make_db();
        let rows = vec![
            row("e1", ExecutionSide::Bought, "AAPL", 100.0, 200.0, Some(1.0), 14, 0, 1),
            row("e2", ExecutionSide::Sold, "AAPL", 100.0, 202.5, Some(1.0), 15, 0, 2),
        ];
        let http = Arc::new(MockHttp::default());
        http.enqueue_ok(anthropic_envelope_with_tool_call(json!({
            "behavioral_tags": ["flat_close", "discipline_on_loser"],
            "leg_observations": [],
            "narrative_md": "A clean round-trip on AAPL with disciplined entry and exit."
        })));

        let gen = build_generator(Arc::clone(&db), rows, Arc::clone(&http));
        let review = gen.generate(date(), "U1").await.expect("ok");
        assert_eq!(review.account, "U1");
        assert_eq!(review.prompt_version, PROMPT_VERSION_RUST);
        assert_eq!(review.behavioral_tags.len(), 2);
        assert!(review.narrative_md.contains("AAPL"));

        // Persisted.
        let store = TradeReviewStore::new(Arc::clone(&db));
        let row = store
            .read(date(), "U1", PROMPT_VERSION_RUST)
            .await
            .unwrap()
            .expect("row");
        assert_eq!(row.behavioral_tags.len(), 2);

        // Forced-tool was set + system prompt was sent.
        let body = http.last_body().expect("called");
        assert_eq!(body["tool_choice"]["name"], tool::TOOL_NAME);
        let sys = body["system"][0]["text"].as_str().expect("system text");
        assert!(sys.starts_with("You are an equity research analyst"));
    }

    #[tokio::test]
    async fn no_tool_call_returns_no_tool_call_error() {
        let (_tmp, db) = make_db();
        let rows = vec![
            row("e1", ExecutionSide::Bought, "X", 10.0, 1.0, Some(0.1), 14, 0, 1),
            row("e2", ExecutionSide::Sold, "X", 10.0, 1.5, Some(0.1), 15, 0, 2),
        ];
        let http = Arc::new(MockHttp::default());
        http.enqueue_ok(anthropic_envelope_text_only());

        let gen = build_generator(db, rows, http);
        let err = gen.generate(date(), "U1").await.expect_err("no tool call");
        assert!(matches!(err, GenerateError::NoToolCall));
    }

    #[tokio::test]
    async fn malformed_tool_input_returns_parse_error() {
        let (_tmp, db) = make_db();
        let rows = vec![
            row("e1", ExecutionSide::Bought, "X", 10.0, 1.0, Some(0.1), 14, 0, 1),
            row("e2", ExecutionSide::Sold, "X", 10.0, 1.5, Some(0.1), 15, 0, 2),
        ];
        let http = Arc::new(MockHttp::default());
        // Empty narrative trips ParseError::EmptyNarrative.
        http.enqueue_ok(anthropic_envelope_with_tool_call(json!({
            "behavioral_tags": [],
            "narrative_md": ""
        })));

        let gen = build_generator(db, rows, http);
        let err = gen.generate(date(), "U1").await.expect_err("parse error");
        assert!(matches!(err, GenerateError::Parse(_)), "got: {err:?}");
    }

    #[tokio::test]
    async fn budget_exhausted_bubbles_llm_error() {
        let (_tmp, db) = make_db();
        // Pre-poison the ledger so cost_today >= budget.
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO llm_calls (kind, setup_id, model, input_tokens, output_tokens, \
                 cache_read_tokens, cost_usd, called_at, loop_name) \
                 VALUES ('thesis', NULL, 'claude-sonnet-4-6', 0, 0, 0, 100.0, ?1, NULL)",
                rusqlite::params![1_700_000_000_i64],
            )?;
            Ok(())
        })
        .await
        .unwrap();
        let rows = vec![
            row("e1", ExecutionSide::Bought, "X", 10.0, 1.0, Some(0.1), 14, 0, 1),
            row("e2", ExecutionSide::Sold, "X", 10.0, 1.5, Some(0.1), 15, 0, 2),
        ];
        let http = Arc::new(MockHttp::default()); // must not be called
        let gen = build_generator(db, rows, http);
        let err = gen.generate(date(), "U1").await.expect_err("budget");
        assert!(matches!(err, GenerateError::Llm(LlmError::BudgetExhausted)));
    }
}
```

- [ ] **Step 3: Run the failing tests.**

Run: `cd src-tauri && cargo test --lib services::trade_reviews::generator::tests`
Expected: 5 tests, all fail (orchestrator returns `NoFills` unconditionally).

- [ ] **Step 4: Implement `generate`.**

Replace the `generate` method body in `mod.rs`:

```rust
pub async fn generate(
    &self,
    date: NaiveDate,
    account: &str,
) -> Result<TradeReview, GenerateError> {
    let fills = self.reader.executions(account, date).await?;
    if fills.is_empty() {
        return Err(GenerateError::NoFills {
            date,
            account: account.to_string(),
        });
    }

    let legs = match_legs(&fills);
    let summary = summary::summarize(&legs);
    let user_prompt = prompt::format_prompt(date, &legs, &summary);

    let request = LlmRequest {
        kind: LlmKind::Review,
        model: MODEL,
        max_tokens: MAX_TOKENS,
        system: vec![SystemBlock {
            text: SYSTEM_PROMPT.to_string(),
            cache: true,
        }],
        messages: vec![Message {
            role: Role::User,
            content: user_prompt,
        }],
        tools: Some(vec![tool::submit_trade_review_schema()]),
        tool_choice: Some(ToolChoice::ForceTool(tool::TOOL_NAME.to_string())),
        setup_id: None,
        loop_name: None,
    };

    let response = self.llm.message(request).await?;

    let tool_call = response
        .tool_calls
        .into_iter()
        .find(|c| c.name == tool::TOOL_NAME)
        .ok_or(GenerateError::NoToolCall)?;

    let parsed = match tool::parse_tool_response(&tool_call.input) {
        Ok(p) => p,
        Err(e) => {
            warn!(date = %date, account = account, "tool input failed to parse: {e}");
            return Err(e.into());
        }
    };

    let store = TradeReviewStore::new(Arc::clone(&self.db));
    let req = WriteTradeReviewRequest {
        date,
        account: account.to_string(),
        prompt_version: PROMPT_VERSION_RUST,
        summary,
        behavioral_tags: parsed.behavioral_tags,
        leg_observations: parsed.leg_observations,
        narrative_md: parsed.narrative_md,
        llm_call_id: None,
    };
    let outcome = store.write(req).await?;
    Ok(outcome.review)
}
```

- [ ] **Step 5: Re-export the orchestrator + error from the trade_reviews module.**

Edit `src-tauri/src/services/trade_reviews/mod.rs`. After the existing `pub use generator::PROMPT_VERSION_RUST;` add:

```rust
#[allow(unused_imports)]
pub use generator::{GenerateError, TradeReviewGenerator};
```

- [ ] **Step 6: Run the tests to confirm green.**

Run: `cd src-tauri && cargo test --lib services::trade_reviews::generator`
Expected: all generator tests (prompt, tool, summary, mod::tests) pass.

- [ ] **Step 7: Pre-commit gates.**

Run: `cd src-tauri && cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings`
Expected: clean.

- [ ] **Step 8: Commit.**

```bash
git add src-tauri/src/services/trade_reviews/
git commit -m "$(cat <<'EOF'
feat(trade-reviews): TradeReviewGenerator orchestrator

Wires AccountReader (executions seam) + match_legs + summarize +
format_prompt + submit_trade_review tool call (forced via LlmService) +
parse_tool_response + TradeReviewStore::write into a single
generate(date, account) method. No sidecar.

Tests cover: empty day → NoFills; happy path round-trip → row
written + populated review returned; LLM omits tool call → NoToolCall;
LLM returns malformed input → Parse; ledger pre-poisoned past budget
→ Llm(BudgetExhausted).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```
