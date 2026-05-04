//! Pure merge-and-shape logic for IBKR's `executions` subscription stream.
//!
//! IBKR delivers two distinct event types for every fill — a
//! `ExecutionData` carrying the contract + price + qty, and a
//! `CommissionReport` carrying the fee + realized P&L — and they may
//! arrive in either order. The drain in
//! [`crate::ibkr::client::orders::IbkrClient::executions`] collects both
//! into a `Vec<Executions>` and hands it here for reconciliation.
//!
//! Splitting this out keeps the live blocking subscription code thin
//! enough to read and the merge logic unit-testable without standing up
//! a real TWS connection.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::America::New_York;
use ibapi::orders::{CommissionReport, ExecutionData, Executions as IBExecutions};
use tracing::{debug, warn};

use crate::ibkr::types::{ExecutionSide, IbkrExecution};

/// Outcome of merging an `Executions` event stream.
///
/// `rows` is the user-visible per-leg list (sorted ascending by
/// `exec_time`). `orphan_commission_ids` is the list of commission
/// reports whose `execution_id` never appeared as an `ExecutionData`
/// within the same drain — surfaced so callers can assert on the warn
/// path without depending on a tracing subscriber capture.
#[derive(Debug, Default)]
pub(super) struct MergeResult {
    pub rows: Vec<IbkrExecution>,
    /// Surfaced for tests; the production drain logs the warn inside
    /// the merge and discards this list.
    #[cfg_attr(not(test), allow(dead_code))]
    pub orphan_commission_ids: Vec<String>,
}

#[derive(Debug, Default)]
struct Buffer {
    exec: Option<ExecutionData>,
    commission: Option<CommissionReport>,
}

/// Reconciles a drained `Executions` event stream into per-leg
/// `IbkrExecution` rows, filtered to `target_date` (interpreted as an
/// ET trading day).
pub(super) fn merge_commission_reports(
    events: Vec<IBExecutions>,
    target_date: NaiveDate,
) -> MergeResult {
    let mut buffers: HashMap<String, Buffer> = HashMap::new();

    for event in events {
        match event {
            IBExecutions::ExecutionData(data) => {
                let id = data.execution.execution_id.clone();
                buffers.entry(id).or_default().exec = Some(data);
            }
            IBExecutions::CommissionReport(report) => {
                let id = report.execution_id.clone();
                buffers.entry(id).or_default().commission = Some(report);
            }
            IBExecutions::Notice(notice) => {
                warn!("Executions stream notice: {notice:?}");
            }
        }
    }

    let mut rows = Vec::new();
    let mut orphan_commission_ids = Vec::new();

    for (exec_id, buf) in buffers {
        match buf.exec {
            Some(data) => {
                if let Some(row) = build_row(data, buf.commission, target_date) {
                    rows.push(row);
                }
            }
            None => {
                if buf.commission.is_some() {
                    warn!(
                        execution_id = %exec_id,
                        "CommissionReport received without matching ExecutionData; dropping"
                    );
                    orphan_commission_ids.push(exec_id);
                }
            }
        }
    }

    rows.sort_by_key(|row| row.exec_time);

    MergeResult {
        rows,
        orphan_commission_ids,
    }
}

fn build_row(
    data: ExecutionData,
    commission: Option<CommissionReport>,
    target_date: NaiveDate,
) -> Option<IbkrExecution> {
    let Some(exec_time) = parse_ibkr_exec_time(&data.execution.time) else {
        warn!(
            "Skipping execution with unparsable time: {}",
            data.execution.time
        );
        return None;
    };

    if exec_time.with_timezone(&New_York).date_naive() != target_date {
        return None;
    }

    let side = match data.execution.side.as_str() {
        "BOT" => ExecutionSide::Bought,
        "SLD" => ExecutionSide::Sold,
        other => {
            warn!("Unknown execution side '{other}', defaulting to Bought");
            ExecutionSide::Bought
        }
    };

    let contract_type = data.contract.security_type.to_string();
    let is_option_like = matches!(contract_type.as_str(), "OPT" | "FOP");

    let expiry = if is_option_like {
        parse_option_expiry(&data.contract.last_trade_date_or_contract_month)
    } else {
        None
    };
    let strike = if is_option_like {
        Some(data.contract.strike)
    } else {
        None
    };
    let right = if is_option_like {
        Some(normalize_right(&data.contract.right))
    } else {
        None
    };
    let multiplier = if is_option_like && !data.contract.multiplier.is_empty() {
        Some(data.contract.multiplier.clone())
    } else {
        None
    };

    let currency = if data.contract.currency.0.is_empty() {
        None
    } else {
        Some(data.contract.currency.0.clone())
    };

    let (commission_amount, realized_pnl, commission_currency) = match commission {
        Some(report) => {
            debug!(
                execution_id = %report.execution_id,
                commission = report.commission,
                "merging commission report into fill"
            );
            let cc = if report.currency.is_empty() {
                None
            } else {
                Some(report.currency)
            };
            (Some(report.commission), report.realized_pnl, cc)
        }
        None => (None, None, None),
    };

    Some(IbkrExecution {
        symbol: data.contract.symbol.0.clone(),
        side,
        qty: data.execution.shares,
        avg_price: data.execution.average_price,
        exec_time,
        order_id: data.execution.order_id,
        exec_id: data.execution.execution_id.clone(),
        account: data.execution.account_number.clone(),
        contract_type,
        expiry,
        strike,
        right,
        multiplier,
        commission: commission_amount,
        realized_pnl,
        currency,
        commission_currency,
    })
}

/// Parses IBKR's execution timestamp (`yyyymmdd  hh:mm:ss [TZ]`) into UTC.
///
/// IBKR emits trailing whitespace and an optional timezone suffix; we
/// treat the bare datetime as ET because that's the contract for
/// `reqExecutions`.
pub(super) fn parse_ibkr_exec_time(raw: &str) -> Option<DateTime<Utc>> {
    let trimmed = raw.trim();
    // Strip optional trailing timezone token (e.g. "20260429 16:00:00 US/Eastern").
    let datetime_part = trimmed
        .rsplit_once(' ')
        .filter(|(_, suffix)| suffix.contains('/') || suffix.chars().all(|c| c.is_alphabetic()))
        .map(|(prefix, _)| prefix.trim())
        .unwrap_or(trimmed);

    // IBKR may collapse the double-space between date and time, so try both.
    let parsed = NaiveDateTime::parse_from_str(datetime_part, "%Y%m%d  %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(datetime_part, "%Y%m%d %H:%M:%S"))
        .or_else(|_| NaiveDateTime::parse_from_str(datetime_part, "%Y%m%d-%H:%M:%S"))
        .ok()?;

    New_York
        .from_local_datetime(&parsed)
        .single()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Parses the SDK's `last_trade_date_or_contract_month` field. Equity
/// options arrive as `YYYYMMDD`; some quarter/index futures arrive as
/// `YYYYMM` with no day, which we cannot represent as a `NaiveDate` —
/// callers see `None` and a `warn!` is emitted.
fn parse_option_expiry(raw: &str) -> Option<NaiveDate> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(d) = NaiveDate::parse_from_str(trimmed, "%Y%m%d") {
        return Some(d);
    }
    if trimmed.len() == 6 && trimmed.chars().all(|c| c.is_ascii_digit()) {
        warn!("Option expiry '{trimmed}' is YYYYMM with no day; surfacing as None");
        return None;
    }
    warn!("Unparseable option expiry '{trimmed}'; surfacing as None");
    None
}

/// Most venues report option `right` as `"C"` / `"P"`; some emit
/// `"CALL"` / `"PUT"`. Normalise to the single-letter form. Anything
/// else is logged and returned verbatim.
fn normalize_right(raw: &str) -> String {
    match raw.trim().to_ascii_uppercase().as_str() {
        "C" | "CALL" => "C".to_string(),
        "P" | "PUT" => "P".to_string(),
        "" => String::new(),
        other => {
            warn!("Unexpected option right '{other}'; storing raw value");
            raw.to_string()
        }
    }
}

#[cfg(test)]
#[path = "executions_merge_tests.rs"]
mod tests;
