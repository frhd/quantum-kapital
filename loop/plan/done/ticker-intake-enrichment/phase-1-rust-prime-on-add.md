# Phase 1 — Rust prime on add: fundamentals + projection + news

> Part of [Ticker Intake Enrichment](master.md). See index for invariants.

**Status:** done (commit 50cc2d1, 2026-05-03)

**Depends on:** none (foundation phase)

**Goal:** Make the projection and news panels populate within seconds of
adding a ticker by chaining the existing
fundamentals → projection → news fetch services as a single orchestrated
post-add task. No new LLM call sites; no new MCP tools. The work is
pure Rust composition of services that already exist, plus a small
column migration for the idempotency watermark.

## Files

- New: `src-tauri/src/services/ticker_primer/mod.rs` —
  `TickerPrimerService` orchestrating fundamentals → projection (cache)
  → news. Public surface: `prime(&self, symbol: &str) -> PrimeOutcome`,
  idempotent on `last_primed_at < 24h`. Holds `Arc` clones of every
  dependency.
- New: `src-tauri/src/services/ticker_primer/tests.rs` — unit tests using
  `MockIbkrClient` + in-memory DB. Cover: fresh prime, idempotent skip,
  partial failure (no fundamentals), re-prime after archive.
- New: schema change adding `last_primed_at INTEGER NULL` to
  `tracked_tickers`. Either an `ALTER TABLE` in
  `src-tauri/src/storage/schema.sql` or a refinery migration under
  `src-tauri/migrations/` — match the existing convention for that
  table.
- New: `AppEvent::TickerPrimingDone { symbol, outcome }` variant in
  `src-tauri/src/events/`. Emitted when the spawned task completes
  (success OR partial). Lets the UI refresh panels without polling.
- Touches: `src-tauri/src/services/tracker_service/mod.rs` — add a
  `mark_primed(symbol)` method (writes `last_primed_at = now`); leave
  `add()` writing the row only. `archive()` already exists; extend it
  to clear `last_primed_at` so re-add gets a fresh prime.
- Touches: `src-tauri/src/ibkr/types/tracker.rs` — add
  `last_primed_at: Option<DateTime<Utc>>` to `TrackedTicker`.
- Touches: `src-tauri/src/mcp/tools/add_ticker.rs` — after
  `tracker.add(...)` succeeds, `tokio::spawn` a `primer.prime(symbol)`
  task. Do **not** block the response; do **not** add a second audit
  row (priming is internal orchestration).
- Touches: `src-tauri/src/ibkr/commands/tracker.rs` — same spawn after
  the UI's add command. Find the existing `#[tauri::command]` add
  handler and mirror the MCP tool's spawn pattern.
- Touches: `src-tauri/src/lib.rs::run` — construct the primer once with
  `Arc` clones of `FundamentalsProvider`, `ProjectionService`,
  `NewsProvider`, `CacheService`, `TrackerService`, and the
  `EventEmitter`. `app.manage(Arc::new(primer))`. Pass the primer
  through to `McpHandler` and the tracker command surface.
- Touches: `src-tauri/src/services/cache_service.rs` — confirm
  `set_projection(symbol, ProjectionResults)` and `get_projection(symbol)`
  exist; add if not. The 7-day TTL stays.

## APIs / commands exposed

No new MCP tools. No new Tauri commands. `TickerPrimerService::prime`
is internal — the only callers are the two `add` paths.

| Internal call | Returns |
|---|---|
| `TickerPrimerService::prime(symbol)` | `PrimeOutcome { fundamentals, projection, news, primed_at }` |

`PrimeOutcome` records per-step status:

```rust
enum StepStatus { Ok, NoData, Err(String) }
struct PrimeOutcome {
    fundamentals: StepStatus,
    projection: StepStatus,    // NoData if fundamentals lacks history
    news: StepStatus,
    primed_at: DateTime<Utc>,
}
```

The outcome is the payload of `AppEvent::TickerPrimingDone`.

## Reuse (no new business logic this phase)

- `CompositeFundamentalsProvider` (manual store + AV fallback) —
  already cache-aware via `cache_service.rs`. Call its existing
  `get(symbol)` method.
- `ProjectionService::generate_projection_results` — pure function over
  fundamentals; no IO, no async. Just call it with the existing
  `ProjectionAssumptions` defaults.
- `IbkrNewsProvider::fetch` — already calls `NewsInterpreter` per
  Phase 8 wiring; populates `news_cache` with verdicts. **Reusing this
  means the Rust LLM budget naturally absorbs prime's LLM cost without
  a new code path.**
- `cache_service.rs` JSON store — already used for fundamentals +
  projection caching with 7d TTL.
- `tokio::spawn` + the existing IBKR rate limiter — no new throttle
  infra.
- `EventEmitter` — same pattern as every other AppEvent.

## Decisions to make in this phase

- **Where does `last_primed_at` live?** `tracked_tickers` column
  (chosen). Alternative: a sibling `ticker_primer_runs` table.
  Column is simpler and the watermark is 1:1 with the watchlist row.
- **What counts as "primed"?** Stamp `last_primed_at` if at minimum the
  fundamentals fetch completed (success **or** explicit "no
  fundamentals available"). Projection failure is non-fatal (no
  baseline data); news failure is non-fatal (the EOD `TrackerRunner`
  retries that night). This way a user who re-adds gets a re-prime
  only if the previous prime made no real attempt — i.e. crashed
  before the fundamentals call.
- **Re-prime on re-add of an archived ticker?** Yes. `archive` clears
  `last_primed_at`. Test covers this.
- **Spawn or block?** Spawn. The MCP call returns instantly,
  `TickerStatusChanged` event already fires, primer emits the follow-up
  `TickerPrimingDone` event when the task completes.
- **Concurrency cap?** None at the primer level — the IBKR rate
  limiter is the global throttle. If batch-add storms become a
  problem, file in `QUESTIONS.md` after the phase.
- **Should the primer respect the existing `cache_service.rs` 7d TTL,
  or force-fetch?** Respect it. Force-fetch would defeat the cache;
  staleness is bounded at 7d which is already the project default for
  fundamentals. If staleness bites, address in a separate plan.
- **MCP audit row for prime?** No. The `add_ticker` audit row already
  records the user-visible action; priming is internal.

## Exit criteria

- `TickerPrimerService` exists with the public surface above.
- Schema applied: `tracked_tickers.last_primed_at` is queryable;
  backfill is NULL on existing rows.
- `AppEvent::TickerPrimingDone` is defined and emitted by the primer.
- Unit test (in `services/ticker_primer/tests.rs`):
  - Priming a fresh symbol with mocked providers populates fundamentals
    cache, projection cache (`cache_service::get_projection`), and
    inserts rows into `news_cache`. Asserts `last_primed_at` is set.
  - Priming a symbol whose `last_primed_at` is < 24h is a no-op:
    `MockIbkrClient` records zero calls.
  - Priming after archive (which cleared `last_primed_at`) re-runs.
  - Fundamentals returning "no data" yields
    `PrimeOutcome.projection = NoData`, news still attempts, and
    `last_primed_at` is still set.
- Integration test (extend `tests/mcp_tool_call.rs` or a new file
  alongside): calling `add_ticker` then awaiting
  `TickerPrimingDone` results in projection + news being readable via
  existing read commands within the test's timeout.
- Cross-phase tracer #1 (master): real `add_ticker` MCP call →
  within 30s the three tables are populated. Documented manually for
  Phase 1 exit; automated as part of the integration test.
- `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`
  clean. No new file exceeds the soft cap (300 LOC) without an
  `// allow-large-file:` justifier.
- Manual smoke: `pnpm tauri dev`, add TSLA via the UI, observe
  projection panel populates within ~10s and news panel within ~30s.
  Tail `/tmp/qk-tauri.log` for primer log lines + `TickerPrimingDone`
  emission.

## Gotchas

- **Existing `cache_service.rs` 7-day TTL.** If projection cache hits
  on stale fundamentals, the primer should not silently surface stale
  numbers. Trust the cache TTL — it's the same trust the rest of the
  app already grants — but log the cache hit so it's visible in
  `/tmp/qk-tauri.log` while the feature is new.
- **`IbkrNewsProvider::fetch` calls `LlmService`** transitively via
  `NewsInterpreter`. Priming will incur LLM spend. After the first
  manual smoke, eyeball the `llm_calls` ledger to confirm budget
  enforcement works as expected and there is no double-call.
- **MCP audit double-counting.** `add_ticker` already records an audit
  row. Don't add a second audit row for the prime.
- **Tokio spawn lifetime.** The primer task must hold `Arc` clones of
  every dependency, not borrow. Standard pattern, easy to get wrong.
  Verify the spawned future is `'static + Send`; clippy will catch
  most cases.
- **Test timing.** Spawned primer is async; integration tests must
  await `TickerPrimingDone` rather than `sleep` polling. Use the
  existing event channel infra.
- **`PrimeOutcome` granularity matters for the UI.** Make per-step
  status explicit; the workspace event listener uses it to decide
  which panel to refresh.
- **Schema migration on existing DBs.** Users running the dev app
  already have `tracked_tickers` rows. The `ALTER TABLE ... ADD
  COLUMN` is non-destructive, but verify the migration runs on a
  pre-existing database before shipping (delete a fresh DB, run, copy
  in an old DB, run again).
- **`archive` clearing `last_primed_at`.** Find every code path that
  archives — there may be more than one. Grep for
  `TrackerStatus::Archived` and `archive(` in `services/tracker_service/`.
