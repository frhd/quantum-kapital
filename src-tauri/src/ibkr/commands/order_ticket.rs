//! Phase 3 — Tauri commands wrapping `services::order_ticket::OrderTicket`.
//!
//! Master Hard Invariant 1: only the Tauri-command-side path may
//! initiate a parent order. `order_ticket_take_setup` here is the
//! single place that calls `OrderTicket::with_brackets`; service-
//! internal call sites are forbidden by the CI grep invariant in
//! `scripts/ci/check-place-order-chokepoint.sh`.

use std::sync::Arc;

use tauri::State;

use crate::services::order_ticket::{
    BracketGroupRecord, OrderTicket, OrderTicketError, TakeSetupArgs, TicketReceipt,
};

#[tauri::command]
pub async fn order_ticket_take_setup(
    setup_id: i64,
    override_qty: Option<u32>,
    override_stop: Option<f64>,
    override_reason: Option<String>,
    ticket: State<'_, Arc<OrderTicket>>,
) -> Result<TicketReceipt, String> {
    ticket
        .with_brackets(TakeSetupArgs {
            setup_id,
            override_qty,
            override_stop_price: override_stop,
            override_reason,
        })
        .await
        .map_err(map_err)
}

#[tauri::command]
pub async fn order_ticket_status(
    parent_order_id: i32,
    ticket: State<'_, Arc<OrderTicket>>,
) -> Result<Option<BracketGroupRecord>, String> {
    ticket.status(parent_order_id).await.map_err(map_err)
}

#[tauri::command]
pub async fn order_ticket_cancel_bracket(
    parent_order_id: i32,
    ticket: State<'_, Arc<OrderTicket>>,
) -> Result<BracketGroupRecord, String> {
    ticket.cancel(parent_order_id).await.map_err(map_err)
}

fn map_err(e: OrderTicketError) -> String {
    e.to_string()
}
