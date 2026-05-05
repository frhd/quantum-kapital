use ibapi::contracts::Contract;
use ibapi::orders::Order;

use crate::ibkr::error::{IbkrError, Result};
use crate::ibkr::types::{
    BracketReceipt, BracketRequest, IbkrExecution, ModifyStopRequest, OrderAction, OrderRequest,
    OrderType,
};

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

    /// Phase 3 — place a parent + stop + N targets as one IBKR
    /// bracket. The transmit-flag dance is the load-bearing detail:
    /// every leg except the last child is submitted with
    /// `transmit=false`, so IBKR holds them queued until the final
    /// `transmit=true` flips the whole group live atomically. Getting
    /// this wrong submits the parent solo before the children are in
    /// place — an unattached entry order with no protective stop.
    ///
    /// All children share the parent's OCA group via `parent_id +
    /// oca_group + oca_type=1`, so a target fill auto-cancels the
    /// stop and a stop fill auto-cancels the targets. Partial-fill
    /// reductions are IBKR's responsibility; we don't model them
    /// client-side.
    pub async fn place_bracket(&self, req: BracketRequest) -> Result<BracketReceipt> {
        let client_clone = self.ibapi_client().await?;

        tokio::task::spawn_blocking(move || -> Result<BracketReceipt> {
            use ibapi::orders::Action;

            let contract = Contract::stock(&req.symbol).build();
            let parent_id = client_clone.next_order_id();
            let stop_id = client_clone.next_order_id();
            let target_ids: Vec<i32> = (0..req.target_rungs.len())
                .map(|_| client_clone.next_order_id())
                .collect();

            let entry_action = match req.entry_action {
                OrderAction::Buy => Action::Buy,
                OrderAction::Sell => Action::Sell,
            };
            let exit_action = match req.entry_action {
                OrderAction::Buy => Action::Sell,
                OrderAction::Sell => Action::Buy,
            };

            let oca_group = format!("br-{parent_id}");

            // Parent: LIMIT entry, transmit=false so children queue
            // before the group fires.
            let parent = Order {
                action: entry_action,
                total_quantity: req.qty,
                order_type: "LMT".to_string(),
                limit_price: Some(req.entry_limit_price),
                transmit: false,
                ..Order::default()
            };

            // Stop child: STOP, parent_id wired in, OCA group set.
            let stop = Order {
                action: exit_action,
                total_quantity: req.qty,
                order_type: "STP".to_string(),
                aux_price: Some(req.stop_price),
                parent_id,
                oca_group: oca_group.clone(),
                oca_type: ibapi::orders::OcaType::CancelWithBlock,
                transmit: false,
                ..Order::default()
            };

            // Submit parent + stop with transmit=false. The last
            // target child below carries transmit=true to fire the
            // batch.
            client_clone
                .place_order(parent_id, &contract, &parent)
                .map_err(IbkrError::from)?;
            client_clone
                .place_order(stop_id, &contract, &stop)
                .map_err(IbkrError::from)?;

            for (idx, (price, qty)) in req.target_rungs.iter().enumerate() {
                let target = Order {
                    action: exit_action,
                    total_quantity: *qty,
                    order_type: "LMT".to_string(),
                    limit_price: Some(*price),
                    parent_id,
                    oca_group: oca_group.clone(),
                    oca_type: ibapi::orders::OcaType::CancelWithBlock,
                    // Last leg fires the whole bracket.
                    transmit: idx + 1 == req.target_rungs.len(),
                    ..Order::default()
                };
                client_clone
                    .place_order(target_ids[idx], &contract, &target)
                    .map_err(IbkrError::from)?;
            }

            Ok(BracketReceipt {
                parent_order_id: parent_id,
                stop_order_id: stop_id,
                target_order_ids: target_ids,
            })
        })
        .await
        .map_err(|e| IbkrError::Unknown(e.to_string()))?
    }

    /// Phase 7 — modify an existing stop child's `aux_price`. Used by
    /// `BracketReviser` to step the trail as the position moves in
    /// the trader's favor. IBKR's API treats `place_order(order_id,
    /// contract, order)` as both a place AND a modify when the
    /// `order_id` is already known to the gateway, so this builds a
    /// fresh `Order` with the same OCA group / parent linkage and
    /// the new `aux_price`, then re-submits at the existing
    /// `order_id`.
    ///
    /// Submitted with `transmit=true` so the modify takes effect
    /// immediately. If `order_id` is unknown to the gateway IBKR
    /// will reject with `OrderRejected`; callers must verify the
    /// bracket is still open before modifying.
    pub async fn modify_stop_price(&self, req: ModifyStopRequest) -> Result<()> {
        let client_clone = self.ibapi_client().await?;

        tokio::task::spawn_blocking(move || -> Result<()> {
            use ibapi::orders::Action;

            let contract = Contract::stock(&req.symbol).build();
            let action = match req.action {
                OrderAction::Buy => Action::Buy,
                OrderAction::Sell => Action::Sell,
            };
            let stop = Order {
                action,
                total_quantity: req.qty,
                order_type: "STP".to_string(),
                aux_price: Some(req.new_stop_price),
                parent_id: req.parent_id,
                oca_group: req.oca_group,
                oca_type: ibapi::orders::OcaType::CancelWithBlock,
                transmit: true,
                ..Order::default()
            };
            client_clone
                .place_order(req.stop_order_id, &contract, &stop)
                .map_err(IbkrError::from)?;
            Ok(())
        })
        .await
        .map_err(|e| IbkrError::Unknown(e.to_string()))?
    }

    /// Returns the day's executions filtered to the requested ET trading date.
    ///
    /// Uses IBKR's `specific_dates` filter on `ExecutionFilter`, which retains
    /// roughly the last 7 trading days — anything older comes back empty. The
    /// drain reads the entire subscription before returning, then merges
    /// `ExecutionData` and `CommissionReport` events by `execution_id` so each
    /// returned row carries its commission and (for closing legs) realized
    /// P&L. See [`merge_commission_reports`] for the merge semantics.
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
