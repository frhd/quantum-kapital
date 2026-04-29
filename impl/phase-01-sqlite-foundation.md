# Phase 01 — SQLite foundation

## Goal

Land a fully-specified SQLite layer (connection pool, migrations runner, complete schema) so every subsequent phase can `INSERT` / `SELECT` without schema work.

## Depends on

Nothing. This is the first phase.

## Out of scope

- Any business logic that writes to the new tables (Phases 02+).
- Frontend changes.
- Migration of existing JSON file caches (`services/cache_service.rs` stays untouched).

## Test plan (write tests FIRST)

All tests in `src-tauri/src/storage/tests.rs`, run with `cargo test --manifest-path src-tauri/Cargo.toml storage::`.

- [x] `db_open_creates_file_and_runs_migrations` — opening `Db::open(tempfile_path)` produces a file, all tables exist, `PRAGMA foreign_keys` is on, journal mode is WAL.
- [x] `db_open_is_idempotent` — calling `Db::open` twice on the same path doesn't error; `IF NOT EXISTS` migrations are safe to re-run.
- [x] `db_with_conn_round_trips_value` — `Db::with_conn(|c| { c.execute("INSERT ...")?; ... })` returns expected value through `spawn_blocking`.
- [x] `db_with_conn_propagates_rusqlite_error` — a `SELECT` against a non-existent column surfaces as `Err(StorageError::Sqlite(_))`.
- [x] `migration_creates_all_baseline_tables` — `sqlite_master` query returns: `tracked_tickers`, `setups`, `alerts`, `bars_cache`, `news_cache`, `llm_calls`.
- [x] `migration_creates_required_indexes` — `idx_setups_symbol`, `idx_setups_status_detected` are present.
- [x] `tracked_tickers_pk_rejects_duplicate_symbol` — two inserts of `'AAPL'` returns a constraint error.
- [x] `setups_fk_cascades_on_ticker_delete` — deleting a `tracked_tickers` row also deletes its `setups` and downstream `alerts` (via fk chain).
- [x] `bars_cache_pk_dedup` — re-inserting `(AAPL, 1day, 1714435200)` upserts cleanly via `INSERT OR REPLACE`.

## Implementation tasks

- [x] Add deps to `src-tauri/Cargo.toml`: `rusqlite = { version = "0.34", features = ["bundled", "chrono"] }`, `r2d2 = "0.8"`, `r2d2_sqlite = "0.27"`.
- [x] Create `src-tauri/src/storage/mod.rs` exposing `Db`, `StorageError`, `Result`.
- [x] Create `src-tauri/src/storage/error.rs` — `StorageError` enum via `thiserror` (`Sqlite`, `Pool`, `Migration`, `Serde`).
- [x] Create `src-tauri/src/storage/migrations.rs` — `run_migrations(conn: &mut rusqlite::Connection)` that runs the embedded SQL.
- [x] Create `src-tauri/src/storage/schema.sql` — full baseline schema (see design doc; copy verbatim and bake all six tables + two indexes upfront so later phases never `CREATE TABLE`).
- [x] In `Db::open(path)`: build `r2d2::Pool<SqliteConnectionManager>`, run migrations on a checkout, set `PRAGMA journal_mode = WAL`, `PRAGMA foreign_keys = ON`, `PRAGMA synchronous = NORMAL`.
- [x] Implement `Db::with_conn<F, T>(f) -> Result<T>` — `tokio::task::spawn_blocking` around a pool checkout + closure invocation.
- [x] Add `mod storage;` to `src-tauri/src/lib.rs`.
- [x] In `lib.rs::run` setup closure (after `AppConfig::load_sync`): resolve DB path via `app.path().app_local_data_dir()?.join("tracker.sqlite")`, create the directory if missing, then `Db::open(path)`. Hand the `Arc<Db>` into a placeholder for now (Phase 04 wires it through `IbkrState`).
- [x] Tests in `src-tauri/src/storage/tests.rs` using `tempfile::NamedTempFile`.

## Verification

- [x] `cargo test --manifest-path src-tauri/Cargo.toml storage::` — all green.
- [x] `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` — clean.
- [x] `cargo fmt --manifest-path src-tauri/Cargo.toml --check` — clean.
- [x] `pnpm tauri dev` boots without errors; on first run the SQLite file appears at `~/.local/share/quantum-kapital/tracker.sqlite` (Linux) and `sqlite3 <path> ".tables"` lists all six tables.

## Files

**Created:**
- `src-tauri/src/storage/mod.rs`
- `src-tauri/src/storage/error.rs`
- `src-tauri/src/storage/migrations.rs`
- `src-tauri/src/storage/schema.sql`
- `src-tauri/src/storage/tests.rs`

**Modified:**
- `src-tauri/Cargo.toml`
- `src-tauri/src/lib.rs`

## Scratchpad

- **Write to** `impl/scratch/schema-decisions.md`: log WAL mode + sync mode choice, any deviation from the baseline schema in `impl.md`, and any deferred indexes (e.g., `news_cache.fetched_at`).

## Done when

`Db::open` runs idempotently, all six baseline tables + two indexes exist, all unit tests pass, the app still boots cleanly.
