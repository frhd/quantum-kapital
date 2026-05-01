use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, TimeZone, Utc};
use tempfile::NamedTempFile;

use crate::config::settings::{AutoScannerConfig, ScanProfile};
use crate::ibkr::error::Result as IbkrResult;
use crate::ibkr::types::{ContractDetails, ScannerData, ScannerSubscription, SecurityType};
use crate::services::candidate_promoter::CandidatePromoter;
use crate::services::candidate_universe::CandidateUniverseService;
use crate::services::tracker_service::TrackerService;
use crate::storage::Db;

use super::{AutoScannerService, MarketScanner};

/// Promote-everything threshold so existing tests' assertions on
/// `tracker.list()` keep their pre-Phase-4 semantics. Tests that want
/// to exercise staging-only behaviour build their own promoter.
const TEST_AUTO_PROMOTE_THRESHOLD: f64 = 0.0;

// ---------------- helpers ----------------

fn scanner_row(rank: i32, symbol: &str) -> ScannerData {
    ScannerData {
        rank,
        contract: ContractDetails {
            symbol: symbol.to_string(),
            sec_type: SecurityType::Stock,
            exchange: "SMART".to_string(),
            primary_exchange: "NASDAQ".to_string(),
            currency: "USD".to_string(),
            local_symbol: symbol.to_string(),
            trading_class: symbol.to_string(),
            contract_id: 100 + rank,
            min_tick: 0.01,
            multiplier: String::new(),
            price_magnifier: 1,
        },
        leg: String::new(),
    }
}

type FakeScanKey = (String, Option<String>);
type FakeScanMap = HashMap<FakeScanKey, Vec<ScannerData>>;

#[derive(Default)]
struct FakeScanner {
    canned: Mutex<FakeScanMap>,
    captured: Mutex<Vec<ScannerSubscription>>,
}

impl FakeScanner {
    fn program(&self, scan_code: &str, industry: Option<&str>, results: Vec<ScannerData>) {
        self.canned.lock().unwrap().insert(
            (scan_code.to_string(), industry.map(str::to_string)),
            results,
        );
    }

    fn captured(&self) -> Vec<ScannerSubscription> {
        self.captured.lock().unwrap().clone()
    }
}

#[async_trait]
impl MarketScanner for FakeScanner {
    async fn scan(&self, subscription: ScannerSubscription) -> IbkrResult<Vec<ScannerData>> {
        self.captured.lock().unwrap().push(subscription.clone());
        let key = (subscription.scan_code, subscription.industry_filter);
        Ok(self
            .canned
            .lock()
            .unwrap()
            .get(&key)
            .cloned()
            .unwrap_or_default())
    }
}

fn make_harness(
    config: AutoScannerConfig,
) -> (
    NamedTempFile,
    Arc<FakeScanner>,
    Arc<TrackerService>,
    AutoScannerService,
) {
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Arc::new(Db::open(tmp.path()).expect("open db"));
    let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));
    let candidates = Arc::new(CandidateUniverseService::new(Arc::clone(&db)));
    let promoter = Arc::new(CandidatePromoter::new(
        Arc::clone(&candidates),
        Arc::clone(&tracker),
        TEST_AUTO_PROMOTE_THRESHOLD,
    ));
    let scanner = Arc::new(FakeScanner::default());
    let service = AutoScannerService::new(
        scanner.clone() as Arc<dyn MarketScanner>,
        Arc::clone(&tracker),
        promoter,
        Arc::clone(&db),
        config,
    );
    (tmp, scanner, tracker, service)
}

fn now_utc() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 4, 30, 14, 35, 0).unwrap()
}

// ---------------- tests ----------------

#[tokio::test]
async fn run_once_is_a_noop_when_disabled() {
    let cfg = AutoScannerConfig::default(); // enabled = false
    let (_tmp, scanner, tracker, svc) = make_harness(cfg);

    let summary = svc.run_once(now_utc()).await.unwrap();
    assert!(summary.added.is_empty());
    assert!(scanner.captured().is_empty(), "no scans should fire");
    assert_eq!(tracker.list(None).await.unwrap().len(), 0);
}

#[tokio::test]
async fn run_once_promotes_top_k_results_for_a_single_profile() {
    let cfg = AutoScannerConfig {
        enabled: true,
        daily_cap: 10,
        profiles: vec![ScanProfile {
            name: "Top Gainers".to_string(),
            scan_code: "TOP_PERC_GAIN".to_string(),
            location_code: "STK.US.MAJOR".to_string(),
            above_price: Some(5.0),
            above_volume: Some(500_000),
            industry_filter: None,
            promote_top_k: 3,
            number_of_rows: 25,
        }],
        industries: Vec::new(),
        ..Default::default()
    };
    let (_tmp, scanner, tracker, svc) = make_harness(cfg);
    scanner.program(
        "TOP_PERC_GAIN",
        None,
        vec![
            scanner_row(1, "NVDA"),
            scanner_row(2, "AMD"),
            scanner_row(3, "TSLA"),
            scanner_row(4, "AVGO"),
            scanner_row(5, "INTC"),
        ],
    );

    let summary = svc.run_once(now_utc()).await.unwrap();
    assert_eq!(summary.added, vec!["NVDA", "AMD", "TSLA"]);
    let watchlist: Vec<String> = tracker
        .list(None)
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.symbol)
        .collect();
    // Watchlist sort is added_at DESC; just check membership.
    for sym in ["NVDA", "AMD", "TSLA"] {
        assert!(
            watchlist.iter().any(|s| s == sym),
            "{sym} should be auto-added"
        );
    }
}

#[tokio::test]
async fn run_once_skips_symbols_already_on_the_watchlist() {
    let cfg = AutoScannerConfig {
        enabled: true,
        daily_cap: 10,
        profiles: vec![ScanProfile {
            name: "Top Gainers".to_string(),
            scan_code: "TOP_PERC_GAIN".to_string(),
            location_code: "STK.US.MAJOR".to_string(),
            above_price: None,
            above_volume: None,
            industry_filter: None,
            promote_top_k: 5,
            number_of_rows: 25,
        }],
        industries: Vec::new(),
        ..Default::default()
    };
    let (_tmp, scanner, tracker, svc) = make_harness(cfg);
    // NVDA is already manually tracked.
    tracker
        .add(
            "NVDA",
            crate::ibkr::types::tracker::TrackerSource::Manual,
            None,
            vec![],
            None,
        )
        .await
        .unwrap();
    scanner.program(
        "TOP_PERC_GAIN",
        None,
        vec![scanner_row(1, "NVDA"), scanner_row(2, "AMD")],
    );

    let summary = svc.run_once(now_utc()).await.unwrap();
    assert_eq!(summary.added, vec!["AMD"]);
    assert!(
        summary
            .skipped
            .iter()
            .any(|s| s.contains("NVDA") && s.to_lowercase().contains("already")),
        "summary.skipped should explain the NVDA dedup; got {:?}",
        summary.skipped
    );
}

#[tokio::test]
async fn run_once_enforces_daily_cap_across_profiles() {
    let cfg = AutoScannerConfig {
        enabled: true,
        daily_cap: 3,
        profiles: vec![
            ScanProfile {
                name: "Top Gainers".to_string(),
                scan_code: "TOP_PERC_GAIN".to_string(),
                location_code: "STK.US.MAJOR".to_string(),
                above_price: None,
                above_volume: None,
                industry_filter: None,
                promote_top_k: 5,
                number_of_rows: 25,
            },
            ScanProfile {
                name: "Hot Volume".to_string(),
                scan_code: "HOT_BY_VOLUME".to_string(),
                location_code: "STK.US.MAJOR".to_string(),
                above_price: None,
                above_volume: None,
                industry_filter: None,
                promote_top_k: 5,
                number_of_rows: 25,
            },
        ],
        industries: Vec::new(),
        ..Default::default()
    };
    let (_tmp, scanner, _tracker, svc) = make_harness(cfg);
    scanner.program(
        "TOP_PERC_GAIN",
        None,
        vec![
            scanner_row(1, "AAA"),
            scanner_row(2, "BBB"),
            scanner_row(3, "CCC"),
            scanner_row(4, "DDD"),
            scanner_row(5, "EEE"),
        ],
    );
    scanner.program(
        "HOT_BY_VOLUME",
        None,
        vec![
            scanner_row(1, "FFF"),
            scanner_row(2, "GGG"),
            scanner_row(3, "HHH"),
        ],
    );

    let summary = svc.run_once(now_utc()).await.unwrap();
    assert_eq!(
        summary.added.len(),
        3,
        "daily_cap=3 should cap total adds across profiles; got {:?}",
        summary.added
    );
}

#[tokio::test]
async fn run_once_propagates_industry_filter_to_subscription() {
    let cfg = AutoScannerConfig {
        enabled: true,
        daily_cap: 10,
        profiles: Vec::new(),
        industries: vec!["Semiconductors".to_string()],
        ..Default::default()
    };
    let (_tmp, scanner, _tracker, svc) = make_harness(cfg);
    scanner.program(
        "TOP_PERC_GAIN",
        Some("Semiconductors"),
        vec![scanner_row(1, "NVDA")],
    );

    let summary = svc.run_once(now_utc()).await.unwrap();
    assert_eq!(summary.added, vec!["NVDA"]);

    let captured = scanner.captured();
    assert!(
        captured
            .iter()
            .any(|s| s.industry_filter.as_deref() == Some("Semiconductors")),
        "captured subs must include the industry filter; got {captured:?}",
    );
}

#[tokio::test]
async fn run_once_persists_source_metadata_for_audit() {
    let cfg = AutoScannerConfig {
        enabled: true,
        daily_cap: 10,
        profiles: vec![ScanProfile {
            name: "Top Gainers".to_string(),
            scan_code: "TOP_PERC_GAIN".to_string(),
            location_code: "STK.US.MAJOR".to_string(),
            above_price: None,
            above_volume: None,
            industry_filter: None,
            promote_top_k: 1,
            number_of_rows: 25,
        }],
        industries: Vec::new(),
        ..Default::default()
    };
    let (_tmp, scanner, tracker, svc) = make_harness(cfg);
    scanner.program("TOP_PERC_GAIN", None, vec![scanner_row(7, "NVDA")]);

    svc.run_once(now_utc()).await.unwrap();
    let row = tracker.get("NVDA").await.unwrap().expect("nvda persisted");
    assert_eq!(
        row.source,
        crate::ibkr::types::tracker::TrackerSource::AutoScanner
    );
    // Phase 4: scanner provenance lives in `candidate_universe.sources`;
    // the watchlist `source_meta` carries a thin wrapper pointing to it.
    let meta = row.source_meta.expect("source_meta JSON present");
    assert_eq!(meta["via"], "candidate_universe");
    assert!(meta["score"].as_f64().is_some(), "score present in meta");
    let sources = meta["sources"].as_array().expect("sources array");
    assert!(sources.iter().any(|s| s["source"]
        .as_str()
        .map(|v| v == "scanner_top_perc_gain")
        .unwrap_or(false)));
    assert!(sources.iter().any(|s| s["rank"].as_i64() == Some(7)));
}

#[tokio::test]
async fn daily_cap_counts_only_today_not_prior_auto_adds() {
    let cfg = AutoScannerConfig {
        enabled: true,
        daily_cap: 1,
        profiles: vec![ScanProfile {
            name: "Top Gainers".to_string(),
            scan_code: "TOP_PERC_GAIN".to_string(),
            location_code: "STK.US.MAJOR".to_string(),
            above_price: None,
            above_volume: None,
            industry_filter: None,
            promote_top_k: 5,
            number_of_rows: 25,
        }],
        industries: Vec::new(),
        ..Default::default()
    };
    let (_tmp, scanner, tracker, svc) = make_harness(cfg);

    // Seed an AutoScanner row dated YESTERDAY UTC. We backdate by
    // updating the SQLite row directly because TrackerService::add
    // stamps `added_at = Utc::now()`.
    tracker
        .add(
            "OLD",
            crate::ibkr::types::tracker::TrackerSource::AutoScanner,
            None,
            vec![],
            None,
        )
        .await
        .unwrap();
    let now = now_utc();
    let yesterday = (now - ChronoDuration::days(2)).timestamp();
    tracker
        .db_for_testing()
        .with_conn(move |c| {
            c.execute(
                "UPDATE tracked_tickers SET added_at = ?1 WHERE symbol = 'OLD'",
                rusqlite::params![yesterday],
            )?;
            Ok(())
        })
        .await
        .unwrap();

    scanner.program("TOP_PERC_GAIN", None, vec![scanner_row(1, "NEW")]);
    let summary = svc.run_once(now).await.unwrap();
    assert_eq!(
        summary.added,
        vec!["NEW"],
        "yesterday's add must not count against today's cap"
    );
}

// ---------------- scheduler ----------------

use super::{AutoScannerScheduler, Clock};
use std::time::Duration;

fn cfg_one_profile(daily_cap: u32, interval_minutes: u32) -> AutoScannerConfig {
    AutoScannerConfig {
        enabled: true,
        daily_cap,
        interval_minutes,
        profiles: vec![ScanProfile {
            name: "Top Gainers".to_string(),
            scan_code: "TOP_PERC_GAIN".to_string(),
            location_code: "STK.US.MAJOR".to_string(),
            above_price: None,
            above_volume: None,
            industry_filter: None,
            promote_top_k: 5,
            number_of_rows: 25,
        }],
        industries: Vec::new(),
        auto_promote_threshold: TEST_AUTO_PROMOTE_THRESHOLD,
    }
}

#[tokio::test]
async fn scheduler_skips_outside_rth() {
    let (_tmp, scanner, _tracker, svc) = make_harness(cfg_one_profile(10, 30));
    scanner.program("TOP_PERC_GAIN", None, vec![scanner_row(1, "NVDA")]);
    // 03:00 UTC on a weekday → outside US RTH (which spans 13:30–20:00 UTC).
    let outside_rth = Utc.with_ymd_and_hms(2026, 4, 30, 3, 0, 0).unwrap();
    let scheduler = AutoScannerScheduler::with_clock(
        Arc::new(svc),
        Duration::from_secs(60),
        Clock::Fixed(outside_rth),
    );
    let outcome = scheduler.tick().await.unwrap();
    assert!(outcome.is_none(), "tick outside RTH must be a no-op");
    assert!(scanner.captured().is_empty());
}

#[tokio::test]
async fn scheduler_runs_once_when_inside_rth_and_cadence_satisfied() {
    let (_tmp, scanner, _tracker, svc) = make_harness(cfg_one_profile(10, 30));
    scanner.program("TOP_PERC_GAIN", None, vec![scanner_row(1, "NVDA")]);
    // 14:35 UTC on 2026-04-30 is inside US RTH.
    let in_rth = Utc.with_ymd_and_hms(2026, 4, 30, 14, 35, 0).unwrap();
    let scheduler = AutoScannerScheduler::with_clock(
        Arc::new(svc),
        Duration::from_secs(60),
        Clock::Fixed(in_rth),
    );
    let outcome = scheduler.tick().await.unwrap();
    let outcome = outcome.expect("tick should fire inside RTH");
    assert_eq!(outcome.added, vec!["NVDA"]);
}

#[tokio::test]
async fn scheduler_rate_limits_back_to_back_ticks_via_interval_minutes() {
    let (_tmp, scanner, _tracker, svc) = make_harness(cfg_one_profile(10, 30));
    // Two scan profiles' worth of canned data so a second tick (if it
    // wrongly fired) would actually have something to add.
    scanner.program(
        "TOP_PERC_GAIN",
        None,
        vec![scanner_row(1, "NVDA"), scanner_row(2, "AMD")],
    );
    let t0 = Utc.with_ymd_and_hms(2026, 4, 30, 14, 35, 0).unwrap();
    let scheduler =
        AutoScannerScheduler::with_clock(Arc::new(svc), Duration::from_secs(60), Clock::Fixed(t0));
    let first = scheduler.tick().await.unwrap();
    assert!(first.is_some(), "first tick should fire");

    // 5 minutes later — still inside the 30-minute cadence window.
    scheduler
        .set_clock(Clock::Fixed(t0 + ChronoDuration::minutes(5)))
        .await;
    let second = scheduler.tick().await.unwrap();
    assert!(
        second.is_none(),
        "second tick within interval_minutes must be a no-op; got {second:?}"
    );

    // 35 minutes later — past the cadence window, runs again.
    scheduler
        .set_clock(Clock::Fixed(t0 + ChronoDuration::minutes(35)))
        .await;
    let third = scheduler.tick().await.unwrap();
    assert!(
        third.is_some(),
        "tick after interval_minutes elapsed should fire again"
    );
}
