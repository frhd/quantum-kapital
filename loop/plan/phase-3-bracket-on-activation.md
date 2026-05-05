# Phase 3 — Bracket-on-Activation

> Part of [Quantum Kapital → Quant-Decisions-In-Code](master.md). See index for invariants.

**Status:** in-progress (started 2026-05-05)

**Depends on:** 1, 2

**Goal:** When the trader takes a setup, the parent entry, stop loss, and target(s) are placed atomically as an IBKR bracket. No manual stop typing in TWS, no "I'll add the stop in a sec" gap. Eliminates the largest behavioral leak revealed by the existing tag taxonomy (`flat_close`, `chase_own_exit`, `discipline_on_loser`).

**Surveillance-only clarification.** The trader explicitly confirms each parent in our UI before send. The system attaches stop + target children to that confirmed parent. No scheduler, detector, agent, or LLM ever initiates a parent. This phase does NOT relax Hard Invariant 1 — it operationalizes the "human confirms once, system executes the exact plan on screen" pattern.

## Files

- New: `src-tauri/src/services/order_ticket/mod.rs` — `OrderTicket::with_brackets(setup_id, sizing, stop_price, targets: Vec<TargetSpec>) -> Result<TicketReceipt>`. Single chokepoint for setup-linked order submission.
- New: `src-tauri/src/services/order_ticket/types.rs` — `TargetSpec { price, qty_pct }`, `TicketReceipt { parent_order_id, child_order_ids, intent_id }`.
- Touches: `src-tauri/src/ibkr/client/orders.rs` — Add `place_bracket(parent, stop, targets)` that builds the IBKR parent + OCO children, transmits as one batch with `transmit=false` on parent until last child queued, then `transmit=true` on last to fire all.
- Touches: `src-tauri/src/ibkr/types/order.rs` — Bracket-aware order request type.
- New: `src-tauri/src/ibkr/commands/order_ticket.rs` — `order_ticket_take_setup(setup_id, override_qty: Option<i64>, override_stop: Option<f64>)`. Reads sizing from P1, intent recorded via P2, brackets sent via this phase.
- Touches: `src-tauri/src/storage/migrations/` — `bracket_groups` table: `parent_order_id`, `setup_id`, `intent_id`, `stop_order_id`, `target_order_ids JSON`, `placed_at`, `last_status`.
- Touches: `src-tauri/src/lib.rs` — Add CI-grep invariant comment block: `place_order` direct calls outside `OrderTicket` should be reviewed.
- New: `src/features/tracker/components/TakeSetupModal.tsx` — Confirmation modal: shows qty (from P1), stop price, target prices/percentages, dollar-risk, R-multiple. Trader clicks "Send" → bracket placed. Cancel = nothing happens.
- New: `src/shared/api/orderTicket.ts` — Tauri wrapper.
- Touches: `src/features/tracker/components/SetupCard.tsx` — "Take Setup" button opens `TakeSetupModal`. Disabled if `sizing` is None or `sizing_skipped` is set.

## Tauri commands exposed

| Command | Purpose |
|---|---|
| `order_ticket_take_setup` | Place bracket for a specific setup. Single human-confirmed entry point. |
| `order_ticket_status` | Read current status of a bracket group (open / filled / stopped / canceled / partial). |
| `order_ticket_cancel_bracket` | Cancel an open bracket group (parent + children). |

## Reuse

- P1 `RiskEngine::size` → `Sizing.qty`. Override allowed but logged.
- P2 `TcaService::record_intent` called inside `OrderTicket::with_brackets` before IBKR submission.
- Existing `IbkrClient::place_order` for child order primitives.
- Existing `EventEmitter` → emit `BracketPlaced`, `BracketStatusChanged`.

## Decisions to make in this phase

- **Default target structure.** Master committed: 50% at 1R + 30% at 2R + 20% runner with ATR-trail. **Decision: ship with 50/30/20 fixed; ATR-trail logic is P7. Phase 3 ships static target prices for all three.** Runner gets a hard 3R limit until P7 trailing lands.
- **OCO semantics.** IBKR brackets are OCA-grouped — fill on a target reduces stop qty proportionally. Test with paper account before merge. **Decision: rely on IBKR's native OCA group with `oca_type = 1` (cancel-with-block); do NOT roll our own OCO.**
- **Override-qty audit.** Trader may override the system's qty in the modal. **Decision: allow override, but persist `qty_override_reason` (free text) and `system_qty` alongside `actual_qty`.** Reviewable in trader-profile.
- **Outside-RTH behavior.** Setups can fire pre-market. **Decision: brackets only submit during RTH; pre-RTH "Take Setup" queues a deferred ticket that fires at the open or expires at 09:35 ET if conditions changed.**
- **Risk-engine staleness.** If `Sizing.equity_at_decision` is from a snapshot > 1 trading day old, the modal must show a "stale equity" banner and require a force-refresh click before send. **Decision: hard block — modal "Send" disabled until snapshot < 24h.**

## Exit criteria

- `cargo test order_ticket::` passes against `MockIbkrClient` extended with bracket simulation: clean fill, stop-out, target-1 partial, full target sweep, manual cancel.
- Paper-account E2E (manual run, documented): take a real setup on IBKR paper, observe parent + 3 children visible in TWS as one OCA group, fill leg by leg, observe stop qty reducing.
- `bracket_groups` row written for every successful submission with all child order IDs populated.
- Frontend `TakeSetupModal` shows correct qty/stop/targets/dollar-risk; refuses to submit when sizing missing or stale; emits cancellation cleanly.
- CI grep tightened: `place_order(` direct calls in `services/` or `strategies/` (excluding `services/order_ticket/`) fail CI.
- Integration test: tracer-bullet from setup detection → P1 sizing → P2 intent → P3 bracket placement → mock fill → P2 slippage capture → executions row with `setup_id` populated.

## Gotchas

- **IBKR transmit-flag dance.** Parent must be submitted with `transmit=false`, children with `parent_id = parent.order_id`, last child with `transmit=true`. Getting this wrong submits the parent solo. Test against paper.
- **Order ID monotonicity.** IBKR requires monotonically increasing client-side order IDs. The wrapper must reserve IDs atomically — racing two `with_brackets` calls breaks the bracket.
- **Cancel-on-fill behavior.** When target-1 fills 50% qty, IBKR auto-reduces the stop to remaining 50%. Our `bracket_groups.last_status` updater must reconcile against IBKR's `orderStatus` events, not assume.
- **OCA vs OCO terminology.** IBKR uses OCA (One-Cancels-All). Document the chosen `oca_type` (1 vs 2 vs 3) — semantics differ on partial fill cancellation.
- **Modal escape hatch.** If a trader is mid-modal and the setup invalidates (price drifts past stop), modal must re-poll and disable send. Don't trust the modal-render-time prices.
- **Idempotency.** Network blip during submit could leave parent placed but children un-placed. `OrderTicket` must persist the parent order ID before attempting children, and on retry resume from the persisted state — never re-submit a parent.
- **Surveillance invariant violation footgun.** A future "auto-take-A-conviction-setups" feature is forbidden by master Hard Invariant 1. If anyone proposes it, the call site for `order_ticket_take_setup` must remain a Tauri command (UI-side), never a service-internal call.
- **Paper vs live config.** TWS paper accounts have different OCA semantics than live in some edge cases (per IBKR docs). Test both before declaring P3 done.
