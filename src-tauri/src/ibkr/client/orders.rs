use std::sync::Arc;

use ibapi::contracts::Contract;
use ibapi::orders::Order;
use tracing::warn;

use crate::ibkr::error::{IbkrError, Result};
use crate::ibkr::types::{ExecutionSide, IbkrExecution, OrderAction, OrderRequest, OrderType};

use super::IbkrClient;

impl IbkrClient {
    pub async fn place_order(&self, order_request: OrderRequest) -> Result<i32> {
        let client_clone = {
            let client = self.client.read().await;
            let client = client.as_ref().ok_or(IbkrError::NotConnected)?;
            Arc::clone(client)
        }; // Lock is dropped here!

        let order_id = tokio::task::spawn_blocking(move || {
            let contract = Contract::stock(&order_request.symbol).build();
            let order_id = client_clone.next_order_id();

            let mut order = Order::default();

            // Set action and order type using the ibapi types
            use ibapi::orders::Action;

            order.action = match order_request.action {
                OrderAction::Buy => Action::Buy,
                OrderAction::Sell => Action::Sell,
            };

            order.total_quantity = order_request.quantity;

            // Set order type - Type is likely a string in ibapi
            match order_request.order_type {
                OrderType::Market => {
                    order.order_type = "MKT".to_string();
                }
                OrderType::Limit => {
                    order.order_type = "LMT".to_string();
                    order.limit_price = order_request.price;
                }
                _ => {
                    return Err(IbkrError::RequestFailed(
                        "Order type not implemented".to_string(),
                    ))
                }
            };

            match client_clone.place_order(order_id, &contract, &order) {
                Ok(_subscription) => {
                    // TODO: Handle order status updates
                    Ok(order_id)
                }
                Err(e) => Err(IbkrError::from(e)),
            }
        })
        .await
        .map_err(|e| IbkrError::Unknown(e.to_string()))?;

        order_id
    }

    /// Returns the day's executions filtered to the requested ET trading date.
    ///
    /// IBKR's `reqExecutions` only delivers fills from the current TWS-day, so
    /// querying a past date returns an empty list. We still parse the IBKR
    /// timestamp through `America/New_York` to keep the contract honest at
    /// the boundary of midnight UTC.
    pub async fn executions(&self, date: chrono::NaiveDate) -> Result<Vec<IbkrExecution>> {
        use chrono_tz::America::New_York;
        use ibapi::orders::{ExecutionFilter, Executions as IbExecutions};

        let client_clone = {
            let client = self.client.read().await;
            let client = client.as_ref().ok_or(IbkrError::NotConnected)?;
            Arc::clone(client)
        };

        let date_yyyymmdd = date.format("%Y%m%d").to_string();

        let executions = tokio::task::spawn_blocking(move || -> Result<Vec<IbkrExecution>> {
            let filter = ExecutionFilter {
                specific_dates: vec![date_yyyymmdd],
                ..ExecutionFilter::default()
            };

            let subscription = client_clone.executions(filter).map_err(IbkrError::from)?;

            let mut out = Vec::new();
            for msg in &subscription {
                match msg {
                    IbExecutions::ExecutionData(data) => {
                        let Some(exec_time) = parse_ibkr_exec_time(&data.execution.time) else {
                            warn!(
                                "Skipping execution with unparsable time: {}",
                                data.execution.time
                            );
                            continue;
                        };
                        // Filter to the requested ET date (defense-in-depth: the
                        // server filter should already constrain this, but we
                        // double-check here in case TWS returns adjacent dates).
                        if exec_time.with_timezone(&New_York).date_naive() != date {
                            continue;
                        }
                        let side = match data.execution.side.as_str() {
                            "BOT" => ExecutionSide::Bought,
                            "SLD" => ExecutionSide::Sold,
                            other => {
                                warn!("Unknown execution side '{other}', defaulting to Bought");
                                ExecutionSide::Bought
                            }
                        };
                        out.push(IbkrExecution {
                            symbol: data.contract.symbol.0.clone(),
                            side,
                            qty: data.execution.shares,
                            avg_price: data.execution.average_price,
                            exec_time,
                            order_id: data.execution.order_id,
                            exec_id: data.execution.execution_id.clone(),
                        });
                    }
                    IbExecutions::CommissionReport(_) => {}
                    IbExecutions::Notice(notice) => {
                        warn!("Executions stream notice: {notice:?}");
                    }
                }
            }
            Ok(out)
        })
        .await
        .map_err(|e| IbkrError::Unknown(e.to_string()))??;

        Ok(executions)
    }
}

/// Parses IBKR's execution timestamp (`yyyymmdd  hh:mm:ss [TZ]`) into UTC.
///
/// IBKR emits trailing whitespace and an optional timezone suffix; we treat
/// the bare datetime as ET because that's the contract for `reqExecutions`.
fn parse_ibkr_exec_time(raw: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    use chrono::{NaiveDateTime, TimeZone};
    use chrono_tz::America::New_York;

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
        .map(|dt| dt.with_timezone(&chrono::Utc))
}
