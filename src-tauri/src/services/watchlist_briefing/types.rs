//! Wire DTOs for the watchlist briefing composer.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolBriefing {
    pub symbol: String,
    /// Each field is `Option<Value>` so missing-due-to-error and
    /// missing-due-to-no-data are distinguishable via the parallel
    /// `errors` list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bars: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub news: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sentiment: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setups: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fundamentals: Option<Value>,
    /// `["news: upstream_failed", "sentiment: cache miss"]` etc.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchlistBriefing {
    pub as_of: i64,
    pub symbols: Vec<String>,
    pub items: Vec<SymbolBriefing>,
}
