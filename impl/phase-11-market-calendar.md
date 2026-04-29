# Phase 11 — Market calendar utility

## Goal

A small `market_calendar` helper that answers: is the US equity market open right now? when does it next open / close? is `date` a holiday? Used by Phases 13 and 14 to gate scheduler ticks.

## Depends on

Nothing structurally — this could ship at any time. Sequenced here because Phases 13/14 need it.

## Out of scope

- Non-US exchanges.
- Half-day handling beyond a hardcoded list.
- Premarket / after-hours subscriptions to bars (we treat 09:30–16:00 ET as the only relevant window).

## Test plan (write tests FIRST)

`src-tauri/src/utils/market_calendar/tests.rs`.

- [ ] `is_rth_true_at_1000_et_on_weekday` — Tue 10:00 ET → true.
- [ ] `is_rth_false_at_0900_et` — pre-open → false.
- [ ] `is_rth_false_at_1601_et` — post-close → false.
- [ ] `is_rth_false_on_saturday` — any time → false.
- [ ] `is_rth_false_on_sunday` — any time → false.
- [ ] `is_rth_false_on_holiday` — e.g. 2026-07-03 (Friday observed for July 4) → false.
- [ ] `next_open_at_returns_today_open_when_called_pre_open` — Mon 08:00 ET → Mon 09:30 ET.
- [ ] `next_open_at_returns_next_business_day_when_called_post_close` — Mon 17:00 ET → Tue 09:30 ET.
- [ ] `next_open_at_skips_weekend` — Fri 17:00 ET → Mon 09:30 ET.
- [ ] `next_open_at_skips_holiday` — day before observed July 4 17:00 ET → following business day 09:30 ET.
- [ ] `next_close_at_within_session` — Tue 14:00 ET → Tue 16:00 ET.
- [ ] `eod_sweep_target_is_1605_et` — `eod_sweep_target(date)` returns 16:05 ET on that date.

## Implementation tasks

- [ ] Create `src-tauri/src/utils/market_calendar/mod.rs`:
  ```rust
  pub fn is_rth_open(now: DateTime<Utc>) -> bool;
  pub fn is_holiday(date: NaiveDate) -> bool;
  pub fn next_open_at(now: DateTime<Utc>) -> DateTime<Utc>;
  pub fn next_close_at(now: DateTime<Utc>) -> DateTime<Utc>;
  pub fn eod_sweep_target(date: NaiveDate) -> DateTime<Utc>; // 16:05 ET
  ```
- [ ] Hardcode US market holidays for the next 3 years in `holidays.rs` as `&[NaiveDate]`. Document in code that this needs annual maintenance.
- [ ] Use `chrono::FixedOffset::west_opt(5 * 3600).unwrap()` for ET (with a TODO note about EST/EDT — hardcode EST for MVP, since most of the trading day matters and DST adds complexity; revisit only if observed bugs).
- [ ] Add `pub mod market_calendar;` to `src-tauri/src/utils/mod.rs`.

## Verification

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml utils::market_calendar` — all green.
- [ ] Eyeball a few edge cases manually (holiday list completeness).
- [ ] `cargo clippy ...`, `cargo fmt --check`.

## Files

**Created:**
- `src-tauri/src/utils/market_calendar/mod.rs`
- `src-tauri/src/utils/market_calendar/holidays.rs`
- `src-tauri/src/utils/market_calendar/tests.rs`

**Modified:**
- `src-tauri/src/utils/mod.rs`

## Scratchpad

None.

## Done when

All twelve tests pass; helper is callable from anywhere via `crate::utils::market_calendar::*`.

## Known follow-ups (intentionally deferred)

- Proper ET = EST/EDT switching. For MVP we hardcode EST and accept that scheduler ticks land an hour off during DST — practically harmless because the EOD sweep is at 16:05 +/- 5 min and the intraday tick is every 5 min anyway. Address only if it causes a bug.
- Half-day market hours (e.g., day after Thanksgiving). Skip for now; system idles slightly later than ideal those days.
