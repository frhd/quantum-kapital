-- V10 — Alpha Vantage call ledger (Phase 5 of the AV strip-out).
--
-- Two paired tables back the AV-side guardrails on the
-- CompositeFundamentalsProvider. They protect AV's daily quota
-- (default soft cap 20, hard cap 25) and the per-symbol-per-day
-- cap (default 1 fetch). Persisted across restarts so a crash
-- mid-sweep cannot reset the count and silently double-up
-- against the AV free tier (master.md § Hard invariant #7,
-- Phase 5 § Gotchas).
--
-- The ledger keys on the user's local-timezone date (matches the
-- morning-sweep cadence; UTC would shift the rollover into the
-- user's evening). Increments use INSERT ... ON CONFLICT to
-- avoid lost-update races between concurrent fetches.

CREATE TABLE IF NOT EXISTS av_call_ledger (
    date       TEXT PRIMARY KEY,
    count      INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS av_per_symbol_ledger (
    date       TEXT NOT NULL,
    symbol     TEXT NOT NULL,
    count      INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (date, symbol)
);
