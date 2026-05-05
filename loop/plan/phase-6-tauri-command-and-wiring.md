# Phase 6 — `generate_trade_review` Tauri command + service wiring

> Part of [In-app trade-review generator](master.md). See master for invariants.

**Status:** in-progress (started 2026-05-05)

**Depends on:** Phase 5 (the orchestrator).

**Goal:** Construct `TradeReviewGenerator` in `lib.rs::run`, hand it to the Tauri runtime via `app.manage`, and add a `#[tauri::command] generate_trade_review(date, account?)` that the frontend can call. Account resolution mirrors the existing `get_trade_review`.

## Files

**Modify:**
- `src-tauri/src/lib.rs` — construct + manage `Arc<TradeReviewGenerator>`; register the new command in `tauri::generate_handler!`.
- `src-tauri/src/ibkr/commands/assessments.rs` — add `generate_trade_review` plus a `fetch_generate_trade_review` helper for unit tests.

## API

```rust
#[tauri::command]
pub async fn generate_trade_review(
    reader: State<'_, Arc<dyn AccountReader>>,
    generator: State<'_, Arc<TradeReviewGenerator>>,
    date: String,
    account: Option<String>,
) -> Result<Option<TradeReview>, String>;
```

`Option<TradeReview>` so the Phase 5 `NoFills` error renders as `Ok(None)` (UI shows a "no fills" state, not a red error). All other errors stringify and bubble.

## Steps

- [ ] **Step 1: Write the failing tests.**

In `src-tauri/src/ibkr/commands/assessments.rs`, add to the existing `#[cfg(test)] mod tests`:

```rust
// (Inside the existing `mod tests` block — alongside the other helpers.)

use crate::ibkr::types::{ExecutionSide, IbkrExecution};
use crate::services::llm_service::{
    AnthropicHttp, AnthropicHttpError, LlmClock, LlmService,
};
use crate::services::trade_reviews::{TradeReviewGenerator, PROMPT_VERSION_RUST};
use async_trait::async_trait;
use chrono::TimeZone;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::Mutex;

#[derive(Default)]
struct EnqueuingHttp {
    canned: Mutex<VecDeque<Result<Value, AnthropicHttpError>>>,
}
impl EnqueuingHttp {
    fn enqueue_ok(&self, v: Value) {
        self.canned.lock().unwrap().push_back(Ok(v));
    }
}
#[async_trait]
impl AnthropicHttp for EnqueuingHttp {
    async fn send_messages(
        &self,
        _api_key: &str,
        _anthropic_version: &str,
        _body: &Value,
    ) -> Result<Value, AnthropicHttpError> {
        self.canned
            .lock()
            .unwrap()
            .pop_front()
            .expect("queue exhausted")
    }
}

struct FixedClock(i64);
impl LlmClock for FixedClock {
    fn now_unix(&self) -> i64 {
        self.0
    }
}

/// Build an IbkrExecution timestamped during ET market hours on 2026-05-04
/// so the seam's `executions(account, date)` returns it.
fn ibkr_fill(
    id: &str,
    side: ExecutionSide,
    qty: f64,
    price: f64,
    time_h_utc: u32,
    order_id: i32,
) -> IbkrExecution {
    IbkrExecution {
        exec_id: id.into(),
        account: "U1".into(),
        symbol: "AAPL".into(),
        contract_type: "STK".into(),
        expiry: None,
        strike: None,
        right: None,
        multiplier: None,
        side,
        qty,
        avg_price: price,
        currency: Some("USD".into()),
        // 14:00–15:00 UTC ≈ 10:00–11:00 ET on 2026-05-04 (DST → -4).
        exec_time: chrono::Utc
            .with_ymd_and_hms(2026, 5, 4, time_h_utc, 0, 0)
            .unwrap(),
        order_id,
        commission: Some(0.5),
        realized_pnl: None,
        commission_currency: Some("USD".into()),
    }
}

/// Build a `MockIbkrClient` that lists `account`, is connected, and serves
/// `fills` via the seam's `executions(account, date)` (filtered + projected
/// to `ExecutionRow` by `MockIbkrClient`'s `AccountReader` impl).
async fn reader_with_fills(
    account: &str,
    fills: Vec<IbkrExecution>,
) -> (Arc<crate::ibkr::mocks::MockIbkrClient>, Arc<dyn AccountReader>) {
    let mock = Arc::new(crate::ibkr::mocks::MockIbkrClient::new());
    mock.set_accounts(vec![account.into()]).await;
    mock.set_connected(true).await;
    mock.set_executions(fills).await;
    let reader: Arc<dyn AccountReader> = Arc::clone(&mock) as Arc<dyn AccountReader>;
    (mock, reader)
}

#[tokio::test]
async fn fetch_generate_trade_review_writes_row_and_returns_review() {
    let (_tmp, db) = make_db();
    let fills = vec![
        ibkr_fill("e1", ExecutionSide::Bought, 100.0, 200.0, 14, 1),
        ibkr_fill("e2", ExecutionSide::Sold, 100.0, 202.0, 15, 2),
    ];
    let (_mock, reader) = reader_with_fills("U1", fills).await;

    let http = Arc::new(EnqueuingHttp::default());
    http.enqueue_ok(json!({
        "id": "msg_01",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4-6",
        "stop_reason": "tool_use",
        "usage": {"input_tokens": 100, "output_tokens": 50},
        "content": [{
            "type": "tool_use",
            "id": "toolu_01",
            "name": "submit_trade_review",
            "input": {"behavioral_tags": ["flat_close"], "narrative_md": "Decent."}
        }]
    }));

    let llm = Arc::new(
        LlmService::new("k".into(), Arc::clone(&db), 5.0)
            .with_http(http as Arc<dyn AnthropicHttp>)
            .with_clock(Arc::new(FixedClock(1_700_000_000))),
    );
    let generator = Arc::new(TradeReviewGenerator::new(
        llm,
        Arc::clone(&reader),
        Arc::clone(&db),
    ));

    let res = fetch_generate_trade_review(
        reader.as_ref(),
        &generator,
        None,
        "2026-05-04",
    )
    .await
    .expect("ok");
    let review = res.expect("Some(review)");
    assert_eq!(review.account, "U1");
    assert_eq!(review.prompt_version, PROMPT_VERSION_RUST);
    assert!(review.narrative_md.starts_with("Decent"));
}

#[tokio::test]
async fn fetch_generate_trade_review_no_fills_returns_none() {
    let (_tmp, db) = make_db();
    let (_mock, reader) = reader_with_fills("U1", vec![]).await;
    let http = Arc::new(EnqueuingHttp::default()); // must not be called
    let llm = Arc::new(
        LlmService::new("k".into(), Arc::clone(&db), 5.0)
            .with_http(http as Arc<dyn AnthropicHttp>)
            .with_clock(Arc::new(FixedClock(1_700_000_000))),
    );
    let generator = Arc::new(TradeReviewGenerator::new(
        llm,
        Arc::clone(&reader),
        Arc::clone(&db),
    ));

    let res = fetch_generate_trade_review(
        reader.as_ref(),
        &generator,
        None,
        "2026-05-04",
    )
    .await
    .expect("ok");
    assert!(res.is_none());
}

#[tokio::test]
async fn fetch_generate_trade_review_invalid_date_errors() {
    let (_tmp, db) = make_db();
    let (_mock, reader) = reader_with_fills("U1", vec![]).await;
    let http = Arc::new(EnqueuingHttp::default());
    let llm = Arc::new(
        LlmService::new("k".into(), Arc::clone(&db), 5.0)
            .with_http(http as Arc<dyn AnthropicHttp>)
            .with_clock(Arc::new(FixedClock(1_700_000_000))),
    );
    let generator = Arc::new(TradeReviewGenerator::new(
        llm,
        Arc::clone(&reader),
        Arc::clone(&db),
    ));
    let err = fetch_generate_trade_review(
        reader.as_ref(),
        &generator,
        None,
        "garbage",
    )
    .await
    .expect_err("invalid date");
    assert!(err.contains("YYYY-MM-DD"), "got: {err}");
}
```

> **Sanity-check the mock surface before writing tests:** `grep -n "fn set_accounts\|fn set_executions\|fn set_connected" src-tauri/src/ibkr/mocks.rs`. The above scaffolding assumes all three exist. They do today — but if the engineer is reading this stale, confirm before relying on them.

- [ ] **Step 2: Run the failing tests.**

Run: `cd src-tauri && cargo test --lib ibkr::commands::assessments::tests::fetch_generate_trade_review`
Expected: all 3 tests fail (no `fetch_generate_trade_review` symbol, no `generate_trade_review` command).

- [ ] **Step 3: Implement the helper + the Tauri command.**

In `src-tauri/src/ibkr/commands/assessments.rs`, after the existing `fetch_trader_profile` add:

```rust
use crate::services::trade_reviews::{GenerateError, TradeReviewGenerator};

pub(crate) async fn fetch_generate_trade_review(
    reader: &dyn AccountReader,
    generator: &Arc<TradeReviewGenerator>,
    account: Option<&str>,
    date: &str,
) -> Result<Option<TradeReview>, String> {
    let parsed = NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|e| format!("invalid date '{date}', expected YYYY-MM-DD: {e}"))?;
    let resolved = resolve_account(reader, account).await?;
    match generator.generate(parsed, &resolved).await {
        Ok(review) => Ok(Some(review)),
        Err(GenerateError::NoFills { .. }) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn generate_trade_review(
    reader: State<'_, Arc<dyn AccountReader>>,
    generator: State<'_, Arc<TradeReviewGenerator>>,
    date: String,
    account: Option<String>,
) -> Result<Option<TradeReview>, String> {
    fetch_generate_trade_review(
        reader.inner().as_ref(),
        generator.inner(),
        account.as_deref(),
        &date,
    )
    .await
}
```

- [ ] **Step 4: Wire the generator in `lib.rs::run`.**

Find the block where `account_reader: Arc<dyn AccountReader>` is constructed (around line ~470). Just below it, add:

```rust
let trade_review_generator = Arc::new(
    crate::services::trade_reviews::TradeReviewGenerator::new(
        Arc::clone(&llm_service),
        Arc::clone(&account_reader),
        Arc::clone(&db),
    ),
);
```

In the `app.manage` block (around line 517+), add:

```rust
app.manage(trade_review_generator);
```

In the `tauri::generate_handler![...]` block (around line 549), add to the assessment row:

```rust
            ibkr::commands::generate_trade_review,
```

(Place it adjacent to `get_trade_review` for readability.)

- [ ] **Step 5: Run the tests to confirm green.**

Run: `cd src-tauri && cargo test --lib ibkr::commands::assessments`
Expected: all assessment tests pass (existing + the 3 new ones).

- [ ] **Step 6: Compile-check the full crate.**

Run: `cd src-tauri && cargo check --all-targets`
Expected: clean. (`tauri::generate_handler!` will catch a missing `pub` or wrong signature.)

- [ ] **Step 7: Pre-commit gates.**

Run: `cd src-tauri && cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings`
Expected: clean.

- [ ] **Step 8: Commit.**

```bash
git add src-tauri/src/lib.rs src-tauri/src/ibkr/commands/assessments.rs
git commit -m "$(cat <<'EOF'
feat(tauri): generate_trade_review command

Wires TradeReviewGenerator in lib.rs::run and exposes it via a
Tauri command that mirrors get_trade_review's account-resolution
shape. Result<Option<TradeReview>, String> — empty day surfaces
as Ok(None) so the UI can render a "no fills" state distinct from
a real error.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```
