//! Phase 2 — `services/tca/` integration tests.
//!
//! Reference cases (from the phase doc's "Exit criteria"):
//! - clean fill (one intent, one full fill, slippage stamped).
//! - partial fills (one intent, two child fills, intent stays open
//!   until cumulative qty == intent.qty).
//! - out-of-band fill (no intent ⇒ `intent_id IS NULL`).
//! - expired intent (intent past window ⇒ no match, fill unattached).
//! - slippage sign by side (long pays positive bps, short pays
//!   positive bps; both convey "trader cost").
//!
//! Plus the integration walk: place a setup-linked intent, simulate
//! fills via the `ExecutionsStore`, run `attach_fills_for_account_*`,
//! verify the linkage columns + the attribution rollup.

use std::sync::Arc;

use chrono::{Duration, NaiveDate, Utc};
use tempfile::NamedTempFile;

use crate::ibkr::types::{ExecutionSide, IbkrExecution};
use crate::services::executions::ExecutionsStore;
use crate::storage::Db;

use super::types::{IntendedPriceSource, IntentSide, IntentStatus};
use super::{NewOrderIntent, TcaService};

fn fresh() -> (NamedTempFile, Arc<Db>, Arc<ExecutionsStore>, TcaService) {
    let tmp = NamedTempFile::new().unwrap();
    let db = Arc::new(Db::open(tmp.path()).unwrap());
    let store = Arc::new(ExecutionsStore::new(Arc::clone(&db)));
    let svc = TcaService::new(Arc::clone(&db), Arc::clone(&store));
    (tmp, db, store, svc)
}

async fn seed_setup(db: &Db, strategy: &str, symbol: &str) -> i64 {
    db.with_conn({
        let strategy = strategy.to_string();
        let symbol = symbol.to_string();
        move |conn| {
            // Need to satisfy the FK on tracked_tickers first.
            conn.execute(
                "INSERT OR IGNORE INTO tracked_tickers (symbol, source, added_at)
                 VALUES (?1, 'manual', strftime('%s','now'))",
                rusqlite::params![symbol],
            )?;
            conn.execute(
                "INSERT INTO setups (
                    symbol, strategy, direction, detected_at, trigger_price,
                    stop_price, targets, raw_signals
                 ) VALUES (?1, ?2, 'long', strftime('%s','now'), 100.0, 99.0, '[]', '{}')",
                rusqlite::params![symbol, strategy],
            )?;
            let id: i64 = conn.query_row("SELECT last_insert_rowid()", [], |r| r.get(0))?;
            Ok(id)
        }
    })
    .await
    .unwrap()
}

fn fill_at(
    exec_id: &str,
    account: &str,
    symbol: &str,
    side: ExecutionSide,
    qty: f64,
    price: f64,
    when: chrono::DateTime<Utc>,
) -> IbkrExecution {
    IbkrExecution {
        symbol: symbol.to_string(),
        side,
        qty,
        avg_price: price,
        exec_time: when,
        order_id: 1,
        exec_id: exec_id.to_string(),
        account: account.to_string(),
        contract_type: "STK".to_string(),
        expiry: None,
        strike: None,
        right: None,
        multiplier: None,
        commission: Some(1.0),
        realized_pnl: Some(50.0),
        currency: Some("USD".to_string()),
        commission_currency: Some("USD".to_string()),
    }
}

#[allow(clippy::too_many_arguments)] // test fixture; one big fn beats N small ones
fn new_intent(
    intent_id: &str,
    setup_id: Option<i64>,
    account: &str,
    symbol: &str,
    side: IntentSide,
    qty: f64,
    price_cents: i64,
    posted_at: chrono::DateTime<Utc>,
    window_minutes: i64,
) -> NewOrderIntent {
    NewOrderIntent {
        intent_id: intent_id.to_string(),
        setup_id,
        account: account.to_string(),
        symbol: symbol.to_string(),
        side,
        qty,
        intended_price_cents: price_cents,
        intended_price_source: if setup_id.is_some() {
            IntendedPriceSource::TriggerPrice
        } else {
            IntendedPriceSource::Manual
        },
        posted_at,
        expires_at: posted_at + Duration::minutes(window_minutes),
    }
}

async fn fetch_exec_linkage(
    db: &Db,
    exec_id: &str,
) -> (
    Option<i64>,
    Option<String>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
) {
    let exec_id = exec_id.to_string();
    db.with_conn(move |conn| {
        let row = conn.query_row(
            "SELECT setup_id, intent_id, intended_price_cents, slippage_bps, slippage_signed
             FROM executions WHERE exec_id = ?1",
            rusqlite::params![exec_id],
            |r| {
                Ok((
                    r.get::<_, Option<i64>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                ))
            },
        )?;
        Ok(row)
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn clean_fill_stamps_setup_id_intent_id_and_slippage() {
    let (_tmp, db, store, svc) = fresh();
    let setup_id = seed_setup(&db, "breakout", "AAPL").await;
    let posted = Utc::now() - Duration::minutes(1);
    svc.record_intent(new_intent(
        "i_1",
        Some(setup_id),
        "DU1",
        "AAPL",
        IntentSide::Buy,
        100.0,
        10_000,
        posted,
        60,
    ))
    .await
    .unwrap();
    let fill = fill_at(
        "e_1",
        "DU1",
        "AAPL",
        ExecutionSide::Bought,
        100.0,
        100.50,
        Utc::now(),
    );
    store.record(&[fill]).await.unwrap();
    let n = svc.attach_fills_for_account_today("DU1").await.unwrap();
    assert_eq!(n, 1);
    let (s_id, i_id, intended, bps, signed) = fetch_exec_linkage(&db, "e_1").await;
    assert_eq!(s_id, Some(setup_id));
    assert_eq!(i_id, Some("i_1".to_string()));
    assert_eq!(intended, Some(10_000));
    assert_eq!(bps, Some(50));
    assert_eq!(signed, Some(50));
    let intent = svc.intents().get("i_1").await.unwrap().unwrap();
    assert_eq!(intent.status, IntentStatus::Matched);
    assert_eq!(intent.matched_qty, 100.0);
}

#[tokio::test]
async fn partial_fills_keep_intent_open_until_cumulative_qty_met() {
    let (_tmp, db, store, svc) = fresh();
    let setup_id = seed_setup(&db, "breakout", "AAPL").await;
    let posted = Utc::now() - Duration::minutes(1);
    svc.record_intent(new_intent(
        "i_partial",
        Some(setup_id),
        "DU1",
        "AAPL",
        IntentSide::Buy,
        100.0,
        10_000,
        posted,
        60,
    ))
    .await
    .unwrap();
    let now = Utc::now();
    let f1 = fill_at(
        "e_a",
        "DU1",
        "AAPL",
        ExecutionSide::Bought,
        60.0,
        100.10,
        now,
    );
    store.record(&[f1]).await.unwrap();
    let n1 = svc.attach_fills_for_account_today("DU1").await.unwrap();
    assert_eq!(n1, 1);
    let intent_after_partial = svc.intents().get("i_partial").await.unwrap().unwrap();
    assert_eq!(intent_after_partial.status, IntentStatus::Open);
    assert!((intent_after_partial.matched_qty - 60.0).abs() < 1e-9);

    let f2 = fill_at(
        "e_b",
        "DU1",
        "AAPL",
        ExecutionSide::Bought,
        40.0,
        100.20,
        now + Duration::minutes(1),
    );
    store.record(&[f2]).await.unwrap();
    let n2 = svc.attach_fills_for_account_today("DU1").await.unwrap();
    assert_eq!(n2, 1);
    let intent_after_full = svc.intents().get("i_partial").await.unwrap().unwrap();
    assert_eq!(intent_after_full.status, IntentStatus::Matched);
    assert!((intent_after_full.matched_qty - 100.0).abs() < 1e-9);

    // Both fills carry the same intent_id.
    let (_, i1, _, _, _) = fetch_exec_linkage(&db, "e_a").await;
    let (_, i2, _, _, _) = fetch_exec_linkage(&db, "e_b").await;
    assert_eq!(i1, Some("i_partial".to_string()));
    assert_eq!(i2, Some("i_partial".to_string()));
}

#[tokio::test]
async fn out_of_band_fill_leaves_intent_id_null() {
    let (_tmp, db, store, svc) = fresh();
    let fill = fill_at(
        "e_oob",
        "DU1",
        "TSLA",
        ExecutionSide::Bought,
        50.0,
        200.0,
        Utc::now(),
    );
    store.record(&[fill]).await.unwrap();
    let n = svc.attach_fills_for_account_today("DU1").await.unwrap();
    assert_eq!(n, 0);
    let (s_id, i_id, intended, bps, signed) = fetch_exec_linkage(&db, "e_oob").await;
    assert!(s_id.is_none());
    assert!(i_id.is_none());
    assert!(intended.is_none());
    assert!(bps.is_none());
    assert!(signed.is_none());
}

#[tokio::test]
async fn expired_intent_does_not_match_subsequent_fill() {
    let (_tmp, db, store, svc) = fresh();
    let setup_id = seed_setup(&db, "breakout", "AAPL").await;
    let posted = Utc::now() - Duration::minutes(120);
    svc.record_intent(new_intent(
        "i_old",
        Some(setup_id),
        "DU1",
        "AAPL",
        IntentSide::Buy,
        100.0,
        10_000,
        posted,
        60, // expires 60m after posted ⇒ expired 60m ago.
    ))
    .await
    .unwrap();
    // Sweep so it flips to expired.
    svc.expire_stale().await.unwrap();
    let fill = fill_at(
        "e_late",
        "DU1",
        "AAPL",
        ExecutionSide::Bought,
        100.0,
        100.50,
        Utc::now(),
    );
    store.record(&[fill]).await.unwrap();
    let n = svc.attach_fills_for_account_today("DU1").await.unwrap();
    assert_eq!(n, 0);
    let (s_id, i_id, _, _, _) = fetch_exec_linkage(&db, "e_late").await;
    assert!(s_id.is_none());
    assert!(i_id.is_none());
    let intent = svc.intents().get("i_old").await.unwrap().unwrap();
    assert_eq!(intent.status, IntentStatus::Expired);
}

#[tokio::test]
async fn long_pays_positive_bps_short_pays_positive_bps() {
    let (_tmp, db, store, svc) = fresh();
    let s_long = seed_setup(&db, "breakout", "AAPL").await;
    let s_short = seed_setup(&db, "parabolic_short", "TSLA").await;
    let posted = Utc::now() - Duration::minutes(1);
    svc.record_intent(new_intent(
        "i_long",
        Some(s_long),
        "DU1",
        "AAPL",
        IntentSide::Buy,
        10.0,
        10_000,
        posted,
        60,
    ))
    .await
    .unwrap();
    svc.record_intent(new_intent(
        "i_short",
        Some(s_short),
        "DU1",
        "TSLA",
        IntentSide::Sell,
        10.0,
        20_000,
        posted,
        60,
    ))
    .await
    .unwrap();
    // Long fills 50bps worse: paid $100.50 vs intended $100.00.
    let f_long = fill_at(
        "e_l",
        "DU1",
        "AAPL",
        ExecutionSide::Bought,
        10.0,
        100.50,
        Utc::now(),
    );
    // Short fills 50bps worse: received $199.00 vs intended $200.00.
    let f_short = fill_at(
        "e_s",
        "DU1",
        "TSLA",
        ExecutionSide::Sold,
        10.0,
        199.00,
        Utc::now(),
    );
    store.record(&[f_long, f_short]).await.unwrap();
    svc.attach_fills_for_account_today("DU1").await.unwrap();
    let (_, _, _, bps_l, signed_l) = fetch_exec_linkage(&db, "e_l").await;
    let (_, _, _, bps_s, signed_s) = fetch_exec_linkage(&db, "e_s").await;
    assert_eq!(bps_l, Some(50));
    assert!(
        signed_l.unwrap() > 0,
        "long signed slippage should be positive"
    );
    assert_eq!(bps_s, Some(50));
    assert!(
        signed_s.unwrap() > 0,
        "short signed slippage should be positive when fill < intended"
    );
}

#[tokio::test]
async fn attribution_rollup_returns_one_row_per_strategy_plus_unattributed() {
    let (_tmp, db, store, svc) = fresh();
    let s_breakout = seed_setup(&db, "breakout", "AAPL").await;
    let s_pivot = seed_setup(&db, "episodic_pivot", "MSFT").await;
    let posted = Utc::now() - Duration::minutes(1);
    svc.record_intent(new_intent(
        "i_b",
        Some(s_breakout),
        "DU1",
        "AAPL",
        IntentSide::Buy,
        10.0,
        10_000,
        posted,
        60,
    ))
    .await
    .unwrap();
    svc.record_intent(new_intent(
        "i_p",
        Some(s_pivot),
        "DU1",
        "MSFT",
        IntentSide::Buy,
        20.0,
        30_000,
        posted,
        60,
    ))
    .await
    .unwrap();
    let now = Utc::now();
    let fills = vec![
        fill_at(
            "e_b",
            "DU1",
            "AAPL",
            ExecutionSide::Bought,
            10.0,
            100.5,
            now,
        ),
        fill_at(
            "e_p",
            "DU1",
            "MSFT",
            ExecutionSide::Bought,
            20.0,
            300.0,
            now,
        ),
        fill_at(
            "e_oob",
            "DU1",
            "GOOG",
            ExecutionSide::Bought,
            5.0,
            150.0,
            now,
        ),
    ];
    store.record(&fills).await.unwrap();
    svc.attach_fills_for_account_today("DU1").await.unwrap();

    // Cover the day in ET so the half-open range catches it.
    let today_et = Utc::now()
        .with_timezone(&chrono_tz::America::New_York)
        .date_naive();
    let yesterday_et = today_et - chrono::Duration::days(1);
    let tomorrow_et = today_et + chrono::Duration::days(1);
    let rows = svc
        .attribution()
        .attribution(yesterday_et, tomorrow_et, "DU1")
        .await
        .unwrap();

    let strategies: std::collections::BTreeSet<Option<String>> =
        rows.iter().map(|r| r.strategy.clone()).collect();
    assert!(strategies.contains(&Some("breakout".to_string())));
    assert!(strategies.contains(&Some("episodic_pivot".to_string())));
    assert!(strategies.contains(&None), "unattributed bucket present");
    for r in &rows {
        assert!(r.n_trades >= 1);
    }
    let breakout = rows
        .iter()
        .find(|r| r.strategy.as_deref() == Some("breakout"))
        .unwrap();
    assert_eq!(breakout.n_with_slippage, 1);
}

#[tokio::test]
async fn attribution_handles_empty_window() {
    let (_tmp, _db, _store, svc) = fresh();
    let d = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let rows = svc.attribution().attribution(d, d, "DU1").await.unwrap();
    assert!(rows.is_empty());
}

#[tokio::test]
async fn slippage_distribution_buckets_by_strategy() {
    let (_tmp, db, store, svc) = fresh();
    let s = seed_setup(&db, "breakout", "AAPL").await;
    let posted = Utc::now() - Duration::minutes(1);
    // Three intents → three fills landing in three different buckets:
    // 0bps (perfect), ~25bps, ~150bps.
    let prices = [
        (100.00, "i_a", "e_a"),
        (100.25, "i_b", "e_b"),
        (101.50, "i_c", "e_c"),
    ];
    let mut fills = Vec::new();
    for (price, intent_id, exec_id) in prices {
        svc.record_intent(new_intent(
            intent_id,
            Some(s),
            "DU1",
            "AAPL",
            IntentSide::Buy,
            10.0,
            10_000,
            posted,
            60,
        ))
        .await
        .unwrap();
        fills.push(fill_at(
            exec_id,
            "DU1",
            "AAPL",
            ExecutionSide::Bought,
            10.0,
            price,
            Utc::now(),
        ));
    }
    store.record(&fills).await.unwrap();
    svc.attach_fills_for_account_today("DU1").await.unwrap();

    let today_et = Utc::now()
        .with_timezone(&chrono_tz::America::New_York)
        .date_naive();
    let dist = svc
        .attribution()
        .slippage_distribution(today_et, today_et, "DU1", None)
        .await
        .unwrap();
    assert_eq!(dist.len(), 1);
    let row = &dist[0];
    assert_eq!(row.strategy, Some("breakout".to_string()));
    let total: i64 = row.buckets.iter().map(|b| b.n).sum();
    assert_eq!(total, 3);
    // Bucket 0 (0–1 bps): the perfect fill.
    assert_eq!(row.buckets[0].n, 1);
    // Bucket (10, 25] for 25bps fill ⇒ idx 3 (10–25). 25bps lands on
    // the boundary; lower-inclusive ⇒ bucket index 4 (25–50).
    let twenty_five = row
        .buckets
        .iter()
        .find(|b| b.lower_bps == 25 && b.upper_bps == 50)
        .unwrap();
    assert_eq!(twenty_five.n, 1);
    let one_fifty = row.buckets.iter().find(|b| b.lower_bps == 100).unwrap();
    assert_eq!(one_fifty.n, 1);
}
