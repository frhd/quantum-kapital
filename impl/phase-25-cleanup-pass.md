# Phase 25 — Cleanup pass (panic removal, file splits, `unwrap` audit)

## Goal

Burn down the structural debt surfaced by the post-Phase-15 codebase audit so the repo enters Phase 26+ with no chunky files, no bare `panic!` in production paths, and a documented escalation rule (`CONTRIBUTING.md`) so the same drift doesn't reaccumulate.

This phase is **mechanical** — no behavior change, no new commands, no schema migrations. Every change is covered by the existing test suite; the bar is "all 215 Rust tests still pass and nothing in the public API moves."

## Depends on

- [x] Phase 15 — final landed feature; everything before is structurally complete and stable to refactor against.

## Out of scope

- Phase 16+ LLM work. This phase is debt-burndown only.
- Behavior changes inside the modules being split. Move code, do not edit logic.
- `services/cache_service.rs` cleanup (`clear_entry` / `clear_expired` / `clear_all` allow-listed for Phase 16 — leave as-is).
- Removing any `#[allow(dead_code)]` block whose CLAUDE.md justification points at a future phase. The audit confirmed every existing block is documented.

## Test plan (write/verify tests FIRST)

Cleanup work is verified by **regression**, not new tests. Before any move:

- [x] Capture the current baseline: `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast 2>&1 | tee /tmp/phase-25-before.txt`. Confirm `215 passed; 0 failed; 1 ignored`.
- [x] Capture clippy + fmt baseline: both clean today; rerun after every commit boundary inside the phase and require they stay clean.
- [x] Capture `pnpm typecheck`, `pnpm lint`, `pnpm format:check` — all green today.

For the file splits, add **one new assertion per split** that imports the moved item from its new module path, proving the public surface is reachable from outside the crate-internal call sites:

- [x] `ibkr_client_split_compiles` — a one-liner integration test that `use`s `crate::ibkr::client::IbkrClient` and constructs it via the existing constructor. (Catches accidental `pub` → `pub(crate)` regressions during the move.)
- [x] `tracker_service_split_compiles` — same shape, importing `TrackerService` and calling `add` / `count_active_setups`.
- [x] `projection_service_split_compiles` — imports `ProjectionService::generate_projections` and runs it over the existing mock fixture.
- [x] `financial_data_service_split_compiles` — imports `FinancialDataService::fetch_news_sentiment` to confirm the `news` submodule wiring survives the split.

## Implementation tasks

### 1. Replace the `holidays.rs` panic (10 min)

- [x] `src-tauri/src/utils/market_calendar/holidays.rs:13-18` — swap `panic!("invalid hardcoded holiday date")` for an `expect()`-style const fallback that names the offending year/month/day. The dates are compile-time constants so the branch is unreachable today; the goal is signal, not safety. Suggested shape:
  ```rust
  const fn d(y: i32, m: u32, day: u32) -> NaiveDate {
      match NaiveDate::from_ymd_opt(y, m, day) {
          Some(date) => date,
          // Unreachable — every entry below is hand-checked against the NYSE calendar.
          // If this fires, a maintainer typo'd a date in the HOLIDAYS table.
          None => panic!("invalid hardcoded holiday date — check the most recent entry added to HOLIDAYS"),
      }
  }
  ```
  (The `panic!` stays — `Result` doesn't compose in a `const fn` context without nightly. The improvement is the message, not the mechanism.)

### 2. `unwrap()` / `expect()` sweep (30–60 min)

Audit surfaced ~45 non-test instances. Most are on hardcoded constants (`FixedOffset::west_opt(5 * 3600).unwrap()`, `and_hms_opt(16, 5, 0).unwrap()`) — those are fine. Target only the ones whose input is **not** a hardcoded literal:

- [x] `services/projection_service.rs` — `.expect("Should generate projections")` × 2. Replace with named `expect` strings that explain the invariant ("daily bars must be sorted by date before scenario generation").
- [x] `services/historical_data_service/mod.rs` — `and_hms_opt().unwrap()` × 2 around session boundaries. Document the invariant in an `expect()` message.
- [x] Run `rg -n '\.unwrap\(\)|\.expect\(' src-tauri/src --type rust | rg -v '#\[cfg\(test\)\]|/tests\.rs|/tests/'` and walk the output. Any call whose receiver is not a `const`/literal gets an `expect()` with a one-line invariant.
- [x] Do **not** introduce new `Result` returns or error variants — this is a documentation pass, not an error-handling refactor.

### 3. Split `ibkr/client.rs` by domain (754 → ~4 files, ~2 hr)

The file mixes IBKR API adapters across all four domains. Split into:

- [x] `src-tauri/src/ibkr/client/mod.rs` — `IbkrClient` struct, constructor, connection lifecycle, shared helpers, `pub use` re-exports for everything below.
- [x] `src-tauri/src/ibkr/client/market_data.rs` — `subscribe_market_data`, real-time quote helpers.
- [x] `src-tauri/src/ibkr/client/orders.rs` — `place_order`, order-status streaming hooks.
- [x] `src-tauri/src/ibkr/client/historical.rs` — `historical_data` (the `HistoricalDataFetcher` blanket impl lives here).
- [x] `src-tauri/src/ibkr/client/streams.rs` — daily P&L stream, scanner stream, `StreamHandle` helpers if not already extracted.
- [x] All call sites (services, commands, `lib.rs::run`) keep using `crate::ibkr::client::IbkrClient` — the `pub use` re-exports preserve the public path.
- [x] Update `CLAUDE.md`'s `ibkr/client.rs:` bullet to reflect the new tree.

### 4. Split `services/tracker_service/mod.rs` (662 → 2 files, ~1 hr)

The file mixes ticker CRUD (Phase 04) with setup-row CRUD + state-machine support fns (Phase 10/12). Split:

- [x] `services/tracker_service/mod.rs` — `TrackerService` struct, `new`, `add`/`remove`/`list`/`get`, `set_tags`, `set_status`, `touch_last_checked`. `pub use setups::*;` to keep call sites unchanged.
- [x] `services/tracker_service/setups.rs` — `insert_setup`, `list_setups`, `get_setup`, `recent_duplicate`, `count_active_setups`, `update_setup_status`. `impl TrackerService { ... }` block in this file.
- [x] `tests.rs` stays at the directory root; no test moves required (private test helpers can `use super::*`).

### 5. Split `services/projection_service.rs` (641 → 2 files, ~1 hr)

- [x] `services/projection_service/mod.rs` — `ProjectionService` struct + entry points (`generate_projections`, `generate_projection_results`). Keeps the public surface stable.
- [x] `services/projection_service/scenarios.rs` — scenario math (revenue / margin / multiple expansion helpers).
- [x] Call sites (Tauri commands, tests) untouched.

### 6. Split `services/financial_data_service.rs` (553 → 3 files, ~45 min)

The `news/` submodule is already extracted. The remaining 553 lines mix the three Alpha Vantage endpoint parsers:

- [x] Convert `financial_data_service.rs` to a directory module `financial_data_service/mod.rs`.
- [x] `financial_data_service/overview.rs` — OVERVIEW endpoint parsing.
- [x] `financial_data_service/income.rs` — INCOME_STATEMENT parsing.
- [x] `financial_data_service/earnings.rs` — EARNINGS parsing + estimate merging.
- [x] `mod.rs` keeps `FinancialDataService::new`, `with_db`, `fetch_fundamental_data` (orchestrator) and `pub use` for the per-endpoint helpers used by tests. `news` and `news_tests` stay where they are.

### 7. Factor `TagEditor` out of `Watchlist.tsx` (326 → ~200 + ~120, 30 min)

- [x] New file `src/features/tracker/components/TagEditor.tsx` — owns the inline tag editing input + chip rendering. Props: `{ tags: string[]; onSave: (next: string[]) => Promise<void>; onCancel: () => void; }`.
- [x] `Watchlist.tsx` imports `TagEditor` and renders it conditionally where the inline editor used to live. Row-level event wiring (which row is being edited) stays in `Watchlist`.
- [x] No CSS / className tweaks in this phase — same Tailwind classes, just relocated.

### 8. Document the file-size rule (already done in `CONTRIBUTING.md`)

- [x] Verify `CONTRIBUTING.md` exists with the soft-cap rule (created alongside this phase).
- [x] Cross-link from `CLAUDE.md` if not already present.

## Verification

After each numbered task above, run the full battery:

- [x] `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` — must show `215 passed; 0 failed; 1 ignored` (or higher if the four split-compiles tests landed).
- [x] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings` — clean.
- [x] `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` — clean.
- [x] `pnpm typecheck && pnpm lint && pnpm format:check` — clean.
- [x] `pnpm tauri dev` boots, the Tracker tab still loads the watchlist, the Scanner tab's "Add to tracker" still wires through, a manual `tracker_run_now` still emits `setup-detected` toasts (smoke test the moved `IbkrClient` paths through real flows).
- [x] Re-run the line-count check: `find src-tauri/src src -name '*.rs' -o -name '*.tsx' -o -name '*.ts' | xargs wc -l | sort -rn | head -20` — no Rust file > 500 lines, no TSX file > 300 lines (or any over-cap files have a justifying comment per `CONTRIBUTING.md`).

## Files

**Created:**
- `src-tauri/src/ibkr/client/mod.rs`, `market_data.rs`, `orders.rs`, `historical.rs`, `streams.rs`
- `src-tauri/src/services/tracker_service/setups.rs`
- `src-tauri/src/services/projection_service/mod.rs` (replacing the file), `scenarios.rs`
- `src-tauri/src/services/financial_data_service/mod.rs` (replacing the file), `overview.rs`, `income.rs`, `earnings.rs`
- `src/features/tracker/components/TagEditor.tsx`

**Deleted (replaced by directory modules):**
- `src-tauri/src/ibkr/client.rs`
- `src-tauri/src/services/projection_service.rs`
- `src-tauri/src/services/financial_data_service.rs`

**Modified:**
- `src-tauri/src/utils/market_calendar/holidays.rs` (panic message)
- `src-tauri/src/services/tracker_service/mod.rs` (extract setups)
- `src-tauri/src/services/historical_data_service/mod.rs` (`expect` messages)
- `src-tauri/src/services/projection_service/mod.rs` (`expect` messages)
- `src/features/tracker/components/Watchlist.tsx` (import + use `TagEditor`)
- `CLAUDE.md` (refresh `ibkr/client.rs`, `tracker_service`, `projection_service`, `financial_data_service` paragraphs to reflect the new tree)
- `impl.md` (this phase's checkbox)

## Scratchpad

None. This phase is purely structural — no calibration thresholds, prompts, or backtest results to capture.

## Done when

- All 215+ Rust tests pass; clippy/fmt/typecheck/lint/prettier all green.
- No Rust file in `src-tauri/src/` exceeds 500 lines without a `// allow-large-file: <reason>` justifier comment at the top.
- No TS/TSX file in `src/` exceeds 300 lines without the same justifier.
- `holidays.rs` panic message names the failure mode.
- `CLAUDE.md` paths line up with the new directory layout.
- `pnpm tauri dev` smoke test passes — IBKR connect, scanner stream, tracker run, watchlist event flow all still work end-to-end.
