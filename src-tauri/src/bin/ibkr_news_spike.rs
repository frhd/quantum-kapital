//! Throwaway spike binary for Phase 6 of the AV → IBKR migration.
//!
//! Captures three fixtures into `tests/fixtures/ibkr_news/` for use by
//! Phase 7's `IbkrNewsProvider` parser tests:
//!
//!   1. `news_providers.json` — list of providers the connected
//!      account is subscribed to (output of `client.news_providers()`).
//!   2. `AAPL_historical.json` — `Vec<NewsArticle>` from
//!      `client.historical_news()` over the last 24h, across every
//!      subscribed provider.
//!   3. `AAPL_article_<id>.json` — `NewsArticleBody` for the first
//!      historical item (output of `client.news_article()`).
//!
//! Unlike the Phase 2 fundamentals spike (which is a stub because
//! `ibapi = "2.11.x"` does not expose `req_fundamental_data`), this
//! binary uses the public news methods directly. The fork from
//! Phase 2 is not needed here.
//!
//! Run after starting TWS / IB Gateway with API access enabled and
//! at least one news subscription on the account:
//!
//! ```sh
//! cargo run --bin ibkr_news_spike --features ibkr-spike -- \
//!     --host 127.0.0.1 --port 7497 --client-id 998 --symbol AAPL
//! ```
//!
//! Gated behind `--features ibkr-spike` so it never builds in CI or
//! pre-commit. Deleted (or kept feature-gated) at the end of Phase 6.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use ibapi::Client;
use serde::Serialize;
use time::OffsetDateTime;

const FIXTURE_DIR: &str = "src-tauri/tests/fixtures/ibkr_news";
const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 7497;
const DEFAULT_CLIENT_ID: i32 = 998;
const DEFAULT_SYMBOL: &str = "AAPL";
const DEFAULT_AAPL_CONID: i32 = 265598;
const TOTAL_RESULTS: u8 = 50;
const LOOKBACK_HOURS: i64 = 24;
const PACING_SLEEP: Duration = Duration::from_secs(2);

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("{msg}\n\nUsage: ibkr_news_spike [--host H] [--port P] [--client-id N] [--symbol SYM] [--conid N]");
            return ExitCode::from(2);
        }
    };

    let out_dir = PathBuf::from(FIXTURE_DIR);
    if let Err(e) = fs::create_dir_all(&out_dir) {
        eprintln!("failed to create {}: {e}", out_dir.display());
        return ExitCode::from(2);
    }

    let url = format!("{}:{}", args.host, args.port);
    eprintln!("connecting to TWS at {url} as clientId={}", args.client_id);
    let client = match Client::connect(&url, args.client_id) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "Client::connect failed: {e}\nIs TWS / Gateway running with API access enabled?"
            );
            return ExitCode::from(2);
        }
    };

    // 1. news_providers
    eprintln!("→ news_providers()");
    let providers = match client.news_providers() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("news_providers failed: {e}");
            return ExitCode::from(2);
        }
    };
    if providers.is_empty() {
        eprintln!(
            "WARNING: account has zero news subscriptions. \
             historical_news will return nothing. Subscribe under \
             TWS → Account → Market Data Subscriptions and re-run."
        );
    }
    let provider_codes: Vec<String> = providers.iter().map(|p| p.code.clone()).collect();
    eprintln!("  {} provider(s): {:?}", providers.len(), provider_codes);
    if let Err(e) = write_json(&out_dir.join("news_providers.json"), &providers) {
        eprintln!("write news_providers.json failed: {e}");
        return ExitCode::from(2);
    }

    if provider_codes.is_empty() {
        eprintln!("no providers — skipping historical_news / news_article steps");
        client.disconnect();
        return ExitCode::from(0);
    }

    thread::sleep(PACING_SLEEP);

    // 2. historical_news for the last LOOKBACK_HOURS
    let end = OffsetDateTime::now_utc();
    let start = end - time::Duration::hours(LOOKBACK_HOURS);
    eprintln!(
        "→ historical_news(conId={}, providers={:?}, {} → {}, total={})",
        args.conid, provider_codes, start, end, TOTAL_RESULTS
    );
    let codes_borrowed: Vec<&str> = provider_codes.iter().map(String::as_str).collect();
    let articles_subscription =
        match client.historical_news(args.conid, &codes_borrowed, start, end, TOTAL_RESULTS) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("historical_news failed: {e}");
                client.disconnect();
                return ExitCode::from(2);
            }
        };

    let mut articles: Vec<ibapi::news::NewsArticle> = Vec::new();
    for article in articles_subscription.iter().take(TOTAL_RESULTS as usize) {
        articles.push(article);
    }
    eprintln!("  captured {} article headline(s)", articles.len());

    let historical_path = out_dir.join(format!("{}_historical.json", args.symbol));
    if let Err(e) = write_articles(&historical_path, &articles) {
        eprintln!("write {} failed: {e}", historical_path.display());
        client.disconnect();
        return ExitCode::from(2);
    }

    if articles.is_empty() {
        eprintln!(
            "no headlines returned — subscribed providers may not cover {} \
             over the last {}h. Phase 6 exit gate wants ≥10 items.",
            args.symbol, LOOKBACK_HOURS
        );
        client.disconnect();
        return ExitCode::from(0);
    }

    thread::sleep(PACING_SLEEP);

    // 3. news_article for the first headline
    let head = &articles[0];
    eprintln!(
        "→ news_article(provider={}, article_id={})",
        head.provider_code, head.article_id
    );
    let body = match client.news_article(&head.provider_code, &head.article_id) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("news_article failed: {e}");
            client.disconnect();
            return ExitCode::from(2);
        }
    };

    let safe_id = sanitize(&head.article_id);
    let body_path = out_dir.join(format!("{}_article_{}.json", args.symbol, safe_id));
    if let Err(e) = write_article_body(&body_path, &body) {
        eprintln!("write {} failed: {e}", body_path.display());
        client.disconnect();
        return ExitCode::from(2);
    }

    eprintln!(
        "DONE. Fixtures written to {}/. Spot-check that headlines look ticker-tagged \
         and the article body is parseable (text/html vs Base64 binary).",
        out_dir.display()
    );
    client.disconnect();
    ExitCode::from(0)
}

#[derive(Debug)]
struct Args {
    host: String,
    port: u16,
    client_id: i32,
    symbol: String,
    conid: i32,
}

fn parse_args() -> Result<Args, String> {
    let mut host = DEFAULT_HOST.to_string();
    let mut port = DEFAULT_PORT;
    let mut client_id = DEFAULT_CLIENT_ID;
    let mut symbol = DEFAULT_SYMBOL.to_string();
    let mut conid = DEFAULT_AAPL_CONID;

    let mut iter = env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--host" => {
                host = iter
                    .next()
                    .ok_or_else(|| "--host needs a value".to_string())?
            }
            "--port" => {
                port = iter
                    .next()
                    .ok_or_else(|| "--port needs a value".to_string())?
                    .parse()
                    .map_err(|e| format!("--port: {e}"))?
            }
            "--client-id" => {
                client_id = iter
                    .next()
                    .ok_or_else(|| "--client-id needs a value".to_string())?
                    .parse()
                    .map_err(|e| format!("--client-id: {e}"))?
            }
            "--symbol" => {
                symbol = iter
                    .next()
                    .ok_or_else(|| "--symbol needs a value".to_string())?
            }
            "--conid" => {
                conid = iter
                    .next()
                    .ok_or_else(|| "--conid needs a value".to_string())?
                    .parse()
                    .map_err(|e| format!("--conid: {e}"))?
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(Args {
        host,
        port,
        client_id,
        symbol,
        conid,
    })
}

fn write_json<T: Serialize>(path: &std::path::Path, value: &T) -> std::io::Result<()> {
    let s = serde_json::to_string_pretty(value)
        .map_err(|e| std::io::Error::other(format!("serialize: {e}")))?;
    fs::write(path, s)?;
    eprintln!("  wrote {}", path.display());
    Ok(())
}

#[derive(Serialize)]
struct WireArticle<'a> {
    time_iso8601: String,
    provider_code: &'a str,
    article_id: &'a str,
    headline: &'a str,
    extra_data: &'a str,
}

fn write_articles(
    path: &std::path::Path,
    articles: &[ibapi::news::NewsArticle],
) -> std::io::Result<()> {
    let wire: Vec<WireArticle<'_>> = articles
        .iter()
        .map(|a| WireArticle {
            time_iso8601: a
                .time
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_else(|_| a.time.unix_timestamp().to_string()),
            provider_code: &a.provider_code,
            article_id: &a.article_id,
            headline: &a.headline,
            extra_data: &a.extra_data,
        })
        .collect();
    write_json(path, &wire)
}

#[derive(Serialize)]
struct WireBody<'a> {
    article_type: &'a str,
    article_text: &'a str,
}

fn write_article_body(
    path: &std::path::Path,
    body: &ibapi::news::NewsArticleBody,
) -> std::io::Result<()> {
    let kind = match body.article_type {
        ibapi::news::ArticleType::Text => "Text",
        ibapi::news::ArticleType::Binary => "Binary",
    };
    write_json(
        path,
        &WireBody {
            article_type: kind,
            article_text: &body.article_text,
        },
    )
}

fn sanitize(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}
