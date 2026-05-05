-- V14__day_reviews.sql
-- One structured trade review per (date, account, prompt_version).
-- prompt_version bumps when the rubric weights, tag enum, or system prompt
-- change materially; old reviews stay queryable but new versions UPSERT
-- as separate rows (so the trader-profile aggregator can group by version).

CREATE TABLE day_reviews (
    date              TEXT    NOT NULL,            -- "YYYY-MM-DD" (ET)
    account           TEXT    NOT NULL,
    prompt_version    INTEGER NOT NULL,
    generated_at      TEXT    NOT NULL,            -- ISO 8601 UTC
    grade             TEXT    NOT NULL,            -- "A"|"B"|"C"|"D"|"F"
    grade_score       REAL    NOT NULL,
    gross_pnl         REAL    NOT NULL,
    net_pnl           REAL    NOT NULL,
    commissions_total REAL    NOT NULL,
    n_round_trips     INTEGER NOT NULL,
    n_carryover       INTEGER NOT NULL,
    win_rate          REAL,                        -- nullable (no closed legs => NULL)
    behavioral_tags   TEXT    NOT NULL,            -- JSON array of enum names (snake_case)
    leg_observations  TEXT    NOT NULL,            -- JSON array of {leg_id, observation_md, tag?}
    summary_json      TEXT    NOT NULL,            -- full LegSummary serialised as JSON
    narrative_md      TEXT    NOT NULL,
    llm_call_id       TEXT,                        -- foreign-key-ish to llm_calls.id
    PRIMARY KEY (date, account, prompt_version)
);

CREATE INDEX idx_day_reviews_date ON day_reviews(date);
CREATE INDEX idx_day_reviews_account_date ON day_reviews(account, date DESC);
