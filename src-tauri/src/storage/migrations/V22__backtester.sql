-- Phase 6 (Quant-decisions): backtester storage.
--
-- backtest_runs: one row per `Backtester::run` invocation. `spec_json`
-- captures the full BacktestSpec so re-runs are reproducible from the
-- row alone; `result_json` captures the BacktestResult headline + per-
-- strategy / per-month / per-regime rollups. Per-trade rows live in
-- the sibling `backtest_trades` table (avoids inflating result_json
-- for 1k+ trade runs).
--
-- result_hash is a 16-hex-char fingerprint of the deterministic spec
-- encoding (`spec_canonical_string` in `services::backtester::spec`)
-- — equal hashes mean equal inputs, so reruns hash-match by construction.
CREATE TABLE backtest_runs (
    run_id          TEXT NOT NULL PRIMARY KEY,
    label           TEXT,
    spec_json       TEXT NOT NULL,
    result_json     TEXT,
    spec_hash       TEXT NOT NULL,
    n_trades        INTEGER NOT NULL DEFAULT 0,
    started_at      TEXT NOT NULL,
    finished_at     TEXT,
    status          TEXT NOT NULL DEFAULT 'running',
    error           TEXT
);

CREATE INDEX idx_backtest_runs_started_at ON backtest_runs (started_at DESC);
CREATE INDEX idx_backtest_runs_spec_hash ON backtest_runs (spec_hash);

CREATE TABLE backtest_trades (
    run_id          TEXT NOT NULL,
    trade_seq       INTEGER NOT NULL,
    symbol          TEXT NOT NULL,
    strategy        TEXT NOT NULL,
    direction       TEXT NOT NULL,
    entry_time      INTEGER NOT NULL,
    entry_price     REAL NOT NULL,
    exit_time       INTEGER NOT NULL,
    exit_price      REAL NOT NULL,
    qty             INTEGER NOT NULL,
    realized_r      REAL NOT NULL,
    realized_pnl    REAL NOT NULL,
    exit_reason     TEXT NOT NULL,
    conviction      TEXT,
    PRIMARY KEY (run_id, trade_seq),
    FOREIGN KEY (run_id) REFERENCES backtest_runs (run_id) ON DELETE CASCADE
);

CREATE INDEX idx_backtest_trades_strategy ON backtest_trades (run_id, strategy);
