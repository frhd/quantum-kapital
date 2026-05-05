-- V23__exit_policy.sql
-- Phase 7 — vol-adjusted exits + trailing/partial/time stops.
--
-- Two surfaces are extended:
--
--   1. `setups.exit_policy_version` + `setups.exit_plan_json` freeze
--      the exit decision the runner made at signal time. The plan is
--      computed by `strategies::exits::ExitPolicy`, persisted before
--      the trader sees the modal, and re-read by `OrderTicket` when
--      the human confirms — so the bracket placer reproduces exactly
--      what the runner promised. Older rows (pre-P7) read with
--      `exit_policy_version = 'v1_static'` and a NULL plan; the
--      bracket placer falls back to the static ladder when the column
--      is empty.
--
--   2. `bracket_groups.trail_state_json` carries the runtime trail
--      state for an active bracket: high-water-mark (long) /
--      low-water-mark (short), the current chandelier stop, and the
--      timestamp of the last modify. Empty for static-policy
--      brackets; the `BracketReviser` populates it on first poll.
--
-- Both columns are NULL-tolerant so the migration is a one-shot
-- ALTER, not a rewrite. Master Hard Invariant 5: pre-P7 rows stay
-- immutable under the new shape; the version column is the
-- discriminator.

ALTER TABLE setups
    ADD COLUMN exit_policy_version TEXT;

ALTER TABLE setups
    ADD COLUMN exit_plan_json TEXT;

ALTER TABLE bracket_groups
    ADD COLUMN trail_state_json TEXT;
