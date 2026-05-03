# Ticker Intake Enrichment: empty-on-add → primed + researched within ~60s

## Context

Today, `add_ticker` writes a single watchlist row and returns
(`src-tauri/src/services/tracker_service/mod.rs:58`,
`src-tauri/src/mcp/tools/add_ticker.rs:38`). Three downstream surfaces stay
empty until something **else** runs:

- **Projection** — computed on-demand by `ProjectionService::generate_projection_results`
  (`src-tauri/src/services/projection_service/mod.rs:46`). Today no caller
  fires this when a ticker is added; the user has to invoke
  `ibkr_generate_projection_results` from the UI.
- **News** — `IbkrNewsProvider::fetch` warms `news_cache` and runs
  `NewsInterpreter` (already gated by `LlmService` budget per Phase 8 wiring),
  but today only the EOD `TrackerRunner` ticks news per-symbol at
  ~16:05 ET. Ad-hoc add at 11am sees no news until close.
- **Research** — `research_notes` rows are written by
  `agent/morning_sweep.py` (07:00 ET cron) or `agent/alert_dive.py`
  (post-alert poller). Neither fires on add.

**Inversion.** Today the **schedulers** drive enrichment and the watchlist
row is a passive marker. End state: **adding a ticker triggers enrichment**.
The work splits along the budget seam — the deterministic enrichment
(fundamentals → projection → news) runs as a Rust task spawned from
`TrackerService::add` callsites; the LLM-driven baseline research note runs
as a new Python agent loop peer to `alert_dive.py`. Both reuse existing
services; neither is allowed to bypass its budget gate.

## End-state architecture

| Subsystem | Responsibility |
|---|---|
| **`TickerPrimerService`** (new, `src-tauri/src/services/ticker_primer/`) | Orchestrates post-add deterministic enrichment: fundamentals fetch → projection compute & cache → news fetch (which transparently runs `NewsInterpreter` per existing wiring). Async, fire-and-forget from `TrackerService::add` callers; idempotent on `last_primed_at < 24h`. |
| **`TrackerService::add` callers** (`add_ticker` MCP tool, UI add command in `src-tauri/src/ibkr/commands/`) | After the row insert succeeds, dispatch `primer.prime(symbol)` on a Tokio task. They do **not** block on enrichment. Existing audit + `TickerStatusChanged` event stay. |
| **Projection cache** (existing `cache_service.rs` JSON store) | Persists the most recent `ProjectionResults` per symbol so the workspace can render synchronously seconds after add. Existing 7d TTL applies. |
| **`agent/ticker_intake.py`** (new) | Python loop peer to `alert_dive.py`. Polls every 60s for watchlist symbols missing a baseline `research_notes` row in the last 7d, gathers fundamentals + news + bars via MCP reads, synthesises a baseline thesis via Anthropic under `BudgetGuard`, persists via `write_research_note` with `written_by = "agent.ticker_intake"`. |
| **`agent/prompts/ticker_intake.md`** (new) | System prompt for the loop. Baseline-note framing — not event-driven (alert_dive) and not ranked (morning_sweep). |
| **MCP audit** | `add_ticker` already audits the watchlist write; the prime task is internal orchestration and writes no audit row. The agent's `write_research_note` calls are audited as today. |
| **LLM budget paths** | `LlmService` ledger (`llm_calls`) absorbs the prime path's spend via `NewsInterpreter`. `BudgetGuard` (`agent/budget_guard.py`) absorbs the agent loop's spend, with the global cap check (`get_llm_budget_status` MCP tool) preventing combined runaway. |

## Hard invariants

1. **Surveillance-only stays.** Neither phase wires order-placement code or
   exposes any new write rail. CI greps continue to assert (no new
   `place_order` / `placeOrder` symbols anywhere under
   `src-tauri/src/services/ticker_primer/` or `agent/ticker_intake.py`).
2. **Prime is async + idempotent.** `TrackerService::add` returns the
   moment the row is inserted; priming runs on a spawned task. Re-priming
   a symbol whose `last_primed_at < 24h` is a no-op (no provider calls).
3. **All LLM calls go through the existing budget paths.** Rust
   `NewsInterpreter` (called transitively by news fetch) goes through
   `LlmService` and the `llm_calls` ledger. Python `ticker_intake` goes
   through `BudgetGuard` per-loop + the global cap check. No new LLM
   call sites bypass either.
4. **MCP surface stays read-only + the existing write rail.** No new MCP
   tools introduced. The agent loop reads via existing read tools and
   writes only via the existing `write_research_note` tool.
5. **Mock-friendly trait seams unchanged.** `IbkrClientTrait` continues
   to be the only IBKR seam; tests use `MockIbkrClient`. The primer is
   testable with no live IBKR client.
6. **`written_by` discipline.** Baseline notes use the literal
   `"agent.ticker_intake"` so eval, UI, and audits can distinguish them
   from `morning_sweep` and `alert_dive` writes.
7. **No regressions on existing schedulers.** EOD `TrackerRunner`,
   morning sweep, alert dive, decay watcher continue to fire on their
   existing cadences. Priming is strictly additive.
8. **Pre-commit sacred.** `cargo fmt --check`, `cargo clippy -D warnings`,
   `prettier --check`, `eslint`, `uv run pytest`. Never `--no-verify`.
9. **File-size caps respected.** Rust soft 300 / hard 500
   (`CONTRIBUTING.md`). Python the same convention as `alert_dive.py`.
   Past hard cap requires `// allow-large-file:` justifier.

Violating the letter of these rules is violating the spirit.

## Defaults committed (overridable per-phase)

- **Prime trigger:** every successful `TrackerService::add` from any
  caller (MCP `add_ticker` and UI add command). No opt-out flag.
- **Prime concurrency:** Tokio `spawn` per add; no global queue. The
  IBKR rate limiter is the throttle.
- **Prime idempotency window:** 24h. Stored as
  `tracked_tickers.last_primed_at` (new column, NULL ⇒ never primed).
- **Re-prime on re-add of an archived ticker:** yes; `archive` clears
  `last_primed_at`.
- **News lookback for prime:** 24h (`IbkrNewsProvider::fetch` default).
- **Projection cache TTL:** 7 days (existing `cache_service.rs` default).
- **Agent poll interval:** 60s.
- **Agent per-symbol cap:** $0.10 USD.
- **Agent per-loop cap:** $0.20 USD (`per_symbol × max_concurrent`).
- **Agent global reserve:** mirror `alert_dive.py`'s `GLOBAL_RESERVE_FRAC = 0.10`.
- **Agent reuse-note window:** 7 days. If any writer
  (`agent.ticker_intake`, `alert_dive`, `morning_sweep`) wrote a note
  for the symbol in the last 7d, skip.
- **Agent model:** `claude-sonnet-4-6` for synthesis (matches
  `alert_dive` default; cheap enough for a baseline note).
- **Agent eligibility predicate:** watchlist symbol with
  `last_primed_at IS NOT NULL` AND no `research_notes` row in the last
  7d. Gating on prime ensures the LLM has fundamentals + news to read.
- **Agent entry points:** `python -m ticker_intake` from `agent/`,
  plus `uv run qk-ticker-intake` script alias.
- **Agent runtime:** systemd service mirroring
  `agent/cron/alert_dive.service`.

## Phase index

| Phase | File | Depends on | Status |
|---|---|---|---|
| 1. `TickerPrimerService` + on-add hook + projection cache | [phase-1-rust-prime-on-add.md](phase-1-rust-prime-on-add.md) | — | done (commit 50cc2d1, 2026-05-03) |
| 2. `agent/ticker_intake.py` + system prompt + systemd unit | [phase-2-ticker-intake-loop.md](phase-2-ticker-intake-loop.md) | 1 | in-progress (started 2026-05-03) |

> **Status convention:** `todo` | `in-progress (started YYYY-MM-DD)` | `done (commit <sha>, YYYY-MM-DD)`. Update both this table AND the phase file's `**Status:**` header at phase start and exit. Don't start a phase whose dependencies aren't `done`.

## Critical files

| Concern | Path |
|---|---|
| Tracker watchlist write | `src-tauri/src/services/tracker_service/mod.rs` |
| `add_ticker` MCP tool (one of two prime callers) | `src-tauri/src/mcp/tools/add_ticker.rs` |
| UI add command (the other prime caller) | `src-tauri/src/ibkr/commands/tracker.rs` |
| Fundamentals (composite manual + AV fallback) | `src-tauri/src/services/fundamentals_provider/` |
| Projection compute | `src-tauri/src/services/projection_service/mod.rs` |
| Projection / fundamentals JSON cache | `src-tauri/src/services/cache_service.rs` |
| News provider trait + IBKR adapter | `src-tauri/src/services/news_provider/`, `src-tauri/src/ibkr/news_provider.rs` |
| News interpretation (LLM via `LlmService`) | `src-tauri/src/services/news_interpreter/mod.rs` |
| LLM budget enforcement + ledger | `src-tauri/src/services/llm_service/mod.rs`, `llm_calls` table |
| Service composition (where new services are constructed and managed) | `src-tauri/src/lib.rs` |
| Schema (column add for `last_primed_at`) | `src-tauri/src/storage/schema.sql` (or `src-tauri/migrations/` if refinery) |
| Tracker domain types | `src-tauri/src/ibkr/types/tracker.rs` |
| Agent loop pattern to mirror | `agent/alert_dive.py` |
| Agent budget guard | `agent/budget_guard.py` |
| Agent MCP client | `agent/mcp_client.py` |
| Agent LLM client | `agent/llm.py` |
| Agent shared input summarisers | `agent/data_summary.py` |
| MCP `write_research_note` writer | `src-tauri/src/mcp/tools/write_research_note.rs` |
| MCP audit | `src-tauri/src/services/mcp_audit/` |
| Existing agent prompts to mirror format | `agent/prompts/alert_dive.md`, `agent/prompts/morning_sweep.md` |
| Backend rules (file caps, layering) | `src-tauri/CLAUDE.md`, `CONTRIBUTING.md` |
| Agent rules / install / cron | `agent/README.md`, `agent/cron/alert_dive.service` |

## Sequencing + cadence

- **W1:** Phase 1. Adds `TickerPrimerService`, the column migration, and
  spawns from both add callers. Visible win: projection panel populates
  within ~10s of add; news cache warms within ~30s; the existing
  `NewsInterpreter` pass runs once per symbol via existing wiring (no
  new LLM call site).
- **W2:** Phase 2. Adds the agent loop. Visible win: a baseline
  research note appears within ~60s of add. Naturally inherits
  `BudgetGuard` per-loop cap and the global cap check.

Phase 1 ships first because it is strictly additive — no new LLM call
sites, no new agent infra, just chaining services that already exist.
Phase 2 must follow because the baseline-note synthesis benefits from
fundamentals + news already being warmed (cheaper read fan-out for the
agent, better LLM input).

## Cross-phase verification

1. **Tracer-bullet (Phase 1 exit):** Add a fresh symbol via the
   `add_ticker` MCP tool. Within 30 seconds:
   `tracked_tickers.last_primed_at` is set, `cache_service` has
   projection results for that symbol, `news_cache` has rows. Verified
   by an integration test that polls each table after the add returns.
   Manual smoke also covers the UI panel populating.
2. **Tracer-bullet (Phase 2 exit):** From a fresh DB, add a symbol,
   wait 90s, then assert `research_notes` has at least one row with
   `written_by = "agent.ticker_intake"`, `symbol` matching, and
   `conviction` in `{A, B, C}`. The integration test injects fakes for
   `McpClient` and `LlmClient` per agent test conventions.
3. **CI invariant — surveillance-only:** existing greps continue to
   assert. New paths
   (`src-tauri/src/services/ticker_primer/`, `agent/ticker_intake.py`)
   contain zero hits for `place_order`, `placeOrder`, order types.
4. **CI invariant — `written_by` discipline:** A unit test in
   `agent/tests/test_ticker_intake.py` asserts the literal
   `"agent.ticker_intake"` is the only writer string in
   `agent/ticker_intake.py` (no copy-paste from `alert_dive.py`).
5. **CI invariant — prime idempotency:** A Rust unit test asserts
   `prime(symbol)` twice within 24h short-circuits the second call
   without invoking any provider mock.
6. **Budget audit:** After Phase 2, an end-to-end test (gated, run
   manually) fires three intakes and asserts `BudgetGuard.spent_usd`
   and `llm_calls.cost_today_usd` both reflect non-zero spend with no
   double-counting. `NewsInterpreter` spend lands only in `llm_calls`;
   agent spend lands only in `BudgetGuard`; the global cap check sees
   the union.

## Open risks

- **Race: agent intake writes a note before EOD ranker / decay watcher.**
  `decay_watcher` and `daily_ranker` read `setups` and `theses`, not
  `research_notes`, so there is no semantic collision today. Document
  the boundary in Phase 2; revisit if ranker grows to consume notes.
- **News fetch latency.** IBKR news fetch can take 5-15s per symbol.
  Priming runs spawned, so the user is not blocked, but the news panel
  is empty for that window. Phase 1 emits a `TickerPrimingDone`
  AppEvent; the workspace can listen and refresh. Detailed UX is out
  of scope for this plan; if the empty-state feels bad, file in
  `QUESTIONS.md` after Phase 1.
- **Batch-add storms.** User pastes 50 symbols. 50 spawned priming
  tasks all hit IBKR fundamentals + news. The IBKR rate limiter is the
  throttle. Phase 1 verifies under load before exit; if storms cause
  problems, add a bounded primer queue in a follow-up.
- **Fundamentals not in manual store + no AV key.** The composite
  provider returns "no fundamentals", projection has no baseline. Prime
  logs and continues; projection cache simply isn't populated; UI
  falls through to the existing "no projection" empty state.
- **Agent loop and morning sweep collide.** Both can write notes for
  the same symbol. Reuse-note window (7d) prevents thrashing. If
  morning sweep writes for a symbol the user added the previous evening,
  both rows exist by design — the eval harness can grade them
  separately.
- **Budget guard divergence under three concurrent agents.**
  `morning_sweep` + `alert_dive` + `ticker_intake` independently track
  per-loop budget but share the global cap. Risk: agent paths trip the
  global cap before the Rust scheduler does. Mitigation: keep
  `GLOBAL_RESERVE_FRAC = 0.10` and consider raising the daily cap if
  needed. Phase 2 documents the new agent in `agent/README.md`.
- **`last_primed_at` column migration.** Non-destructive
  `ALTER TABLE tracked_tickers ADD COLUMN last_primed_at INTEGER NULL`.
  Existing rows backfill NULL ⇒ "never primed" ⇒ they will be primed
  the next time the user touches them. Acceptable.
- **Coordination with the workspace plan (retired in `a3fbf16`).** The
  workspace's per-symbol panels read existing read commands; Phase 1's
  `TickerPrimingDone` event is a hint, not a requirement. The workspace
  needs no source change for this plan to ship.
