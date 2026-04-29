# Phase 24 — Daily journal skill

## Goal

A `/journal` Claude Code skill that, when invoked manually after market close, renders a single retrospective markdown file (`journal/YYYY-MM-DD.md`) summarizing today's market, scanner highlights, detected setups, and executed trades — with each fill cross-referenced to the originating setup's thesis. Synthesis happens in Claude Code itself; the only backend addition is one Tauri command (`ibkr_get_executions`) that exposes today's IBKR fills. No SQLite schema change. No LLM cost.

## Depends on

- [x] Phase 04 — `tracked_tickers` rows persisted (scanner highlights source).
- [ ] Phase 10 — `setups` rows persisted (detected setups source).
- [ ] Phase 17 — `setups.thesis` populated by the thesis generator (trade reasoning source).

Phase 11 (market calendar) is a soft dep — useful for "today's session window" but not blocking. v1 uses a naive `America/New_York` ET-date check.

## Out of scope

- Automated / cron invocation. v1 is manual only; a scheduled variant is a follow-up.
- `daily_journals` SQLite table or in-app journal panel (Shape C from brainstorming, deferred).
- Options trades — equities only, consistent with the rest of the system.
- Per-trade P&L attribution. v1 surfaces NLV-delta from `ibkr_get_account_summary` but does not attribute P&L to individual fills.
- Multi-account aggregation.

## Test plan (write tests FIRST)

Backend (`src-tauri/src/ibkr/tests/`):

- [ ] `executions_filters_to_requested_date` — `MockIbkrClient::executions(NaiveDate::from_ymd(2026, 4, 29))` returns only fills whose `exec_time` falls in that ET date; fills from prior/next ET dates are excluded.
- [ ] `executions_serializes_for_frontend` — `IbkrExecution` round-trips through `serde_json` with snake_case fields matching the existing convention in `ibkr/types/orders.rs` (`symbol`, `side`, `qty`, `avg_price`, `exec_time`, `order_id`, `exec_id`).
- [ ] `executions_empty_when_no_fills` — empty result returns `Ok(vec![])`, no panic, no error.
- [ ] `command_invokes_client_with_correct_date` — the `ibkr_get_executions` Tauri handler parses the date string argument, calls `client.executions(parsed_date)`, and forwards the result.
- [ ] `command_rejects_malformed_date` — handler returns a typed error (not a panic) when the date string is not `YYYY-MM-DD`.

Manual E2E (skill):

- [ ] With the app running on a weekday after 16:00 ET, run `/journal` from a Claude Code session inside the repo. Expected: `journal/YYYY-MM-DD.md` is created with all four sections populated.
- [ ] Re-running `/journal` on the same date overwrites the file (idempotent).
- [ ] Trade row with a matching setup detected within the prior 5 trading days renders the linked setup's `thesis` markdown beneath it.
- [ ] Trade row with no matching setup is rendered with a "no detected setup" flag (still listed; reasoning section is empty rather than fabricated).
- [ ] Running on a non-trading day (weekend) produces a mostly-empty file without errors.
- [ ] `journal/` is gitignored — `git status` shows no untracked entries inside it after a render.

## Implementation tasks

Backend:

- [ ] Add `IbkrExecution` to `src-tauri/src/ibkr/types/orders.rs`:
  ```rust
  pub struct IbkrExecution {
      pub symbol: String,
      pub side: ExecutionSide,   // Bought | Sold (existing enum if present, else add)
      pub qty: f64,
      pub avg_price: f64,
      pub exec_time: DateTime<Utc>,
      pub order_id: i32,
      pub exec_id: String,
  }
  ```
  serde-tagged snake_case to match the rest of the module.
- [ ] Add `executions(date: NaiveDate) -> Result<Vec<IbkrExecution>>` to `IbkrClient` (`src-tauri/src/ibkr/client.rs`). Wrap the `ibapi` executions/commissions stream; filter to the requested ET date by converting `exec_time` to `America/New_York` and comparing dates.
- [ ] Add `MockIbkrClient::executions` (`src-tauri/src/ibkr/mocks.rs`) with a setter so tests can inject fills.
- [ ] Add Tauri command `ibkr_get_executions(date: String) -> Result<Vec<IbkrExecution>, String>` in `src-tauri/src/ibkr/commands/trading.rs`. Parses the date as `YYYY-MM-DD`, calls `client.executions`, maps errors via the existing string-conversion pattern.
- [ ] Register `ibkr_get_executions` in `src-tauri/src/lib.rs` `tauri::generate_handler![]`.
- [ ] One-line update to `CLAUDE.md` listing the new command alongside the other `ibkr_*` entries in the Tauri Commands section.

Skill:

- [ ] Create `.claude/skills/daily-journal/SKILL.md` with frontmatter:
  ```
  ---
  name: daily-journal
  description: Render today's after-market-close trading journal to journal/YYYY-MM-DD.md. Use when the user types /journal or asks for the daily journal entry. Prerequisites: Tauri app running, TWS/Gateway connected.
  ---
  ```
  Body is a fixed checklist:
  1. Compute today's ET date. Resolve the SQLite path (`app_local_data_dir()/tracker.sqlite`) — Linux: `~/.local/share/com.quantum.kapital/tracker.sqlite` (or whatever the Tauri identifier resolves to).
  2. Read SQLite directly (`sqlite3` CLI or `Bash` heredoc):
     - `tracked_tickers` rows where `added_at` falls in today's ET session window AND `source='scanner'`.
     - `setups` rows where `detected_at` falls in today's session — include `id, symbol, strategy, direction, trigger_price, stop_price, targets, raw_signals, thesis, thesis_json, status`.
     - `news_cache` rows for symbols touched today (best-effort context).
  3. Invoke Tauri commands via the running app. Skill must state explicitly that the app + TWS must be running, and fail-soft if `ibkr_get_executions` returns empty (render the section with "no fills today").
  4. For each fill: scan `setups` for the same symbol with `detected_at` within the prior 5 trading days; if a match exists, embed `setups.thesis` (markdown) and the structured `trigger_price`, `stop_price`, `targets` under the trade row. If no match, render the trade with a "no detected setup" flag.
  5. Render `journal/YYYY-MM-DD.md` with the four sections (template embedded in the skill body). Idempotent — overwrites on re-run.
- [ ] Add `journal/` to `.gitignore`.

## Verification

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml ibkr::tests::command_tests::executions` — all new tests green.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml ibkr::tests::client_tests` — existing tests still green.
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` clean.
- [ ] `cargo fmt --manifest-path src-tauri/Cargo.toml` clean.
- [ ] Manual E2E checklist above.
- [ ] `pnpm typecheck` clean (no frontend changes expected, but verify `ibkr_get_executions` is callable from `src/shared/api/ibkr.ts` if you choose to expose it there for parity).

## Files

**Created:**

- `.claude/skills/daily-journal/SKILL.md`
- (no Rust files — extensions only)

**Modified:**

- `src-tauri/src/ibkr/types/orders.rs` (`IbkrExecution`)
- `src-tauri/src/ibkr/client.rs` (`executions` method)
- `src-tauri/src/ibkr/mocks.rs` (`MockIbkrClient::executions`)
- `src-tauri/src/ibkr/commands/trading.rs` (`ibkr_get_executions`)
- `src-tauri/src/ibkr/tests/command_tests.rs` (executions test cases)
- `src-tauri/src/lib.rs` (register command)
- `.gitignore` (add `journal/`)
- `CLAUDE.md` (one-line entry under the Tauri Commands list)

## Scratchpad

None.

## Done when

`/journal` invoked on a weekday evening (with the app + TWS running) produces a `journal/YYYY-MM-DD.md` containing market overview (SPY/QQQ/IWM/VIX), scanner highlights for today, detected setups for today, and trades for today — each trade with a thesis when the setup linkage exists. The Tauri command is unit-tested. Re-runs are idempotent. The output directory is gitignored.
