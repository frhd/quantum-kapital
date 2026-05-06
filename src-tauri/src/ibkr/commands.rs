// This file now re-exports all commands from the sub-modules
// for backward compatibility

pub mod accounts;
pub mod analysis;
pub mod assessments;
pub mod auto_scanner;
pub mod backtest;
pub mod candidates;
pub mod connection;
pub mod eval;
pub mod event_calendar;
pub mod exits;
pub mod market_data;
pub mod news;
pub mod order_ticket;
pub mod portfolio_risk;
pub mod research;
pub mod risk;
pub mod scanner;
pub mod sentiment;
pub mod share;
pub mod tca;
pub mod tracker;
pub mod trade_review_metrics;
pub mod trades;
pub mod trading;

// Re-export all commands at the root level for backward compatibility
pub use accounts::*;
pub use analysis::*;
pub use assessments::*;
pub use auto_scanner::*;
pub use backtest::*;
pub use candidates::*;
pub use connection::*;
pub use eval::*;
pub use event_calendar::*;
pub use exits::*;
pub use market_data::*;
pub use news::*;
pub use order_ticket::*;
pub use portfolio_risk::*;
pub use research::*;
pub use risk::*;
pub use scanner::*;
pub use sentiment::*;
pub use share::*;
pub use tca::*;
pub use tracker::*;
pub use trade_review_metrics::*;
pub use trades::*;
pub use trading::*;
