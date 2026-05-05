# Phase 2 — Setup ↔ Execution linkage + TCA

> Part of [Quantum Kapital → Quant-Decisions-In-Code](master.md). See index for invariants.

**Status:** in-progress (started 2026-05-05)

**Depends on:** none (foundation phase)

**Goal:** Tie every fill back to the setup it answers, capture arrival-vs-fill slippage in basis points, and unlock per-strategy attribution. Today there's a SQL gap: `setups` and `executions` share no foreign key. After this phase, the realized PnL of every executed setup is queryable by detector class, conviction, symbol, and time-of-day.

## Files

- New: `src-tauri/src/services/tca/mod.rs` — `TcaService`: `record_intent(...)`, `attach_fill(...)`, `slippage_bps(...)`.
- New: `src-tauri/src/services/tca/intent.rs` — `OrderIntent { intent_id, setup_id, symbol, side, qty, intended_price, posted_at }`. The intent is the linkage primary key.
- New: `src-tauri/src/services/tca/types.rs` — `SlippageRecord`, `AttributionRow`.
- Touches: `src-tauri/src/services/executions/mod.rs` — When a fill arrives, look up matching open `OrderIntent` (by symbol + side + qty + time window) and write `setup_id` / `intended_price` / `slippage_bps` columns.
- Touches: `src-tauri/src/services/trade_legs/` — FIFO matcher gains `strategy: Option<String>` and `setup_id: Option<i64>` carryover from underlying executions.
- Touches: `src-tauri/src/storage/migrations/` — Migrations:
  - `executions` adds `setup_id INTEGER`, `intent_id TEXT`, `intended_price_cents INTEGER`, `slippage_bps INTEGER`, `slippage_signed INTEGER`.
  - `order_intents` new table: PK `intent_id`, indexed by `setup_id`, `symbol`, `posted_at`.
  - `trade_legs` adds `strategy TEXT`, `setup_id INTEGER` (nullable, NULL for pre-P2 legs).
- Touches: `src-tauri/src/ibkr/commands/orders.rs` — When user places an order through our UI, accept optional `setup_id` argument; record intent before sending to IBKR.
- New: `src-tauri/src/services/tca/attribution.rs` — SQL views: per-strategy realized PnL, win-rate, avg slippage, profit-factor.
- New: `src/features/trade-review/components/TcaPanel.tsx` — Slippage histogram by strategy + symbol-liquidity bucket.
- New: `src/shared/api/tca.ts` — `tca_get_attribution`, `tca_get_slippage_distribution`.

## Tauri commands exposed

| Command | Purpose |
|---|---|
| `tca_get_attribution` | Per-strategy rollup over date range: trade count, gross PnL, net PnL, avg R, win-rate, profit factor, avg slippage. |
| `tca_get_slippage_distribution` | Slippage histogram (bps) by strategy and symbol-liquidity-bucket. |
| `tca_record_manual_intent` | Trader-initiated: record intent for an order placed outside our UI (e.g., directly in TWS). Best-effort linkage. |

## Reuse

- Existing `executions` ingest in `services/executions/` — extend, don't fork.
- Existing FIFO matcher in `services/trade_legs/` — augment with strategy carryover.
- `SetupCandidate.trigger_price` is the canonical "intended price" when intent is recorded at activation.
- Existing live-quote service for fallback intended-price (when trigger_price is stale by > 5 min, use live mid as intended).

## Decisions to make in this phase

- **Intent ↔ fill matching window.** Orders may sit unfilled for minutes. Window: 60 minutes from `posted_at` to first matching fill; if no match by then, intent expires unmatched. **Decision: 60 min for limit, 5 min for market; configurable per-side.**
- **Partial-fill handling.** A 100-share intent can match 60-then-40 fills. **Decision: intent stays open until cumulative qty = intent qty; each fill carries the same `intent_id`.** Slippage computed weighted-avg over child fills.
- **Out-of-band fills (TWS direct).** Trader sometimes types orders into TWS directly. **Decision: on fill, attempt match against any unmatched intent for same symbol/side/qty in same session; if no match, leave `intent_id NULL`. Surface in UI as "unattributed fill."** Operator can manually link via `tca_record_manual_intent`.
- **Intended-price source priority.** `setup.trigger_price` (if setup_id given) > live quote at order-placement (cached) > order limit price. **Decision: prefer trigger_price; record `intended_price_source` enum so analysis can filter.**
- **Backfill policy for pre-P2 executions.** No setup_id, no intent. **Decision: leave as NULL. P4 attribution view treats NULL strategy as "unattributed" bucket; doesn't drop them.**

## Exit criteria

- `cargo test tca::` passes reference cases: clean fill, partial fills, out-of-band fill, expired intent, slippage sign by side (long pays positive bps, short pays negative).
- Integration test: place a setup-linked order against `MockIbkrClient`, simulate fill, verify `executions.setup_id` and `executions.slippage_bps` populated, verify `trade_legs.strategy` populated.
- `tca_get_attribution` over the last 30 days of seeded data returns one row per detector class with non-NULL `n_trades`, `realized_pnl`, `avg_slippage_bps`.
- Frontend TCA panel renders slippage histogram and attribution table from real fixture data.
- Migration is reversible on a backed-up `tracker.sqlite`; existing rows survive with NULL new columns.
- CI grep: any new call to `place_order` from outside `OrderTicket` (P3 will introduce) is allowed for now but flagged with TODO comment.

## Gotchas

- **IBKR fill timestamps are not monotonic across reconnects.** Use server-side `executions.timestamp` ordering, not arrival-at-our-process time.
- **Slippage sign convention.** Long: positive bps means paid more than intended. Short: positive bps means received less than intended. Document on `slippage_bps` column. Tests must cover both.
- **Currency.** All cents columns are INTEGER. Convert to decimal for display, never round mid-pipeline. Watch for cents overflow at portfolio scale (use `i64`, not `i32`).
- **Pre-P2 setups have no intent.** When P3 brackets land, brackets attached to manually-placed parents should still produce intents — accept `setup_id` on the bracket-attach path even though no parent intent existed yet.
- **SQLite indexes.** Add indexes on `executions(setup_id)`, `executions(intent_id)`, `order_intents(symbol, side, posted_at)` — attribution queries scan these heavily.
- **AVOID synthesizing `intent_id` from execution_id.** They're disjoint identifiers. If you find yourself wanting "ah just use exec_id as intent_id," stop and re-read the model — intents pre-exist fills.
- **MCP read-only invariant.** `tca_record_manual_intent` is a Tauri command (UI only), NOT exposed over MCP. MCP can read attribution; cannot write intents.
