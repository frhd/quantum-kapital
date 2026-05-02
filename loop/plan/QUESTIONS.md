# Cross-phase questions / open issues

Logged during phase execution per `loop/plan/master.md`. Each entry: who
found it, when, and the safer interpretation taken so the loop could
continue. Resolve and prune as phases progress.

---

## P1: `decay_watcher::tests::respects_budget_kill_switch` — pre-existing failure on `main`

- **Found:** Phase 1, 2026-05-02
- **Symptom:** `cargo test --lib services::decay_watcher::tests::respects_budget_kill_switch`
  panics with `MockHttp queue exhausted` at `src/services/decay_watcher/tests.rs:59`.
- **Reproducible on `main`** (verified with `git stash` before Phase 1
  changes). Not introduced by the AV burn-fix work; the failing test
  has nothing to do with the AV path.
- **Safer interpretation:** treat as an unrelated test-suite regression
  on `main`; do not block Phase 1 on it. The Phase 1 exit criteria say
  `cargo test` green; the underlying reason here is in the LLM decay
  watcher mock setup, which is orthogonal.
- **TODO:** investigate independently (likely a stale fixture / missing
  enqueue after a recent prompt change). Phase 1 commits do not touch
  `decay_watcher/`.

---

## P2: Phase 2 spike capture is human-in-the-loop — TWS + Reuters subscription required

- **Found:** Phase 2, 2026-05-02
- **What's blocked:** Three of Phase 2's five exit criteria need a
  live IBKR account with the Reuters Worldwide Fundamentals
  subscription enabled and TWS / IB Gateway running locally:
  1. The four `AAPL_*.xml` fixtures under
     `src-tauri/tests/fixtures/ibkr_fundamentals/` (>1KB each).
  2. The Python capture script run-to-completion against a paper or
     live TWS at `127.0.0.1:7497`.
  3. Subscription confirmation (screenshot or written note here).
- **Why this is human-in-the-loop:** The /loop session has no TWS
  instance and cannot subscribe to Reuters on the user's behalf. The
  capture-script blueprint in
  `loop/plan/notes/ibkr-fundamentals-xml.md` is ready to run as soon
  as the user is at the desk with TWS up.
- **Crate-path decision recorded autonomously:** See the same notes
  file. `ibapi = "2.11.x"` does **not** expose `req_fundamental_data`
  and the `MessageBus` is `pub(crate)`, so we cannot synthesise a
  raw outgoing frame from outside. Decision: fork `ibapi` and add
  `Client::fundamental_data` for Phase 4; use the official Python
  `ibapi` package for Phase 2 fixture capture. The Rust spike
  binary at `src-tauri/src/bin/ibkr_fundamentals_spike.rs` is a
  feature-gated stub pointing at the notes file until the fork
  exists.
- **Safer interpretation taken:** Phase 2 stays `in-progress` (not
  flipped to `done`) until the four fixtures land and the
  subscription is confirmed. Phases 3, 4, 5 all depend on Phase 2
  being `done`, so they are correctly blocked on the user. The /loop
  ends this iteration with `loop/BREAK` so we don't burn iterations
  re-discovering the same blocker.
- **Action items for the user (when TWS is up):**
  1. Confirm the Reuters Worldwide Fundamentals subscription under
     TWS → Account → Market Data Subscriptions.
  2. `pip install ibapi` (official PyPI package) and run the
     Python capture script from
     `loop/plan/notes/ibkr-fundamentals-xml.md` § "Capture script
     blueprint". It writes the four XML fixtures.
  3. Verify each fixture is non-empty and parseable
     (`xmllint --noout src-tauri/tests/fixtures/ibkr_fundamentals/*.xml`).
  4. Resume the /loop or kick a fresh iteration; Phase 2 can then
     flip to `done` and Phase 3 unblocks.
