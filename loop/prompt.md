Drive @loop/plan/master.md to completion. Sub-plans: @loop/plan/phase-N-*.md.

**ONE PHASE PER ITERATION.** End your turn after the phase is `done` and pushed — `loop.sh` will start a fresh iteration with clean context for the next phase. Touch `loop/BREAK` only when zero phases remain eligible.

EACH ITERATION:
1. Read the master plan's Phase index.
2. Pick the FIRST phase with Status=todo whose dependencies are all `done (commit <sha>, ...)`. If no eligible phase remains, the project is complete — run `touch loop/BREAK` and exit (or end your final message with the literal token `<<<LOOP_DONE>>>` — the Stop hook will write BREAK for you).
3. Open the sub-plan. If not already in-progress, flip BOTH the master-plan index entry AND the sub-plan's `**Status:**` header to `in-progress (started <today YYYY-MM-DD>)`. Commit (`chore(plan): mark phase N in-progress`).
4. Work the phase per its Scope / Files / Tools / Exit criteria. Discipline:
   - TDD red→green→refactor. Use trait seams (`IbkrClientTrait`, `QuoteFetcher`, etc.) — never a live IBKR client in tests.
   - All LLM calls go through LlmService (budget-enforced) — even in tests, via the trait seam.
   - SURVEILLANCE-ONLY: never add `place_order` / `modify_order` / `cancel_order` MCP tools, never wire orders into the tracker. Ever.
   - Every MCP write is audited (`written_by` column).
   - Pre-commit (`cargo fmt --check`, `cargo clippy -D warnings`, `prettier`, `eslint`) MUST pass. Never `--no-verify` — fix the underlying issue.
   - File-size caps: soft 300 (Rust) / 200 (TS) — past hard caps requires `// allow-large-file: <reason>`.
5. When exit criteria are met: run pre-commit + the relevant `cargo`/`pnpm` tests, commit the work, flip BOTH status entries to `done (commit <sha>, <today YYYY-MM-DD>)`, commit (`chore(plan): mark phase N done`), `git push`, then end your turn with a brief summary of what landed.

OPERATING RULES:
- COMMIT FREQUENTLY. Every logical step (failing test, passing impl, refactor, status flip) is its own commit. Many small commits >> one giant commit. Phase boundaries MUST be marked by `chore(plan): mark phase N {in-progress,done}` so `git log --oneline` is a readable progress narrative.
- PUSH after every phase `done` commit (more often is fine). `git push` on the current branch only — never `--force`, never to a different branch.
- DO NOT create new branches, open PRs, or merge anything. All 9 phases land as commits on the current branch.
- DO NOT rebase, force-push, or `git reset --hard`. DO NOT amend prior commits — append new ones.
- If blocked by genuine ambiguity (multiple defensible designs, missing requirement), append the question to @loop/plan/QUESTIONS.md with the phase name, then continue with the safer / narrower interpretation. Do not block.
- Verify before claiming done: run the exit-criteria checks, don't assume. Read your own diff before committing.
- Hard invariants in the master plan (`### Hard invariants`) override anything here.

USE SUBAGENTS IN PARALLEL when a phase has 2+ independent sub-tasks (multiple MCP tools, independent migrations, schema + handler split). Model tier:
- haiku — search/grep, running tests, mechanical lookups
- sonnet — code exploration, multi-step implementation, code review
- opus — design, planning, complex reasoning

Subagents must coordinate when touching the same files; serialize those sub-tasks instead.
