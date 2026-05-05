//! Executions store + ingest worker. Persists IBKR fills so the
//! assessment stack can query multi-day history. Forward-only.

pub mod ingest;
pub mod store;

pub use ingest::{ExecutionsIngestor, LiveExecutionsFetcher};
#[allow(unused_imports)]
pub use store::BackfillSummary;
pub use store::ExecutionsStore;

#[cfg(test)]
mod tests;
