//! Executions store + ingest worker. Persists IBKR fills so the
//! assessment stack can query multi-day history. Forward-only.
//!
//! `dead_code` allowed here while the wiring lands incrementally —
//! the store gets exercised by tests immediately, then by the
//! ingestor (Task 7) and `ProdAccountReader` (Task 8). Removed once
//! the production composition root constructs `ExecutionsStore`.

#![allow(dead_code, unused_imports)]

pub mod store;

pub use store::{ExecutionsStore, RecordSummary};

#[cfg(test)]
mod tests;
