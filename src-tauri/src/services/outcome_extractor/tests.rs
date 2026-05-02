//! Unit tests for the outcome extractor.

use chrono::NaiveDate;

use super::*;
use crate::ibkr::types::historical::HistoricalBar;
use crate::services::research_notes::Conviction;

fn bar(time: &str, high: f64, low: f64, close: f64) -> HistoricalBar {
    HistoricalBar {
        time: time.to_string(),
        open: close,
        high,
        low,
        close,
        volume: 1_000,
        wap: close,
        count: 1,
    }
}

// ---- Parsing -----------------------------------------------------------

#[test]
fn parse_entry_zone_dash_range() {
    let levels = parse_entry_zone("100-105");
    assert_eq!(levels.entry_zone_low, Some(100.0));
    assert_eq!(levels.entry_zone_high, Some(105.0));
}

#[test]
fn parse_entry_zone_to_phrasing() {
    let levels = parse_entry_zone("buy at 100 to 105 area");
    assert_eq!(levels.entry_zone_low, Some(100.0));
    assert_eq!(levels.entry_zone_high, Some(105.0));
}

#[test]
fn parse_entry_zone_single_point() {
    let levels = parse_entry_zone("~100.5");
    assert_eq!(levels.entry_zone_low, Some(100.5));
    assert_eq!(levels.entry_zone_high, Some(100.5));
}

#[test]
fn parse_entry_zone_empty() {
    let levels = parse_entry_zone("");
    assert!(levels.entry_zone_low.is_none());
    assert!(levels.entry_zone_high.is_none());
}

#[test]
fn parse_entry_zone_handles_decimal_dollar() {
    let levels = parse_entry_zone("$100.50 - $105.25");
    assert_eq!(levels.entry_zone_low, Some(100.50));
    assert_eq!(levels.entry_zone_high, Some(105.25));
}

#[test]
fn parse_entry_zone_swaps_when_descending() {
    let levels = parse_entry_zone("105-100");
    assert_eq!(levels.entry_zone_low, Some(100.0));
    assert_eq!(levels.entry_zone_high, Some(105.0));
}

#[test]
fn parse_invalidation_close_under() {
    assert_eq!(parse_invalidation("close < 95"), Some(95.0));
}

#[test]
fn parse_invalidation_below_phrasing() {
    assert_eq!(parse_invalidation("below 95"), Some(95.0));
}

#[test]
fn parse_invalidation_bare_number() {
    assert_eq!(parse_invalidation("95"), Some(95.0));
}

#[test]
fn parse_invalidation_handles_first_number_wins() {
    // Agents sometimes stack levels; first wins per docstring.
    assert_eq!(parse_invalidation("95 / 90 / 85"), Some(95.0));
}

#[test]
fn parse_invalidation_empty_returns_none() {
    assert!(parse_invalidation("").is_none());
}

// ---- RealizedAction ----------------------------------------------------

#[test]
fn realized_action_aggregates_window() {
    let bars = vec![
        bar("20260501", 102.0, 99.0, 101.0),
        bar("20260502", 110.0, 100.0, 108.0),
        bar("20260503", 109.0, 105.0, 107.0),
    ];
    let r = RealizedAction::from_bars(&bars).expect("non-empty");
    assert_eq!(r.high, 110.0);
    assert_eq!(r.low, 99.0);
    // Latest close per timestamp.
    assert_eq!(r.close, 107.0);
}

#[test]
fn realized_action_empty_returns_none() {
    assert!(RealizedAction::from_bars(&[]).is_none());
}

// ---- Classifier --------------------------------------------------------

fn cfg_default() -> OutcomeExtractorConfig {
    OutcomeExtractorConfig::default()
}

#[test]
fn classify_skipped_short_circuits() {
    let levels = ParsedLevels {
        entry_zone_low: Some(100.0),
        entry_zone_high: Some(105.0),
        invalidation: Some(95.0),
    };
    let realized = RealizedAction {
        high: 200.0,
        low: 0.0,
        close: 50.0,
    };
    assert_eq!(
        classify(&levels, &realized, &cfg_default(), true),
        OutcomeClass::Skipped
    );
}

#[test]
fn classify_unparseable_when_no_entry_zone() {
    let levels = ParsedLevels {
        entry_zone_low: None,
        entry_zone_high: None,
        invalidation: Some(95.0),
    };
    let realized = RealizedAction {
        high: 110.0,
        low: 90.0,
        close: 100.0,
    };
    assert_eq!(
        classify(&levels, &realized, &cfg_default(), false),
        OutcomeClass::Unparseable
    );
}

#[test]
fn classify_invalidation_takes_precedence_over_entry() {
    // Even if price ranged through the entry zone, hitting
    // invalidation is the dominant outcome.
    let levels = ParsedLevels {
        entry_zone_low: Some(100.0),
        entry_zone_high: Some(105.0),
        invalidation: Some(95.0),
    };
    let realized = RealizedAction {
        high: 106.0,
        low: 94.0,
        close: 99.0,
    };
    assert_eq!(
        classify(&levels, &realized, &cfg_default(), false),
        OutcomeClass::HitInvalidation
    );
}

#[test]
fn classify_hit_entry_when_zone_overlaps() {
    let levels = ParsedLevels {
        entry_zone_low: Some(100.0),
        entry_zone_high: Some(105.0),
        invalidation: Some(95.0),
    };
    let realized = RealizedAction {
        high: 107.0,
        low: 99.0,
        close: 103.0,
    };
    assert_eq!(
        classify(&levels, &realized, &cfg_default(), false),
        OutcomeClass::HitEntry
    );
}

#[test]
fn classify_hit_target_when_price_overshoots() {
    // Zone 100-105, target multiplier 2.0 → target = 105 + 2*5 = 115.
    let levels = ParsedLevels {
        entry_zone_low: Some(100.0),
        entry_zone_high: Some(105.0),
        invalidation: Some(95.0),
    };
    let realized = RealizedAction {
        high: 115.5,
        low: 100.0,
        close: 113.0,
    };
    assert_eq!(
        classify(&levels, &realized, &cfg_default(), false),
        OutcomeClass::HitTarget
    );
}

#[test]
fn classify_no_movement_within_band() {
    let levels = ParsedLevels {
        entry_zone_low: Some(100.0),
        entry_zone_high: Some(100.0),
        invalidation: Some(95.0),
    };
    // Price hugs 100 within ±0.5%.
    let realized = RealizedAction {
        high: 100.4,
        low: 99.7,
        close: 100.1,
    };
    assert_eq!(
        classify(&levels, &realized, &cfg_default(), false),
        OutcomeClass::NoMovement
    );
}

#[test]
fn classify_drifted_when_outside_no_movement_but_no_entry_or_invalidation() {
    let levels = ParsedLevels {
        entry_zone_low: Some(100.0),
        entry_zone_high: Some(105.0),
        invalidation: Some(80.0),
    };
    // Price ranged 90–98 — never entered the 100-105 zone, never
    // hit the 80 invalidation, but moved well beyond ±0.5% of
    // mid (102.5).
    let realized = RealizedAction {
        high: 98.0,
        low: 90.0,
        close: 95.0,
    };
    assert_eq!(
        classify(&levels, &realized, &cfg_default(), false),
        OutcomeClass::Drifted
    );
}

#[test]
fn classify_single_point_entry_zone_hits_entry_not_target() {
    // Zero-width zone → range == 0 → no hit_target credit even on
    // a big move.
    let levels = ParsedLevels {
        entry_zone_low: Some(100.0),
        entry_zone_high: Some(100.0),
        invalidation: Some(80.0),
    };
    let realized = RealizedAction {
        high: 130.0,
        low: 99.0,
        close: 125.0,
    };
    assert_eq!(
        classify(&levels, &realized, &cfg_default(), false),
        OutcomeClass::HitEntry
    );
}

// ---- Persistence -------------------------------------------------------

use crate::mcp::tools::test_support::make_db;

#[tokio::test]
async fn record_outcome_inserts_then_upserts_on_pack_date_symbol() {
    let (_tmp, db) = make_db();
    let pack_date = NaiveDate::parse_from_str("2026-05-02", "%Y-%m-%d").unwrap();
    let first = NewOutcome {
        pack_date,
        symbol: "tsla".into(),
        outcome_class: OutcomeClass::HitEntry,
        conviction: Some(Conviction::A),
        entry_zone_low: Some(100.0),
        entry_zone_high: Some(105.0),
        invalidation_lvl: Some(95.0),
        realized_high: 107.0,
        realized_low: 99.0,
        realized_close: 103.0,
        eval_window_days: 1,
        prediction_id: None,
    };
    let row = record_outcome(&db, first.clone()).await.unwrap();
    assert!(row.id > 0);
    assert_eq!(row.symbol, "TSLA");
    assert_eq!(row.outcome_class, OutcomeClass::HitEntry);

    // Upsert on (date, symbol).
    let mut second = first.clone();
    second.outcome_class = OutcomeClass::HitInvalidation;
    second.realized_low = 90.0;
    let row2 = record_outcome(&db, second).await.unwrap();
    assert_eq!(row2.id, row.id, "must upsert, not insert");
    assert_eq!(row2.outcome_class, OutcomeClass::HitInvalidation);

    // list_outcomes_since picks it up.
    let rows = list_outcomes_since(&db, pack_date).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].outcome_class, OutcomeClass::HitInvalidation);
}

#[tokio::test]
async fn list_outcomes_since_filters_by_date() {
    let (_tmp, db) = make_db();
    let d1 = NaiveDate::parse_from_str("2026-04-30", "%Y-%m-%d").unwrap();
    let d2 = NaiveDate::parse_from_str("2026-05-01", "%Y-%m-%d").unwrap();
    let d3 = NaiveDate::parse_from_str("2026-05-02", "%Y-%m-%d").unwrap();
    for d in [d1, d2, d3] {
        record_outcome(
            &db,
            NewOutcome {
                pack_date: d,
                symbol: "AAPL".into(),
                outcome_class: OutcomeClass::Drifted,
                conviction: None,
                entry_zone_low: None,
                entry_zone_high: None,
                invalidation_lvl: None,
                realized_high: 1.0,
                realized_low: 0.5,
                realized_close: 0.75,
                eval_window_days: 1,
                prediction_id: None,
            },
        )
        .await
        .unwrap();
    }

    let rows = list_outcomes_since(&db, d2).await.unwrap();
    assert_eq!(rows.len(), 2);
    let dates: Vec<NaiveDate> = rows.iter().map(|r| r.pack_date).collect();
    assert_eq!(dates, vec![d3, d2], "newest pack_date first");
}

// ---- parse_idea integration --------------------------------------------

#[test]
fn parse_idea_combines_zone_and_invalidation() {
    let idea = RankedIdea {
        symbol: "TSLA".into(),
        thesis_md: "x".into(),
        conviction: Some(Conviction::A),
        entry_zone: Some("100-105".into()),
        invalidation: Some("close < 95".into()),
        evidence_refs: Vec::new(),
    };
    let levels = parse_idea(&idea);
    assert_eq!(levels.entry_zone_low, Some(100.0));
    assert_eq!(levels.entry_zone_high, Some(105.0));
    assert_eq!(levels.invalidation, Some(95.0));
}
