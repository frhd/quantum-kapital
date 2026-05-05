//! `aggregate` — pure SQL aggregator over `day_reviews`.
//!
//! Reads every review for `account` on or after `today_et - window_days`
//! and produces a [`TraderProfile`] envelope. Tag-name strings in the
//! `behavioral_tags` JSON column are deserialised back into the
//! [`BehavioralTag`] enum; unknown tags (introduced by a future
//! `prompt_version` bump) are silently dropped from the frequency /
//! P&L counters — the row still contributes to `n_reviews`.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{Duration, NaiveDate, Utc};
use chrono_tz::America::New_York;
use rusqlite::params;
use serde::Deserialize;

use crate::services::trade_reviews::tags::BehavioralTag;
use crate::services::trade_reviews::types::LegObservation;
use crate::storage::{Db, StorageError};

use super::types::{
    PnlByTag, RecentIncident, TagFrequency, TraderProfile, Trendline, WindowSummary,
};

/// Maximum number of recent leg-level incidents surfaced into the
/// profile. Bounded so the playbook prompt stays under the per-call
/// token budget.
const MAX_RECENT_INCIDENTS: usize = 10;

/// Trendline split: last 7 calendar days vs the 21 days preceding them.
const LAST_WINDOW_DAYS: i64 = 7;
const PRIOR_WINDOW_DAYS: i64 = 28;

/// Deserialiser shim for tag strings in the `behavioral_tags` JSON
/// column. Wraps `BehavioralTag` so a single bad string doesn't fail
/// the whole row.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum MaybeTag {
    Known(BehavioralTag),
    Unknown(serde::de::IgnoredAny),
}

/// Aggregate `day_reviews` for `account` over the trailing
/// `window_days` ET trading days.
pub async fn aggregate(
    db: &Arc<Db>,
    account: &str,
    window_days: u32,
) -> Result<TraderProfile, StorageError> {
    let today_et = Utc::now().with_timezone(&New_York).date_naive();
    let since = today_et - Duration::days(window_days as i64);
    let last_window_since = today_et - Duration::days(LAST_WINDOW_DAYS);
    let prior_window_since = today_et - Duration::days(PRIOR_WINDOW_DAYS);
    let prior_window_until = last_window_since;

    let account_owned = account.to_string();
    let since_str = since.to_string();

    let rows: Vec<RawRow> = db
        .with_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT date, behavioral_tags, leg_observations, net_pnl, grade_score
                 FROM day_reviews
                 WHERE account = ?1 AND date >= ?2
                 ORDER BY date DESC",
            )?;
            let iter = stmt.query_map(params![account_owned, since_str], |r| {
                Ok(RawRow {
                    date: r.get::<_, String>(0)?,
                    tags_json: r.get::<_, String>(1)?,
                    legs_json: r.get::<_, String>(2)?,
                    net_pnl: r.get::<_, f64>(3)?,
                    grade_score: r.get::<_, f64>(4)?,
                })
            })?;
            let mut out = Vec::new();
            for r in iter {
                out.push(r?);
            }
            Ok(out)
        })
        .await?;

    let n_reviews = rows.len() as i64;

    let mut tag_count: BTreeMap<BehavioralTag, i64> = BTreeMap::new();
    let mut pnl_per_tag: BTreeMap<BehavioralTag, (i64, f64)> = BTreeMap::new();
    let mut last_window_pnl = 0.0_f64;
    let mut last_window_score_sum = 0.0_f64;
    let mut last_window_n = 0_i64;
    let mut prior_window_pnl = 0.0_f64;
    let mut prior_window_score_sum = 0.0_f64;
    let mut prior_window_n = 0_i64;
    let mut last_window_tags: BTreeMap<String, i64> = BTreeMap::new();
    let mut prior_window_tags: BTreeMap<String, i64> = BTreeMap::new();
    let mut incidents: Vec<RecentIncident> = Vec::new();

    for raw in &rows {
        let date = match NaiveDate::parse_from_str(&raw.date, "%Y-%m-%d") {
            Ok(d) => d,
            Err(_) => continue,
        };
        let tags = parse_tags(&raw.tags_json);
        for tag in &tags {
            *tag_count.entry(*tag).or_insert(0) += 1;
            let entry = pnl_per_tag.entry(*tag).or_insert((0, 0.0));
            entry.0 += 1;
            entry.1 += raw.net_pnl;
        }

        if date >= last_window_since {
            last_window_n += 1;
            last_window_pnl += raw.net_pnl;
            last_window_score_sum += raw.grade_score;
            for tag in &tags {
                *last_window_tags
                    .entry(tag_to_string(*tag))
                    .or_insert(0) += 1;
            }
        } else if date >= prior_window_since && date < prior_window_until {
            prior_window_n += 1;
            prior_window_pnl += raw.net_pnl;
            prior_window_score_sum += raw.grade_score;
            for tag in &tags {
                *prior_window_tags
                    .entry(tag_to_string(*tag))
                    .or_insert(0) += 1;
            }
        }

        if date >= last_window_since && incidents.len() < MAX_RECENT_INCIDENTS {
            push_incidents(&mut incidents, date, &raw.legs_json);
        }
    }

    let mut tag_frequencies: Vec<TagFrequency> = tag_count
        .iter()
        .map(|(tag, count)| TagFrequency {
            tag: *tag,
            count: *count,
            pct_of_reviews: if n_reviews > 0 {
                *count as f64 / n_reviews as f64
            } else {
                0.0
            },
        })
        .collect();
    tag_frequencies.sort_by_key(|f| std::cmp::Reverse(f.count));

    let mut pnl_by_tag: Vec<PnlByTag> = pnl_per_tag
        .iter()
        .map(|(tag, (n, total))| PnlByTag {
            tag: *tag,
            n_days: *n,
            net_pnl_total: *total,
            net_pnl_per_day_avg: if *n > 0 { *total / *n as f64 } else { 0.0 },
        })
        .collect();
    pnl_by_tag.sort_by(|a, b| {
        b.net_pnl_total
            .partial_cmp(&a.net_pnl_total)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let trendline = Trendline {
        last_7d: WindowSummary {
            n_reviews: last_window_n,
            tag_counts: last_window_tags,
            net_pnl: last_window_pnl,
            avg_grade_score: if last_window_n > 0 {
                last_window_score_sum / last_window_n as f64
            } else {
                0.0
            },
        },
        prior_21d: WindowSummary {
            n_reviews: prior_window_n,
            tag_counts: prior_window_tags,
            net_pnl: prior_window_pnl,
            avg_grade_score: if prior_window_n > 0 {
                prior_window_score_sum / prior_window_n as f64
            } else {
                0.0
            },
        },
    };

    Ok(TraderProfile {
        account: account.to_string(),
        window_days,
        since_date: since,
        n_reviews,
        tag_frequencies,
        pnl_by_tag,
        trendline,
        recent_incidents: incidents,
    })
}

struct RawRow {
    date: String,
    tags_json: String,
    legs_json: String,
    net_pnl: f64,
    grade_score: f64,
}

fn parse_tags(tags_json: &str) -> Vec<BehavioralTag> {
    let parsed: Vec<MaybeTag> = serde_json::from_str(tags_json).unwrap_or_default();
    parsed
        .into_iter()
        .filter_map(|t| match t {
            MaybeTag::Known(tag) => Some(tag),
            MaybeTag::Unknown(_) => None,
        })
        .collect()
}

fn tag_to_string(tag: BehavioralTag) -> String {
    serde_json::to_value(tag)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| format!("{tag:?}"))
}

fn push_incidents(out: &mut Vec<RecentIncident>, date: NaiveDate, legs_json: &str) {
    let legs: Vec<LegObservation> = match serde_json::from_str(legs_json) {
        Ok(v) => v,
        Err(_) => return,
    };
    for leg in legs {
        if out.len() >= MAX_RECENT_INCIDENTS {
            break;
        }
        let tag = match leg.tag {
            Some(t) => t,
            None => continue,
        };
        let symbol = leg
            .symbol
            .clone()
            .unwrap_or_else(|| leg.leg_id.clone());
        out.push(RecentIncident {
            date,
            symbol,
            tag,
            leg_observation: leg.observation_md,
        });
    }
}
