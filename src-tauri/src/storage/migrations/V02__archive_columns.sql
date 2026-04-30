-- V02 — soft-archive rail for tracked_tickers and setups.
-- Nullable unix-epoch column; NULL means active, set means archived.
-- TrackerService reads exclude rows with `archived_at IS NOT NULL` by
-- default, which removes archived tickers from detector runs, decay-
-- watcher checks, TTL expirations, alert emission, and LLM spend without
-- any change in the runners themselves.

ALTER TABLE tracked_tickers ADD COLUMN archived_at INTEGER;
ALTER TABLE setups          ADD COLUMN archived_at INTEGER;
