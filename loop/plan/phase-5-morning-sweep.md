# Phase 5 — Pre-market research agent loop

> Part of [Quantum Kapital → Autonomous Researcher](master.md). See index for invariants.

**Status:** in-progress (started 2026-05-02)

**Depends on:** Phases 1, 2, 3, 4 (MCP read+write, sentiment, candidate staging)

**Goal:** First Claude Agent SDK loop. Runs at 07:00 ET weekdays. Produces the morning pack — 3-5 ranked ideas with thesis, conviction, invalidation.

## Files

- New repo subtree: `agent/` (Python, separate from Rust workspace; own `pyproject.toml`, `uv` or `poetry`)
  - `agent/morning_sweep.py` — orchestrates the loop
  - `agent/mcp_client.py` — connects to local MCP server via stdio
  - `agent/prompts/morning_sweep.md` — system prompt: task, output schema, conviction rubric
  - `agent/budget_guard.py` — wraps `get_llm_budget_status` checks
- New: `agent/cron/morning_sweep.cron` (Linux/macOS) or `.service` (systemd)
- New script: `loop/morning_sweep_dev.sh` — runs the loop manually for dev/test (also useful for `claude mcp` interactive mode)
- New file: `agent/README.md` — local install + cron setup

## Loop logic

1. `get_llm_budget_status()` — abort if >50% of daily cap already spent.
2. `get_candidates(score_min=N) ∪ get_watchlist()` → unified candidate set.
3. For top K (configurable, default 10), gather:
   - `get_bars(symbol, "1d", 252)` — 1y daily
   - `get_bars(symbol, "5m", 78)` — last RTH session
   - `get_fundamentals(symbol)`
   - `get_news(symbol, since=24h)` — with verdicts already populated by `news_interpreter`
   - `get_sentiment(symbol, since=24h)`
   - `get_setups(symbol, since=30d)` — historical context
4. Ranking step: LLM scores each on technical setup, fundamentals fit, sentiment, news catalyst.
5. Synthesis step: top 3-5 ideas → `{symbol, thesis_md, conviction (A/B/C), entry_zone, invalidation, evidence_refs}`.
6. `write_morning_pack(today, ranked_ideas)`.
7. Optional: `add_ticker(symbol, reason, source="agent_morning_sweep")` for non-watchlist promotions.
8. `get_llm_budget_status()` — log final spend for the loop.

## Trigger

OS-level cron / launchd at 07:00 ET on weekdays. Skip-holiday logic via the trading calendar already in `src-tauri/src/utils/`.

Cron invokes:
1. Ensure MCP server is reachable. v1: launches Tauri app if not open. v2 (Phase 9): daemon is always-on.
2. `python -m agent.morning_sweep`
3. On failure: write to `agent.log`; surface `MorningSweepFailed` event.

## Budget guardrail

- Per-loop USD cap (default `$0.50`), set in `agent/config.toml`.
- Loop respects `get_llm_budget_status()` mid-flight at major step boundaries (between ranking and synthesis).
- Gracefully degrades: if budget exhausted mid-loop, write a partial pack with whatever was processed and a `partial: true` flag.

## Models

- Tool-orchestration / data-gathering steps: `claude-haiku-4-5` (cheap, fast).
- Ranking: `claude-sonnet-4-6` (judgment).
- Synthesis: `claude-sonnet-4-6`.

## Reuse

- All MCP tools from Phases 1, 2, 3, 4.
- `LlmService` budget enforcement (via MCP `get_llm_budget_status`).
- Trading calendar in `utils/` (already exists for tracker scheduler).

## Exit criteria

- Every weekday by 07:30 ET, a morning pack with 3-5 ideas appears in the React UI.
- Each idea is clickable → setup detail with thesis + evidence chain (alerts, news, sentiment links).
- Average loop cost < $0.50 per run, logged in `llm_calls` ledger.
- Holiday days produce no pack and no error.

## Shadow mode (first 2 weeks)

Pack is generated but flagged `shadow=true` in DB and UI ("Shadow Pack — Researcher in evaluation"). You compare against your own picks before treating it as real. After 10 trading days, review and decide whether to drop the shadow flag.

## Gotchas

- **Prompt iteration is the long pole.** Expect 2 weeks of prompt + rubric tuning. Budget time accordingly.
- **MCP availability at 07:00.** If Tauri app must be open, you need to remember to leave it running overnight or have cron launch it. Phase 9 daemon eliminates this.
- **Cost estimation.** Run loop in dry-mode (no `write_morning_pack`) for 3-5 days before turning on cron. Confirm per-loop spend matches your budget assumption.
- **Output discipline.** Without a strict output schema (forced tool with JSON schema), agent will write prose where you want structured data. Use `ToolChoice::ForceTool("write_morning_pack")` at the synthesis step.
