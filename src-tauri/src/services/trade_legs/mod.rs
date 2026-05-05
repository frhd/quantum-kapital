//! `trade_legs` — FIFO leg matcher over a day of fills.

pub mod fifo;
pub mod types;

#[allow(unused_imports)]
pub use fifo::{compute_totals, match_legs};
#[allow(unused_imports)]
pub use types::{LegTag, LegTotals, SymbolTotals, TradeLeg};

#[cfg(test)]
mod tests;
