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
- Either `ANTHROPIC_API_KEY` exported in the shell or in `agent/.env`,
  OR the `claude` CLI v2.1+ on PATH (see "LLM backend selection" below).
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
./agent/scripts/morning_sweep_dev.sh

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

The dev-mode wrapper (`agent/scripts/morning_sweep_dev.sh`) sources `agent/.env`,
activates the venv, and calls the loop. The cron line invokes the same
script. macOS users with `launchd` can convert the cron line; the script is
the same.

The trading-calendar check inside `morning_sweep.py` early-exits on weekends
and US market holidays (hardcoded for 2024-2026; mirror the Rust list at
`src-tauri/src/utils/market_calendar/holidays.rs`). Cron may safely fire every
weekday — non-trading days produce no output and no error.

## LLM backend selection

Two transports ship; pick one with the `QK_LLM_BACKEND` env var (read by
both the Tauri app and the agent loops, so a single shell-level flip
toggles both at once):

| Value | Transport | Auth |
|---|---|---|
| `anthropic` (default) | `anthropic.AsyncAnthropic` SDK → `api.anthropic.com` | `ANTHROPIC_API_KEY` |
| `claude_cli` | spawn `claude -p` (one subprocess per call) | the user's Claude Code subscription (no key) |

Set `QK_LLM_BACKEND=claude_cli` in `agent/.env` (or the systemd unit's
`EnvironmentFile`) to flip. The change takes effect on the next loop
restart — each loop builds its `LlmClient` once at startup and caches
it. A `[llm].backend` entry in `config.toml` is the secondary fallback;
the env var wins when set.

### Caveats specific to `claude_cli`

- **Cost is best-effort.** The CLI envelope reports `total_cost_usd`,
  but it can be 0 under subscription auth. When that happens
  `BudgetGuard` falls back to a per-token estimate from
  `budget_guard._PRICES_USD_PER_MTOK` so the per-loop kill-switch still
  trips deterministically — but the daily ledger's USD figure becomes
  rough rather than exact. Token counts stay accurate.
- **Pricing tables are mirrored.** `agent/budget_guard.py` mirrors
  `src-tauri/src/services/llm_service/prices.rs`. A unit test
  (`tests/test_prices.py`) parses the Rust table and asserts every
  `(input, output)` pair matches the Python side. If you change one,
  change the other.
- **Surveillance lockdown is unconditional.** Every CLI spawn passes
  `--tools ""`, `--strict-mcp-config`, `--mcp-config '{"mcpServers":{}}'`,
  and `--permission-mode dontAsk`. The argv unit test
  (`tests/test_llm_cli.py`) pins these literals so a regression fails
  CI rather than silently re-enabling tools.
- **`ANTHROPIC_*` does not leak into the subprocess.** The CLI's auth
  precedence prefers env vars over keychain/OAuth, so leaking
  `ANTHROPIC_API_KEY` into the child would silently disable
  subscription mode. Tests assert the spawn env is `PATH`+`HOME` only.
- **Single forced tool only.** Multi-tool, `tool_choice=auto`, and
  multi-turn (assistant-role) messages are rejected at argv-build.
  Every current call site uses a single forced tool, so this is not a
  practical limitation today.

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

## Ticker-intake agent

A long-running poller (Phase 2). Every 60s it scans the watchlist for
symbols that the Rust `TickerPrimerService` (Phase 1) has primed but
that don't yet have a recent baseline `research_notes` row, gathers
context via MCP read tools, asks the LLM to synthesise a starting-point
thesis, and persists it via `write_research_note`.

```sh
# Single tick (use for cron-style invocation or manual smoke-testing).
uv run qk-ticker-intake --once

# Cost-free smoke (skips the LLM call, exercises the orchestration only).
uv run qk-ticker-intake --dry-run

# Continuous polling (the systemd service uses this form).
uv run qk-ticker-intake --interval 60 --concurrent 2
```

Eligibility predicate: watchlist symbol with `last_primed_at IS NOT
NULL` AND no recent baseline note (per `--reuse-window-days`, default
7). Re-priming after `archive_ticker` clears `last_primed_at` brings a
symbol back into eligibility. Production uses an in-memory dedup cache
since the MCP surface lacks a `list_research_notes` read; the cache is
scoped to the daemon's runtime — see
`loop/plan/QUESTIONS.md::Phase 2`.

Budget guardrails: per-symbol USD cap (`--per-symbol-usd`, default
$0.10) plus the global daily ceiling. With three concurrent agent
loops (`morning_sweep` + `alert_dive` + `ticker_intake`) sharing the
global cap, `GLOBAL_RESERVE_FRAC = 0.10` keeps a 10% headroom for the
schedulers. Bump the daily cap if intake regularly trips the floor.

The system prompt lives at `agent/prompts/ticker_intake.md`. The
systemd unit lives at `agent/cron/ticker_intake.service`.

## Files

- `morning_sweep.py` — orchestration + CLI entry.
- `alert_dive.py` — per-alert dive poller + CLI entry (Phase 6).
- `ticker_intake.py` — baseline-note poller + CLI entry (Phase 2).
- `mcp_client.py` — async wrapper over the stdio MCP server.
- `budget_guard.py` — server- and loop-budget enforcement.
- `data_summary.py` — compact strings for the LLM (252d bars, fundamentals, news, sentiment, setups).
- `ranker.py` — LLM step #1: score each candidate on 0-1 rubric (forced tool: `score_candidates`).
- `synthesizer.py` — LLM step #2: emit ranked ideas (forced tool: `write_morning_pack`).
- `llm.py` — Anthropic SDK seam + `make_llm_client` factory.
- `llm_cli.py` — `claude -p` subprocess backend (subscription auth).
- `config.py` + `config.toml` — typed config.
- `prompts/morning_sweep.md` — morning-sweep system prompt.
- `prompts/alert_dive.md` — alert-dive system prompt.
- `prompts/ticker_intake.md` — ticker-intake system prompt.
- `tests/` — pytest unit tests; mock both MCP and Anthropic.
- `cron/morning_sweep.cron` — example crontab line.
- `cron/alert_dive.service` — systemd unit for the long-running dive poller.
- `cron/ticker_intake.service` — systemd unit for the long-running ticker-intake poller.
