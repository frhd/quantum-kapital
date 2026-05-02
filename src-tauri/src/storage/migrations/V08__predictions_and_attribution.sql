-- V08 — Phase 8 eval harness.
--
-- Three additive changes:
--   * `predictions`              — one row per ranked-idea snapshot at the
--                                  moment a morning pack (or other agent
--                                  output) is written. Lets us correlate a
--                                  call with its eventual `outcome` even
--                                  if the source pack is later overwritten.
--   * `outcomes.prediction_id`   — backlink so the eval harness can JOIN
--                                  predictions ↔ outcomes for calibration
--                                  stats. Nullable because legacy rows
--                                  predate the predictions table.
--   * `llm_calls.loop_name`      — attribution column so the eval dashboard
--                                  can bucket LLM spend by agent loop
--                                  (e.g. "agent_morning_sweep",
--                                  "agent_alert_dive", "agent_eod_review").
--                                  Nullable; existing rows + non-loop
--                                  callers (intraday detector enrichment)
--                                  stay null.
--
-- All new columns/tables are additive; no existing rows migrated.

CREATE TABLE IF NOT EXISTS predictions (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    source           TEXT NOT NULL,                     -- "agent_morning_sweep" | "agent_alert_dive" | ...
    symbol           TEXT NOT NULL,
    conviction       TEXT,                              -- "A" | "B" | "C" | NULL
    entry_zone       TEXT,                              -- raw freeform string from agent
    invalidation     TEXT,                              -- raw freeform string from agent
    target           TEXT,                              -- raw freeform string from agent
    thesis_md        TEXT,                              -- markdown rationale snapshot
    morning_pack_id  TEXT,                              -- pack date if from morning sweep
    predicted_at     INTEGER NOT NULL                   -- unix seconds
);
CREATE INDEX IF NOT EXISTS idx_predictions_predicted_at ON predictions(predicted_at DESC);
CREATE INDEX IF NOT EXISTS idx_predictions_symbol       ON predictions(symbol, predicted_at DESC);
CREATE INDEX IF NOT EXISTS idx_predictions_pack         ON predictions(morning_pack_id);
CREATE INDEX IF NOT EXISTS idx_predictions_source       ON predictions(source, predicted_at DESC);

ALTER TABLE outcomes    ADD COLUMN prediction_id INTEGER REFERENCES predictions(id) ON DELETE SET NULL;
CREATE INDEX IF NOT EXISTS idx_outcomes_prediction ON outcomes(prediction_id);

ALTER TABLE llm_calls   ADD COLUMN loop_name TEXT;
CREATE INDEX IF NOT EXISTS idx_llm_calls_loop_called_at ON llm_calls(loop_name, called_at DESC);
