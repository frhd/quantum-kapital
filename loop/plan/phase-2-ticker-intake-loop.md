# Phase 2 — `agent/ticker_intake.py`: baseline research note for newly-added tickers

> Part of [Ticker Intake Enrichment](master.md). See index for invariants.

**Status:** in-progress (started 2026-05-03)

**Depends on:** 1

**Goal:** Within ~60 seconds of a ticker being added, write a baseline
`research_notes` row that summarises the thesis (or absence thereof).
Mirrors `alert_dive.py`'s polling shape but is event-agnostic — it
reacts to "watchlist symbol with no recent baseline note" rather than
to alert fires. Uses the existing MCP read surface and
`write_research_note` write rail.

## Files

- New: `agent/ticker_intake.py` — main loop. `run_tick`, `run_loop`,
  `__main__`. Mirrors `alert_dive.py` structure (hard cap target: keep
  under ~450 LOC; split into `agent/ticker_intake/` package if it
  grows).
- New: `agent/prompts/ticker_intake.md` — system prompt for the loop.
  Baseline-thesis framing — not event-driven (`alert_dive`) and not
  ranked (`morning_sweep`).
- New: `agent/tests/test_ticker_intake.py` — unit tests with injected
  fakes for `McpClient`, `LlmClient`, `BudgetGuard`. Mirrors
  `agent/tests/test_alert_dive.py` patterns.
- New: `agent/cron/ticker_intake.service` — systemd unit for the
  long-running daemon, restart-on-failure. Mirror
  `agent/cron/alert_dive.service`.
- Touches: `agent/pyproject.toml` — add `qk-ticker-intake` script
  alias pointing at `ticker_intake:main`.
- Touches: `agent/README.md` — section describing the new loop, its
  install + run, budget knobs, and how it relates to the existing
  three loops.
- Touches: `agent/config.toml` — defaults section
  `[ticker_intake]` with `poll_interval_secs`, `per_symbol_usd`,
  `per_loop_usd`, `reuse_note_window_days`, `max_concurrent`.

No Rust changes this phase. The new writer string `agent.ticker_intake`
is free-form on the `research_notes.written_by` column; no schema or
enum update.

## Tools used (no new tools introduced)

| Read | What it returns |
|---|---|
| `get_watchlist` | watchlist rows including `last_primed_at` (Phase 1) |
| `get_fundamentals` | fundamentals snapshot (composite manual + AV) |
| `get_news` | news rows (with `verdict` from `news_interpreter`) |
| `get_bars` | 1y daily + last RTH 5m (matches `alert_dive` data summary) |
| `get_setups` | recent detector setups (30d) — optional context |
| `get_llm_budget_status` | global cap check before each tick |

| Write | What it stores |
|---|---|
| `write_research_note` | `research_notes` row with `written_by = "agent.ticker_intake"` |

A "needs intake" candidate diff is computed agent-side:
`get_watchlist()` minus the symbol set returned by
`research_list_notes` (or whatever existing read tool surfaces recent
notes per symbol). If no read tool surfaces "notes per symbol in last
N days" cleanly, log that gap in `QUESTIONS.md` and use whatever the
closest existing tool is — do not introduce a new MCP tool for this
phase.

## Reuse (no new business logic this phase)

- `agent/budget_guard.py::BudgetGuard` — `ensure_can_spend()`,
  per-loop ledger, global cap check via `get_llm_budget_status`. Set
  `per_loop_usd = per_symbol_usd × max_concurrent`.
- `agent/llm.py::AnthropicLlmClient` — direct Anthropic SDK call.
  Same model default as alert_dive (`claude-sonnet-4-6`).
- `agent/mcp_client.py::McpClient` — unix-socket transport to the
  running app's MCP.
- `agent/data_summary.py` — already has helpers for summarising
  bars / news / fundamentals into LLM-friendly text. Reuse verbatim.
- `agent/synthesizer.py` — if its single-symbol synthesis is
  reusable for baseline notes, prefer that over duplicating prompt
  scaffolding. Decide in this phase (see Decisions).
- `agent/cron/alert_dive.service` — copy + adapt for the systemd
  unit. Same restart, same env loading.

## Models

| Step | Model | Why |
|---|---|---|
| Synthesis (single-symbol baseline note) | `claude-sonnet-4-6` | Matches `alert_dive`. Sonnet is enough for a 200-400 word baseline thesis. Opus is overkill for this; Haiku is too thin on synthesis. |

No orchestration / ranking step in this loop — it's strictly per-symbol
synthesis. If `synthesizer.py` reuse pulls in the morning-sweep ranker
step, drop the ranker for this loop (no ranking; we're writing a
single note, not picking from a candidate pool).

## Decisions to make in this phase

- **Polling vs. event-driven.** Polling. Mirrors `alert_dive`. The
  MCP socket doesn't expose a server-push subscription; building one
  is out of scope.
- **Eligibility predicate.** Watchlist symbol with
  `last_primed_at IS NOT NULL` AND no `research_notes` row in the
  last 7 days. Gating on prime ensures the LLM input has fundamentals
  + news.
- **Concurrency.** `DEFAULT_MAX_CONCURRENT = 2`, same as alert_dive.
- **Reuse-note window.** 7 days across **all writers**. If
  `morning_sweep` already wrote, skip — that's strictly more
  thorough than a baseline.
- **Synth reuse.** If `synthesizer.py::synthesize` accepts a single
  symbol with the same input bundle, reuse it; if it's bound to the
  morning-pack rubric (multiple symbols, ranking), write a one-shot
  `synthesise_baseline()` helper inline. Prefer reuse only if it
  cuts net code; do not fight the abstraction.
- **Intake-during-prime.** If `last_primed_at` is set but the news
  fetch failed (`PrimeOutcome.news = Err`), should the agent still
  intake? Yes — fundamentals alone is enough for a baseline. The
  prompt instructs the LLM to call out missing news explicitly.
- **First-tick-on-startup.** If the daemon starts and the watchlist
  has 30 symbols never primed, do we hammer the LLM on the first
  tick? No — the predicate gates on `last_primed_at IS NOT NULL`,
  so unprimed symbols are skipped. They'll come into eligibility as
  the user adds them and Phase 1 primes.

## Exit criteria

- `python -m ticker_intake --dry-run` runs end-to-end against a live
  MCP socket: pulls candidates, calls Anthropic in dry-run mode (or
  stops short of the write), exits 0.
- `python -m ticker_intake` (live, single tick via a `--once` flag)
  writes one `research_notes` row per eligible symbol with
  `written_by = "agent.ticker_intake"` and `conviction ∈ {A,B,C}`.
- Unit tests cover:
  - Eligibility predicate (primed + no recent note ⇒ candidate;
    primed + recent note ⇒ skip; not primed ⇒ skip).
  - Reuse-note window short-circuits at 7d.
  - `BudgetGuard` per-loop short-circuit.
  - `BudgetGuard` global cap short-circuit (mocked
    `get_llm_budget_status` returns "over").
  - LLM tool-call parse: a malformed response is rejected without
    crashing the loop.
  - MCP write call records `written_by = "agent.ticker_intake"`.
  - `written_by` discipline: a grep test asserts no other writer
    string appears in `agent/ticker_intake.py`.
- `agent/tests/test_ticker_intake.py` passes under `uv run pytest`.
- `agent/README.md` documents the loop alongside `morning_sweep`,
  `alert_dive`, and `eod_review`.
- Cross-phase tracer #2 (master): from a fresh DB, add a symbol,
  wait 90s, assert a `research_notes` row exists with the expected
  `written_by`. Documented manually for Phase 2 exit; can be
  automated against fakes as part of the unit suite.
- `agent/cron/ticker_intake.service` exists, mirrors the alert_dive
  unit, and has been smoke-tested on the developer's machine
  (`systemctl --user start qk-ticker-intake.service`, observe one
  tick + a successful write within a minute, then stop).
- `pre-commit` clean: `prettier --check` (no impact, but the hook
  runs project-wide), `eslint` (no impact), Python `ruff` /
  formatter if the agent has one configured.

## Gotchas

- **Prime-not-done race.** If the loop ticks before Phase 1's
  primer has populated fundamentals + news, the LLM input is
  anaemic. The eligibility predicate gates on `last_primed_at IS NOT
  NULL`, but a partial prime (`PrimeOutcome.news = Err`) still
  satisfies that gate. The prompt instructs the LLM to flag missing
  inputs rather than fabricate.
- **Budget cap drift.** With $0.10 × 2 = $0.20 per tick at 60s
  cadence, a busy day can burn through a $5 budget in ~25 minutes.
  Confirm the per-loop and global caps line up with the user's
  daily budget; document in `agent/README.md`.
- **`written_by` discipline.** Hardcode the literal
  `"agent.ticker_intake"` once at module top
  (`WRITER = "agent.ticker_intake"`); tests grep for any other
  string in this file. Copying from `alert_dive.py` and forgetting
  to rename is the failure mode this guards.
- **Audit trail.** Every `write_research_note` call lands in
  `mcp_audit` with `caller = "agent"`. No Rust change needed; verify
  via integration test or a manual `mcp_audit::list` after a
  smoke run.
- **Long-running daemon failure modes.** If the MCP socket dies
  (Tauri app restart, Phase 9 daemon migration), the loop should
  retry with exponential backoff, not crash the systemd unit.
  `alert_dive.py` already handles this; mirror.
- **Prompt drift.** A baseline thesis is *not* an alert dive (no
  event) and *not* a morning pack (no ranking). Resist copying
  `alert_dive.md` verbatim. Lead the prompt with "the user just
  added this symbol; what's the one-paragraph baseline they should
  hold in their head?".
- **Unit-test fakes.** `agent/tests/test_alert_dive.py` already
  shows the fake-injection patterns for `McpClient` and `LlmClient`.
  Reuse those fakes; do not invent a new mocking layer.
- **`research_list_notes` shape.** If the existing read tool
  returns notes paginated globally (not per-symbol), the agent has
  to fan out a request per watchlist symbol. That's fine for small
  watchlists but file in `QUESTIONS.md` if it becomes hot.
- **Trading-day awareness.** Unlike `morning_sweep`, this loop runs
  whenever a user adds a ticker — including weekends. Skip the
  trading-day gate. The prompt should still note "weekend / pre-open
  data" caveats when applicable, driven by the inputs.
