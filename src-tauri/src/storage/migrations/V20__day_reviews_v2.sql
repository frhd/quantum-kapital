-- V20__day_reviews_v2.sql
-- Phase 4 (quant-decisions roadmap): replace the conflated
-- `score = clamp(net_pnl/100, ±25) + Σ(tag_weights)` formula with two
-- independently surfaced numbers and a risk-metrics blob.
--
-- 1. New v2 columns on `day_reviews`:
--    - `score_v2`           — Σ(realized_R × conviction_weight) over closed
--                              legs. Nullable: NULL for pre-P4 rows that the
--                              phase explicitly does NOT retroactively
--                              recompute.
--    - `discipline_v2`      — Σ(tag_weights). Negative for typical days
--                              (range usually -30..0). Surfaced as a
--                              deficit, never summed with `score_v2`.
--    - `risk_metrics_json`  — RiskMetrics { sharpe, sortino, calmar,
--                              profit_factor, expectancy_r, max_dd,
--                              max_dd_duration, win_rate, avg_win_r,
--                              avg_loss_r } as JSON. Nullable for pre-P4.
--    - `equity_curve_json`  — Vec<EquityPoint { date, equity, daily_pnl }>
--                              as JSON. Nullable for pre-P4.
--    - `formula_version`    — `'v1'` for pre-P4 rows, `'v2'` for new
--                              writes. NEVER silently upgrade an old row.
--
-- 2. Legacy `grade` / `grade_score` columns relax to NULLABLE so v2 rows
--    can omit them. The v1 read-back path uses the stored value as-is;
--    the v2 read path ignores the column. We rebuild the table because
--    SQLite has no `ALTER COLUMN ... DROP NOT NULL` syntax.
--
-- 3. Pre-existing rows are migrated verbatim with `formula_version='v1'`
--    so the post-migration UI knows to render the v1 badge against them.

PRAGMA foreign_keys = OFF;

CREATE TABLE day_reviews_new (
    date              TEXT    NOT NULL,
    account           TEXT    NOT NULL,
    prompt_version    INTEGER NOT NULL,
    generated_at      TEXT    NOT NULL,
    grade             TEXT,                          -- nullable post-P4
    grade_score       REAL,                          -- nullable post-P4
    gross_pnl         REAL    NOT NULL,
    net_pnl           REAL    NOT NULL,
    commissions_total REAL    NOT NULL,
    n_round_trips     INTEGER NOT NULL,
    n_carryover       INTEGER NOT NULL,
    win_rate          REAL,
    behavioral_tags   TEXT    NOT NULL,
    leg_observations  TEXT    NOT NULL,
    summary_json      TEXT    NOT NULL,
    narrative_md      TEXT    NOT NULL,
    llm_call_id       TEXT,
    -- New v2 columns ----------------------------------------------------
    score_v2          REAL,
    discipline_v2     REAL,
    risk_metrics_json TEXT,
    equity_curve_json TEXT,
    formula_version   TEXT    NOT NULL DEFAULT 'v1',
    PRIMARY KEY (date, account, prompt_version)
);

INSERT INTO day_reviews_new (
    date, account, prompt_version, generated_at, grade, grade_score,
    gross_pnl, net_pnl, commissions_total, n_round_trips, n_carryover,
    win_rate, behavioral_tags, leg_observations, summary_json,
    narrative_md, llm_call_id,
    score_v2, discipline_v2, risk_metrics_json, equity_curve_json,
    formula_version
)
SELECT
    date, account, prompt_version, generated_at, grade, grade_score,
    gross_pnl, net_pnl, commissions_total, n_round_trips, n_carryover,
    win_rate, behavioral_tags, leg_observations, summary_json,
    narrative_md, llm_call_id,
    NULL, NULL, NULL, NULL,
    'v1'
FROM day_reviews;

DROP TABLE day_reviews;
ALTER TABLE day_reviews_new RENAME TO day_reviews;

CREATE INDEX idx_day_reviews_date ON day_reviews(date);
CREATE INDEX idx_day_reviews_account_date ON day_reviews(account, date DESC);
CREATE INDEX idx_day_reviews_formula_version ON day_reviews(formula_version);

PRAGMA foreign_keys = ON;
