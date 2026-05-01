-- V06 — Alert enrichment markers (Phase 6).
--
-- The per-alert deep-dive agent attaches a research note to each new
-- alert within 1-2 minutes of firing. Two columns track that lifecycle:
--
--   * `enriched_at`       — unix seconds when the dive completed (or
--                           when the alert was skipped, e.g. because the
--                           global LLM budget was exhausted). NULL means
--                           "still eligible for enrichment".
--   * `research_note_id`  — FK into `research_notes`. NULL when the
--                           alert was skipped or is still in flight.
--
-- The watermark in the polling loop is just a perf hint; the source of
-- truth for "needs enrichment" is `enriched_at IS NULL`. That keeps
-- crash-recovery correct: a dive that crashes mid-flight is retried on
-- the next tick because no row was ever marked.
--
-- Both columns are additive and nullable; existing rows continue to read
-- back unchanged.

ALTER TABLE alerts ADD COLUMN enriched_at      INTEGER;
ALTER TABLE alerts ADD COLUMN research_note_id INTEGER REFERENCES research_notes(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_alerts_enriched_at ON alerts(enriched_at);
