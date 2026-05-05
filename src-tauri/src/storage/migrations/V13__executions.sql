-- V13__executions.sql
-- Persists IBKR execution fills so the assessment stack (Phases 2/4/6) can
-- query multi-day history. Forward-only; no backfill from IBKR is possible.

CREATE TABLE executions (
    exec_id              TEXT    PRIMARY KEY,
    account              TEXT    NOT NULL,
    symbol               TEXT    NOT NULL,
    contract_type        TEXT    NOT NULL,    -- "STK" | "OPT" | ...
    expiry               TEXT,                -- "YYYY-MM-DD" | NULL (non-options)
    strike               REAL,
    "right"              TEXT,                -- "C" | "P" | NULL
    multiplier           TEXT,                -- "100" for standard equity opts
    side                 TEXT    NOT NULL,    -- "bought" | "sold"
    qty                  REAL    NOT NULL,
    avg_price            REAL    NOT NULL,
    currency             TEXT,
    exec_time            TEXT    NOT NULL,    -- ISO 8601 UTC
    order_id             INTEGER NOT NULL,
    commission           REAL,                -- NULL until report arrives
    realized_pnl         REAL,                -- NULL for opening legs / unreported
    commission_currency  TEXT,
    ingested_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    commission_patched_at TEXT
);

CREATE INDEX idx_executions_account_time
    ON executions(account, exec_time);

CREATE INDEX idx_executions_account_symbol_time
    ON executions(account, symbol, exec_time);

CREATE INDEX idx_executions_pending_commission
    ON executions(account, exec_time)
    WHERE commission IS NULL;
