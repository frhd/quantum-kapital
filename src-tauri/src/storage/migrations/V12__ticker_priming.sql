-- V12 — ticker priming watermark.
-- Nullable unix-epoch column; NULL means "never primed". TickerPrimerService
-- stamps it after a successful prime; archive_ticker clears it so a re-prime
-- runs on unarchive. The 24h idempotency window in the primer reads this
-- column directly.

ALTER TABLE tracked_tickers ADD COLUMN last_primed_at INTEGER;
