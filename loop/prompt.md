Drive @loop/plan/master.md to completion. Sub-plans: @loop/plan/phase-N-*.md.

**ONE PHASE PER ITERATION.** End your turn after the phase is `done` and pushed; the next iteration runs in a fresh context. Touch `loop/BREAK` only when zero phases remain eligible.

EACH ITERATION:
1. Read the master plan's Phase index. Resume any `in-progress` phase; otherwise pick the FIRST `todo` phase whose dependencies are all `done (commit <sha>, ...)`. If neither exists, project complete — `touch loop/BREAK` (or end your message with `<<<LOOP_DONE>>>`; the Stop hook will write BREAK).
2. If the phase isn't already `in-progress`, flip BOTH the master-plan index entry AND the sub-plan's `**Status:**` header to `in-progress (started <today YYYY-MM-DD>)` and commit (`chore(plan): mark phase N in-progress`). On resume, skip this step.
3. Work the phase per its Scope / Files / Tools / Exit criteria, following `CLAUDE.md` disciplines.
4. On exit criteria met: run pre-commit + relevant `cargo`/`pnpm` tests, commit the work, flip BOTH status entries to `done (commit <sha>, <today YYYY-MM-DD>)`, commit (`chore(plan): mark phase N done`), `git push`, then end your turn with a brief summary.

OPERATING RULES:
- Commit frequently — every logical step is its own commit. Phase boundaries marked by `chore(plan): mark phase N {in-progress,done}`.
- Linear history on the current branch only: no new branches, PRs, merges, rebases, force-pushes, `--amend`, or `git reset --hard`.
- If blocked by genuine ambiguity, log to @loop/plan/QUESTIONS.md and continue with the safer interpretation.
- Read your own diff before committing.
- Master plan's `## Hard invariants` overrides anything here.

USE SUBAGENTS IN PARALLEL for phases with 2+ independent sub-tasks. Tier: haiku for search/tests, sonnet for implementation, opus for design, architecture, and review. Serialize sub-tasks touching the same files.