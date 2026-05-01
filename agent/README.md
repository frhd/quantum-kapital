# Quantum Kapital agent

Headless research agents that talk to the Quantum Kapital MCP server (the
`mcp-server` binary in `src-tauri/`). v1 ships the **morning sweep** — produces
a ranked pre-market pack of 3-5 ideas every weekday.

This subtree is intentionally separate from the Rust workspace: it owns its
own `pyproject.toml` and venv, so changes here don't touch the desktop app's
build or release pipeline.

## Prereqs

- Python 3.11+ (3.13 tested).
- `uv` package manager: `curl -LsSf https://astral.sh/uv/install.sh | sh`.
- The Rust app's MCP server binary built and on disk:
  `cd src-tauri && cargo build --release --bin mcp-server`.
- `ANTHROPIC_API_KEY` exported in the shell or in `agent/.env`.
- The Tauri desktop app running (so the unix socket the bridge connects to
  is bound). Phase 9 will lift this requirement via a daemon.

## Install

```sh
cd agent
uv venv
uv pip install -e ".[dev]"
```

The dev extra pulls `pytest` + `pytest-asyncio`. Production install (no extra)
pulls `anthropic` + `mcp`.

## Run a single sweep manually

```sh
# From the repo root (resolves config.toml's relative server_bin path).
./loop/morning_sweep_dev.sh

# Or directly:
cd agent && uv run morning_sweep --dry-run
```

Useful flags:

- `--dry-run` — runs the full loop including the LLM calls but skips the final
  `write_morning_pack` MCP write. Use for cost calibration.
- `--shadow` — appends a `[SHADOW PACK]` banner inside every `thesis_md`. The
  pack still lands in the DB; the UI / your eyes treat it as research-only.
- `--date YYYY-MM-DD` — overrides `today`. Useful for backfill or replay.
- `--force` — bypasses the trading-day check (runs on weekends/holidays).

## Tests

```sh
cd agent
uv run pytest
```

Unit tests use injected fakes for both the MCP client and the Anthropic SDK,
so they need neither the binary nor an API key.

## Cron / systemd

A starter crontab line lives in `agent/cron/morning_sweep.cron`. Open it,
substitute `<repo>` and `<account>`, and copy the line into `crontab -e`.

The dev-mode wrapper (`loop/morning_sweep_dev.sh`) sources `agent/.env`,
activates the venv, and calls the loop. The cron line invokes the same
script. macOS users with `launchd` can convert the cron line; the script is
the same.

The trading-calendar check inside `morning_sweep.py` early-exits on weekends
and US market holidays (hardcoded for 2024-2026; mirror the Rust list at
`src-tauri/src/utils/market_calendar/holidays.rs`). Cron may safely fire every
weekday — non-trading days produce no output and no error.

## Budget

Two layers protect against runaway spend:

1. **Server-side, daily**: `LlmService` in Rust enforces a per-day USD cap
   across every model call. The agent queries it via the MCP
   `get_llm_budget_status` tool at loop start AND between rank/synthesis. If
   the day is already past `abort_if_global_spend_above` (default 50%) when
   the loop starts, the loop skips entirely.
2. **Client-side, per-loop**: `BudgetGuard` accumulates the loop's own
   per-call spend and refuses the next call when the running total would
   exceed `per_loop_usd` (default $0.50). Models priced from
   `budget_guard._PRICES_USD_PER_MTOK` — keep in sync with
   `src-tauri/src/services/llm_service/prices.rs`.

If either layer trips mid-loop, the partial run still logs and exits 0 with a
`skipped_reason` — cron treats it as "no pack today".

## Shadow mode (first 2 weeks)

After enabling cron, run with `--shadow` for the first ~10 trading days.
Compare each day's pack against your own picks before trusting it. Drop
`--shadow` once the calibration looks right (Phase 8's eval harness will give
this a number).

## Alert-dive agent

A long-running poller (Phase 6). Every 30s it pulls every tracker alert
whose deep-dive isn't yet attached, gathers context via MCP read tools,
asks the LLM to write a per-alert research note, persists it via
`write_research_note`, and idempotently stamps `mark_alert_enriched` so
the same alert is never enriched twice.

```sh
# Single tick (use for cron-style invocation or manual smoke-testing).
uv run qk-alert-dive --once

# Continuous polling (the systemd service uses this form).
uv run qk-alert-dive --interval 30 --concurrent 2
```

Budget guardrails: per-alert USD cap (`--per-alert-usd`, default $0.05)
plus the global daily ceiling. If 90%+ of the daily LLM budget is gone,
the loop stamps every pending alert as "skipped" instead of running
synthesis — the UI shows a "deep dive skipped (budget)" badge via the
`AlertDiveSkipped` event.

The systemd unit lives at `agent/cron/alert_dive.service`.

## Files

- `morning_sweep.py` — orchestration + CLI entry.
- `alert_dive.py` — per-alert dive poller + CLI entry (Phase 6).
- `mcp_client.py` — async wrapper over the stdio MCP server.
- `budget_guard.py` — server- and loop-budget enforcement.
- `data_summary.py` — compact strings for the LLM (252d bars, fundamentals, news, sentiment, setups).
- `ranker.py` — LLM step #1: score each candidate on 0-1 rubric (forced tool: `score_candidates`).
- `synthesizer.py` — LLM step #2: emit ranked ideas (forced tool: `write_morning_pack`).
- `llm.py` — Anthropic SDK seam.
- `config.py` + `config.toml` — typed config.
- `prompts/morning_sweep.md` — morning-sweep system prompt.
- `prompts/alert_dive.md` — alert-dive system prompt.
- `tests/` — pytest unit tests; mock both MCP and Anthropic.
- `cron/morning_sweep.cron` — example crontab line.
- `cron/alert_dive.service` — systemd unit for the long-running dive poller.
