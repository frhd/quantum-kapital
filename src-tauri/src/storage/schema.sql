CREATE TABLE IF NOT EXISTS tracked_tickers (
    symbol         TEXT PRIMARY KEY,
    source         TEXT NOT NULL,
    source_meta    TEXT,
    status         TEXT NOT NULL DEFAULT 'watching',
    tags           TEXT NOT NULL DEFAULT '[]',
    notes          TEXT,
    added_at       INTEGER NOT NULL,
    last_checked_at INTEGER,
    in_play_until  INTEGER,
    cool_down_until INTEGER
);

CREATE TABLE IF NOT EXISTS setups (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    symbol         TEXT NOT NULL,
    strategy       TEXT NOT NULL,
    direction      TEXT NOT NULL,
    detected_at    INTEGER NOT NULL,
    trigger_price  REAL NOT NULL,
    stop_price     REAL NOT NULL,
    targets        TEXT NOT NULL,
    raw_signals    TEXT NOT NULL,
    thesis         TEXT,
    thesis_json    TEXT,
    status         TEXT NOT NULL DEFAULT 'active',
    invalidated_at INTEGER,
    invalidation_reason TEXT,
    FOREIGN KEY(symbol) REFERENCES tracked_tickers(symbol) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_setups_symbol ON setups(symbol);
CREATE INDEX IF NOT EXISTS idx_setups_status_detected ON setups(status, detected_at);

CREATE TABLE IF NOT EXISTS alerts (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    setup_id    INTEGER NOT NULL,
    kind        TEXT NOT NULL,
    fired_at    INTEGER NOT NULL,
    payload     TEXT NOT NULL,
    seen        INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY(setup_id) REFERENCES setups(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS bars_cache (
    symbol     TEXT NOT NULL,
    bar_size   TEXT NOT NULL,
    bar_time   INTEGER NOT NULL,
    open       REAL NOT NULL,
    high       REAL NOT NULL,
    low        REAL NOT NULL,
    close      REAL NOT NULL,
    volume     INTEGER NOT NULL,
    wap        REAL,
    PRIMARY KEY(symbol, bar_size, bar_time)
);

CREATE TABLE IF NOT EXISTS news_cache (
    symbol      TEXT NOT NULL,
    fetched_at  INTEGER NOT NULL,
    payload     TEXT NOT NULL,
    PRIMARY KEY(symbol)
);

CREATE TABLE IF NOT EXISTS llm_calls (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    kind       TEXT NOT NULL,
    setup_id   INTEGER,
    model      TEXT NOT NULL,
    input_tokens  INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    cache_read_tokens INTEGER DEFAULT 0,
    cost_usd   REAL NOT NULL,
    called_at  INTEGER NOT NULL
);
