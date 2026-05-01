# Quantum Kapital → Autonomous Researcher: Quarter Roadmap

## Context

Quantum Kapital today is a single-user surveillance cockpit: IBKR for portfolio + market data, Alpha Vantage for fundamentals, a deterministic detector pipeline producing `setups` + `alerts`, and forced-single-tool LLM calls for per-setup reasoning. You bring the tickers; the app watches them.

Goal: evolve into a fully-automated researcher producing actionable advice — proactively discovers candidates from market-wide momentum and social sentiment, reasons across them with a real LLM agent via MCP, writes durable artifacts (morning packs, deep-dive notes, EOD reviews) you act on manually.

The architectural inversion: today the LLM is *a function the app calls* (single-shot, forced tool, fixed output); end state is the LLM is *a process that calls the app* (multi-turn, tool-using, self-directed) via MCP.

Outcome this quarter: every weekday a ranked morning pack with thesis + conviction + invalidation; every alert auto-enriched with deep-dive reasoning; every EOD generates a calibration entry in the journal proving (or disproving) the researcher is any good.

## End-state architecture (five-role app + agent)

Rust does five things; agent does one via MCP.

| Role | Rust responsibility |
|---|---|
| **Persistent state** | SQLite + refinery. New tables: `social_sentiment`, `candidate_universe`, `research_notes`, `predictions`, `outcomes`, `mcp_audit`. |
| **Tool surface (MCP)** | New `src-tauri/src/mcp/` exposing read + structured-write tools as a stdio MCP server (Tauri sidecar in v1). |
| **Continuous ingestion** | Existing schedulers + new `SocialSentimentScheduler`. Fill state independent of any LLM. |
| **Deterministic surveillance** | Detectors, state machine, decay watcher, alerts. Reasoning moves out; *triggering* stays in. |
| **Human surface** | React UI + new views: morning pack, research notes, candidate universe, eval dashboard. |

Agent (Python, Claude Agent SDK):
- **Pre-market sweep** (07:00 ET) → `write_morning_pack`
- **Per-alert deep-dive** (polling on new alerts) → `write_research_note` linked to setup
- **EOD review** (17:00 ET) → `append_journal_entry`

Interactive: Claude Code via CLI on the same MCP server.

## Hard invariants

1. **Surveillance-only stays.** No `place_order` / `modify_order` / `cancel_order` MCP tools. Ever.
2. **All LLM calls go through `LlmService` budget enforcement** — agent loops included. They call `get_llm_budget_status()` mid-flight.
3. **Every MCP write is audited** — `written_by` column distinguishes `agent` from `user` and identifies the agent loop.
4. **Test discipline preserved.** Trait seams (`IbkrClientTrait`, `QuoteFetcher`, etc.) for new services. Mocked MCP transport for tool tests; mocked MCP responses for agent loop tests.
5. **`pre-commit` is sacred** (`cargo fmt --check`, `cargo clippy -D warnings`, `prettier`, `eslint`). Never `--no-verify`.

## Defaults committed (overridable per-phase)

- **MCP server v1**: Tauri sidecar. Phase 9 extracts to standalone daemon for app-closed schedules.
- **Headless agent**: Claude Agent SDK (Python). Not custom Rust orchestrator.
- **Interactive agent**: Claude Code via CLI, MCP stdio config in `~/.claude/mcp_servers.json`.
- **Personal use only.** Basic "not financial advice" disclaimer in morning pack output.
- **No X.com in v1.** Reddit + Stocktwits + Apewisdom. Revisit if signal warrants.
- **Models**: `claude-sonnet-4-6` for synthesis/ranking; `claude-haiku-4-5` for sentiment classification + tool-orchestration steps.

## Phase index

Each phase is a standalone file with scope, files, tools, exit criteria, and gotchas.

| Phase | File | Depends on | Status |
|---|---|---|---|
| 1. MCP server (read tools) | [phase-1-mcp-read.md](phase-1-mcp-read.md) | — | done (commit 7992d46, 2026-05-01) |
| 2. MCP write tools + research artifacts | [phase-2-mcp-write.md](phase-2-mcp-write.md) | 1 | done (commit 00b1b1a, 2026-05-01) |
| 3. Social sentiment ingestion | [phase-3-sentiment.md](phase-3-sentiment.md) | 1 | done (commit 0bd8511, 2026-05-02) |
| 4. Universe staging + scanner expansion | [phase-4-universe-staging.md](phase-4-universe-staging.md) | 1, 2 | done (commit 7269f2c, 2026-05-02) |
| 5. Pre-market research agent loop | [phase-5-morning-sweep.md](phase-5-morning-sweep.md) | 1, 2, 3, 4 | done (commit 55fbc73, 2026-05-02) |
| 6. Per-alert deep-dive agent | [phase-6-alert-dive.md](phase-6-alert-dive.md) | 1, 2 | done (commit 6f6c15a, 2026-05-02) |
| 7. EOD review + journal | [phase-7-eod-review.md](phase-7-eod-review.md) | 1, 2, 5 | todo |
| 8. Eval harness | [phase-8-eval-harness.md](phase-8-eval-harness.md) | 5 (data); meaningful at ~30d | todo |
| 9. Daemon refactor (optional) | [phase-9-daemon.md](phase-9-daemon.md) | independent | todo |

> **Status convention:** values are `todo` | `in-progress (started YYYY-MM-DD)` | `done (commit <sha>, YYYY-MM-DD)`. Update both this table AND the phase file's `**Status:**` header at phase start and exit. Don't start a phase whose dependencies aren't `done`.

## Critical files (cross-cutting reference)

| Concern | Path |
|---|---|
| LLM service + budget ledger | `src-tauri/src/services/llm_service/{mod.rs,prices.rs,types.rs}` |
| Existing tool-use patterns to mirror | `services/{thesis_generator,news_interpreter,decay_watcher}/mod.rs` |
| Tracker pipeline | `services/{tracker_runner,tracker_state_machine,tracker_service}/` |
| Scheduler pattern (template) | `services/{eod_scheduler,intraday_scheduler}/mod.rs` |
| IBKR scanner (already exposed) | `src-tauri/src/ibkr/client/streams.rs` (`scan_one_shot`), `services/auto_scanner/mod.rs` |
| Caching | `services/cache_service.rs` (file), `bars_cache` in `services/historical_data_service/mod.rs` (SQLite) |
| Storage + migrations | `src-tauri/src/storage/mod.rs`, `src-tauri/migrations/V*.sql` |
| Service wiring (MCP slots in here) | `src-tauri/src/lib.rs` |
| Config + API keys | `src-tauri/src/config/settings.rs`, `src-tauri/.env.example` |
| Trait seams (mock-friendly) | `ibkr/mocks.rs`, `{QuoteFetcher,NewsFetcher,BarsFetcher,MarketScanner}` |
| Daily journal skill | `.claude/skills/daily-journal/` |

## Sequencing + cadence (12 focused weeks)

- W1-2: Phase 1 (MCP read)
- W3-4: Phase 2 (MCP write + research artifacts)
- W5-6: Phase 3 (sentiment) + Phase 4 (staging) in parallel
- W7-8: Phase 5 (morning sweep) — expect prompt iteration
- W9: Phase 6 (alert dive)
- W10-11: Phase 7 (EOD) + Phase 8 (eval scaffolding)
- W12: Buffer / shadow-mode eval / first calibration look

Phase 9 (daemon) is independent — schedule when overnight ingestion / app-closed sweeps become required.

## Cross-phase verification

1. **Tracer-bullet test before Phase 5:** After Phases 1-4, drive Claude Code through a full research session ending in `write_morning_pack`. If painful, the MCP tool surface needs more work before automating.
2. **Shadow mode for Phase 5:** First 2 weeks the morning pack is flagged "shadow" — compare against your own picks before trusting it.
3. **Eval harness gates "actionable" claim.** Until calibration shows A-conviction wins meaningfully more than B/C, the morning pack is "research output," not "actionable advice." UI language reflects this.
4. **Budget alerting:** Events `BudgetWarning` (70%) and `BudgetExceeded` (100%) surfaced in UI status bar.
5. **Surveillance-only audit in CI:** Test that greps the MCP tool registry for any order-related name; build fails if found.

## Open risks

- **Reddit OAuth flakiness for long-running services.** If `roux` (Rust) is painful, fall back to Python sidecar with PRAW. Decide in Phase 3 spike.
- **Ticker false positives in social text** — `$A`, `$TO`, `$IT`, `$YOU`, `$DIS`, `$ALL` are real tickers AND common words. Filter strategy needs iteration.
- **MCP sidecar lifecycle** — if Tauri app crashes, MCP dies and agent loops fail mid-flight. v1 mitigation: agent retries with backoff. v2: Phase 9 daemon.
- **Cost spiral in agent loops.** Multi-turn agents rack up calls fast. Per-loop AND global budget guardrails; both must work. Pre-Phase-5 dry-run to estimate cost per sweep.
- **Calibration takes weeks.** Phase 8 numbers aren't meaningful until ~30 trading days. Plan for "we don't know yet" period.
- **Trust drift.** Once daily morning packs land, tempting to skip the evidence chain and act on conviction labels. Eval harness keeps this honest. Budget reading time too.
