use ibapi::contracts::Contract;
use ibapi::orders::Order;

use crate::ibkr::error::{IbkrError, Result};
use crate::ibkr::types::{IbkrExecution, OrderAction, OrderRequest, OrderType};

use super::executions_merge::merge_commission_reports;
use super::IbkrClient;

impl IbkrClient {
    pub async fn place_order(&self, order_request: OrderRequest) -> Result<i32> {
        let client_clone = self.ibapi_client().await?;

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
    /// querying a past date returns an empty list. The drain reads the entire
    /// subscription before returning, then merges `ExecutionData` and
    /// `CommissionReport` events by `execution_id` so each returned row
    /// carries its commission and (for closing legs) realized P&L. See
    /// [`merge_commission_reports`] for the merge semantics.
    pub async fn executions(&self, date: chrono::NaiveDate) -> Result<Vec<IbkrExecution>> {
        use ibapi::orders::ExecutionFilter;

        let client_clone = self.ibapi_client().await?;

        let date_yyyymmdd = date.format("%Y%m%d").to_string();

        let executions = tokio::task::spawn_blocking(move || -> Result<Vec<IbkrExecution>> {
            let filter = ExecutionFilter {
                specific_dates: vec![date_yyyymmdd],
                ..ExecutionFilter::default()
            };

            let subscription = client_clone.executions(filter).map_err(IbkrError::from)?;

            let events: Vec<_> = subscription.iter().collect();
            // Orphans (CommissionReport without a matching ExecutionData)
            // are warned about inside the merge; we drop the
            // `orphan_commission_ids` field on the floor here because the
            // production drain has no other use for it.
            Ok(merge_commission_reports(events, date).rows)
        })
        .await
        .map_err(|e| IbkrError::Unknown(e.to_string()))??;

        Ok(executions)
    }
}
