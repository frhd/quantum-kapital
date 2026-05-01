-- V05 — Candidate universe staging layer (Phase 4).
--
-- Decouples "scanner found a ticker" from "ticker is in the watchlist".
-- All scanner profiles (broad IBKR scans, sentiment surges, future
-- earnings movers) emit into this table; the watchlist only grows when
-- a candidate is explicitly promoted — either by the agent via
-- `promote_candidate` MCP tool or by the auto-promoter when the merged
-- score crosses a configured threshold.
--
-- One row per `symbol` with `sources` carrying the per-source provenance
-- as a JSON array — keeps the candidate browser to a single query and
-- avoids the cross-source dedup gotcha called out in the phase plan.
-- The merged `score` is `MAX(per_source.score)` so a single hot signal
-- promotes the row even if other sources are quiet.
--
-- `decay_at` is the unix-second eviction deadline; a daily decay job
-- deletes rows where `decay_at <= now AND promoted_at IS NULL`.
-- Promoted rows are kept for audit/history (the watchlist row carries
-- the live state).

CREATE TABLE IF NOT EXISTS candidate_universe (
    symbol      TEXT    PRIMARY KEY,             -- upper-cased ticker
    score       REAL    NOT NULL DEFAULT 0,      -- merged max score across sources
    sources     TEXT    NOT NULL,                -- JSON array of { source, score, rank, meta }
    reason_md   TEXT,                            -- short markdown summary of why we care
    first_seen  INTEGER NOT NULL,                -- unix seconds, immutable
    last_seen   INTEGER NOT NULL,                -- unix seconds, refreshed each upsert
    decay_at    INTEGER NOT NULL,                -- unix seconds; eviction deadline
    promoted_at INTEGER                          -- unix seconds; NULL until promoted into watchlist
);

CREATE INDEX IF NOT EXISTS idx_candidate_universe_decay
    ON candidate_universe(decay_at);
CREATE INDEX IF NOT EXISTS idx_candidate_universe_score_desc
    ON candidate_universe(score DESC);
CREATE INDEX IF NOT EXISTS idx_candidate_universe_last_seen_desc
    ON candidate_universe(last_seen DESC);
CREATE INDEX IF NOT EXISTS idx_candidate_universe_promoted_at
    ON candidate_universe(promoted_at);
