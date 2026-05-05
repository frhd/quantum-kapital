-- V18__tca_linkage.sql
-- Phase 2 (quant-decisions roadmap): wire every IBKR fill back to the
-- setup it answered.
--
-- 1. New columns on `executions` carry the linkage + slippage:
--    - `setup_id`             — denormalised from the matched intent so
--                                attribution queries don't need a join
--                                through `order_intents` for every read.
--    - `intent_id`            — the matched `order_intents.intent_id`.
--                                NULL when no intent existed (out-of-band
--                                TWS fills, pre-P2 rows).
--    - `intended_price_cents` — copied from the intent at match time.
--    - `intended_price_source`— enum string: 'trigger_price' |
--                                'live_quote' | 'limit_price' |
--                                'manual'.
--    - `slippage_bps`         — absolute slippage in basis points
--                                (rounded). NULL when no intent.
--    - `slippage_signed`      — signed slippage in cents per share.
--                                Long: positive ↔ paid more than
--                                intended. Short: positive ↔ received
--                                less than intended. (Both convey
--                                "trader cost" with the same sign.)
--                                NULL when no intent.
--
-- 2. `order_intents` is the source-of-truth row written when the user
--    confirms an order in our UI (P3 will extend with bracket
--    children). Keyed by `intent_id` (TEXT — generated as
--    `intent_<setup_id>_<ulid>` or `intent_manual_<ulid>`). The intent
--    pre-exists the fill; one intent matches one or many child fills
--    until cumulative qty reaches `qty` (partial-fill case).
--
-- 3. Indexes target the read paths in `services/tca/attribution.rs`:
--    - `executions(setup_id)` — per-setup PnL rollup.
--    - `executions(intent_id)` — partial-fill aggregation.
--    - `order_intents(symbol, side, posted_at)` — out-of-band fill
--      matching scans the last hour of unmatched intents per symbol/
--      side.
--
-- Pre-P2 executions stay NULL on every new column. Attribution views
-- treat NULL setup_id as the "unattributed" bucket.

ALTER TABLE executions ADD COLUMN setup_id INTEGER REFERENCES setups(id);
ALTER TABLE executions ADD COLUMN intent_id TEXT;
ALTER TABLE executions ADD COLUMN intended_price_cents INTEGER;
ALTER TABLE executions ADD COLUMN intended_price_source TEXT;
ALTER TABLE executions ADD COLUMN slippage_bps INTEGER;
ALTER TABLE executions ADD COLUMN slippage_signed INTEGER;

CREATE TABLE IF NOT EXISTS order_intents (
    intent_id              TEXT    PRIMARY KEY,
    setup_id               INTEGER REFERENCES setups(id),
    account                TEXT    NOT NULL,
    symbol                 TEXT    NOT NULL,
    side                   TEXT    NOT NULL,    -- "buy" | "sell"
    qty                    REAL    NOT NULL,
    intended_price_cents   INTEGER NOT NULL,
    intended_price_source  TEXT    NOT NULL,    -- 'trigger_price' | 'live_quote' | 'limit_price' | 'manual'
    posted_at              TEXT    NOT NULL,    -- ISO 8601 UTC
    expires_at             TEXT    NOT NULL,    -- ISO 8601 UTC; posted_at + window
    status                 TEXT    NOT NULL DEFAULT 'open',  -- 'open' | 'matched' | 'expired'
    matched_qty            REAL    NOT NULL DEFAULT 0.0,
    created_at             TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE INDEX IF NOT EXISTS idx_order_intents_setup
    ON order_intents(setup_id);

CREATE INDEX IF NOT EXISTS idx_order_intents_open_lookup
    ON order_intents(account, symbol, side, posted_at)
    WHERE status = 'open';

CREATE INDEX IF NOT EXISTS idx_executions_setup
    ON executions(setup_id);

CREATE INDEX IF NOT EXISTS idx_executions_intent
    ON executions(intent_id);
