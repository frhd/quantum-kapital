-- V25__regime_gating.sql
-- Phase 9 (quant-decisions roadmap): regime gating. Detectors declare
-- preferred market regimes; the runner consults a deterministic
-- `RegimeService` between blackout and concentration gates and skips
-- off-regime hits with `skipped_reason = 'off_regime'`.
--
-- Two surfaces extended here:
--
--   1. `regime_snapshots` — append-only history of classifications
--      (raw + stable). Daily-close + 15-min intraday recomputes both
--      land here so the operator can replay the regime sequence that
--      drove a past gate decision. The 3-day persistence rule is
--      computed at gate time from the most recent N rows; the table
--      stores raw axes only.
--
--   2. `setups.regime_at_decision_json` — JSON snapshot of the regime
--      that was active when the gate evaluated this candidate. Set on
--      both off-regime skips (full payload incl. preferred_regimes the
--      detector wanted) and on fired setups (regime only — preferred
--      isn't carried since the match was clean). Pre-P9 rows stay
--      NULL; the UI suppresses the badge in that case.
--
-- Override audit reuses the unified `gate_overrides` table from V24
-- with `gate_kind = 'regime'`. No new override table is needed.
--
-- All columns NULL-tolerant for back-compat. Older `setups` rows read
-- with `regime_at_decision_json = NULL`.

ALTER TABLE setups ADD COLUMN regime_at_decision_json TEXT;

CREATE TABLE IF NOT EXISTS regime_snapshots (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    at_unix         INTEGER NOT NULL,    -- unix seconds, UTC
    -- JSON: { "raw": {trend,vol,breadth,corr}, "stable": {...} }
    -- Both views are persisted: `raw` is what the classifier produced
    -- this snapshot, `stable` is the gate-time view after the 3-day
    -- per-axis persistence rule. Storing both lets a recompute
    -- replay the gate decision without re-deriving the rule from
    -- the prior history.
    regime_json     TEXT NOT NULL,
    -- JSON: { "spy_50ma": 450.5, "spy_200ma": 430.0, "vix_level": 15.2,
    --         "vix_trend": "rising", "breadth_pct_above_50ma": 0.62,
    --         "corr_20d": 0.41, "missing_inputs": ["vix"] }
    -- Audit payload so a stale-input recompute can be diagnosed
    -- without re-running the inputs module.
    inputs_json     TEXT NOT NULL,
    -- 'daily_close' | 'intraday' | 'force_recompute'. Lets the
    -- timeline distinguish the canonical end-of-day classification
    -- from the noisier intraday refresh.
    source          TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_regime_snapshots_at
    ON regime_snapshots(at_unix DESC);

CREATE INDEX IF NOT EXISTS idx_regime_snapshots_source_at
    ON regime_snapshots(source, at_unix DESC);
