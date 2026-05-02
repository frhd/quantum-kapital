-- V09 — manual fundamentals store (Phase 4 of the AV strip-out).
--
-- The IBKR `reqFundamentalData` migration was abandoned 2026-05-02
-- (deprecated API + missing entitlement). The replacement is a
-- manual-paste path: an MCP `set_fundamentals` write tool persists
-- rows here, and `CompositeFundamentalsProvider` reads them ahead of
-- the surviving Alpha Vantage adapter. See `loop/plan/master.md`.
--
--   * symbol         primary key (uppercase, validated by the tool)
--   * as_of_date     operator-asserted ISO 8601 date the snapshot is "as of"
--   * source         free-form provenance string (e.g. "Bloomberg paste")
--   * payload_json   serialized `FundamentalData` (camelCase via serde)
--   * written_at     unix seconds — when the row landed
--   * written_by     `mcp_audit.caller` cross-ref ("interactive" | "agent_<loop>")

CREATE TABLE IF NOT EXISTS manual_fundamentals (
    symbol       TEXT PRIMARY KEY,
    as_of_date   TEXT NOT NULL,
    source       TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    written_at   INTEGER NOT NULL,
    written_by   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_manual_fundamentals_written_at
    ON manual_fundamentals(written_at DESC);
