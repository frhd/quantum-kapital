//! Throwaway spike binary for Phase 2 of the AV → IBKR Reuters migration.
//!
//! Purpose: connect to a running TWS / IB Gateway and capture
//! `ReportSnapshot`, `ReportsFinSummary`, `ReportsFinStatements`, and
//! `RESC` XML payloads for AAPL into `tests/fixtures/ibkr_fundamentals/`.
//! Phase 4's parser tests then run offline against those fixtures.
//!
//! Status (2026-05-02): the published `ibapi = "2.11.x"` crate does NOT
//! expose a `req_fundamental_data` method on `Client`. The protocol
//! constants are present (`OutgoingMessages::RequestFundamentalData = 52`,
//! `IncomingMessages::FundamentalData = 51`) but `MessageBus` is
//! `pub(crate)`, so callers cannot send a raw outgoing frame through
//! the existing connection. Crate-path decision is documented in
//! `loop/plan/notes/ibkr-fundamentals-xml.md` and in the
//! `## Decisions to make` section of `loop/plan/phase-2-ibkr-spike.md`.
//!
//! See the notes file for the two recommended capture paths:
//!   1. Python via the official `ibapi` PyPI package (fastest).
//!   2. Forked Rust `ibapi` exposing `req_fundamental_data` (preferred
//!      for Phase 4; spike capture can piggy-back on the fork).
//!
//! This binary is gated behind `--features ibkr-spike` so it never
//! builds in CI or pre-commit. It exits with a non-zero status and a
//! pointer to the notes file until one of the two paths above is in
//! place.

use std::process::ExitCode;

fn main() -> ExitCode {
    eprintln!(
        "ibkr_fundamentals_spike: not implemented.\n\n\
         Phase 2 capture cannot proceed against ibapi 2.11.x (no public \
         req_fundamental_data). See loop/plan/notes/ibkr-fundamentals-xml.md \
         for the chosen capture path (Python script or forked Rust ibapi) \
         and the fixture filenames this binary is expected to write."
    );
    ExitCode::from(2)
}
