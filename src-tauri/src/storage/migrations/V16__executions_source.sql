-- V16__executions_source.sql
-- Phase 1 follow-on: tag the origin of each `executions` row so the
-- live-ingest path (`reqExecutions`, current-day-only) and the Flex
-- Web Service backfill (`bin/flex_backfill.rs`, arbitrary historical
-- ranges) can coexist without ambiguity.
--
-- Existing rows default to 'live' since they were all written by the
-- live ingestor. Backfill inserts use 'flex'.

ALTER TABLE executions
    ADD COLUMN source TEXT NOT NULL DEFAULT 'live';

CREATE INDEX idx_executions_account_source_time
    ON executions(account, source, exec_time);
