-- V11 — structured thesis levels on research notes.
--
-- Powers the Research-tab "thesis at a glance" card: with these populated,
-- the UI can compare the live quote against the note's own invalidation
-- and target levels and label the status (Intact / Near invalidation /
-- Invalidated / Target hit) without parsing the markdown body. Every
-- field is nullable; old rows render as "Unknown" status with just an
-- age + live-price readout. The MCP `write_research_note` tool exposes
-- these as optional inputs and snapshots `price_at_write` from the live
-- quote when omitted.
--
--   * price_at_write     — last price observed when the note was written.
--                          Anchors the "price drift since write" readout.
--   * invalidation_price — level the thesis dies at. Combined with
--                          `invalidation_kind` to decide breach.
--   * invalidation_kind  — one of `close_below` | `close_above` |
--                          `intraday_breach`. Validated client-side and
--                          again at the MCP tool boundary.
--   * targets_json       — JSON array `[{label, price}]`. Order is
--                          author intent (T1 first, T2 second, …).
--   * catalyst_date      — ISO date for a known upcoming catalyst (e.g.
--                          earnings). Optional; powers a future countdown.
--
-- All columns are additive; existing rows are unaffected.

ALTER TABLE research_notes ADD COLUMN price_at_write     REAL;
ALTER TABLE research_notes ADD COLUMN invalidation_price REAL;
ALTER TABLE research_notes ADD COLUMN invalidation_kind  TEXT;
ALTER TABLE research_notes ADD COLUMN targets_json       TEXT;
ALTER TABLE research_notes ADD COLUMN catalyst_date      TEXT;
