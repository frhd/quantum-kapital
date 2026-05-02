//! Parsers converting [`IbkrHeadline`] rows into the canonical
//! [`NewsItem`] shape. The conversion is intentionally lossy on
//! sentiment fields — IBKR does not return per-article scores, so
//! `overall_sentiment_score` / `overall_sentiment_label` /
//! `ticker_sentiment` all stay at their `None` / `Vec::new()`
//! defaults. The Phase 6 sentiment-loss audit established that this is
//! tolerable; the per-symbol `NewsInterpreter` verdict path is the
//! signal downstream consumers actually read.

use std::collections::HashMap;

use crate::ibkr::types::news::NewsItem;

use super::client::{IbkrHeadline, IbkrNewsProviderInfo};

/// Convert a batch of IBKR headlines into [`NewsItem`]s. `providers`
/// supplies the `code → name` lookup so `NewsItem.source` is the
/// human-readable provider name (e.g. `"Dow Jones Global Equity
/// Trader"`) when known, or the raw provider code as a fallback when
/// the directory does not list it.
pub fn headlines_to_news_items(
    headlines: &[IbkrHeadline],
    providers: &[IbkrNewsProviderInfo],
) -> Vec<NewsItem> {
    let lookup: HashMap<&str, &str> = providers
        .iter()
        .map(|p| (p.code.as_str(), p.name.as_str()))
        .collect();
    headlines
        .iter()
        .map(|h| headline_to_news_item(h, &lookup))
        .collect()
}

fn headline_to_news_item(h: &IbkrHeadline, lookup: &HashMap<&str, &str>) -> NewsItem {
    let title = strip_metadata_block(&h.headline);
    // v1 summary policy — see Phase 7 plan "decisions": no per-article
    // body fetch, so the summary is derived from the headline + any
    // preview the headline list returns. `extra_data` is empty for the
    // Phase 6 AAPL fixture; when populated, providers tend to put a
    // short blurb there, so prefer it when non-empty.
    let summary = if h.extra_data.trim().is_empty() {
        title.clone()
    } else {
        h.extra_data.clone()
    };
    let source = lookup
        .get(h.provider_code.as_str())
        .map(|n| (*n).to_string())
        .unwrap_or_else(|| h.provider_code.clone());

    NewsItem {
        time_published: h.time,
        title,
        summary,
        source,
        // IBKR's `historical_news` does not return a URL — the body is
        // fetched separately via `news_article` and v1 of the provider
        // does not call that path (see Phase 7 plan). Empty string is
        // the same convention `parse_news_response` uses for malformed
        // AV rows.
        url: String::new(),
        overall_sentiment_score: None,
        overall_sentiment_label: None,
        ticker_sentiment: Vec::new(),
    }
}

/// Strip a leading `{A:<conids>:L:<locales>}` metadata block from a
/// raw IBKR headline. Returns the headline unchanged when no leading
/// block is present.
///
/// Examples (from the Phase 6 fixture):
/// - `"{A:800015:L:en}Apple Boosts ..."` → `"Apple Boosts ..."`
/// - `"{A:800015,800015:L:en,en}Review -- Barron's"` → `"Review -- Barron's"`
/// - `"Apple Boosts ..."` → unchanged
pub fn strip_metadata_block(raw: &str) -> String {
    let trimmed = raw.trim_start();
    if let Some(rest) = trimmed.strip_prefix("{A:") {
        if let Some(end_idx) = rest.find('}') {
            // SAFETY: `end_idx` is a byte index returned by `find`, so
            // splitting at `end_idx + 1` lands on a UTF-8 boundary.
            return rest[end_idx + 1..].trim_start().to_string();
        }
    }
    raw.trim_start().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn provider(code: &str, name: &str) -> IbkrNewsProviderInfo {
        IbkrNewsProviderInfo {
            code: code.to_string(),
            name: name.to_string(),
        }
    }

    fn headline(code: &str, h: &str) -> IbkrHeadline {
        IbkrHeadline {
            time: Utc.with_ymd_and_hms(2026, 5, 2, 1, 30, 0).unwrap(),
            provider_code: code.to_string(),
            article_id: format!("{code}$abc"),
            headline: h.to_string(),
            extra_data: String::new(),
        }
    }

    #[test]
    fn strip_metadata_simple_block() {
        assert_eq!(
            strip_metadata_block("{A:800015:L:en}Apple Boosts Mac Mini"),
            "Apple Boosts Mac Mini"
        );
    }

    #[test]
    fn strip_metadata_multi_id_block() {
        assert_eq!(
            strip_metadata_block("{A:800015,800015:L:en,en}Review -- Barron's"),
            "Review -- Barron's"
        );
    }

    #[test]
    fn strip_metadata_no_block_pass_through() {
        assert_eq!(
            strip_metadata_block("Apple beats earnings"),
            "Apple beats earnings"
        );
    }

    #[test]
    fn strip_metadata_unclosed_block_pass_through() {
        // Defensive — if the upstream payload is malformed, don't lose
        // the headline content. The unclosed `{A:...` is improbable
        // in practice but a panic here would silently drop alerts.
        assert_eq!(
            strip_metadata_block("{A:800015 unterminated"),
            "{A:800015 unterminated"
        );
    }

    #[test]
    fn provider_name_lookup_used_when_available() {
        let providers = vec![provider("DJ-N", "Dow Jones Global Equity Trader")];
        let h = headline("DJ-N", "{A:800015:L:en}Some Headline");
        let items = headlines_to_news_items(&[h], &providers);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].source, "Dow Jones Global Equity Trader");
        assert_eq!(items[0].title, "Some Headline");
    }

    #[test]
    fn provider_name_lookup_falls_back_to_code() {
        let providers = vec![provider("DJ-N", "Dow Jones")];
        let h = headline("MYSTERY", "Headline");
        let items = headlines_to_news_items(&[h], &providers);
        assert_eq!(items[0].source, "MYSTERY");
    }

    #[test]
    fn extra_data_populates_summary_when_present() {
        let providers = vec![provider("BRFG", "Briefing.com")];
        let h = IbkrHeadline {
            time: Utc.with_ymd_and_hms(2026, 5, 2, 1, 30, 0).unwrap(),
            provider_code: "BRFG".to_string(),
            article_id: "BRFG$x".to_string(),
            headline: "{A:800015:L:en}Apple Earnings".to_string(),
            extra_data: "Apple beat consensus on iPhone sales".to_string(),
        };
        let items = headlines_to_news_items(&[h], &providers);
        assert_eq!(items[0].title, "Apple Earnings");
        assert_eq!(items[0].summary, "Apple beat consensus on iPhone sales");
    }

    #[test]
    fn sentiment_fields_default_to_none_and_empty() {
        let providers = vec![provider("DJ-N", "Dow Jones")];
        let h = headline("DJ-N", "Plain headline");
        let items = headlines_to_news_items(&[h], &providers);
        assert!(items[0].overall_sentiment_score.is_none());
        assert!(items[0].overall_sentiment_label.is_none());
        assert!(items[0].ticker_sentiment.is_empty());
        assert!(
            items[0].url.is_empty(),
            "v1 IBKR provider does not synthesise a URL"
        );
    }
}
