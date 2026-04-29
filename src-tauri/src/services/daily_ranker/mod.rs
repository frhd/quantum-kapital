//! Phase 20 — Daily ranker.
//!
//! After the EOD detector sweep, [`DailyRanker::rank_today`] sends today's
//! active setups to Claude Sonnet 4.6 in a single forced-tool call and
//! parses the structured response into a [`MorningPack`]. The pack is
//! persisted to `morning_packs` (one row per ET trading day, last write
//! wins) and emitted on `AppEvent::MorningPackReady`.
//!
//! Failure handling is intentionally graceful: every transient or
//! configuration LLM problem (`BudgetExhausted`, `Auth`, `Upstream`,
//! `Network`, `NoApiKey`, missing tool call, malformed tool input)
//! falls back to a naive top-N ranked by `conviction_signal`. The user
//! still gets a list — the ranker just never blocks the EOD pipeline.

#![allow(dead_code)] // exercised by EodScheduler (Phase 20) + tracker commands.

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use tracing::warn;

use crate::events::{AppEvent, EventEmitter};
use crate::ibkr::types::tracker::Setup;
use crate::services::llm_service::{
    LlmError, LlmKind, LlmRequest, LlmService, Message, Role, SystemBlock, ToolChoice, ToolSchema,
};
use crate::services::tracker_service::{TrackerError, TrackerService};
use crate::storage::{Db, StorageError};

#[cfg(test)]
mod tests;

pub const TOOL_NAME: &str = "emit_morning_pack";
pub const MODEL: &str = "claude-sonnet-4-6";
const MAX_TOKENS: u32 = 1024;

const SYSTEM_PROMPT: &str = include_str!("../llm_service/prompts/ranker_v1.md");
const TOOL_SCHEMA_JSON: &str = include_str!("../llm_service/prompts/ranker_tool.json");

#[derive(Error, Debug)]
pub enum RankerError {
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    #[error("tracker: {0}")]
    Tracker(#[from] TrackerError),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("malformed ranker tool input: {0}")]
    Malformed(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankedSetup {
    pub setup_id: i64,
    pub rank: u8,
    pub why_top_pick: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MorningPack {
    pub date: NaiveDate,
    pub ranked: Vec<RankedSetup>,
    pub generated_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct DailyRanker {
    llm: Arc<LlmService>,
    tracker: Arc<TrackerService>,
    db: Arc<Db>,
    emitter: Arc<EventEmitter>,
}

impl DailyRanker {
    pub fn new(
        llm: Arc<LlmService>,
        tracker: Arc<TrackerService>,
        db: Arc<Db>,
        emitter: Arc<EventEmitter>,
    ) -> Self {
        Self {
            llm,
            tracker,
            db,
            emitter,
        }
    }

    /// Build the `LlmRequest` for the supplied candidate setups + cap.
    /// Public so tests can assert on shape without invoking the network.
    pub fn build_request(setups: &[Setup], top_n: usize) -> LlmRequest {
        let payload = json!({
            "top_n": top_n,
            "setups": setups.iter().map(summarize_setup).collect::<Vec<_>>(),
        });

        LlmRequest {
            kind: LlmKind::Ranker,
            model: MODEL,
            max_tokens: MAX_TOKENS,
            system: vec![SystemBlock {
                text: SYSTEM_PROMPT.to_string(),
                cache: true,
            }],
            messages: vec![Message {
                role: Role::User,
                content: serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string()),
            }],
            tools: Some(vec![tool_schema()]),
            tool_choice: Some(ToolChoice::ForceTool(TOOL_NAME.to_string())),
            setup_id: None,
        }
    }

    /// Parse the `emit_morning_pack` tool input into ranked entries.
    /// Public for direct testing of the parsing path.
    pub fn parse_ranked(input: &Value, top_n: usize) -> Result<Vec<RankedSetup>, RankerError> {
        let arr = input
            .get("ranked")
            .and_then(|v| v.as_array())
            .ok_or_else(|| RankerError::Malformed("missing ranked array".into()))?;
        let mut out: Vec<RankedSetup> = Vec::with_capacity(arr.len());
        for entry in arr {
            let setup_id = entry
                .get("setup_id")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| RankerError::Malformed("missing setup_id".into()))?;
            let rank = entry
                .get("rank")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| RankerError::Malformed("missing rank".into()))?;
            let why = entry
                .get("why_top_pick")
                .and_then(|v| v.as_str())
                .ok_or_else(|| RankerError::Malformed("missing why_top_pick".into()))?
                .to_string();
            out.push(RankedSetup {
                setup_id,
                rank: rank.min(u8::MAX as u64) as u8,
                why_top_pick: why,
            });
        }
        out.sort_by_key(|r| r.rank);
        out.truncate(top_n);
        Ok(out)
    }

    /// Rank today's active setups. Steps:
    /// 1. Pull all `Active` setups detected on `date` (ET).
    /// 2. If none → emit empty `MorningPackReady` and return.
    /// 3. Call the LLM ranker; on any failure fall back to naive
    ///    top-N by `conviction_signal`.
    /// 4. Persist to `morning_packs` (date → JSON, latest wins).
    /// 5. Emit `AppEvent::MorningPackReady { date, ranked_count }`.
    pub async fn rank_today(
        &self,
        date: NaiveDate,
        top_n: usize,
    ) -> Result<MorningPack, RankerError> {
        let setups = self.todays_active_setups(date).await?;

        if setups.is_empty() {
            let pack = MorningPack {
                date,
                ranked: Vec::new(),
                generated_at: Utc::now(),
            };
            self.persist(&pack).await?;
            self.emit(date, 0).await;
            return Ok(pack);
        }

        let ranked = match self.rank_via_llm(&setups, top_n).await {
            Some(r) if !r.is_empty() => r,
            _ => naive_ranking(&setups, top_n),
        };

        let pack = MorningPack {
            date,
            ranked,
            generated_at: Utc::now(),
        };
        self.persist(&pack).await?;
        self.emit(date, pack.ranked.len()).await;
        Ok(pack)
    }

    async fn rank_via_llm(&self, setups: &[Setup], top_n: usize) -> Option<Vec<RankedSetup>> {
        let request = Self::build_request(setups, top_n);
        let response = match self.llm.message(request).await {
            Ok(r) => r,
            Err(e) => {
                handle_llm_error(e);
                return None;
            }
        };
        let tool_call = response
            .tool_calls
            .into_iter()
            .find(|c| c.name == TOOL_NAME)?;
        match Self::parse_ranked(&tool_call.input, top_n) {
            Ok(r) => Some(r),
            Err(e) => {
                warn!("ranker tool input failed to parse: {e}");
                None
            }
        }
    }

    async fn todays_active_setups(&self, date: NaiveDate) -> Result<Vec<Setup>, RankerError> {
        // ET trading day: 04:00 ET → 20:00 ET roughly covers detection
        // windows. We use 00:00 ET → 24:00 ET so any setup whose
        // `detected_at` lands inside the ET calendar day is included.
        let et_offset = chrono::FixedOffset::west_opt(5 * 3600).expect("ET offset");
        let day_start = date
            .and_hms_opt(0, 0, 0)
            .expect("valid hms")
            .and_local_timezone(et_offset)
            .single()
            .expect("unambiguous local")
            .with_timezone(&Utc);
        let day_end = day_start + chrono::Duration::hours(24);

        let all = self.tracker.list_setups(None, Some(day_start)).await?;
        Ok(all
            .into_iter()
            .filter(|s| {
                s.detected_at < day_end
                    && matches!(s.status, crate::ibkr::types::tracker::SetupStatus::Active)
            })
            .collect())
    }

    pub async fn get_pack(&self, date: NaiveDate) -> Result<Option<MorningPack>, RankerError> {
        let date_str = date.to_string();
        let row = self
            .db
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT payload FROM morning_packs WHERE date = ?1",
                    rusqlite::params![date_str],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(StorageError::from)
            })
            .await?;

        match row {
            Some(json) => Ok(Some(serde_json::from_str(&json)?)),
            None => Ok(None),
        }
    }

    pub async fn get_latest(&self) -> Result<Option<MorningPack>, RankerError> {
        let row = self
            .db
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT payload FROM morning_packs ORDER BY date DESC LIMIT 1",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(StorageError::from)
            })
            .await?;

        match row {
            Some(json) => Ok(Some(serde_json::from_str(&json)?)),
            None => Ok(None),
        }
    }

    async fn persist(&self, pack: &MorningPack) -> Result<(), RankerError> {
        let date_str = pack.date.to_string();
        let payload = serde_json::to_string(pack)?;
        let generated_at = pack.generated_at.timestamp();
        self.db
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO morning_packs (date, payload, generated_at) \
                     VALUES (?1, ?2, ?3) \
                     ON CONFLICT(date) DO UPDATE SET payload = excluded.payload, \
                                                    generated_at = excluded.generated_at",
                    rusqlite::params![date_str, payload, generated_at],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    async fn emit(&self, date: NaiveDate, ranked_count: usize) {
        if let Err(e) = self
            .emitter
            .emit(AppEvent::MorningPackReady { date, ranked_count })
            .await
        {
            warn!("MorningPackReady emit failed: {e}");
        }
    }
}

fn handle_llm_error(e: LlmError) {
    match &e {
        LlmError::BudgetExhausted => warn!("ranker budget exhausted; falling back to naive top-N"),
        LlmError::Auth | LlmError::NoApiKey => {
            warn!("ranker LLM auth/key issue; falling back to naive top-N: {e}")
        }
        LlmError::Upstream { .. } | LlmError::Network(_) => {
            warn!("ranker LLM transport failure; falling back to naive top-N: {e}")
        }
        LlmError::Malformed(_) | LlmError::UnknownModel(_) => {
            warn!("ranker LLM response invalid; falling back to naive top-N: {e}")
        }
        LlmError::Storage(_) | LlmError::Serde(_) => {
            warn!("ranker LLM internal error; falling back to naive top-N: {e}")
        }
    }
}

fn naive_ranking(setups: &[Setup], top_n: usize) -> Vec<RankedSetup> {
    let mut indexed: Vec<&Setup> = setups.iter().collect();
    indexed.sort_by(|a, b| {
        let signal = |s: &Setup| -> f64 {
            s.raw_signals
                .get("conviction_signal")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0)
        };
        signal(b)
            .partial_cmp(&signal(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    indexed
        .into_iter()
        .take(top_n)
        .enumerate()
        .map(|(i, s)| RankedSetup {
            setup_id: s.id,
            rank: (i + 1) as u8,
            why_top_pick: "Fallback ranking — LLM ranker unavailable; sorted by detector \
                            conviction signal."
                .to_string(),
        })
        .collect()
}

fn summarize_setup(s: &Setup) -> Value {
    // Pull conviction grade from thesis_json when available.
    let conviction_letter = s
        .thesis_json
        .as_ref()
        .and_then(|j| j.get("conviction"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    json!({
        "setup_id": s.id,
        "symbol": s.symbol,
        "strategy": s.strategy,
        "direction": s.direction,
        "trigger_price": s.trigger_price,
        "stop_price": s.stop_price,
        "targets": s.targets,
        "raw_signals": s.raw_signals,
        "thesis_md": s.thesis,
        "conviction_letter": conviction_letter,
        "detected_at": s.detected_at,
    })
}

fn tool_schema() -> ToolSchema {
    let parsed: Value =
        serde_json::from_str(TOOL_SCHEMA_JSON).expect("ranker_tool.json is valid JSON");
    ToolSchema {
        name: parsed
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(TOOL_NAME)
            .to_string(),
        description: parsed
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("Emit the ranked morning pack.")
            .to_string(),
        input_schema: parsed
            .get("input_schema")
            .cloned()
            .unwrap_or_else(|| json!({})),
    }
}
