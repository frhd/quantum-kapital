//! Phase 2 — TCA Tauri commands.
//!
//! Read-only attribution + slippage distribution for the
//! trade-review surface, plus a write rail for retroactive manual
//! intents (out-of-band TWS fills the trader wants to attribute).
//!
//! `tca_record_manual_intent` is a Tauri command **only** — never
//! exposed over MCP. The MCP surface stays read-only-plus-ack
//! (master Hard Invariant; CLAUDE.md). MCP can read attribution +
//! slippage if needed in a future tool, but it cannot write
//! intents.

use std::sync::Arc;

use chrono::{Duration, NaiveDate, Utc};
use tauri::State;

use crate::ibkr::state::IbkrState;
use crate::services::tca::{
    AttributionRow, IntendedPriceSource, IntentSide, NewOrderIntent, SlippageDistributionRow,
    TcaService,
};

fn parse_iso_date(date: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|e| format!("invalid date '{date}', expected YYYY-MM-DD: {e}"))
}

#[tauri::command]
pub async fn tca_get_attribution(
    tca: State<'_, Arc<TcaService>>,
    state: State<'_, IbkrState>,
    date_from: String,
    date_to: String,
    account: Option<String>,
) -> Result<Vec<AttributionRow>, String> {
    let from = parse_iso_date(&date_from)?;
    let to = parse_iso_date(&date_to)?;
    if to < from {
        return Err("date_to must be >= date_from".to_string());
    }
    let resolved = resolve_account_arg(&state, account.as_deref()).await?;
    tca.inner()
        .attribution()
        .attribution(from, to, &resolved)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn tca_get_slippage_distribution(
    tca: State<'_, Arc<TcaService>>,
    state: State<'_, IbkrState>,
    date_from: String,
    date_to: String,
    account: Option<String>,
) -> Result<Vec<SlippageDistributionRow>, String> {
    let from = parse_iso_date(&date_from)?;
    let to = parse_iso_date(&date_to)?;
    if to < from {
        return Err("date_to must be >= date_from".to_string());
    }
    let resolved = resolve_account_arg(&state, account.as_deref()).await?;
    tca.inner()
        .attribution()
        .slippage_distribution(from, to, &resolved, None)
        .await
        .map_err(|e| e.to_string())
}

/// Wire DTO for `tca_record_manual_intent`. The intent is built
/// server-side from these fields — `intent_id` is generated, the
/// expiry window defaults to 60 min, and the source is fixed to
/// `Manual`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ManualIntentArgs {
    pub setup_id: Option<i64>,
    pub symbol: String,
    pub side: String, // "buy" | "sell"
    pub qty: f64,
    pub intended_price: f64,
    /// Optional override; defaults to "first managed account".
    pub account: Option<String>,
}

#[tauri::command]
pub async fn tca_record_manual_intent(
    tca: State<'_, Arc<TcaService>>,
    state: State<'_, IbkrState>,
    args: ManualIntentArgs,
) -> Result<String, String> {
    let side = IntentSide::parse(&args.side)
        .ok_or_else(|| format!("invalid side '{}', expected 'buy' or 'sell'", args.side))?;
    if !args.qty.is_finite() || args.qty <= 0.0 {
        return Err("qty must be > 0".to_string());
    }
    if !args.intended_price.is_finite() || args.intended_price <= 0.0 {
        return Err("intended_price must be > 0".to_string());
    }
    let account = resolve_account_arg(&state, args.account.as_deref()).await?;
    let now = Utc::now();
    let intent_id = format!(
        "intent_manual_{}_{}",
        now.timestamp_nanos_opt().unwrap_or(0),
        args.symbol.replace(' ', "_"),
    );
    let new_intent = NewOrderIntent {
        intent_id: intent_id.clone(),
        setup_id: args.setup_id,
        account,
        symbol: args.symbol,
        side,
        qty: args.qty,
        intended_price_cents: (args.intended_price * 100.0).round() as i64,
        intended_price_source: IntendedPriceSource::Manual,
        posted_at: now,
        expires_at: now + Duration::minutes(60),
    };
    tca.inner()
        .record_intent(new_intent)
        .await
        .map_err(|e| e.to_string())?;
    Ok(intent_id)
}

async fn resolve_account_arg(state: &IbkrState, requested: Option<&str>) -> Result<String, String> {
    if let Some(a) = requested {
        return Ok(a.to_string());
    }
    let accounts = state
        .client
        .get_accounts()
        .await
        .map_err(|e| e.to_string())?;
    if accounts.len() == 1 {
        return Ok(accounts.into_iter().next().unwrap());
    }
    if accounts.is_empty() {
        return Err("no IBKR accounts available".to_string());
    }
    Err(format!(
        "multiple accounts available: {}; pass `account` arg",
        accounts.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_iso_date_accepts_canonical_format() {
        assert!(parse_iso_date("2026-05-04").is_ok());
        assert!(parse_iso_date("2026/05/04").is_err());
        assert!(parse_iso_date("not a date").is_err());
    }
}
