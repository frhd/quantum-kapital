# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

A Tauri 2 desktop app pairing an Interactive Brokers connection with a strategy-driven **Tracker** subsystem (watchlist → detectors → LLM reasoning → alerts). Surveillance only — order placement exists but is not wired into the tracker.

Stack-specific rules:

- **`src-tauri/CLAUDE.md`** — Rust backend, IBKR adapter, services, tracker pipeline, cargo commands
- **`src/CLAUDE.md`** — React 19 + Vite frontend, feature folders, Tauri command wrappers
- **`agent/`** — Python (uv) subtree. Headless research agents (morning sweep, alert dive) that talk to the running app via the MCP socket. Owns its own `pyproject.toml`, venv, and tests; **not** part of the Rust/Tauri build. Entry points, budget rules, and shadow-mode rollout in `agent/README.md`. The hardcoded US-holiday list in `agent/morning_sweep.py` mirrors `src-tauri/src/utils/market_calendar/holidays.rs` — keep them in lockstep.
- **`loop/`** — Claude Code's own orchestration harness (`loop.sh`, `prompt.md`, `settings.json`, `stop-hook.sh`). Not production code. If a multi-phase roadmap is started, it lives under `loop/plan/` (master + `phase-N-*.md`) — use the `superpowers:writing-phased-plans` skill when extending it.

## Running the app

`pnpm tauri dev` from the repo root brings up Vite + the Rust backend with hot reload. **Use `pnpm`, not npm** — the lockfile is `pnpm-lock.yaml`. Frontend-only and prod-build commands are in `src/CLAUDE.md`.

The `tauri` script is wrapped by `scripts/tauri.sh`: `pnpm tauri dev` sets `RUST_LOG=info,quantum_kapital_lib=debug,rmcp=info,ibapi=info` and tees combined stdout/stderr to `/tmp/qk-tauri.log` (truncated each session). Tail/grep that file when debugging — Claude Code reads it on demand. Other subcommands (`build`, `info`, `icon`) pass through untouched. Override the log path with `QK_TAURI_LOG=...`, the filter with `RUST_LOG=...`, or set `QK_TAURI_LOG_APPEND=1` to keep history across sessions.

## Secrets

Backend reads `src-tauri/.env`. The frontend never sees these — all external API calls go through Rust services.

- `ANTHROPIC_API_KEY` — required for the LLM features (thesis generator, decay watcher, news interpreter, daily ranker).
- `ALPHA_VANTAGE_API_KEY` — used only by the **fundamentals fallback** path (`CompositeFundamentalsProvider` → AV adapter, when a symbol isn't in the manual MCP store). News is fully on IBKR after Phase 8; AV is no longer consulted for news. The composite serves manual-store rows when the AV key is unset, so missing it only impacts symbols the user hasn't curated.

## LLM budget

`LlmService` enforces a daily USD budget against the `llm_calls` ledger before every call. Any new code that calls an LLM must go through `LlmService` — never bypass it, even in tests; use the trait seam.

## Tracker is surveillance-only

The tracker pipeline (detectors → state machine → alerts) MUST NOT call order-placement code paths. Order commands exist in the IBKR adapter for manual UI use only. Wiring them into the tracker requires explicit project-level approval.

The same rule binds the MCP server (`src-tauri/src/mcp/` + `bin/mcp-server.rs`): the external tool surface is **read-only plus an `ack_alert` rail** — no order tools, ever. Acks are audited through `services/mcp_audit/`.

## Test discipline

TDD with red → green → refactor at every phase. Tests use the `IbkrClientTrait` seam (`MockIbkrClient` in `src-tauri/src/ibkr/mocks.rs`), never a live IBKR client. Any service that touches IBKR should be testable through that trait.

## Pre-commit

`.pre-commit-config.yaml` runs `cargo fmt --check`, `cargo clippy -D warnings`, `prettier --check`, and `eslint` on every commit. **Never bypass with `--no-verify`** — fix the underlying issue. If clippy fails on code you didn't touch, that's a real regression to investigate.

## File-size caps

Soft 300 (Rust) / 200 (TS/TSX); hard 500 / 350. Past the hard cap requires an `// allow-large-file: <reason>` justifier at the top of the file. Full rules and check commands in `CONTRIBUTING.md`.

## Cargo from repo root

`src-tauri/Cargo.toml` is the only manifest — no workspace at the root. From the repo root, every cargo command needs `--manifest-path src-tauri/Cargo.toml`. Backend command examples (and the `cd src-tauri/` shortcut) are in `src-tauri/CLAUDE.md`.
