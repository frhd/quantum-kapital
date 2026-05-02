//! Stub spike binary for the abandoned IBKR fundamentals migration.
//!
//! The published `ibapi = "2.11.x"` crate does NOT expose a
//! `req_fundamental_data` method on `Client`. The protocol constants
//! are present (`OutgoingMessages::RequestFundamentalData = 52`,
//! `IncomingMessages::FundamentalData = 51`) but `MessageBus` is
//! `pub(crate)`, so callers cannot send a raw outgoing frame through
//! the existing connection. Independently, the `reqFundamentalData`
//! API is officially DEPRECATED in IBKR's TWS API docs and a test
//! call against the user's account returned error 10358
//! "Fundamentals data is not allowed" for every reportType.
//!
//! The migration was abandoned 2026-05-02 in favour of an
//! operator-curated manual path (MCP `set_fundamentals` tool) with
//! AV retained as an opportunistic fallback. This binary is gated
//! behind `--features ibkr-spike` so it never builds in CI or
//! pre-commit, and exits with a non-zero status.

use std::process::ExitCode;

fn main() -> ExitCode {
    eprintln!(
        "ibkr_fundamentals_spike: abandoned. The reqFundamentalData API is \
         deprecated and this account lacks the entitlement (error 10358). \
         Manual MCP set_fundamentals + AV fallback is the production path."
    );
    ExitCode::from(2)
}
