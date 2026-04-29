# Phase 21 — Alert feed UI

## Goal

A persistent, scrollable feed of recent setup events (`detected`, `invalidated`, `target_hit`, `thesis_changed`) with mark-as-seen behavior. Lives next to the Watchlist in the Tracker tab so users can review what fired even if they missed the toast.

## Depends on

- [x] Phase 15 — events emitted.
- [x] Phase 01 — `alerts` table (already baked).

## Out of scope

- Email / push / external notifications. Desktop-only, in-app feed.
- Filtering / search beyond the basic kind filter.

## Test plan (write tests FIRST)

Backend:

- [x] `alert_inserted_on_setup_detected` — when `SetupDetected` event fires, an `alerts(kind='detected')` row is inserted with the event payload as JSON.
- [x] `alert_inserted_on_setup_invalidated` — same for `invalidated`.
- [x] `alerts_dedup_per_event` — emitting the same setup_id+kind twice within 1s does not create a second alert row (use `(setup_id, kind)` uniqueness on most-recent-N).
- [x] `tracker_list_alerts_pagination` — `tracker_list_alerts(limit, offset, since, kind, only_unseen)` honors all four filters.
- [x] `tracker_mark_alerts_seen` — marks the listed `id`s as seen=1; unaffected ids remain unseen.

Frontend (manual E2E):

- [ ] Trigger a detection → AlertFeed shows the new alert with an unseen indicator (dot).
- [ ] Click "mark all seen" → indicator clears; persists across reload.
- [ ] Filter dropdown: only `Invalidated` → AlertFeed only shows invalidations.
- [ ] Click an alert → opens TickerAnalysis with the symbol pre-loaded.
- [ ] Soak: 100 events fire in succession → AlertFeed remains responsive (virtualize if needed; not strictly required at <500 rows).

## Implementation tasks

Backend:

- [x] Decision: **inline inserts** at the emit sites in `tracker_runner.rs` and `tracker_state_machine.rs`. Use a small helper `record_alert(db, setup_id, kind, payload)` in `services/alerts/`. Application-level dedup (1s window keyed on `(setup_id, kind)`) replaces the unique-constraint approach so legitimate later kinds don't conflict with stale rows.
- [x] Add Tauri commands:
  - `tracker_list_alerts(limit, offset, since: Option<DateTime<Utc>>, kind: Option<AlertKind>, only_unseen: bool) -> Vec<Alert>`
  - `tracker_mark_alerts_seen(ids: Vec<i64>) -> usize`
- [x] Add `Alert` + `AlertKind` types in `ibkr/types/tracker.rs` (Detected / Invalidated / TargetHit / ThesisChanged; snake_case serde).

Frontend:

- [x] Create `src/features/tracker/components/AlertFeed.tsx` — header (title + kind filter + unseen-only toggle + mark-all-seen button) and a scroll list of `<AlertRow>` components ordered newest-first; hydrates via `tracker_list_alerts` and refetches whenever a tracker event lands.
- [x] `useAlerts` hook — manages list, mark-as-seen calls, paginated `loadMore`.
- [x] AlertRow click → marks the row seen and calls `onSelectSymbol(payload.symbol)`.

## Verification

- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib services::alerts services::tracker_runner services::tracker_state_machine` — alert wiring tests all green (record/list/mark-seen + 2 emit-site integration tests).
- [ ] Manual E2E checklist above (deferred — unattended automation phase).
- [x] `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `pnpm build`, `pnpm typecheck`, `pnpm lint` (0 errors).

## Files

**Created:**
- `src-tauri/src/services/alerts/{mod,tests}.rs` (record/list/mark-seen + 6 unit tests)
- `src/features/tracker/components/AlertFeed.tsx`
- `src/features/tracker/components/AlertRow.tsx`
- `src/features/tracker/hooks/useAlerts.ts`

**Modified:**
- `src-tauri/src/services/mod.rs` (register `alerts` module)
- `src-tauri/src/services/tracker_runner/mod.rs` (record_alert on detected)
- `src-tauri/src/services/tracker_state_machine/mod.rs` (record_alert on invalidated/target_hit)
- `src-tauri/src/services/tracker_service/mod.rs` (test-only `db_for_testing` accessor)
- `src-tauri/src/ibkr/types/tracker.rs` (`Alert`, `AlertKind`)
- `src-tauri/src/ibkr/commands/tracker.rs` (list/mark-seen commands)
- `src-tauri/src/lib.rs` (register commands)
- `src-tauri/src/storage/schema.sql` (alert indexes)
- `src/features/tracker/components/TrackerTab.tsx` (mount AlertFeed)
- `src/features/tracker/types.ts` (`Alert`, `AlertKind`)
- `src/shared/api/ibkr.ts` (alert API methods)

## Scratchpad

None.

## Done when

Alerts persist across restarts, dedup correctly, frontend feed updates in real time, mark-as-seen persists, filter works.
