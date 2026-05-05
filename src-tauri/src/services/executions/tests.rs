//! Integration tests for `ExecutionsStore`. Uses an in-memory SQLite Db
//! built via the existing `test_support::make_db()` helper.

use std::sync::Arc;

use chrono::{NaiveDate, TimeZone, Utc};

use super::store::ExecutionsStore;
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
        .record(&[
            stk("U1A", "U1", 1.0, 1.0),
            stk("U2A", "U2", 1.0, 1.0),
        ])
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
