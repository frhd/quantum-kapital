//! Ticker-validity filter for sentiment scrapes.
//!
//! Reddit comments and Stocktwits messages are full of `$A`, `$YOU`,
//! `$TO`, `$DIS`, `$ALL` — real symbols AND ordinary words used as
//! emphasis. Without filtering, a sentiment score for "AAPL" picks up
//! every "$A" mention and the dataset becomes noise. Strategy:
//!
//! 1. **Hard blocklist of common-word tickers.** A handful of tickers
//!    collide so often with English filler that the only safe answer
//!    is "never accept these from a free-text scrape." Concrete cases:
//!    `$A` (Agilent), `$YOU` (Clear Secure), `$TO` (no listing — purely
//!    a preposition), `$IT` (Gartner), `$DIS` (Disney — but "dis" the
//!    slang verb is too common), `$ALL` (Allstate), `$ON`, `$AT`, `$BE`,
//!    `$WE`, `$ARE`. Configurable so callers with structured input can
//!    relax the list.
//! 2. **Length floor.** Single-letter tickers (`$F`, `$T`) are real but
//!    statistically dominated by typos and price/quantity tokens like
//!    "$5". Default minimum length: 2 characters.
//! 3. **Optional whitelist.** When a caller has a curated symbol set
//!    (e.g. union of `tracked_tickers` + a top-2000 list), passing it
//!    in restricts output to that set — strictly more precise than the
//!    rules above.
//!
//! All filters are case-insensitive; output preserves the upper-cased
//! form. The filter is a free function rather than a stateful struct
//! because the rules are pure data.

use std::collections::HashSet;

/// Common-word tickers that we never accept from free-text scrapes.
/// See module docs for rationale; lower-case so the comparison is
/// case-insensitive after `to_ascii_lowercase`.
const COMMON_WORD_BLOCKLIST: &[&str] = &[
    "a", "an", "the", "to", "be", "do", "go", "we", "us", "you", "i", "me", "my", "no", "so", "or",
    "of", "on", "at", "is", "it", "if", "as", "by", "in", "all", "are", "dis", "any", "for", "and",
    "but", "now", "out",
];

/// Behaviour knobs for [`extract_valid_tickers`]. Defaults match the
/// "free-text Reddit / Stocktwits scrape" use case described in the
/// module docs.
#[derive(Debug, Clone)]
pub struct TickerFilterConfig {
    /// Minimum ticker length (after stripping the leading `$`). Default 2.
    pub min_len: usize,
    /// Maximum ticker length. Default 5 (US tickers + class suffixes
    /// like `BRK.B` aren't carried here — class-share dotted forms
    /// don't appear in raw scrape text).
    pub max_len: usize,
    /// Lower-case common words rejected outright.
    pub blocklist: HashSet<String>,
    /// Optional whitelist of valid uppercase symbols. When `Some`, ONLY
    /// symbols in the set are accepted; the blocklist still applies as
    /// a belt-and-braces guard.
    pub whitelist: Option<HashSet<String>>,
}

impl Default for TickerFilterConfig {
    fn default() -> Self {
        Self {
            min_len: 2,
            max_len: 5,
            blocklist: COMMON_WORD_BLOCKLIST
                .iter()
                .map(|s| s.to_string())
                .collect(),
            whitelist: None,
        }
    }
}

/// Return `true` iff `candidate` (already stripped of any leading `$`)
/// passes every rule in `cfg`. Pure function over the upper-cased form
/// of the candidate; safe to call from tests without I/O.
pub fn is_valid_ticker(candidate: &str, cfg: &TickerFilterConfig) -> bool {
    let upper = candidate.trim().to_ascii_uppercase();
    if upper.is_empty() {
        return false;
    }
    // ASCII letters only; anything else (digits, punctuation, unicode)
    // means this isn't a ticker at all.
    if !upper.chars().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    if upper.len() < cfg.min_len || upper.len() > cfg.max_len {
        return false;
    }
    if cfg.blocklist.contains(&upper.to_ascii_lowercase()) {
        return false;
    }
    if let Some(allow) = &cfg.whitelist {
        if !allow.contains(&upper) {
            return false;
        }
    }
    true
}

/// Extract the unique set of valid tickers from a free-text body. The
/// scan looks for `$XYZ`-style cashtags first; if `cfg.whitelist` is
/// supplied the function ALSO accepts bare uppercase tokens that match
/// the whitelist (so a structured Reddit JSON field with no `$` prefix
/// still produces tickers when the caller has narrowed the universe).
///
/// Output order is the order of first appearance in `text`, upper-cased.
pub fn extract_valid_tickers(text: &str, cfg: &TickerFilterConfig) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();

    // Cashtag pass: split on whitespace, look for `$XYZ` prefixes. This
    // intentionally avoids a regex dependency — the matcher is trivial
    // and the tokenizer below is the same one we'd use anyway.
    for raw_tok in text.split(|c: char| !c.is_ascii_alphanumeric() && c != '$') {
        if let Some(rest) = raw_tok.strip_prefix('$') {
            let upper = rest.to_ascii_uppercase();
            if is_valid_ticker(&upper, cfg) && seen.insert(upper.clone()) {
                out.push(upper);
            }
        }
    }

    // Whitelist pass over bare tokens, only when the caller has supplied
    // a whitelist. Without one, accepting bare uppercase tokens would
    // false-positive on every initialism in the text.
    if cfg.whitelist.is_some() {
        for raw_tok in text.split(|c: char| !c.is_ascii_alphanumeric()) {
            if raw_tok.is_empty() {
                continue;
            }
            // Bare-token pass requires the original token to be all-
            // upper-case so "Apple" doesn't promote to "APPLE".
            if raw_tok.chars().all(|c| c.is_ascii_uppercase()) {
                let upper = raw_tok.to_string();
                if is_valid_ticker(&upper, cfg) && seen.insert(upper.clone()) {
                    out.push(upper);
                }
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_common_word_tickers_a_to_you() {
        let cfg = TickerFilterConfig::default();
        for word in ["A", "TO", "YOU", "IT", "DIS", "ALL", "ON", "BE", "WE"] {
            assert!(
                !is_valid_ticker(word, &cfg),
                "common-word ticker `{word}` should be rejected"
            );
        }
    }

    #[test]
    fn accepts_real_multi_letter_tickers() {
        let cfg = TickerFilterConfig::default();
        for sym in ["AAPL", "TSLA", "MSFT", "NVDA", "GOOGL", "AMD", "QQQ"] {
            assert!(
                is_valid_ticker(sym, &cfg),
                "real ticker `{sym}` should pass"
            );
        }
    }

    #[test]
    fn rejects_length_outliers_and_non_alpha() {
        let cfg = TickerFilterConfig::default();
        assert!(!is_valid_ticker("F", &cfg), "single-letter rejected");
        assert!(!is_valid_ticker("TOOLONG", &cfg), "len > 5 rejected");
        assert!(!is_valid_ticker("AA1", &cfg), "non-alpha rejected");
        assert!(!is_valid_ticker("BRK.B", &cfg), "dotted form rejected");
        assert!(!is_valid_ticker("", &cfg), "empty rejected");
    }

    #[test]
    fn extract_pulls_cashtags_from_free_text() {
        let cfg = TickerFilterConfig::default();
        let body = "$TSLA pumping, but careful — $A is also moving \
                    while $TOOLONG and $YOU are ignored.";
        let got = extract_valid_tickers(body, &cfg);
        assert_eq!(got, vec!["TSLA"]);
    }

    #[test]
    fn extract_dedupes_and_preserves_first_seen_order() {
        let cfg = TickerFilterConfig::default();
        let body = "$NVDA up, $AMD up, $NVDA still up";
        let got = extract_valid_tickers(body, &cfg);
        assert_eq!(got, vec!["NVDA", "AMD"]);
    }

    #[test]
    fn extract_with_whitelist_promotes_bare_tokens() {
        let cfg = TickerFilterConfig {
            whitelist: Some(
                ["AAPL".to_string(), "TSLA".to_string()]
                    .into_iter()
                    .collect(),
            ),
            ..TickerFilterConfig::default()
        };
        // No `$`, but AAPL is in the whitelist so it should be picked up.
        let got = extract_valid_tickers("AAPL beats, TSLA misses, RANDOM noise", &cfg);
        assert_eq!(got, vec!["AAPL", "TSLA"]);
    }

    #[test]
    fn extract_with_whitelist_still_rejects_blocklist_collision() {
        // Even if `A` were whitelisted by accident, the blocklist must
        // win — common-word collision is the entire point of this guard.
        let cfg = TickerFilterConfig {
            whitelist: Some(["A".to_string(), "AAPL".to_string()].into_iter().collect()),
            ..TickerFilterConfig::default()
        };
        let got = extract_valid_tickers("$A is a letter, $AAPL is a stock", &cfg);
        assert_eq!(got, vec!["AAPL"]);
    }
}
