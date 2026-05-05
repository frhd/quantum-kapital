//! Integration tests for `ExecutionsStore`. Uses an in-memory SQLite Db
//! built via the existing `test_support::make_db()` helper.

use std::sync::Arc;

use chrono::{NaiveDate, TimeZone, Utc};

use super::ingest::ExecutionsIngestor;
use super::store::ExecutionsStore;
use crate::ibkr::mocks::MockIbkrClient;
use crate::ibkr::types::{ExecutionSide, IbkrExecution};
use crate::mcp::tools::test_support::make_db;

fn stk(exec_id: &str, account: &str, qty: f64, price: f64) -> IbkrExecution {
    IbkrExecution {
        symbol: "TSLA".to_string(),
        side: ExecutionSide::Bought,
        qty,
        avg_price: price,
        exec_time: Utc.with_ymd_and_hms(2026, 5, 4, 14, 30, 0).unwrap(),
        order_id: 1,
        exec_id: exec_id.to_string(),
        account: account.to_string(),
        contract_type: "STK".to_string(),
        expiry: None,
        strike: None,
        right: None,
        multiplier: None,
        commission: Some(0.65),
        realized_pnl: None,
        currency: Some("USD".to_string()),
        commission_currency: Some("USD".to_string()),
    }
}

#[tokio::test]
async fn store_upserts_idempotently() {
    let (_tmp, db) = make_db();
    let store = ExecutionsStore::new(Arc::clone(&db));
    let row = stk("E1", "DU123", 100.0, 250.0);

    store
        .record(std::slice::from_ref(&row))
        .await
        .expect("first record");
    store
        .record(std::slice::from_ref(&row))
        .await
        .expect("second record");

    let rows = store
        .query("DU123", NaiveDate::from_ymd_opt(2026, 5, 4).unwrap(), None)
        .await
        .expect("query ok");
    assert_eq!(rows.len(), 1, "expected 1 row, got {}", rows.len());
}

#[tokio::test]
async fn store_patches_commission_on_late_arrival() {
    let (_tmp, db) = make_db();
    let store = ExecutionsStore::new(Arc::clone(&db));
    let mut row = stk("E2", "DU123", 100.0, 250.0);
    row.commission = None;
    row.realized_pnl = None;
    store.record(&[row.clone()]).await.unwrap();

    // Late report arrives.
    row.commission = Some(0.99);
    row.realized_pnl = Some(42.5);
    let summary = store.record(&[row]).await.unwrap();
    assert_eq!(summary.commission_patched, 1);
    assert_eq!(summary.inserted, 0);

    let rows = store
        .query("DU123", NaiveDate::from_ymd_opt(2026, 5, 4).unwrap(), None)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].commission, Some(0.99));
    assert_eq!(rows[0].realized_pnl, Some(42.5));
}

#[tokio::test]
async fn store_does_not_overwrite_populated_commission() {
    let (_tmp, db) = make_db();
    let store = ExecutionsStore::new(Arc::clone(&db));
    let mut row = stk("E3", "DU123", 100.0, 250.0);
    row.commission = Some(0.65);
    store.record(&[row.clone()]).await.unwrap();

    row.commission = Some(0.99); // would clobber if not protected
    let summary = store.record(&[row]).await.unwrap();
    assert_eq!(summary.skipped_redundant, 1);
    assert_eq!(summary.commission_patched, 0);

    let rows = store
        .query("DU123", NaiveDate::from_ymd_opt(2026, 5, 4).unwrap(), None)
        .await
        .unwrap();
    assert_eq!(rows[0].commission, Some(0.65));
}

#[tokio::test]
async fn store_query_filters_by_et_date_across_utc_midnight() {
    let (_tmp, db) = make_db();
    let store = ExecutionsStore::new(Arc::clone(&db));

    // 23:59 ET on 2026-05-04 (EDT, UTC-4) ⇒ 03:59 UTC on 2026-05-05.
    let mut late_on_4 = stk("LATE", "DU123", 1.0, 1.0);
    late_on_4.exec_time = Utc.with_ymd_and_hms(2026, 5, 5, 3, 59, 0).unwrap();

    // 00:01 ET on 2026-05-05 (EDT) ⇒ 04:01 UTC on 2026-05-05.
    let mut early_on_5 = stk("EARLY", "DU123", 1.0, 1.0);
    early_on_5.exec_id = "EARLY".into();
    early_on_5.exec_time = Utc.with_ymd_and_hms(2026, 5, 5, 4, 1, 0).unwrap();

    store.record(&[late_on_4, early_on_5]).await.unwrap();

    let day4 = store
        .query("DU123", NaiveDate::from_ymd_opt(2026, 5, 4).unwrap(), None)
        .await
        .unwrap();
    let day5 = store
        .query("DU123", NaiveDate::from_ymd_opt(2026, 5, 5).unwrap(), None)
        .await
        .unwrap();
    assert_eq!(day4.len(), 1, "expected LATE on 2026-05-04");
    assert_eq!(day4[0].exec_id, "LATE");
    assert_eq!(day5.len(), 1, "expected EARLY on 2026-05-05");
    assert_eq!(day5[0].exec_id, "EARLY");
}

#[tokio::test]
async fn store_query_isolates_accounts() {
    let (_tmp, db) = make_db();
    let store = ExecutionsStore::new(Arc::clone(&db));
    store
        .record(&[stk("U1A", "U1", 1.0, 1.0), stk("U2A", "U2", 1.0, 1.0)])
        .await
        .unwrap();

    let u1 = store
        .query("U1", NaiveDate::from_ymd_opt(2026, 5, 4).unwrap(), None)
        .await
        .unwrap();
    let u2 = store
        .query("U2", NaiveDate::from_ymd_opt(2026, 5, 4).unwrap(), None)
        .await
        .unwrap();
    assert_eq!(u1.len(), 1);
    assert_eq!(u1[0].account, "U1");
    assert_eq!(u2.len(), 1);
    assert_eq!(u2[0].account, "U2");
}

#[tokio::test]
async fn ingestor_skips_when_ibkr_disconnected() {
    let (_tmp, db) = make_db();
    let store = Arc::new(ExecutionsStore::new(Arc::clone(&db)));
    let mock = Arc::new(MockIbkrClient::new());
    mock.set_accounts(vec!["DU123".to_string()]).await;
    mock.set_connected(false).await;

    let ingestor = ExecutionsIngestor::new(Arc::clone(&store), Arc::clone(&mock) as _);
    // One tick should not panic and should not populate the store —
    // a disconnected fetcher returns NotConnected, the ingestor logs.
    ingestor.tick_once().await;

    let rows = store
        .query("DU123", NaiveDate::from_ymd_opt(2026, 5, 4).unwrap(), None)
        .await
        .unwrap();
    assert!(rows.is_empty());
}

#[tokio::test]
async fn account_reader_serves_past_days_from_store() {
    use crate::mcp::ibkr_seam::{AccountReader, LiveAccountClient, ProdAccountReader};

    let (_tmp, db) = make_db();
    let store = Arc::new(ExecutionsStore::new(Arc::clone(&db)));

    // Seed the store with a fill on a *past* date relative to "today".
    // Use yesterday-ET so the wrapper's `date < today_et` branch fires
    // regardless of when the suite runs.
    use chrono_tz::America::New_York;
    let yesterday_et = Utc::now()
        .with_timezone(&New_York)
        .date_naive()
        .pred_opt()
        .unwrap();
    let yesterday_et_noon = New_York
        .from_local_datetime(&yesterday_et.and_hms_opt(12, 0, 0).unwrap())
        .single()
        .unwrap()
        .with_timezone(&Utc);
    let mut prior = stk("PRIOR", "DU123", 100.0, 250.0);
    prior.exec_time = yesterday_et_noon;
    store.record(std::slice::from_ref(&prior)).await.unwrap();

    let mock = Arc::new(MockIbkrClient::new());
    mock.set_accounts(vec!["DU123".into()]).await;
    mock.set_connected(true).await;
    // Mock has NO fills loaded — if the wrapper hits live IBKR for a
    // past day the result will be empty, which would fail the assert.

    let reader = ProdAccountReader::new(
        Arc::clone(&mock) as Arc<dyn LiveAccountClient>,
        Arc::clone(&store),
    );

    let rows = reader
        .executions("DU123", yesterday_et)
        .await
        .expect("reader ok");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].exec_id, "PRIOR");
}

#[tokio::test]
async fn account_reader_back_fills_past_day_from_live_when_store_empty() {
    use crate::mcp::ibkr_seam::{AccountReader, LiveAccountClient, ProdAccountReader};
    use chrono_tz::America::New_York;

    let (_tmp, db) = make_db();
    let store = Arc::new(ExecutionsStore::new(Arc::clone(&db)));

    let yesterday_et = Utc::now()
        .with_timezone(&New_York)
        .date_naive()
        .pred_opt()
        .unwrap();
    let yesterday_et_noon = New_York
        .from_local_datetime(&yesterday_et.and_hms_opt(12, 0, 0).unwrap())
        .single()
        .unwrap()
        .with_timezone(&Utc);

    // Store starts empty. Live IBKR has the fill — simulating the
    // "app wasn't running yesterday, but TWS still has the executions"
    // path that the trade-review generator depends on.
    let mut live = stk("BACKFILL", "DU123", 100.0, 250.0);
    live.exec_time = yesterday_et_noon;
    let mock = Arc::new(MockIbkrClient::new());
    mock.set_accounts(vec!["DU123".into()]).await;
    mock.set_connected(true).await;
    mock.set_executions(vec![live]).await;

    let reader = ProdAccountReader::new(
        Arc::clone(&mock) as Arc<dyn LiveAccountClient>,
        Arc::clone(&store),
    );

    let rows = reader
        .executions("DU123", yesterday_et)
        .await
        .expect("reader ok");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].exec_id, "BACKFILL");

    // The fallback should have persisted the fill so the next read is
    // served from the store without re-hitting IBKR.
    let stored = store
        .query("DU123", yesterday_et, None)
        .await
        .expect("store ok");
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].exec_id, "BACKFILL");
}

#[tokio::test]
async fn account_reader_returns_empty_when_store_and_live_both_empty_for_past_day() {
    use crate::mcp::ibkr_seam::{AccountReader, LiveAccountClient, ProdAccountReader};
    use chrono_tz::America::New_York;

    let (_tmp, db) = make_db();
    let store = Arc::new(ExecutionsStore::new(Arc::clone(&db)));

    let yesterday_et = Utc::now()
        .with_timezone(&New_York)
        .date_naive()
        .pred_opt()
        .unwrap();

    let mock = Arc::new(MockIbkrClient::new());
    mock.set_accounts(vec!["DU123".into()]).await;
    mock.set_connected(true).await;
    // No fills loaded — store empty + live empty.

    let reader = ProdAccountReader::new(
        Arc::clone(&mock) as Arc<dyn LiveAccountClient>,
        Arc::clone(&store),
    );

    let rows = reader
        .executions("DU123", yesterday_et)
        .await
        .expect("reader ok");
    assert!(rows.is_empty());
}

#[tokio::test]
async fn account_reader_propagates_live_error_when_store_empty_for_past_day() {
    use crate::ibkr::error::IbkrError;
    use crate::mcp::ibkr_seam::{AccountReader, LiveAccountClient, ProdAccountReader};
    use chrono_tz::America::New_York;

    let (_tmp, db) = make_db();
    let store = Arc::new(ExecutionsStore::new(Arc::clone(&db)));

    let yesterday_et = Utc::now()
        .with_timezone(&New_York)
        .date_naive()
        .pred_opt()
        .unwrap();

    let mock = Arc::new(MockIbkrClient::new());
    mock.set_accounts(vec!["DU123".into()]).await;
    // Disconnected → live drain returns NotConnected. The wrapper
    // should propagate so the UI can surface the real cause instead of
    // pretending the day had no fills.
    mock.set_connected(false).await;

    let reader = ProdAccountReader::new(
        Arc::clone(&mock) as Arc<dyn LiveAccountClient>,
        Arc::clone(&store),
    );

    let err = reader
        .executions("DU123", yesterday_et)
        .await
        .expect_err("expected NotConnected");
    assert!(matches!(err, IbkrError::NotConnected), "got: {err:?}");
}
