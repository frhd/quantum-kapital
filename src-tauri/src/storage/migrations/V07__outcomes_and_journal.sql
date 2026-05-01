-- V07 — Phase 7 EOD review.
--
-- Two additive tables:
--   * `outcomes`         — one row per (pack_date, symbol) scoring an
--                          agent-authored prediction against realized
--                          intraday/daily price action. Keyed by
--                          `(pack_date, symbol)` so re-runs upsert.
--   * `journal_entries`  — append-only-by-section store the
--                          `append_journal_entry` MCP tool writes into.
--                          The daily-journal skill reads from here to
--                          render the agent-authored section into
--                          `journal/YYYY-MM-DD.md`. Sections are unique
--                          per `(journal_date, section)` so a re-run of
--                          the EOD review overwrites cleanly without
--                          touching the user's manual notes.
--
-- All new columns/tables are additive; no existing rows migrated.

CREATE TABLE IF NOT EXISTS outcomes (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    pack_date        TEXT NOT NULL,                     -- YYYY-MM-DD
    symbol           TEXT NOT NULL,
    outcome_class    TEXT NOT NULL,                     -- hit_entry|hit_target|hit_invalidation|drifted|no_movement|skipped|unparseable
    conviction       TEXT,                              -- A|B|C|null
    entry_zone_low   REAL,
    entry_zone_high  REAL,
    invalidation_lvl REAL,
    realized_high    REAL NOT NULL,
    realized_low     REAL NOT NULL,
    realized_close   REAL NOT NULL,
    eval_window_days INTEGER NOT NULL,
    evaluated_at     INTEGER NOT NULL,                  -- unix seconds
    UNIQUE(pack_date, symbol)
);
CREATE INDEX IF NOT EXISTS idx_outcomes_pack_date  ON outcomes(pack_date);
CREATE INDEX IF NOT EXISTS idx_outcomes_symbol     ON outcomes(symbol, pack_date DESC);

CREATE TABLE IF NOT EXISTS journal_entries (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    journal_date TEXT NOT NULL,                          -- YYYY-MM-DD
    section      TEXT NOT NULL,                          -- e.g. "EOD Review (Agent)"
    body_md      TEXT NOT NULL,
    written_by   TEXT NOT NULL,                          -- "agent_<loop>" | "interactive" | "user"
    written_at   INTEGER NOT NULL,                       -- unix seconds
    UNIQUE(journal_date, section)
);
CREATE INDEX IF NOT EXISTS idx_journal_entries_date ON journal_entries(journal_date DESC);
