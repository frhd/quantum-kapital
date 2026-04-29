# Phase 21 — Alert feed UI

## Goal

A persistent, scrollable feed of recent setup events (`detected`, `invalidated`, `target_hit`, `thesis_changed`) with mark-as-seen behavior. Lives next to the Watchlist in the Tracker tab so users can review what fired even if they missed the toast.

## Depends on

- [ ] Phase 15 — events emitted.
- [ ] Phase 01 — `alerts` table (already baked).

## Out of scope

- Email / push / external notifications. Desktop-only, in-app feed.
- Filtering / search beyond the basic kind filter.

## Test plan (write tests FIRST)

Backend:

- [ ] `alert_inserted_on_setup_detected` — when `SetupDetected` event fires, an `alerts(kind='detected')` row is inserted with the event payload as JSON.
- [ ] `alert_inserted_on_setup_invalidated` — same for `invalidated`.
- [ ] `alerts_dedup_per_event` — emitting the same setup_id+kind twice within 1s does not create a second alert row (use `(setup_id, kind)` uniqueness on most-recent-N).
- [ ] `tracker_list_alerts_pagination` — `tracker_list_alerts(limit, offset, since, kind, only_unseen)` honors all four filters.
- [ ] `tracker_mark_alerts_seen` — marks the listed `id`s as seen=1; unaffected ids remain unseen.

Frontend (manual E2E):

- [ ] Trigger a detection → AlertFeed shows the new alert with an unseen indicator (dot).
- [ ] Click "mark all seen" → indicator clears; persists across reload.
- [ ] Filter dropdown: only `Invalidated` → AlertFeed only shows invalidations.
- [ ] Click an alert → opens TickerAnalysis with the symbol pre-loaded.
- [ ] Soak: 100 events fire in succession → AlertFeed remains responsive (virtualize if needed; not strictly required at <500 rows).

## Implementation tasks

Backend:

- [ ] Create `src-tauri/src/services/alert_recorder.rs` — subscribes (in-process) to `EventEmitter` outputs and inserts `alerts` rows. Or, add the insert directly inline at each emit site (simpler, fewer moving parts) — **recommend inline** for fewer indirections; remove this file in favor of a helper.
- [ ] Decision: **inline inserts** at the emit sites in `tracker_runner.rs` and `tracker_state_machine.rs`. Use a small helper `record_alert(db, setup_id, kind, payload)`.
- [ ] Add Tauri commands:
  - `tracker_list_alerts(limit, offset, since: Option<DateTime<Utc>>, kind: Option<AlertKind>, only_unseen: bool) -> Vec<Alert>`
  - `tracker_mark_alerts_seen(ids: Vec<i64>) -> ()`
- [ ] Add `Alert` type in `ibkr/types/tracker.rs`:
  ```rust
  pub struct Alert {
      pub id: i64, pub setup_id: i64, pub kind: AlertKind,
      pub fired_at: DateTime<Utc>, pub payload: serde_json::Value, pub seen: bool,
  }
  pub enum AlertKind { Detected, Invalidated, TargetHit, ThesisChanged }
  ```

Frontend:

- [ ] Create `src/features/tracker/components/AlertFeed.tsx`:
  - Right-side panel (or below Watchlist on narrow viewports).
  - Header: title + filter dropdown + "mark all seen" button.
  - Body: list of `<AlertRow>` components ordered newest-first.
  - Hooks into `useTrackerEvents` to add alerts at runtime; backfills via `tracker_list_alerts` on mount.
- [ ] `useAlerts` hook — manages list, mark-as-seen calls, pagination on scroll.
- [ ] AlertRow click → calls `onSelectSymbol(payload.symbol)` (existing pattern).

## Verification

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml ibkr::commands::tracker_tests` — extended with alert command tests, all green.
- [ ] Manual E2E checklist above.
- [ ] `cargo clippy ...`, `cargo fmt --check`, `pnpm build`.

## Files

**Created:**
- `src/features/tracker/components/AlertFeed.tsx`
- `src/features/tracker/components/AlertRow.tsx`
- `src/features/tracker/hooks/useAlerts.ts`

**Modified:**
- `src-tauri/src/services/tracker_runner.rs` (record_alert)
- `src-tauri/src/services/tracker_state_machine.rs` (record_alert)
- `src-tauri/src/ibkr/types/tracker.rs` (`Alert`, `AlertKind`)
- `src-tauri/src/ibkr/commands/tracker.rs` (list/mark-seen commands)
- `src-tauri/src/lib.rs` (register commands)
- `src/features/tracker/components/TrackerTab.tsx` (mount AlertFeed)
- `src/shared/api/ibkr.ts` (alert API methods)

## Scratchpad

None.

## Done when

Alerts persist across restarts, dedup correctly, frontend feed updates in real time, mark-as-seen persists, filter works.
