---
name: writing-phased-plans
description: Use when extending the multi-phase roadmap in loop/plan/ (master + phase-N files), adding a new phase, logging a cross-phase issue in QUESTIONS.md, or pairing a new agent loop with a system prompt in agent/prompts/.
---

# Writing Phased Plans (with Agent Prompts)

Two paired conventions for big multi-phase work in this repo:

- **`loop/plan/`** — `master.md` indexes the program; one `phase-N-<slug>.md` per phase; `QUESTIONS.md` logs cross-phase issues raised during execution.
- **`agent/prompts/`** — one system-prompt markdown file per LLM agent loop, paired with the phase that introduces the loop.

Phase files are self-contained but link back to `master.md` for invariants and critical-files reference. The agent prompt for a loop is listed in that phase's `Files` section.

## When to use

- Writing a new multi-phase roadmap (4+ phases).
- Adding a phase to an existing roadmap.
- Introducing an LLM agent loop (multi-turn, tool-using).
- Logging a cross-phase issue raised during a `/loop` execution.

Don't use for: single-PR changes, bug fixes, refactors that fit in one phase, one-off scripts.

## master.md — required sections

1. **Title** — `# <Project> → <End-state>: <Timeframe>`. One-sentence framing of the architectural inversion or transformation.
2. **`## Context`** — what the system is today vs. where it needs to go. Name the inversion explicitly (e.g., "today X is a function the app calls; end state is X is a process that calls the app").
3. **`## End-state architecture`** — table of subsystem roles and responsibilities. List the agent role separately if applicable.
4. **`## Hard invariants`** — numbered list. Things NO phase may violate (e.g., surveillance-only, all LLM calls through `LlmService`, audit trails on writes, mock-friendly trait seams, pre-commit hooks). Include: "Violating the letter of the rules is violating the spirit."
5. **`## Defaults committed`** — defaults overridable per-phase (which model, which SDK, which transport). Lock these in once so phases don't re-debate.
6. **`## Phase index`** — markdown table: `Phase | File | Depends on | Status`. Each row links to the phase file. Status uses the convention below. Add a one-line note under the table reminding maintainers to update both the table AND the phase file's status header at start/exit.
7. **`## Critical files`** — cross-cutting reference table mapping concern → path. Phase files reference this instead of duplicating paths.
8. **`## Sequencing + cadence`** — week-by-week mapping or rough ordering, including parallel phases and any phase explicitly punted ("schedule when X becomes required").
9. **`## Cross-phase verification`** — gates that span multiple phases (tracer-bullet test before phase X, shadow mode for phase Y, CI invariant test that greps for forbidden symbols).
10. **`## Open risks`** — known program-level risks paired with the phase that should address each.

## Status convention

`todo` | `in-progress (started YYYY-MM-DD)` | `done (commit <sha>, YYYY-MM-DD)`

Update **both** the master index AND the phase file's `**Status:**` header at phase start and at phase exit. Don't start a phase whose dependencies aren't `done`.

## phase-N-<slug>.md — required sections

```markdown
# Phase N — <one-line goal>

> Part of [<Project>](master.md). See index for invariants.

**Status:** <status string>

**Depends on:** <list of phase numbers, or "none (foundation phase)">

**Goal:** <2-3 sentences: what this phase produces and why now>

## Files

- New: `<path>` — purpose
- New: `<path>` — purpose
- Touches: `<path>` — what changes

## <Tools|Endpoints|APIs> exposed (if applicable)

| Tool | Wraps |
|---|---|
| ... | ... |

## Reuse (no new business logic this phase / explicit list)

- Existing services / patterns / traits this phase builds on. Forces the
  author to look before they invent.

## Decisions to make in this phase

- Choices unresolved at phase start (e.g., SDK vs hand-rolled, error model,
  cadence). Decide early, not mid-phase.

## Exit criteria

- Observable, testable bullets. Prefer end-to-end claims ("from a Claude
  Code session: '<natural-language ask>' → real answer with multiple visible
  tool calls") over unit-level.
- Include unit + integration test bullets.

## Gotchas

- Known pitfalls, lifecycle issues, drift hazards. Things that bit you in a
  prior phase or that you can predict will bite this one.
```

Phase files are self-contained for execution; cross-phase context lives in `master.md`. Don't repeat invariants or critical-file paths in the phase body.

## QUESTIONS.md — cross-phase log

Append-only log under `## Phase N (YYYY-MM-DD)` headings. Contents are issues raised during a `/loop` execution that the phase intentionally did not fix:

- Pre-existing flakes the phase didn't introduce.
- Scope-cut deferrals (with rationale and the phase that should pick them up).
- Decisions punted to a later phase.
- Phase-deferral rationale when a phase is marked `optional` and not run.

Each entry names the file/test/symbol so the next maintainer pass can find it.

## agent/prompts/<loop_name>.md — agent system prompt

One file per LLM agent loop introduced by a phase. Required structure:

```markdown
# <Loop Name> — System Prompt

You are <role> for <constraints — single user, surveillance only, etc.>.
Your job is to <single sentence> and produce <output type>.

This is **not financial advice**. <Surveillance-only / no orders / human
reviews every output before acting.> Your bar is "<concrete user value
phrased as a question>", not "<tempting overreach>".

## Inputs you receive

For each <unit> the orchestrator hands you:

- **<Input>** — shape, source, freshness (e.g., "1y daily bars (252
  sessions)", "news (last 24h) — already passed through `news_interpreter`,
  so each item has a `verdict` field").

## What you produce

A single <unit> is:

- **<field>** — what it is, what counts as "good", what does not. Concrete
  examples ("e.g. `148.50–149.20`") beat abstract definitions.
- **<field>** — required vs. optional, default behavior when absent.

## <Conviction|Quality> rubric (if grading)

- **A — <label>.** Concrete thresholds. At most one of {legs} is "neutral";
  none are negative.
- **B — <label>.** One strong leg, another uncertain.
- **C — <label>.** Premature; wait for confirmation.

If you cannot find <N> candidates that clear the lowest bar, return fewer.
**Do not pad.**

## Discipline

1. Be specific. Numbers, dates, ids — not adjectives.
2. Skepticism beats enthusiasm.
3. No look-ahead. You only know what is in the inputs.
4. No order placement. You produce research; the user trades.
5. Output schema is enforced by the `<tool_name>` tool. Use it exactly once.

## Output

Emit `<tool_name>` once. Do not write prose outside the tool call at the
synthesis step.
```

Pair the prompt with the phase that introduces it: that phase's `Files`
section lists `agent/prompts/<loop>.md`. The phase's `Models` section names
which Claude model the loop uses for which step (orchestration vs.
ranking vs. synthesis).

## Quick checks before committing a plan change

- Phase file's `**Status:**` header and master-index row match.
- New phase's `Depends on` lines up with prior phases marked `done`.
- Hard invariants live in `master.md`, not duplicated in the phase body.
- Exit criteria are testable from the outside — an observer could call
  done/not-done without reading source.
- New agent prompt forbids order placement and names the single tool that
  emits the structured result.
- New agent loop's writes flow through `LlmService` budget enforcement and
  show up in `mcp_audit` with a distinguishing `written_by` value.
