# Behavioral assessment via MCP: trade reviews, today's playbook, trader profile — ~3 weeks

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the per-conversation "rate yesterday's trades / what's the setup today?" assessment into cached, structured artifacts surfaced through the MCP read surface — so every LLM client and the desktop UI consume the same, deterministic, behaviorally-aware output without re-deriving it from raw fills.

**Architecture:** Three layers. **L1 — deterministic Rust aggregators** (`get_trade_legs`, `get_watchlist_briefing`) collapse fan-outs into single calls. **L2 — extended Python agent loops** (existing `morning_sweep.py`, existing `eod_review.py`) write structured rows to two new SQLite tables (`day_reviews`, `playbooks`). **L3 — three new MCP read tools** (`get_trade_review`, `get_today_playbook`, `get_trader_profile`) serve the artifacts back. **The moat:** Phase 6 conditions tomorrow's playbook on the trader's behavioral history (e.g. deprioritize TSLA 0DTE if `chase_own_exit` has fired ≥3× in the last 7 days).

**Tech Stack:** Rust (rusqlite, refinery, rmcp, tokio, serde), Python 3.11+ (uv, anthropic SDK + claude-cli backend, asyncio, mcp), React 19 + TypeScript + Tailwind 4 + Vite (frontend).

---

## Context

The MCP surface today exposes raw data — `get_executions`, `get_quote`, `get_bars`, `get_news`, `get_watchlist`, `get_candidates`, `get_setups`, `get_outcomes`, `get_morning_pack`. When an LLM client (Claude Code or the in-app chat) is asked "rate yesterday" or "what's the setup today?", it has to fan out 12+ tool calls, manually FIFO-match fills, manually sum commissions, and synthesise a grade fresh every time — non-deterministic, expensive in tokens, and behaviorally amnesiac. The session that produced THIS plan demonstrated the cost: a single "rate yesterday" question consumed 4 tool calls and ~30 seconds of LLM reasoning to compute leg-level P&L by hand; "today's setup" consumed 12 calls and a similar reasoning budget — both producing valuable output that immediately evaporates because nothing persists it.

The agent stack already has the right shape but not the right outputs. `agent/morning_sweep.py` writes a `morning_pack` (free-form `ranked_ideas`) every weekday at 07:00 ET; `agent/eod_review.py` writes a `journal_entries` markdown commentary every weekday at 17:00 ET — but the journal is **prose**, not structured, and the EOD review scores against `get_outcomes` (predictions vs realized bars), **not** against actual fills. Neither output feeds the other; neither is queryable for behavioral patterns over time.

The deferred `loop/plan/phase-4-persistence.md` (now retired) speced an executions-store; it was deferred because no consumer needed multi-day fill history. **This plan is that consumer.** Phase 1 ships the persistence layer first; Phases 2–7 build the assessment stack on top.

**Inversion.** Today the assessment is a per-conversation LLM artifact that dies at the end of every session. End state: the assessment is a **persisted, structured, queryable artifact** that compounds — every review feeds tomorrow's playbook, every playbook outcome feeds the next review's behavioral context.

## End-state architecture

| Component | Layer | Responsibility |
|---|---|---|
| **Executions store** (Phase 1) | L0 storage | SQLite `executions` table; idempotent UPSERT keyed by `exec_id`; late-arriving commission patches existing rows. Background worker drains live IBKR every 5 min during market hours. |
| **`get_trade_legs(date)`** (Phase 2) | L1 aggregator | FIFO-matches buys+sells per `(account, symbol, contract_type, expiry, strike, right)` group; returns round-trip + carryover legs with realized P&L net of commissions. Pure function over the executions store. |
| **`get_watchlist_briefing(symbols?, lookback_days?)`** (Phase 3) | L1 aggregator | Single MCP call returns `{symbol, quote, bars, news, sentiment, setups, fundamentals}` per watchlist row. Composes existing services; per-symbol error envelope so partial failures don't block. |
| **`day_reviews` table + extended `eod_review.py`** (Phase 4) | L2 artifact | New table keyed by `date` + `prompt_version`. Extended `eod_review.py` calls `get_trade_legs(yesterday)`, computes the grade **deterministically** in Rust from leg metrics + behavioral_tag weights, asks the LLM only for narrative + tag selection (forced-tool with closed enum), persists via new `write_trade_review` MCP write rail. |
| **`get_trade_review(date)`** (Phase 4) | L3 read | Pure read of `day_reviews`. Returns `null` envelope if no review yet for that date. |
| **`playbooks` table + extended `morning_sweep.py`** (Phase 5) | L2 artifact | New table keyed by `date` + `generation_id`. Extended `morning_sweep.py` calls a new `write_playbook` MCP rail with structured `ranked_setups` (trigger/entry/invalidation/target/conviction) + `skip_list`. Distinct from the existing free-form `morning_pack` — packs are research notes, playbooks are actionable orders-shaped objects. |
| **`get_today_playbook(date)`** (Phase 5) | L3 read | Returns the latest `generation_id` for the given date. Multiple generations per day allowed (intraday refresh on material change). |
| **`get_trader_profile(window_days?)`** (Phase 6) | L3 read | Pure SQL aggregate over the last N `day_reviews`: tag frequencies, P&L by tag, behavioral trendline (last 7d vs prior 21d). No LLM. |
| **Behavioral feedback into `morning_sweep.py`** (Phase 6) | L2 prompt | Loop fetches `get_trader_profile` at start, prepends to system prompt, conditions `ranked_setups` on the user's prior behavioral incidents. **The moat.** |
| **UI panels** (Phase 7) | L4 surface | New `src/features/trade-review/`, `src/features/playbook/`, `src/features/trader-profile/`. Tauri commands wrap the same Rust services as the MCP read tools; no duplicate logic. |

## Hard invariants

1. **Surveillance-only stays.** Every new MCP tool — both reads (`get_trade_legs`, `get_watchlist_briefing`, `get_trade_review`, `get_today_playbook`, `get_trader_profile`) and writes (`write_trade_review`, `write_playbook`) — is non-trading. No phase may add an order-placement code path. The MCP tool surface stays "read-only + `ack_alert` + agent-write rails" per `CLAUDE.md`.
2. **MCP writes are agent-write rails, not user-write rails.** `write_trade_review` and `write_playbook` mirror the existing `write_morning_pack` and `write_research_note` pattern: audited via `services/mcp_audit/`, callable only from agent loops (in practice — there's no enforced auth, but the convention holds and the tools' doc strings say "agent-only").
3. **Behavioral tags are a CLOSED ENUM.** Defined once in Rust (`services/trade_reviews/tags.rs`), mirrored once in Python (`agent/trade_review.py`), validated at the MCP boundary. The LLM picks from the enum; it does NOT freeform new tags. A mirror-test pins both lists in sync.
4. **Grade is computed deterministically.** A pure Rust function `compute_grade(leg_summary, tags) -> Grade` maps leg metrics + tag weights to A/B/C/D/F. The LLM writes the **narrative** justifying the grade; it does NOT pick the grade. Re-running yesterday's review must produce the same grade across runs (same inputs → same output).
5. **Idempotency.** `day_reviews` UPSERT keyed on `(date, prompt_version)`; `playbooks` UPSERT keyed on `(date, generation_id)`. Re-running cron is a no-op or upgrade; never a duplicate row, never a partial overwrite.
6. **All new LLM call sites go through the existing `LlmService` (Rust) or `BudgetGuard` (Python).** No bypass. Both layers' kill-switches must trip on the new code paths exactly as they do for the existing `morning_sweep` / `eod_review`.
7. **Time-zone discipline.** All dates in tool args are `YYYY-MM-DD` interpreted as the **ET trading day**. All timestamps in the wire DTOs are UTC ISO 8601. The frontend handles presentation TZ.
8. **No live IBKR in tests.** All Phase 1+ tests use `MockIbkrClient` (existing) or the `AccountReader` fake. All Phase 2+ tests use the in-memory test DB from `mcp::tools::test_support::make_db()`. All Python tests inject fakes for both `mcp_client` and `LlmClient`.
9. **Pre-commit sacred.** `cargo fmt --check`, `cargo clippy -D warnings`, `prettier --check`, `eslint`. Never `--no-verify`. Fix the underlying issue.
10. **File-size caps.** Rust soft 300 / hard 500. TS/TSX soft 200 / hard 350. Past hard cap requires `// allow-large-file: <reason>`. When a service's writer + reader paths approach the soft cap, split (`mod.rs` + `store.rs` + `query.rs` + ...).

Violating the letter of these rules is violating the spirit.

## Defaults committed (overridable per-phase)

- **Date format:** ISO 8601 `YYYY-MM-DD`, ET trading day. Same convention as the existing tools.
- **Account resolution:** reuse `resolve_account(...)` from `mcp/tools/mod.rs`. Optional `account` arg; defaults to the sole managed account; errors with the available IDs when multiple are connected.
- **Currency:** USD only.
- **Behavioral tags v1 enum** (12 values; expandable in later phases):
  - `chase_own_exit` — re-entered same instrument within 5 min of taking profit on it (weight: −10)
  - `late_otm_lottery` — opened OTM 0DTE within 60 min of expiry (weight: −10)
  - `gamma_window_violation` — held 0DTE position past 15:30 ET into expiry day (weight: −5)
  - `single_name_concentration` — >70% of day's gross notional on one underlying (weight: −5)
  - `position_sizing_ungraduated` — late-day setup sized same as morning conviction setup (weight: −5)
  - `post_loss_revenge` — opened new position within 5 min of a losing close (weight: −5)
  - `flat_close` — all positions closed by EOD (weight: +5)
  - `discipline_on_loser` — losing leg cut within 10 min of breach (weight: +5)
  - `scaled_in_winner` — added to a position already showing profit (weight: +3)
  - `scaled_in_loser` — added to a position already showing loss (averaging down) (weight: −7)
  - `thesis_match_executed` — traded a symbol that appeared in this morning's playbook (weight: +5)
  - `off_thesis_trade` — traded a symbol not in any recent playbook (weight: −3)
- **Grade banding:** `score = clamp(net_pnl_normalized + sum(tag_weights), -50, +50)`. Bands: `score ≥ 25 → A`, `≥ 10 → B`, `≥ -5 → C`, `≥ -20 → D`, else `F`. Refined per-phase if it produces grade-clustering issues during dogfooding. `net_pnl_normalized = clamp(net_pnl_usd / 100, -25, +25)` (so a +$2,500 day caps at +25, a −$5,000 day floors at −25).
- **`prompt_version` semantics:** integer that bumps when (a) the rubric weights change, (b) the tag enum gains/loses a value, or (c) the LLM system prompt for the review changes materially. Bumping forces a re-grade on next cron tick; old reviews remain queryable.
- **Empty days:** `get_trade_review(date)` for a date with no fills returns `{date, review: null}`; `get_today_playbook(date)` for a date with no playbook returns `{date, playbook: null}`. Not errors.
- **Sort order:** trade legs ascending by `opened_at`; playbook setups by `conviction` desc then `symbol` asc; trader profile tag rows by `frequency` desc.

## Phase index

| Phase | File | Depends on | Status |
|---|---|---|---|
| 1. Persist executions to SQLite | [phase-1-persist-executions.md](phase-1-persist-executions.md) | — | done (commit d0e0c7a, 2026-05-05) |
| 2. `get_trade_legs` MCP aggregator (FIFO leg-matching) | [phase-2-trade-legs-tool.md](phase-2-trade-legs-tool.md) | 1 | done (commit c75c2f5, 2026-05-05) |
| 3. `get_watchlist_briefing` MCP fan-out aggregator | [phase-3-watchlist-briefing.md](phase-3-watchlist-briefing.md) | — (parallel-able with 2) | done (commit d748ff4, 2026-05-05) |
| 4. `day_reviews` schema + extended `eod_review.py` + `get_trade_review` / `write_trade_review` MCP tools | [phase-4-trade-review.md](phase-4-trade-review.md) | 2 | done (commit d8afab3, 2026-05-05) |
| 5. `playbooks` schema + extended `morning_sweep.py` + `get_today_playbook` / `write_playbook` MCP tools | [phase-5-playbook.md](phase-5-playbook.md) | 3 | done (commit f892057, 2026-05-05) |
| 6. `get_trader_profile` MCP tool + behavioral feedback wired into `morning_sweep.py` | [phase-6-trader-profile.md](phase-6-trader-profile.md) | 4, 5 | done (commit e60e993, 2026-05-05) |
| 7. UI surfacing: Trade Review card, Today's Playbook panel, Trader Profile dashboard | [phase-7-ui-surfacing.md](phase-7-ui-surfacing.md) | 4, 5, 6 | todo |

> **Status convention:** `todo` | `in-progress (started YYYY-MM-DD)` | `done (commit <sha>, YYYY-MM-DD)`. Update both this table AND the phase file's `**Status:**` header at phase start and exit. Don't start a phase whose dependencies aren't `done`.

## Critical files

| Concern | Path |
|---|---|
| Existing executions DTO + adapter | `src-tauri/src/ibkr/types/orders.rs`, `src-tauri/src/ibkr/client/orders.rs` |
| MCP IBKR seam (production) | `src-tauri/src/mcp/ibkr_seam.rs` |
| MCP handler + tool registration | `src-tauri/src/mcp/handler.rs`, `src-tauri/src/mcp/server.rs` |
| MCP tool helpers + audit | `src-tauri/src/mcp/tools/mod.rs`, `src-tauri/src/services/mcp_audit/` |
| MCP test fakes / fixtures | `src-tauri/src/mcp/tools/test_support.rs` |
| Reference read tools (mirror these) | `src-tauri/src/mcp/tools/executions.rs`, `positions.rs`, `get_morning_pack.rs` |
| Reference write tools (mirror these) | `src-tauri/src/mcp/tools/write_morning_pack.rs`, `write_research_note.rs` |
| Reference service (artifact persistence) | `src-tauri/src/services/agent_morning_packs/` |
| Storage migrations | `src-tauri/src/storage/migrations/` (next free: V13) |
| Storage runner + pool | `src-tauri/src/storage/migrations.rs`, `src-tauri/src/storage/mod.rs` |
| Service composition root | `src-tauri/src/lib.rs` (`run()` function) |
| LLM service + budget ledger | `src-tauri/src/services/llm_service/` |
| Existing agent loops (mirror these) | `agent/morning_sweep.py`, `agent/eod_review.py` |
| Agent MCP client + budget guard | `agent/mcp_client.py`, `agent/budget_guard.py` |
| Agent prompts | `agent/prompts/morning_sweep.md`, `agent/prompts/eod_review.md` |
| Agent config | `agent/config.toml`, `agent/config.py` |
| Agent tests | `agent/tests/` |
| FE feature folder conventions | `src/CLAUDE.md` |
| FE feature peers (mirror these) | `src/features/portfolio/`, `src/features/candidates/`, `src/features/trades/` (Phase 3 of the retired plan, currently shipped) |
| FE Tauri command wrappers | `src/shared/api/` |
| Repo-level rules | `CLAUDE.md`, `src-tauri/CLAUDE.md`, `src/CLAUDE.md` |

## Sequencing + cadence

- **Days 1–3 (Phase 1):** persistence layer ships. Backend-only. Visible win: restart the app, `get_executions(yesterday)` returns yesterday's fills (assuming the app was running yesterday and ingested them). Until this lands, none of Phases 2/4/6 can be tested with real multi-day data.
- **Days 4–6 (Phase 2):** `get_trade_legs(date)` ships. Visible win: from a Claude Code session, one tool call returns leg-by-leg P&L matching IBKR Trade Log to ±$0.01 — collapsing the 4-call manual aggregation that produced this plan's trade-review section.
- **Days 4–6 in parallel (Phase 3):** `get_watchlist_briefing()` ships. Visible win: one tool call replaces the 12-call fan-out that produced this plan's setup-ranking section.
- **Days 7–9 (Phase 4):** `day_reviews` table + extended `eod_review.py` + `get_trade_review` / `write_trade_review`. Visible win: at 17:00 ET cron tick, structured trade review row written; LLM client asking "rate today" gets a cached, deterministic response in one call.
- **Days 10–12 (Phase 5):** `playbooks` table + extended `morning_sweep.py` + `get_today_playbook` / `write_playbook`. Visible win: at 07:00 ET cron tick, structured playbook row written; LLM client asking "what's the setup?" gets cached, deterministic ranked setups.
- **Days 13–15 (Phase 6):** `get_trader_profile` + behavioral feedback into morning_sweep. Visible win: a same-day re-run of morning_sweep with a profile loaded that has 3 `chase_own_exit` incidents on TSLA produces a playbook that explicitly flags TSLA in the `skip_list` with reason `"recent chase_own_exit pattern"`.
- **Days 16–20 (Phase 7):** UI panels. Visible win: open the desktop app at any time, see today's playbook, yesterday's review, and the trailing 30-day trader profile dashboard with tag frequencies + P&L-by-tag chart.

Phases 1 → 2 → 4 → 5 → 6 → 7 is the strict critical path. Phase 3 is parallelisable with Phase 2 (no shared files), so a two-engineer split shaves 3 days off the total.

## Cross-phase verification

1. **Tracer-bullet (Phase 1 exit):** restart `pnpm tauri dev`, query `get_executions(today=2026-05-04)` from the desktop app and `get_executions(2026-05-03)` (yesterday) from a fresh Claude Code session. Both return populated fills. The yesterday call serves rows from the store with no live IBKR drain (verifiable in `/tmp/qk-tauri.log`).
2. **Tracer-bullet (Phase 2 exit):** with at least one round-trip and one carryover fill in the store from Phase 1, `get_trade_legs(date)` returns a `legs[]` array where (a) every closed `(symbol, contract_type, strike, expiry, right)` group has `buy_qty == sell_qty`, (b) `gross_pnl - commission_total == net_pnl` to ±$0.01, (c) `totals.net_pnl` matches `sum(legs[].net_pnl)`, and (d) carryover legs have `closed_at == null`. Cross-checked against IBKR Trade Log to ±$0.01.
3. **Tracer-bullet (Phase 3 exit):** `get_watchlist_briefing()` returns one row per `get_watchlist()` row with `quote`, `bars`, `news`, `sentiment`, `setups`, `fundamentals` fields all populated (or with explicit per-field `null + error_reason` envelope on partial failures). Latency under 5s for a 4-symbol watchlist (each constituent ≤2s, fan-out concurrent).
4. **Tracer-bullet (Phase 4 exit):** at 17:00 ET cron tick on a real trading day with fills, `eod_review.py` writes BOTH the existing journal entry AND a new `day_reviews` row. `get_trade_review(today)` returns the row with `grade ∈ {A,B,C,D,F}`, `behavioral_tags ⊆ enum`, `narrative_md` non-empty. Re-running the cron tick within the same minute is a no-op (idempotent UPSERT). Re-running with a bumped `prompt_version` writes a new row alongside the old.
5. **Tracer-bullet (Phase 5 exit):** at 07:00 ET cron tick on a real trading day, `morning_sweep.py` writes BOTH the existing morning pack AND a new `playbooks` row. `get_today_playbook(today)` returns the latest generation with `ranked_setups[]` items each carrying `{symbol, bias ∈ ["long","short"], trigger: str, entry: str, invalidation: str, target_1: str, target_2?: str, conviction ∈ ["A","B","C"], rationale_md, evidence_refs[]}` and a `skip_list[]` of `{symbol, reason}`. Schema validation pinned by a serde round-trip test.
6. **Tracer-bullet (Phase 6 exit, the MOAT):** seed `day_reviews` with 5 reviews where `chase_own_exit` fired on TSLA in 3 of them. Run `morning_sweep.py` twice, once with the new behavioral feedback enabled, once with `--no-profile`. Diff the playbook outputs: with-profile must surface TSLA in `skip_list` with `reason: "recent chase_own_exit pattern (3 of last 7 days)"`; without-profile must NOT.
7. **Tracer-bullet (Phase 7 exit):** open the desktop app, navigate to `/review/yesterday` — see grade, P&L summary, behavioral tags as chips, leg table; navigate to `/playbook/today` — see ranked setups + skip list; navigate to `/profile` — see tag frequencies bar chart, P&L-by-tag heatmap, behavioral trendline (last 7d vs prior 21d).
8. **CI invariant — surveillance-only:** a test in `src-tauri/tests/` asserts no MCP tool source file imports `place_order`, `OrderRequest`, or anything from `ibkr/commands/trading.rs`. Extend the existing test from the retired plan (Phase 2's surveillance test).
9. **CI invariant — grade determinism:** a unit test in `services/trade_reviews/grade.rs` builds a fixed `LegSummary` + tag list and asserts the same grade across 1000 invocations.
10. **CI invariant — idempotent UPSERT:** unit tests on both `day_reviews` and `playbooks` services assert that calling the writer twice with the same key writes one row total.
11. **CI invariant — behavioral_tag enum mirror:** a Python test (`agent/tests/test_tag_mirror.py`) parses the Rust enum source and asserts the Python `BEHAVIORAL_TAGS` list matches name-for-name. Mirrors the existing pricing-table mirror test pattern.
12. **CI invariant — read-only audit on reads:** tests assert `get_trade_legs`, `get_watchlist_briefing`, `get_trade_review`, `get_today_playbook`, `get_trader_profile` write zero `mcp_audit` rows.
13. **CI invariant — write-rail audit on writes:** tests assert `write_trade_review`, `write_playbook` write exactly one `mcp_audit` row per call (mirroring `write_morning_pack`).

## Open risks

- **Forward-only history.** `day_reviews` and `playbooks` start collecting on the day Phases 4 and 5 ship respectively. No backfill from before. The trader-profile view (Phase 6) explicitly notes "based on N reviews since YYYY-MM-DD" so the user understands the window. — owned by Phase 4 (semantics) and Phase 7 (UI copy).
- **Grading rubric drift.** Bumping `prompt_version` forces re-grade on the next cron tick; old reviews stay queryable but become inconsistent with new ones. Mitigation: the trader-profile aggregator (Phase 6) groups by `prompt_version` so trends within one rubric stay coherent. — owned by Phase 4 (schema) and Phase 6 (aggregator).
- **LLM cost.** Each trading day adds 1 extended `eod_review` call (already exists, just bigger prompt) and 1 extended `morning_sweep` call (already exists, just bigger prompt). Net cost increment estimated <$0.05/day; well within the daily budget cap. Both calls run through the existing budget kill-switch. — owned by Phase 4 + Phase 5.
- **Tag enum drift between Rust and Python.** The closed enum lives in two places: Rust storage (`services/trade_reviews/tags.rs`) and the Python prompt (`agent/trade_review.py`). Mirror-test pins them in sync. — owned by Phase 4.
- **Multi-leg combo orders.** A spread produces multiple `ExecutionData` rows (one per leg) with different `exec_id`s; the FIFO leg-matcher will treat them as independent legs. v1 acceptable — flag as `tags: ["complex_strategy"]` if heuristic detects same `order_id` across multiple legs. v2 (deferred) groups them into a single combo leg. — owned by Phase 2.
- **Playbook freshness vs intraday change.** A playbook written at 07:00 ET may be invalidated by 11:00 if a watchlist name gaps or news drops. v1 ships with one daily playbook generation; the schema admits multiple `generation_id`s per date so an intraday refresh hook can be added later without migration. — owned by Phase 5.
- **`get_watchlist_briefing` partial failures.** A single news-cache miss shouldn't fail the whole call. Per-symbol error envelope (`{symbol, quote, ..., errors: ["news: upstream_failed"]}`). The agent loops downstream must handle the partial shape. — owned by Phase 3.
- **Existing `eod_review.py` still writes the journal entry.** Phase 4 EXTENDS it; the journal entry continues to ship as a sibling output. Don't replace — the journal is a separate consumer surface (`journal/YYYY-MM-DD.md` rendered file). — owned by Phase 4.
- **Persistence first means a long blocker.** Phases 4–7 cannot dogfood until Phase 1 has been running for at least one full trading session (because there's no historical backfill). Plan accordingly: Phase 1 ships → wait one trading day → Phases 2/4 can be tested on real data. — flagged here.

## Out of scope

- **Order entry / automation.** Surveillance-only. Forever.
- **Multi-currency P&L aggregation.** USD only.
- **Tax-lot accounting / cost basis.** IBKR Activity Statement is authoritative.
- **Backfill of pre-Phase-1 fill history.** IBKR's `reqExecutions` is current-TWS-day-only; no API path. Out of scope for v1, separate program if ever needed.
- **Backfill of pre-Phase-4 trade reviews / pre-Phase-5 playbooks.** Forward-only. No historical reconstruction.
- **Real-time intraday playbook refresh.** Schema admits multiple `generation_id`s per date but v1 only ships one generation per day. Intraday refresh is a future trigger-driven program.
- **Combo / spread leg grouping.** v1 treats each `ExecutionData` row as an independent leg; combo grouping is v2.
- **LLM-generated grade.** The grade is computed deterministically. The LLM only writes the narrative.
- **Cross-account aggregation.** Each `day_review` and `playbook` row is per-account.
