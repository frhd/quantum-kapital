-- V04 — Social sentiment ingestion (Phase 3).
--
-- One row per (source, symbol) sample. The scheduler upserts roughly
-- once per cadence; historical rows accrete so the agent can ask
-- `get_sentiment(symbol, since=...)` and see a small time-series.
--
-- `score` is the source-specific signed sentiment normalised to
-- `[-1, 1]` at the service layer (NULL for sources that publish only
-- counts/ranks, e.g. a raw mention scrape). `mentions_24h` is the
-- 24-hour mention/post count when the source surfaces it. The raw
-- upstream JSON payload is preserved in `raw_payload` so we can
-- re-derive scores if our normalisation drifts.

CREATE TABLE IF NOT EXISTS social_sentiment (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    source          TEXT    NOT NULL,                 -- "reddit_wsb" | "stocktwits" | "apewisdom"
    symbol          TEXT    NOT NULL,                 -- upper-cased ticker
    score           REAL,                             -- normalised signed sentiment in [-1, 1] (NULL = no score)
    mentions_24h    INTEGER,                          -- raw mention / post count when known (NULL otherwise)
    sentiment_label TEXT,                             -- "bullish" | "bearish" | "neutral" | NULL
    rank            INTEGER,                          -- source-specific rank (NULL when not applicable)
    raw_payload     TEXT    NOT NULL,                 -- JSON: original upstream object (or `{}` on stale)
    is_stale        INTEGER NOT NULL DEFAULT 0,       -- 1 = "we tried, source had no signal"
    fetched_at      INTEGER NOT NULL                  -- unix seconds
);

CREATE INDEX IF NOT EXISTS idx_social_sentiment_symbol_at
    ON social_sentiment(symbol, fetched_at DESC);
CREATE INDEX IF NOT EXISTS idx_social_sentiment_source_at
    ON social_sentiment(source, fetched_at DESC);
CREATE INDEX IF NOT EXISTS idx_social_sentiment_symbol_source_at
    ON social_sentiment(symbol, source, fetched_at DESC);
