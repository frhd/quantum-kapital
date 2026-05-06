//! Phase 8 — `symbol → sector` lookup with a static SP500-ish
//! fallback. The plan's intent: reuse the fundamentals provider's
//! sector field where available, fall back to a small embedded JSON
//! for the rest. Today's `FundamentalData` shape doesn't carry a
//! sector field (see `ibkr/types/fundamentals.rs`), so the fallback
//! is currently the *only* source. When the fundamentals provider
//! grows a sector field, swap the lookup order in `lookup`.
//!
//! Unknown symbols return `None`, which the gate handles by
//! placing them in a synthetic `"unknown"` bucket — the gate refuses
//! to *block* on the unknown bucket (only `warn`), per master gotcha
//! "do not block setup just because sector unknown".

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

/// Coarse sector label. Lowercase snake_case so it can drop straight
/// into the JSON exposure payload without re-formatting.
pub type Sector = &'static str;

/// Static fallback. Curated against a small slice of the most
/// commonly-traded US large-caps that show up in the trader's
/// watchlist. Not exhaustive; rare names land in the `"unknown"`
/// bucket and the gate treats that bucket leniently.
const FALLBACK_SECTORS: &[(&str, Sector)] = &[
    // Tech megacaps
    ("AAPL", "tech"),
    ("MSFT", "tech"),
    ("GOOGL", "tech"),
    ("GOOG", "tech"),
    ("META", "tech"),
    ("AMZN", "consumer_discretionary"),
    ("NFLX", "communications"),
    ("ORCL", "tech"),
    ("CRM", "tech"),
    ("ADBE", "tech"),
    ("CSCO", "tech"),
    ("INTC", "semis"),
    ("AMD", "semis"),
    ("NVDA", "semis"),
    ("TSM", "semis"),
    ("AVGO", "semis"),
    ("QCOM", "semis"),
    ("MU", "semis"),
    ("AMAT", "semis"),
    ("ASML", "semis"),
    ("ARM", "semis"),
    ("MRVL", "semis"),
    ("LRCX", "semis"),
    ("KLAC", "semis"),
    // Tesla + auto
    ("TSLA", "consumer_discretionary"),
    ("F", "consumer_discretionary"),
    ("GM", "consumer_discretionary"),
    ("RIVN", "consumer_discretionary"),
    ("LCID", "consumer_discretionary"),
    // Banks + financials
    ("JPM", "financials"),
    ("BAC", "financials"),
    ("WFC", "financials"),
    ("GS", "financials"),
    ("MS", "financials"),
    ("C", "financials"),
    ("V", "financials"),
    ("MA", "financials"),
    ("AXP", "financials"),
    ("PYPL", "financials"),
    ("SQ", "financials"),
    ("COIN", "financials"),
    // Energy
    ("XOM", "energy"),
    ("CVX", "energy"),
    ("COP", "energy"),
    ("OXY", "energy"),
    ("SLB", "energy"),
    ("EOG", "energy"),
    // Healthcare / pharma
    ("JNJ", "healthcare"),
    ("PFE", "healthcare"),
    ("MRK", "healthcare"),
    ("ABBV", "healthcare"),
    ("LLY", "healthcare"),
    ("UNH", "healthcare"),
    ("CVS", "healthcare"),
    ("MRNA", "healthcare"),
    // Consumer staples
    ("WMT", "consumer_staples"),
    ("COST", "consumer_staples"),
    ("PG", "consumer_staples"),
    ("KO", "consumer_staples"),
    ("PEP", "consumer_staples"),
    // Industrials
    ("BA", "industrials"),
    ("CAT", "industrials"),
    ("GE", "industrials"),
    ("HON", "industrials"),
    ("UPS", "industrials"),
    ("FDX", "industrials"),
    ("DE", "industrials"),
    // Telecom / communications
    ("T", "communications"),
    ("VZ", "communications"),
    ("DIS", "communications"),
    ("CMCSA", "communications"),
    // Real estate
    ("AMT", "real_estate"),
    ("PLD", "real_estate"),
    ("SPG", "real_estate"),
    // ETFs — bucket as their dominant sector for concentration
    // accounting; trader can override per-symbol via the live cache.
    ("SPY", "broad_market"),
    ("QQQ", "tech"),
    ("IWM", "broad_market"),
    ("DIA", "broad_market"),
    ("XLK", "tech"),
    ("XLF", "financials"),
    ("XLE", "energy"),
    ("SMH", "semis"),
    ("SOXX", "semis"),
    ("ARKK", "tech"),
    ("VIX", "broad_market"),
    ("UVXY", "broad_market"),
];

/// Resolves a symbol to a sector. Cheap to clone; the live overrides
/// cache lives behind a `RwLock` so a Tauri command can patch the
/// mapping without restarting.
pub struct SectorMap {
    overrides: RwLock<HashMap<String, Sector>>,
}

impl SectorMap {
    /// Construct a map with the embedded fallback table loaded.
    pub fn new() -> Self {
        Self {
            overrides: RwLock::new(HashMap::new()),
        }
    }

    /// Convenience constructor wrapping in `Arc` for the typical
    /// `app.manage` wiring path.
    pub fn arc() -> Arc<Self> {
        Arc::new(Self::new())
    }

    /// Look up a symbol's sector. Override cache wins; falls through
    /// to the static table; returns `None` for unknown symbols.
    pub async fn lookup(&self, symbol: &str) -> Option<Sector> {
        let upper = symbol.to_uppercase();
        if let Some(s) = self.overrides.read().await.get(&upper) {
            return Some(*s);
        }
        FALLBACK_SECTORS
            .iter()
            .find(|(sym, _)| *sym == upper.as_str())
            .map(|(_, sector)| *sector)
    }

    /// Synchronous variant for hot paths that already hold a borrow
    /// on positions; reads the static table only (override cache is
    /// async-only). Acceptable: overrides are rare and the gate
    /// path can take the small consistency hit.
    pub fn lookup_static(symbol: &str) -> Option<Sector> {
        let upper = symbol.to_uppercase();
        FALLBACK_SECTORS
            .iter()
            .find(|(sym, _)| *sym == upper.as_str())
            .map(|(_, sector)| *sector)
    }

    /// Add or replace a per-symbol override. Used by a future
    /// Tauri command + by tests.
    pub async fn set_override(&self, symbol: &str, sector: Sector) {
        self.overrides
            .write()
            .await
            .insert(symbol.to_uppercase(), sector);
    }
}

impl Default for SectorMap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_static_finds_canonical_symbols() {
        assert_eq!(SectorMap::lookup_static("NVDA"), Some("semis"));
        assert_eq!(SectorMap::lookup_static("nvda"), Some("semis"));
        assert_eq!(SectorMap::lookup_static("JPM"), Some("financials"));
        assert_eq!(SectorMap::lookup_static("UNKNOWN_TICKER_123"), None);
    }

    #[tokio::test]
    async fn override_wins_over_fallback() {
        let map = SectorMap::new();
        assert_eq!(map.lookup("AAPL").await, Some("tech"));
        map.set_override("AAPL", "consumer_discretionary").await;
        assert_eq!(map.lookup("AAPL").await, Some("consumer_discretionary"));
    }

    #[tokio::test]
    async fn unknown_returns_none() {
        let map = SectorMap::new();
        assert!(map.lookup("ZZZZZ").await.is_none());
    }
}
