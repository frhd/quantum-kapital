// This file now re-exports all commands from the sub-modules
// for backward compatibility

pub mod accounts;
pub mod analysis;
pub mod assessments;
pub mod auto_scanner;
pub mod candidates;
pub mod connection;
pub mod eval;
pub mod market_data;
pub mod news;
pub mod research;
pub mod risk;
pub mod scanner;
pub mod sentiment;
pub mod share;
pub mod tracker;
pub mod trades;
pub mod trading;

// Re-export all commands at the root level for backward compatibility
pub use accounts::*;
pub use analysis::*;
pub use assessments::*;
pub use auto_scanner::*;
pub use candidates::*;
pub use connection::*;
pub use eval::*;
pub use market_data::*;
pub use news::*;
pub use research::*;
pub use risk::*;
pub use scanner::*;
pub use sentiment::*;
pub use share::*;
pub use tracker::*;
pub use trades::*;
pub use trading::*;
