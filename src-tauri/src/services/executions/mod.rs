//! Executions store + ingest worker. Persists IBKR fills so the
//! assessment stack can query multi-day history. Forward-only.

pub mod ingest;
pub mod store;

pub use ingest::{ExecutionsIngestor, LiveExecutionsFetcher};
pub use store::ExecutionsStore;

#[cfg(test)]
mod tests;
