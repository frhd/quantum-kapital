-- V19__bracket_groups.sql
-- Phase 3 (quant-decisions roadmap): persistence shape for the
-- `services/order_ticket/` bracket-on-activation chokepoint.
--
-- One row per human-confirmed bracket submission: the parent entry
-- order id keys the row, with the OCA-grouped stop child + N target
-- children carried alongside. `system_qty` records what the risk
-- engine sized; `parent_qty` records what the trader actually
-- shipped (same as system_qty unless the trader overrode in the
-- TakeSetup modal). Override-without-reason is rejected upstream;
-- here we just persist whatever the modal sent.
--
-- `last_status` is updated by the order-status reconciler that lands
-- with the bracket placement (open / filled / partial / stopped /
-- canceled). Pre-P3 there is no reconciler yet — the row is created
-- in `open` and left there until manually swept by the cancel
-- command.

CREATE TABLE IF NOT EXISTS bracket_groups (
    parent_order_id        INTEGER PRIMARY KEY,
    setup_id               INTEGER NOT NULL REFERENCES setups(id),
    intent_id              TEXT    NOT NULL REFERENCES order_intents(intent_id),
    account                TEXT    NOT NULL,
    symbol                 TEXT    NOT NULL,
    -- "long" | "short". Mirrors `setups.direction` so a bracket can
    -- be inspected without a join on the originating setup.
    direction              TEXT    NOT NULL,
    parent_qty             INTEGER NOT NULL,
    -- The risk engine's suggested qty at decision time. Equal to
    -- `parent_qty` unless the trader overrode in the modal.
    system_qty             INTEGER NOT NULL,
    qty_override_reason    TEXT,
    entry_limit_cents      INTEGER NOT NULL,
    stop_order_id          INTEGER NOT NULL,
    stop_price_cents       INTEGER NOT NULL,
    -- JSON array of i32 IBKR order ids for the OCA-grouped target
    -- children. Order matches `targets_json` so the n-th id is the
    -- order that backs the n-th `TargetSpec`.
    target_order_ids_json  TEXT    NOT NULL,
    -- JSON array of `{label, price, qty}` describing the static
    -- 50/30/20 target ladder. Stored alongside the order ids so a
    -- post-hoc audit can reproduce the modal exactly.
    targets_json           TEXT    NOT NULL,
    placed_at              TEXT    NOT NULL,    -- ISO 8601 UTC
    -- 'open' | 'filled' | 'partial' | 'stopped' | 'canceled'
    last_status            TEXT    NOT NULL DEFAULT 'open',
    last_status_at         TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_bracket_groups_setup
    ON bracket_groups(setup_id);

CREATE INDEX IF NOT EXISTS idx_bracket_groups_intent
    ON bracket_groups(intent_id);

CREATE INDEX IF NOT EXISTS idx_bracket_groups_open
    ON bracket_groups(account, last_status_at DESC)
    WHERE last_status IN ('open', 'partial');
