//! Phase 7 part B — live [`IbkrNewsClient`] impl. Wraps the released
//! `ibapi = "2.11.x"` news APIs (`Client::news_providers`,
//! `Client::contract_details`, `Client::historical_news`) on
//! [`IbkrClient`] and translates payloads + errors into the
//! `services::news_provider` domain shape.
//!
//! All blocking ibapi calls run on `spawn_blocking` so the async
//! runtime stays unblocked, matching the rest of this module's
//! pattern (see `historical.rs`, `market_data.rs`).

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use ibapi::contracts::Contract;
use time::{Duration as TimeDuration, OffsetDateTime};

use crate::ibkr::error::IbkrError;
use crate::services::news_provider::ibkr::client::{
    IbkrHeadline, IbkrNewsClient, IbkrNewsProviderInfo,
};
use crate::services::news_provider::NewsError;

use super::IbkrClient;

#[async_trait]
impl IbkrNewsClient for IbkrClient {
    async fn news_providers(&self) -> Result<Vec<IbkrNewsProviderInfo>, NewsError> {
        let client = self.ibapi_client().await.map_err(ibkr_to_news_error)?;
        let res = tokio::task::spawn_blocking(move || client.news_providers())
            .await
            .map_err(|e| NewsError::Other(format!("join: {e}")))?;
        match res {
            Ok(rows) => Ok(rows
                .into_iter()
                .map(|p| IbkrNewsProviderInfo {
                    code: p.code,
                    name: p.name,
                })
                .collect()),
            Err(e) => Err(ibapi_to_news_error(e, None)),
        }
    }

    async fn historical_news(
        &self,
        symbol: &str,
        provider_codes: &[String],
        lookback_hours: u32,
        total_results: u8,
    ) -> Result<Vec<IbkrHeadline>, NewsError> {
        if provider_codes.is_empty() {
            return Err(NewsError::NoSubscription {
                provider_code: "<none subscribed>".to_string(),
            });
        }

        let client = self.ibapi_client().await.map_err(ibkr_to_news_error)?;

        let symbol_owned = symbol.to_string();
        let codes_owned: Vec<String> = provider_codes.to_vec();

        // Resolve conid up-front. `req_historical_news` requires a
        // numeric contract id; `Contract::stock(symbol).build()`
        // alone is rejected with code 200 ("No security definition…")
        // because the routing exchange isn't pinned.
        let client_for_resolve = Arc::clone(&client);
        let resolve_symbol = symbol_owned.clone();
        let conid = tokio::task::spawn_blocking(move || -> Result<i32, ibapi::Error> {
            let contract = Contract::stock(&resolve_symbol).build();
            let details = client_for_resolve.contract_details(&contract)?;
            details
                .into_iter()
                .next()
                .map(|d| d.contract.contract_id)
                .ok_or_else(|| ibapi::Error::Simple(format!("no contract for {resolve_symbol}")))
        })
        .await
        .map_err(|e| NewsError::Other(format!("conid join: {e}")))?
        .map_err(|e| ibapi_to_news_error(e, Some(&symbol_owned)))?;

        // historical_news start/end window. AV's `lookback_hours`
        // semantic is "rows inside the last N hours" — we mirror that
        // by computing now / now-N here, in UTC. ibapi takes
        // `time::OffsetDateTime` (no chrono).
        let end = OffsetDateTime::now_utc();
        let start = end - TimeDuration::hours(i64::from(lookback_hours));

        let client_for_news = client;
        let codes_for_news = codes_owned.clone();
        let news_res =
            tokio::task::spawn_blocking(move || -> Result<Vec<IbkrHeadline>, ibapi::Error> {
                let codes_borrowed: Vec<&str> = codes_for_news.iter().map(String::as_str).collect();
                let subscription = client_for_news.historical_news(
                    conid,
                    &codes_borrowed,
                    start,
                    end,
                    total_results,
                )?;
                let mut out = Vec::with_capacity(total_results as usize);
                for article in subscription.iter().take(total_results as usize) {
                    let ts = article.time.unix_timestamp();
                    let chrono_time =
                        chrono::DateTime::<Utc>::from_timestamp(ts, 0).unwrap_or_else(Utc::now);
                    out.push(IbkrHeadline {
                        time: chrono_time,
                        provider_code: article.provider_code,
                        article_id: article.article_id,
                        headline: article.headline,
                        extra_data: article.extra_data,
                    });
                }
                Ok(out)
            })
            .await
            .map_err(|e| NewsError::Other(format!("historical_news join: {e}")))?;

        match news_res {
            Ok(headlines) => Ok(headlines),
            Err(e) => Err(ibapi_to_news_error(e, Some(&codes_owned.join(",")))),
        }
    }
}

/// Translate the [`IbkrError`] returned by [`IbkrClient::ibapi_client`]
/// into the [`NewsError`] surface. The only branch we hit on this path
/// is `NotConnected` (the only variant `ibapi_client` returns); all
/// other branches are defensive.
fn ibkr_to_news_error(err: IbkrError) -> NewsError {
    match err {
        IbkrError::NotConnected => NewsError::NotConnected,
        other => NewsError::Other(other.to_string()),
    }
}

/// Map an `ibapi::Error` into the [`NewsError`] surface. Subscription
/// denial is identified heuristically — TWS surfaces these as
/// `Error::Message(code, msg)` with codes that vary by request path
/// (322 historically, 10168/10169 on newer servers). We classify by
/// keyword so the variant remains accurate as TWS evolves.
fn ibapi_to_news_error(err: ibapi::Error, context: Option<&str>) -> NewsError {
    use ibapi::Error as Ie;
    match err {
        Ie::ConnectionFailed | Ie::ConnectionReset | Ie::Shutdown => NewsError::NotConnected,
        Ie::Message(code, msg) => {
            let lower = msg.to_ascii_lowercase();
            let looks_like_subscription = lower.contains("not subscribed")
                || lower.contains("no permission")
                || lower.contains("news permission")
                || lower.contains("news subscription")
                || lower.contains("not allowed")
                || code == 322
                || code == 10168
                || code == 10169;
            if looks_like_subscription {
                NewsError::NoSubscription {
                    provider_code: context
                        .map(str::to_string)
                        .unwrap_or_else(|| "<unknown>".to_string()),
                }
            } else if lower.contains("pacing") || lower.contains("rate") {
                NewsError::RateLimited { retry_after: None }
            } else {
                NewsError::Other(format!("[{code}] {msg}"))
            }
        }
        other => NewsError::Other(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_322_classified_as_no_subscription() {
        let err = ibapi::Error::Message(322, "News not subscribed".to_string());
        let mapped = ibapi_to_news_error(err, Some("DJ-N"));
        match mapped {
            NewsError::NoSubscription { provider_code } => assert_eq!(provider_code, "DJ-N"),
            other => panic!("expected NoSubscription, got {other:?}"),
        }
    }

    #[test]
    fn message_with_rate_keyword_classified_as_rate_limited() {
        let err =
            ibapi::Error::Message(100, "Pacing violation: too many news requests".to_string());
        assert!(matches!(
            ibapi_to_news_error(err, None),
            NewsError::RateLimited { .. }
        ));
    }

    #[test]
    fn connection_reset_classified_as_not_connected() {
        let err = ibapi::Error::ConnectionReset;
        assert!(matches!(
            ibapi_to_news_error(err, None),
            NewsError::NotConnected
        ));
    }

    #[test]
    fn unrelated_message_classified_as_other() {
        let err = ibapi::Error::Message(99, "something else".to_string());
        let mapped = ibapi_to_news_error(err, None);
        assert!(matches!(mapped, NewsError::Other(_)));
    }
}
