# Phase 04 — Tracker persistence (backend)

## Goal

CRUD over the `tracked_tickers` table via a `TrackerService` and the matching Tauri commands. After this phase the watchlist is real on the Rust side; only the UI is missing.

## Depends on

- [ ] Phase 01 — `Db` and `tracked_tickers` table.

## Out of scope

- Setups, alerts (those tables exist but stay empty until Phase 10 / 15).
- Status state machine logic — Phase 12 owns that. Phase 04 stores `status` as a string but does not enforce transitions.
- Frontend (Phase 05).

## Test plan (write tests FIRST)

`src-tauri/src/services/tracker_service/tests.rs`.

- [ ] `add_inserts_row_and_returns_typed_value` — `add(symbol, source, source_meta, tags, notes)` returns a `TrackedTicker` whose fields match the inserted row.
- [ ] `add_duplicate_symbol_errors` — second `add('AAPL', ...)` returns `TrackerError::AlreadyTracked`.
- [ ] `remove_deletes_row` — `remove('AAPL')` then `list(None)` excludes it.
- [ ] `remove_non_existent_is_idempotent` — `remove('NOSUCH')` returns `Ok(())` not an error.
- [ ] `list_filters_by_status` — three rows in three statuses; `list(Some(InPlay))` returns one.
- [ ] `set_tags_replaces_tag_array` — initial tags `[Breakout]`; `set_tags('AAPL', [EpisodicPivot, ParabolicShort])` overwrites; `list` reflects.
- [ ] `set_status_updates_status_and_in_play_until` — set to `InPlay` with `in_play_until = now+3d`; row reflects.
- [ ] `tags_round_trip_via_json` — custom variant `StrategyTag::Custom("squeeze")` serializes and deserializes.
- [ ] `source_meta_round_trip` — JSON value with nested object survives a round trip.

## Implementation tasks

- [ ] Create `src-tauri/src/ibkr/types/tracker.rs`:
  ```rust
  pub struct TrackedTicker {
      pub symbol: String,
      pub source: TrackerSource,
      pub source_meta: Option<serde_json::Value>,
      pub status: TrackerStatus,
      pub tags: Vec<StrategyTag>,
      pub notes: Option<String>,
      pub added_at: DateTime<Utc>,
      pub last_checked_at: Option<DateTime<Utc>>,
      pub in_play_until: Option<DateTime<Utc>>,
  }
  pub enum TrackerSource { Scanner, Manual, News }
  pub enum TrackerStatus { Watching, InPlay, SetupActive, CoolDown }
  pub enum StrategyTag { Breakout, EpisodicPivot, ParabolicShort, Custom(String) }
  ```
  All `serde::{Serialize, Deserialize}`. Enums serialize as snake_case strings; `Custom` uses the inner string.
- [ ] Re-export `TrackedTicker`, `TrackerSource`, `TrackerStatus`, `StrategyTag` from `ibkr/types/mod.rs`.
- [ ] Create `src-tauri/src/services/tracker_service.rs`:
  - `TrackerService { db: Arc<Db> }`
  - `pub async fn add(...) -> Result<TrackedTicker>`
  - `pub async fn remove(symbol: &str) -> Result<()>`
  - `pub async fn list(status_filter: Option<TrackerStatus>) -> Result<Vec<TrackedTicker>>`
  - `pub async fn get(symbol: &str) -> Result<Option<TrackedTicker>>`
  - `pub async fn set_tags(symbol: &str, tags: Vec<StrategyTag>) -> Result<TrackedTicker>`
  - `pub async fn set_status(symbol: &str, status: TrackerStatus, in_play_until: Option<DateTime<Utc>>) -> Result<TrackedTicker>`
  - `pub async fn touch_last_checked(symbol: &str) -> Result<()>`
- [ ] Add `TrackerError` (`AlreadyTracked`, `NotFound`, `Storage(StorageError)`).
- [ ] Wire into `IbkrState`:
  - Add `pub db: Arc<Db>` and `pub tracker: Arc<TrackerService>` fields.
  - Modify `IbkrState::new` signature to accept `db: Arc<Db>`; build `tracker` inside.
  - Update `lib.rs::run` to construct `Db` and pass it.
  - Existing call sites (only `lib.rs`) need a one-line edit.
- [ ] Add Tauri commands in `commands/tracker.rs` (extend the file from Phase 02):
  - `tracker_add`, `tracker_remove`, `tracker_list`, `tracker_get`, `tracker_set_tags`, `tracker_set_status`.
- [ ] Register all six commands in `lib.rs::run` `generate_handler!`.

## Verification

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml services::tracker_service` — green.
- [ ] Manual via Tauri devtools console:
  ```
  invoke('tracker_add', { symbol: 'AAPL', source: 'manual', sourceMeta: null, tags: ['breakout'], notes: 'test' })
  invoke('tracker_list', { status: null })
  invoke('tracker_set_tags', { symbol: 'AAPL', tags: ['episodic_pivot'] })
  invoke('tracker_remove', { symbol: 'AAPL' })
  ```
- [ ] `sqlite3 ~/.local/share/quantum-kapital/tracker.sqlite "SELECT * FROM tracked_tickers"` shows expected state.
- [ ] `cargo clippy ...`, `cargo fmt --check`.

## Files

**Created:**
- `src-tauri/src/ibkr/types/tracker.rs`
- `src-tauri/src/services/tracker_service.rs` (+ tests submodule)

**Modified:**
- `src-tauri/src/ibkr/state.rs` (db + tracker fields)
- `src-tauri/src/ibkr/types/mod.rs`
- `src-tauri/src/services/mod.rs`
- `src-tauri/src/ibkr/commands/tracker.rs`
- `src-tauri/src/ibkr/commands/mod.rs`
- `src-tauri/src/lib.rs` (Db construction + IbkrState wiring + command registration)

## Scratchpad

- **Write** to `impl/scratch/schema-decisions.md` only if you deviate from the design (e.g., split tags into a separate table — currently kept as JSON array in column).

## Done when

All six commands work end-to-end against a real SQLite file, tags + status + source_meta round-trip cleanly through JSON, no schema migrations needed (`CREATE TABLE IF NOT EXISTS` from Phase 01 is sufficient).
