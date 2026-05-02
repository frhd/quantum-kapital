//! `get_outcomes` — Phase 7 read tool.
//!
//! Returns one row per `(pack_date, symbol)` for every agent morning-
//! pack idea since `since`, joined with realized price action and
//! the originating thesis snippet.
//!
//! On first call for a given `(pack_date, symbol)`, the tool fetches
//! daily bars covering the eval window, runs the
//! [`outcome_extractor`] classifier, and persists the result to
//! `outcomes`. Subsequent calls read from the cached row — the
//! classifier is deterministic so a re-run on the same window
//! reproduces the same class.
//!
//! Bar-fetch failures (e.g. IBKR unavailable, symbol not seeded in
//! the bars cache) skip materialization for the offending row but
//! never fail the whole tool — already-scored rows still come back.

use chrono::{Duration as ChronoDuration, NaiveDate, Utc};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashSet;
use tracing::warn;

use crate::ibkr::types::historical::BarSize;
use crate::mcp::handler::McpHandler;
use crate::mcp::tools::map_tool_result;
use crate::services::agent_morning_packs::{self, RankedIdea};
use crate::services::historical_data_service::Lookback;
use crate::services::outcome_extractor::{
    self, classify, parse_idea, NewOutcome, OutcomeClass, OutcomeExtractorConfig, RealizedAction,
};
use crate::services::predictions;

/// Default eval window in calendar days. 1 covers same-day
/// hit_entry / hit_invalidation per Phase 7 plan; agents can pass
/// up to 5 to credit hit_target on a longer follow-through.
const DEFAULT_EVAL_WINDOW_DAYS: i64 = 1;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetOutcomesArgs {
    /// `YYYY-MM-DD` (ET trading day) — only outcomes for packs with
    /// `pack_date >= since` are returned. Required.
    pub since: String,
    /// Window of trading days (calendar-day approximation) the
    /// extractor evaluates. Defaults to 1 (same-day) per Phase 7 plan.
    /// Capped at 10 server-side.
    #[serde(default)]
    pub eval_window_days: Option<i64>,
}

#[tool_router(router = get_outcomes_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_outcomes",
        description = "Score agent-authored morning-pack predictions whose `pack_date >= since` (`YYYY-MM-DD`, ET) against realized daily bars. Idempotent — already-scored rows return cached classifications. Each row carries `{ pack_date, symbol, outcome_class (hit_entry|hit_target|hit_invalidation|drifted|no_movement|skipped|unparseable), conviction, entry_zone_low/high, invalidation_lvl, realized_high/low/close, eval_window_days, evaluated_at, thesis_md }`. Returns `{ items, count }` ordered newest pack_date first."
    )]
    pub async fn get_outcomes(
        &self,
        Parameters(args): Parameters<GetOutcomesArgs>,
    ) -> Result<CallToolResult, McpError> {
        let since = match NaiveDate::parse_from_str(args.since.trim(), "%Y-%m-%d") {
            Ok(d) => d,
            Err(e) => {
                return map_tool_result::<(), String>(Err(format!(
                    "since must be YYYY-MM-DD: {e}"
                )));
            }
        };
        let window = args
            .eval_window_days
            .unwrap_or(DEFAULT_EVAL_WINDOW_DAYS)
            .clamp(1, 10);

        if let Err(e) = self.materialize_outcomes(since, window).await {
            return map_tool_result::<(), String>(Err(e));
        }

        // Read back every persisted row + join the originating thesis
        // markdown so the agent has both pieces in one structured
        // payload.
        let rows = match outcome_extractor::list_outcomes_since(&self.db, since).await {
            Ok(rs) => rs,
            Err(e) => return map_tool_result::<(), String>(Err(e.to_string())),
        };

        // Build a (pack_date, symbol) → thesis_md lookup once.
        let packs = match agent_morning_packs::list_packs_since(&self.db, since).await {
            Ok(ps) => ps,
            Err(e) => return map_tool_result::<(), String>(Err(e.to_string())),
        };
        let lookup = build_thesis_lookup(&packs);

        let items: Vec<_> = rows
            .into_iter()
            .map(|r| {
                let key = (r.pack_date, r.symbol.clone());
                let thesis = lookup.get(&key).cloned();
                json!({
                    "pack_date": r.pack_date.to_string(),
                    "symbol": r.symbol,
                    "outcome_class": r.outcome_class.as_str(),
                    "conviction": r.conviction.map(|c| c.as_str()),
                    "entry_zone_low": r.entry_zone_low,
                    "entry_zone_high": r.entry_zone_high,
                    "invalidation_lvl": r.invalidation_lvl,
                    "realized_high": r.realized_high,
                    "realized_low": r.realized_low,
                    "realized_close": r.realized_close,
                    "eval_window_days": r.eval_window_days,
                    "evaluated_at": r.evaluated_at.timestamp(),
                    "thesis_md": thesis,
                })
            })
            .collect();

        map_tool_result::<_, String>(Ok(items))
    }

    /// For every (pack_date, idea) since `since` that is missing from
    /// `outcomes`, fetch bars + classify + persist. Bar-fetch failures
    /// log and skip the offending row.
    async fn materialize_outcomes(&self, since: NaiveDate, window: i64) -> Result<(), String> {
        let packs = agent_morning_packs::list_packs_since(&self.db, since)
            .await
            .map_err(|e| e.to_string())?;
        if packs.is_empty() {
            return Ok(());
        }

        let existing = outcome_extractor::list_outcomes_since(&self.db, since)
            .await
            .map_err(|e| e.to_string())?;
        let scored: HashSet<(NaiveDate, String)> = existing
            .into_iter()
            .map(|r| (r.pack_date, r.symbol))
            .collect();

        let cfg = OutcomeExtractorConfig::default();
        let today = Utc::now().date_naive();

        for pack in packs {
            // Skip future-dated packs — no realized action yet.
            if pack.date > today {
                continue;
            }
            for idea in &pack.ranked_ideas {
                let symbol_norm = idea.symbol.to_uppercase();
                if scored.contains(&(pack.date, symbol_norm.clone())) {
                    continue;
                }
                let mut window_bars = Vec::new();
                let mut fetch_failed = false;
                // `window` is the inclusive day count: 1 ⇒ pack_date
                // only (same-day), 5 ⇒ pack_date through pack_date+4.
                for offset in 0..window {
                    let d = pack.date + ChronoDuration::days(offset);
                    if d > today {
                        break;
                    }
                    match self
                        .historical_service
                        .fetch_bars(&symbol_norm, BarSize::Day1, Lookback::TradingDay(d))
                        .await
                    {
                        Ok(bars) => window_bars.extend(bars),
                        Err(e) => {
                            warn!(
                                "get_outcomes: bar fetch failed for {} {} on {}: {}",
                                pack.date, symbol_norm, d, e
                            );
                            fetch_failed = true;
                            break;
                        }
                    }
                }
                if fetch_failed {
                    continue;
                }

                let realized = match RealizedAction::from_bars(&window_bars) {
                    Some(r) => r,
                    None => {
                        warn!(
                            "get_outcomes: skipping {} {} — no bars in eval window",
                            pack.date, symbol_norm
                        );
                        continue;
                    }
                };

                let levels = parse_idea(idea);
                let skipped = idea_marked_skipped(idea);
                let outcome_class = classify(&levels, &realized, &cfg, skipped);

                // Phase 8: backlink outcome → predictions row.
                let prediction_id = match predictions::find_for_pack(
                    &self.db,
                    &pack.date.to_string(),
                    &symbol_norm,
                )
                .await
                {
                    Ok(p) => p.map(|row| row.id),
                    Err(e) => {
                        warn!(
                            "get_outcomes: prediction lookup failed for {} {}: {}",
                            pack.date, symbol_norm, e
                        );
                        None
                    }
                };

                if let Err(e) = outcome_extractor::record_outcome(
                    &self.db,
                    NewOutcome {
                        pack_date: pack.date,
                        symbol: symbol_norm,
                        outcome_class,
                        conviction: idea.conviction,
                        entry_zone_low: levels.entry_zone_low,
                        entry_zone_high: levels.entry_zone_high,
                        invalidation_lvl: levels.invalidation,
                        realized_high: realized.high,
                        realized_low: realized.low,
                        realized_close: realized.close,
                        eval_window_days: window,
                        prediction_id,
                    },
                )
                .await
                {
                    warn!(
                        "get_outcomes: persist failed for {} {}: {}",
                        pack.date, idea.symbol, e
                    );
                }
            }
        }
        Ok(())
    }
}

fn build_thesis_lookup(
    packs: &[agent_morning_packs::AgentMorningPack],
) -> std::collections::HashMap<(NaiveDate, String), String> {
    let mut out = std::collections::HashMap::new();
    for pack in packs {
        for idea in &pack.ranked_ideas {
            out.insert(
                (pack.date, idea.symbol.to_uppercase()),
                idea.thesis_md.clone(),
            );
        }
    }
    out
}

/// Convention for "skipped" predictions: a thesis whose first
/// non-whitespace token is `SKIP:` flags that the agent saw nothing
/// worth taking — record the row as `outcome_class=skipped` so the
/// eval harness can compute realized regret without dropping it.
fn idea_marked_skipped(idea: &RankedIdea) -> bool {
    idea.thesis_md
        .trim_start()
        .to_ascii_uppercase()
        .starts_with("SKIP:")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ibkr::types::historical::HistoricalBar;
    use crate::mcp::tools::test_support::{handler_for_db, make_db};
    use crate::services::agent_morning_packs::{NewAgentMorningPack, RankedIdea};
    use crate::services::research_notes::Conviction;

    fn idea(symbol: &str, entry: &str, inv: &str) -> RankedIdea {
        RankedIdea {
            symbol: symbol.into(),
            thesis_md: format!("thesis for {symbol}"),
            conviction: Some(Conviction::A),
            entry_zone: Some(entry.into()),
            invalidation: Some(inv.into()),
            evidence_refs: Vec::new(),
        }
    }

    /// Seed `bars_cache` directly so the get_outcomes tool's
    /// `historical_service` cache-first path is satisfied — the test
    /// support handler wires a `PanickingFetcher`, so any cache miss
    /// would surface as a panic.
    async fn seed_bar(
        db: &std::sync::Arc<crate::storage::Db>,
        symbol: &str,
        bar_time_yyyymmdd: &str,
        high: f64,
        low: f64,
        close: f64,
    ) {
        let bars = vec![HistoricalBar {
            time: bar_time_yyyymmdd.to_string(),
            open: close,
            high,
            low,
            close,
            volume: 1_000_000,
            wap: close,
            count: 1,
        }];
        // Reach into the cache layer via the public service: write a
        // bar with the right symbol/bar_size/timestamp. We use the
        // SQLite path directly since cache::write_cache is private.
        let bar_size = "1day".to_string();
        let symbol_owned = symbol.to_string();
        let bars_owned = bars.clone();
        db.with_conn(move |conn| {
            for bar in bars_owned {
                let ts = crate::ibkr::types::historical::parse_ibkr_time(&bar.time).unwrap();
                conn.execute(
                    "INSERT OR REPLACE INTO bars_cache \
                     (symbol, bar_size, bar_time, open, high, low, close, volume, wap) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    rusqlite::params![
                        symbol_owned,
                        bar_size,
                        ts,
                        bar.open,
                        bar.high,
                        bar.low,
                        bar.close,
                        bar.volume,
                        bar.wap
                    ],
                )?;
            }
            Ok(())
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn get_outcomes_invalid_since_errors() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .get_outcomes(Parameters(GetOutcomesArgs {
                since: "garbage".into(),
                eval_window_days: None,
            }))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }

    #[tokio::test]
    async fn get_outcomes_returns_empty_when_no_packs() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .get_outcomes(Parameters(GetOutcomesArgs {
                since: "2026-04-01".into(),
                eval_window_days: None,
            }))
            .await
            .expect("ok");
        let body = r.structured_content.expect("structured");
        assert_eq!(body["count"].as_u64().unwrap(), 0);
    }

    #[tokio::test]
    async fn get_outcomes_materializes_then_returns_rows() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db.clone());

        // Pack date is yesterday-ish; bars seeded for that date.
        let pack_date = (Utc::now() - ChronoDuration::days(1)).date_naive();
        let bar_str = pack_date.format("%Y%m%d").to_string();

        // Long idea 100-105 invalidation 95; bars hit 107 — hit_entry.
        agent_morning_packs::write_pack(
            &handler.db,
            NewAgentMorningPack {
                date: pack_date,
                ranked_ideas: vec![idea("TSLA", "100-105", "close < 95")],
                written_by: "agent_morning_sweep".into(),
            },
        )
        .await
        .unwrap();
        seed_bar(&db, "TSLA", &bar_str, 107.0, 99.0, 103.0).await;

        let r = handler
            .get_outcomes(Parameters(GetOutcomesArgs {
                since: pack_date.format("%Y-%m-%d").to_string(),
                eval_window_days: None,
            }))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let body = r.structured_content.expect("structured");
        assert_eq!(body["count"].as_u64().unwrap(), 1);
        let item = &body["items"][0];
        assert_eq!(item["symbol"], "TSLA");
        assert_eq!(item["outcome_class"], "hit_entry");
        assert_eq!(item["thesis_md"], "thesis for TSLA");

        // Idempotent: a second call doesn't dup rows.
        let r2 = handler
            .get_outcomes(Parameters(GetOutcomesArgs {
                since: pack_date.format("%Y-%m-%d").to_string(),
                eval_window_days: None,
            }))
            .await
            .expect("ok");
        let body2 = r2.structured_content.expect("structured");
        assert_eq!(body2["count"].as_u64().unwrap(), 1);
    }

    #[tokio::test]
    async fn get_outcomes_skipped_idea_records_skipped() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db.clone());

        let pack_date = (Utc::now() - ChronoDuration::days(1)).date_naive();
        let bar_str = pack_date.format("%Y%m%d").to_string();
        let mut skip_idea = idea("AAPL", "100-105", "close < 95");
        skip_idea.thesis_md = "SKIP: nothing worth taking today".into();
        agent_morning_packs::write_pack(
            &handler.db,
            NewAgentMorningPack {
                date: pack_date,
                ranked_ideas: vec![skip_idea],
                written_by: "agent_morning_sweep".into(),
            },
        )
        .await
        .unwrap();
        seed_bar(&db, "AAPL", &bar_str, 200.0, 50.0, 100.0).await;

        let r = handler
            .get_outcomes(Parameters(GetOutcomesArgs {
                since: pack_date.format("%Y-%m-%d").to_string(),
                eval_window_days: None,
            }))
            .await
            .expect("ok");
        let body = r.structured_content.expect("structured");
        assert_eq!(body["count"].as_u64().unwrap(), 1);
        assert_eq!(body["items"][0]["outcome_class"], "skipped");
    }
}
