//! Phase 7 — Tauri commands for the assessment stack.
//!
//! These commands are the FE-facing counterparts to the
//! `get_trade_review`, `get_today_playbook`, and `get_trader_profile`
//! MCP read tools. They wrap the same underlying services so the desktop
//! UI and an MCP client see byte-identical artifacts.
//!
//! Account resolution mirrors the MCP tools: optional `account` arg
//! defaults to the sole managed account; multi-account without an
//! explicit choice surfaces the available IDs.

use std::sync::Arc;

use chrono::NaiveDate;
use tauri::State;

use crate::mcp::ibkr_seam::AccountReader;
use crate::mcp::tools::resolve_account;
use crate::services::playbooks::{Playbook, PlaybookStore};
use crate::services::trade_reviews::{
    GenerateError, TradeReview, TradeReviewGenerator, TradeReviewStore,
};
use crate::services::trader_profile::{aggregate, TraderProfile};
use crate::storage::Db;

const TRADER_PROFILE_DEFAULT_WINDOW_DAYS: u32 = 30;
const TRADER_PROFILE_MIN_WINDOW_DAYS: u32 = 1;
const TRADER_PROFILE_MAX_WINDOW_DAYS: u32 = 365;

pub(crate) async fn fetch_trade_review(
    reader: &dyn AccountReader,
    db: &Arc<Db>,
    account: Option<&str>,
    date: &str,
    prompt_version: Option<i32>,
) -> Result<Option<TradeReview>, String> {
    let parsed = NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|e| format!("invalid date '{date}', expected YYYY-MM-DD: {e}"))?;
    let resolved = resolve_account(reader, account).await?;
    let store = TradeReviewStore::new(Arc::clone(db));
    let outcome = match prompt_version {
        Some(v) => store.read(parsed, &resolved, v).await,
        None => store.read_latest(parsed, &resolved).await,
    };
    outcome.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_trade_review(
    reader: State<'_, Arc<dyn AccountReader>>,
    db: State<'_, Arc<Db>>,
    date: String,
    account: Option<String>,
    prompt_version: Option<i32>,
) -> Result<Option<TradeReview>, String> {
    fetch_trade_review(
        reader.inner().as_ref(),
        db.inner(),
        account.as_deref(),
        &date,
        prompt_version,
    )
    .await
}

pub(crate) async fn fetch_today_playbook(
    reader: &dyn AccountReader,
    db: &Arc<Db>,
    account: Option<&str>,
    date: &str,
    generation_id: Option<i32>,
) -> Result<Option<Playbook>, String> {
    let parsed = NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|e| format!("invalid date '{date}', expected YYYY-MM-DD: {e}"))?;
    let resolved = resolve_account(reader, account).await?;
    let store = PlaybookStore::new(Arc::clone(db));
    let outcome = match generation_id {
        Some(g) => store.read_generation(parsed, &resolved, g).await,
        None => store.read_latest(parsed, &resolved).await,
    };
    outcome.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_today_playbook(
    reader: State<'_, Arc<dyn AccountReader>>,
    db: State<'_, Arc<Db>>,
    date: String,
    account: Option<String>,
    generation_id: Option<i32>,
) -> Result<Option<Playbook>, String> {
    fetch_today_playbook(
        reader.inner().as_ref(),
        db.inner(),
        account.as_deref(),
        &date,
        generation_id,
    )
    .await
}

pub(crate) async fn fetch_trader_profile(
    reader: &dyn AccountReader,
    db: &Arc<Db>,
    account: Option<&str>,
    window_days: Option<u32>,
) -> Result<TraderProfile, String> {
    let resolved = resolve_account(reader, account).await?;
    let window = window_days
        .unwrap_or(TRADER_PROFILE_DEFAULT_WINDOW_DAYS)
        .clamp(
            TRADER_PROFILE_MIN_WINDOW_DAYS,
            TRADER_PROFILE_MAX_WINDOW_DAYS,
        );
    aggregate(db, &resolved, window)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_trader_profile(
    reader: State<'_, Arc<dyn AccountReader>>,
    db: State<'_, Arc<Db>>,
    account: Option<String>,
    window_days: Option<u32>,
) -> Result<TraderProfile, String> {
    fetch_trader_profile(
        reader.inner().as_ref(),
        db.inner(),
        account.as_deref(),
        window_days,
    )
    .await
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ibkr::mocks::MockIbkrClient;
    use crate::mcp::tools::test_support::make_db;
    use crate::services::playbooks::{
        Conviction, RankedSetup, SetupBias, SkipEntry, WritePlaybookRequest,
    };
    use crate::services::trade_reviews::{BehavioralTag, LegSummary, WriteTradeReviewRequest};
    use std::sync::Arc;

    async fn make_reader_with_account(account: &str) -> Arc<dyn AccountReader> {
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec![account.to_string()]).await;
        mock.set_connected(true).await;
        mock as Arc<dyn AccountReader>
    }

    fn sample_review_request(
        date: NaiveDate,
        account: &str,
        prompt_version: i32,
    ) -> WriteTradeReviewRequest {
        WriteTradeReviewRequest {
            date,
            account: account.into(),
            prompt_version,
            summary: LegSummary {
                gross_pnl: 100.0,
                net_pnl: 90.0,
                commissions_total: 10.0,
                n_round_trips: 1,
                n_carryover: 0,
                win_rate: Some(1.0),
                by_symbol: Default::default(),
            },
            behavioral_tags: vec![BehavioralTag::FlatClose],
            leg_observations: vec![],
            narrative_md: format!("v{prompt_version}"),
            llm_call_id: None,
        }
    }

    fn sample_playbook_request(date: NaiveDate, account: &str) -> WritePlaybookRequest {
        WritePlaybookRequest {
            date,
            account: account.into(),
            ranked_setups: vec![RankedSetup {
                symbol: "AAPL".into(),
                bias: SetupBias::Long,
                trigger: "trigger".into(),
                entry: "100".into(),
                invalidation: "lose 95".into(),
                target_1: "110".into(),
                target_2: None,
                conviction: Conviction::B,
                rationale_md: "thesis".into(),
                evidence_refs: vec![],
            }],
            skip_list: vec![SkipEntry {
                symbol: "TSLA".into(),
                reason: "no edge".into(),
            }],
            llm_call_id: None,
        }
    }

    #[tokio::test]
    async fn fetch_trade_review_returns_persisted_row() {
        let (_tmp, db) = make_db();
        let store = TradeReviewStore::new(Arc::clone(&db));
        let date = NaiveDate::from_ymd_opt(2026, 5, 4).unwrap();
        store
            .write(
                sample_review_request(date, "U1", 1),
                crate::services::trade_reviews::ReviewV2Fields::v1_only(),
            )
            .await
            .unwrap();
        let reader = make_reader_with_account("U1").await;

        let review = fetch_trade_review(reader.as_ref(), &db, None, "2026-05-04", Some(1))
            .await
            .expect("ok");
        let review = review.expect("row");
        assert_eq!(review.account, "U1");
        assert_eq!(review.prompt_version, 1);
        assert_eq!(review.narrative_md, "v1");
    }

    #[tokio::test]
    async fn fetch_trade_review_absent_returns_none() {
        let (_tmp, db) = make_db();
        let reader = make_reader_with_account("U1").await;
        let review = fetch_trade_review(reader.as_ref(), &db, None, "2026-05-04", None)
            .await
            .expect("ok");
        assert!(review.is_none());
    }

    #[tokio::test]
    async fn fetch_trade_review_invalid_date_errors() {
        let (_tmp, db) = make_db();
        let reader = make_reader_with_account("U1").await;
        let err = fetch_trade_review(reader.as_ref(), &db, None, "garbage", None)
            .await
            .expect_err("invalid date");
        assert!(err.contains("YYYY-MM-DD"), "got: {err}");
    }

    #[tokio::test]
    async fn fetch_today_playbook_returns_latest_generation() {
        let (_tmp, db) = make_db();
        let store = PlaybookStore::new(Arc::clone(&db));
        let date = NaiveDate::from_ymd_opt(2026, 5, 5).unwrap();
        store
            .write(sample_playbook_request(date, "U1"))
            .await
            .unwrap();
        store
            .write(sample_playbook_request(date, "U1"))
            .await
            .unwrap();
        let reader = make_reader_with_account("U1").await;

        let pb = fetch_today_playbook(reader.as_ref(), &db, None, "2026-05-05", None)
            .await
            .expect("ok")
            .expect("row");
        assert_eq!(pb.generation_id, 2);
        assert_eq!(pb.ranked_setups.len(), 1);
        assert_eq!(pb.skip_list[0].symbol, "TSLA");
    }

    #[tokio::test]
    async fn fetch_today_playbook_absent_returns_none() {
        let (_tmp, db) = make_db();
        let reader = make_reader_with_account("U1").await;
        let pb = fetch_today_playbook(reader.as_ref(), &db, None, "2026-05-05", None)
            .await
            .expect("ok");
        assert!(pb.is_none());
    }

    #[tokio::test]
    async fn fetch_trader_profile_returns_zero_review_envelope_when_empty() {
        let (_tmp, db) = make_db();
        let reader = make_reader_with_account("U1").await;
        let profile = fetch_trader_profile(reader.as_ref(), &db, None, Some(30))
            .await
            .expect("ok");
        assert_eq!(profile.account, "U1");
        assert_eq!(profile.window_days, 30);
        assert_eq!(profile.n_reviews, 0);
        assert!(profile.tag_frequencies.is_empty());
    }

    #[tokio::test]
    async fn fetch_trader_profile_clamps_window_days() {
        let (_tmp, db) = make_db();
        let reader = make_reader_with_account("U1").await;
        let p = fetch_trader_profile(reader.as_ref(), &db, None, Some(0))
            .await
            .expect("ok");
        assert_eq!(p.window_days, TRADER_PROFILE_MIN_WINDOW_DAYS);
        let p = fetch_trader_profile(reader.as_ref(), &db, None, Some(10_000))
            .await
            .expect("ok");
        assert_eq!(p.window_days, TRADER_PROFILE_MAX_WINDOW_DAYS);
    }

    #[tokio::test]
    async fn fetch_trader_profile_default_window_when_omitted() {
        let (_tmp, db) = make_db();
        let reader = make_reader_with_account("U1").await;
        let p = fetch_trader_profile(reader.as_ref(), &db, None, None)
            .await
            .expect("ok");
        assert_eq!(p.window_days, TRADER_PROFILE_DEFAULT_WINDOW_DAYS);
    }

    #[tokio::test]
    async fn fetch_trader_profile_multi_account_without_arg_errors() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["U1".into(), "U2".into()]).await;
        mock.set_connected(true).await;
        let reader: Arc<dyn AccountReader> = mock as Arc<dyn AccountReader>;
        let err = fetch_trader_profile(reader.as_ref(), &db, None, None)
            .await
            .expect_err("multi-account without arg");
        assert!(err.contains("U1") && err.contains("U2"), "got: {err}");
    }

    // ---------------- generate_trade_review ----------------

    use crate::ibkr::types::{ExecutionSide, IbkrExecution};
    use crate::services::llm_service::{AnthropicHttp, AnthropicHttpError, LlmClock, LlmService};
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

    async fn reader_with_fills(
        account: &str,
        fills: Vec<IbkrExecution>,
    ) -> (Arc<MockIbkrClient>, Arc<dyn AccountReader>) {
        let mock = Arc::new(MockIbkrClient::new());
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

        let res = fetch_generate_trade_review(reader.as_ref(), &generator, None, "2026-05-04")
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

        let res = fetch_generate_trade_review(reader.as_ref(), &generator, None, "2026-05-04")
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
        let err = fetch_generate_trade_review(reader.as_ref(), &generator, None, "garbage")
            .await
            .expect_err("invalid date");
        assert!(err.contains("YYYY-MM-DD"), "got: {err}");
    }
}
