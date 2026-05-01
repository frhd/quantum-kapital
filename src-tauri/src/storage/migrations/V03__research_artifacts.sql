-- V03 — research artifacts persisted by agent + interactive MCP writes.
--
-- Three new tables and a small extension to `alerts`:
--   * `research_notes`       — durable LLM-authored research, optionally
--                              linked to a setup or alert.
--   * `mcp_audit`            — append-only log of every MCP write, with
--                              caller provenance (agent loop name or
--                              "interactive"). Enforces the "every MCP
--                              write is audited" hard invariant.
--   * `agent_morning_packs`  — agent-authored ranked-ideas packs. Distinct
--                              from the deterministic `morning_packs`
--                              persisted by `DailyRanker` (different
--                              payload schema, different writer).
--   * `alerts.decision`,
--     `alerts.decision_note_id`,
--     `alerts.decided_at`    — `ack_alert` decision rail.
--
-- All new columns/tables are additive; no existing rows are migrated.

CREATE TABLE IF NOT EXISTS research_notes (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    symbol        TEXT NOT NULL,
    body_md       TEXT NOT NULL,
    conviction    TEXT,                           -- "A" | "B" | "C" | NULL
    evidence_refs TEXT,                           -- JSON array; null means none
    written_by    TEXT NOT NULL,                  -- "user" | "agent_<loop>" | "interactive"
    written_at    INTEGER NOT NULL,               -- unix seconds
    setup_id      INTEGER,
    alert_id      INTEGER,
    FOREIGN KEY(setup_id) REFERENCES setups(id) ON DELETE SET NULL,
    FOREIGN KEY(alert_id) REFERENCES alerts(id)  ON DELETE SET NULL
);
CREATE INDEX IF NOT EXISTS idx_research_notes_symbol_at ON research_notes(symbol, written_at DESC);
CREATE INDEX IF NOT EXISTS idx_research_notes_setup    ON research_notes(setup_id);
CREATE INDEX IF NOT EXISTS idx_research_notes_alert    ON research_notes(alert_id);

CREATE TABLE IF NOT EXISTS mcp_audit (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    tool           TEXT NOT NULL,
    input          TEXT NOT NULL,                 -- JSON
    result_summary TEXT,
    caller         TEXT,                          -- "agent_<loop>" | "interactive" | NULL
    called_at      INTEGER NOT NULL               -- unix seconds
);
CREATE INDEX IF NOT EXISTS idx_mcp_audit_called_at ON mcp_audit(called_at DESC);
CREATE INDEX IF NOT EXISTS idx_mcp_audit_tool      ON mcp_audit(tool, called_at DESC);

CREATE TABLE IF NOT EXISTS agent_morning_packs (
    date          TEXT PRIMARY KEY,               -- YYYY-MM-DD
    payload       TEXT NOT NULL,                  -- JSON: { ranked_ideas: [...] }
    written_by    TEXT NOT NULL,
    written_at    INTEGER NOT NULL                -- unix seconds
);

ALTER TABLE alerts ADD COLUMN decision         TEXT;
ALTER TABLE alerts ADD COLUMN decision_note_id INTEGER;
ALTER TABLE alerts ADD COLUMN decided_at       INTEGER;
