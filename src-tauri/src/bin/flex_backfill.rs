//! `flex_backfill` — one-shot importer that seeds the executions store
//! with historical fills via IBKR's Flex Web Service.
//!
//! The TWS API's `reqExecutions` endpoint returns only the current
//! TWS-day. The Flex Web Service is the parallel reporting surface
//! that supports arbitrary historical ranges. This binary fetches one
//! XML report per invocation, parses Trade rows, and UPSERTs them into
//! the existing `executions` table with `source='flex'`.
//!
//! Setup, token generation, and field-list configuration live in
//! `docs/ibkr-flex-backfill.md`.
//!
//! Usage:
//!   IBKR_FLEX_TOKEN=... IBKR_FLEX_QUERY_ID=... \
//!     cargo run --bin flex_backfill -- --from 2025-01-01 --to 2026-05-04 --dry-run
//!
//! Surveillance-only: this binary only reads from IBKR's reporting
//! API and writes to the local `executions` SQLite store. No order
//! placement, no live IBKR mutation.
// allow-large-file: Single-purpose backfill binary (HTTP + XML parse +
// CLI + stats + write) is hard to split without spreading thin.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use chrono_tz::America::New_York;
use quick_xml::events::Event;
use quick_xml::Reader;

use quantum_kapital_lib::{Db, ExecutionSide, ExecutionsStore, IbkrExecution};

const FLEX_BASE: &str =
    "https://gdcdyn.interactivebrokers.com/Universal/servlet/FlexStatementService";
const POLL_RETRY_CODE: &str = "1019";
const POLL_MAX_ATTEMPTS: usize = 12;
const POLL_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug)]
struct Cli {
    from: NaiveDate,
    to: NaiveDate,
    dry_run: bool,
    keep_cash: bool,
    db_path: Option<PathBuf>,
    fixture: Option<PathBuf>,
}

fn print_usage() {
    eprintln!(
        "usage: flex_backfill --from YYYY-MM-DD --to YYYY-MM-DD [--dry-run] [--keep-cash]\n\
                       [--db PATH] [--fixture PATH]\n\
        \n\
        Env vars (required unless --fixture is given):\n\
          IBKR_FLEX_TOKEN     Flex Web Service token\n\
          IBKR_FLEX_QUERY_ID  Numeric Activity Flex Query ID\n\
        \n\
        --dry-run     Parse + stats only, no DB writes.\n\
        --keep-cash   Don't drop CASH rows (default: drop — auto-FX residuals\n\
                      and forex pairs that the FIFO matcher can't process).\n\
        --db PATH     Override the SQLite path (default: OS app-data dir).\n\
        --fixture PATH Read XML from a local file instead of fetching from IBKR.\n",
    );
}

fn parse_cli() -> Result<Cli, String> {
    let mut from: Option<NaiveDate> = None;
    let mut to: Option<NaiveDate> = None;
    let mut dry_run = false;
    let mut keep_cash = false;
    let mut db_path: Option<PathBuf> = None;
    let mut fixture: Option<PathBuf> = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--from" => {
                from = Some(parse_date(&args.next().ok_or("--from needs a value")?)?);
            }
            "--to" => {
                to = Some(parse_date(&args.next().ok_or("--to needs a value")?)?);
            }
            "--dry-run" => dry_run = true,
            "--keep-cash" => keep_cash = true,
            "--db" => db_path = Some(PathBuf::from(args.next().ok_or("--db needs a value")?)),
            "--fixture" => {
                fixture = Some(PathBuf::from(args.next().ok_or("--fixture needs a value")?));
            }
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            other => return Err(format!("unknown arg: {other}")),
        }
    }
    let from = from.ok_or("--from is required")?;
    let to = to.ok_or("--to is required")?;
    if from > to {
        return Err(format!("--from {from} must be <= --to {to}"));
    }
    Ok(Cli {
        from,
        to,
        dry_run,
        keep_cash,
        db_path,
        fixture,
    })
}

fn parse_date(s: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| format!("date {s:?}: {e}"))
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_max_level(tracing::Level::INFO)
        .init();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

async fn run() -> Result<(), String> {
    let cli = parse_cli().inspect_err(|_| print_usage())?;

    let xml = if let Some(path) = &cli.fixture {
        std::fs::read_to_string(path).map_err(|e| format!("read {path:?}: {e}"))?
    } else {
        let token = env::var("IBKR_FLEX_TOKEN")
            .map_err(|_| "IBKR_FLEX_TOKEN env var not set".to_string())?;
        let query_id = env::var("IBKR_FLEX_QUERY_ID")
            .map_err(|_| "IBKR_FLEX_QUERY_ID env var not set".to_string())?;
        fetch_flex_report(&token, &query_id).await?
    };

    let parsed = parse_trades(&xml)?;
    println!("parsed {} <Trade> rows from XML", parsed.len());

    let dropped_cash;
    let kept: Vec<RawTrade> = if cli.keep_cash {
        dropped_cash = 0;
        parsed
    } else {
        let n = parsed.len();
        let kept: Vec<RawTrade> = parsed
            .into_iter()
            .filter(|r| r.asset_category != "CASH")
            .collect();
        dropped_cash = n - kept.len();
        kept
    };
    if dropped_cash > 0 {
        println!("dropped {dropped_cash} CASH rows (auto-FX residuals / forex; FIFO matcher has no support)");
    }

    let mut in_range = 0usize;
    let mut out_of_range = 0usize;
    let mut mapped: Vec<IbkrExecution> = Vec::with_capacity(kept.len());
    let mut map_errors: Vec<String> = Vec::new();
    for raw in &kept {
        match map_to_execution(raw) {
            Ok(exec) => {
                let et_day = exec.exec_time.with_timezone(&New_York).date_naive();
                if et_day >= cli.from && et_day <= cli.to {
                    mapped.push(exec);
                    in_range += 1;
                } else {
                    out_of_range += 1;
                }
            }
            Err(e) => map_errors.push(format!("trade_id={}: {e}", raw.trade_id)),
        }
    }
    println!(
        "date-range filter: {in_range} kept, {out_of_range} outside [{}..={}]",
        cli.from, cli.to
    );
    if !map_errors.is_empty() {
        println!("{} mapping errors:", map_errors.len());
        for err in map_errors.iter().take(10) {
            println!("  - {err}");
        }
        if map_errors.len() > 10 {
            println!("  ... ({} more)", map_errors.len() - 10);
        }
    }

    print_stats(&mapped);

    if cli.dry_run {
        println!("(dry-run; no DB writes)");
        return Ok(());
    }

    let db_path = cli
        .db_path
        .clone()
        .or_else(default_db_path)
        .ok_or("could not resolve default DB path; pass --db PATH")?;
    println!("opening DB at {db_path:?}");
    let db = Arc::new(Db::open(&db_path).map_err(|e| format!("open db: {e}"))?);
    let store = ExecutionsStore::new(db);
    let summary = store
        .record_backfill(&mapped)
        .await
        .map_err(|e| format!("record_backfill: {e}"))?;
    println!(
        "backfill summary: inserted={} skipped_existing={} skipped_live_match={}",
        summary.inserted, summary.skipped_existing, summary.skipped_live_match,
    );
    Ok(())
}

fn default_db_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("com.quantyc.qqk").join("tracker.sqlite"))
}

// ---- HTTP --------------------------------------------------------------

async fn fetch_flex_report(token: &str, query_id: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| format!("build http client: {e}"))?;
    let send_url = format!("{FLEX_BASE}.SendRequest?t={token}&q={query_id}&v=3");
    println!("sending Flex request...");
    let send_xml = client
        .get(&send_url)
        .send()
        .await
        .map_err(|e| format!("SendRequest: {e}"))?
        .error_for_status()
        .map_err(|e| format!("SendRequest http: {e}"))?
        .text()
        .await
        .map_err(|e| format!("SendRequest body: {e}"))?;
    let reference = extract_tag(&send_xml, "ReferenceCode").ok_or_else(|| {
        let code = extract_tag(&send_xml, "ErrorCode").unwrap_or_else(|| "?".into());
        let msg = extract_tag(&send_xml, "ErrorMessage").unwrap_or_else(|| "?".into());
        format!("SendRequest failed (code={code}, msg={msg})")
    })?;
    println!("statement reference: {reference}");
    let get_url = format!("{FLEX_BASE}.GetStatement?t={token}&q={reference}&v=3");
    for attempt in 1..=POLL_MAX_ATTEMPTS {
        tokio::time::sleep(POLL_INTERVAL).await;
        let body = client
            .get(&get_url)
            .send()
            .await
            .map_err(|e| format!("GetStatement: {e}"))?
            .error_for_status()
            .map_err(|e| format!("GetStatement http: {e}"))?
            .text()
            .await
            .map_err(|e| format!("GetStatement body: {e}"))?;
        // Successful response is a `<FlexQueryResponse>` document; the
        // "still generating" case returns a `<FlexStatementResponse>`
        // with ErrorCode=1019.
        if let Some(code) = extract_tag(&body, "ErrorCode") {
            if code == POLL_RETRY_CODE {
                println!("  still generating (attempt {attempt}/{POLL_MAX_ATTEMPTS})...");
                continue;
            }
            let msg = extract_tag(&body, "ErrorMessage").unwrap_or_else(|| "?".into());
            return Err(format!("GetStatement error code={code}: {msg}"));
        }
        return Ok(body);
    }
    Err("Flex statement did not finish within poll budget".into())
}

fn extract_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end_rel = xml[start..].find(&close)?;
    Some(xml[start..start + end_rel].trim().to_string())
}

// ---- XML parsing -------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq)]
struct RawTrade {
    account_id: String,
    currency: Option<String>,
    asset_category: String,
    sub_category: Option<String>,
    symbol: String,
    trade_id: String,
    multiplier: Option<String>,
    strike: Option<String>,
    expiry: Option<String>,
    date_time: String,
    put_call: Option<String>,
    quantity: String,
    trade_price: String,
    ib_commission: Option<String>,
    ib_commission_currency: Option<String>,
    notes: Option<String>,
    fifo_pnl_realized: Option<String>,
    buy_sell: String,
    ib_order_id: String,
    ib_exec_id: Option<String>,
}

fn parse_trades(xml: &str) -> Result<Vec<RawTrade>, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out: Vec<RawTrade> = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) if e.name().as_ref() == b"Trade" => {
                let mut raw = RawTrade::default();
                for attr in e.attributes().flatten() {
                    let key = attr.key.as_ref();
                    let val = attr
                        .unescape_value()
                        .map_err(|e| format!("xml attr decode: {e}"))?
                        .into_owned();
                    match key {
                        b"accountId" => raw.account_id = val,
                        b"currency" => raw.currency = nonempty(val),
                        b"assetCategory" => raw.asset_category = val,
                        b"subCategory" => raw.sub_category = nonempty(val),
                        b"symbol" => raw.symbol = val,
                        b"tradeID" => raw.trade_id = val,
                        b"multiplier" => raw.multiplier = nonempty(val),
                        b"strike" => raw.strike = nonempty(val),
                        b"expiry" => raw.expiry = nonempty(val),
                        b"dateTime" => raw.date_time = val,
                        b"putCall" => raw.put_call = nonempty(val),
                        b"quantity" => raw.quantity = val,
                        b"tradePrice" => raw.trade_price = val,
                        b"ibCommission" => raw.ib_commission = nonempty(val),
                        b"ibCommissionCurrency" => raw.ib_commission_currency = nonempty(val),
                        b"notes" => raw.notes = nonempty(val),
                        b"fifoPnlRealized" => raw.fifo_pnl_realized = nonempty(val),
                        b"buySell" => raw.buy_sell = val,
                        b"ibOrderID" => raw.ib_order_id = val,
                        b"ibExecID" => raw.ib_exec_id = nonempty(val),
                        _ => {}
                    }
                }
                out.push(raw);
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("xml: {e}")),
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

fn nonempty(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

// ---- Mapping -----------------------------------------------------------

fn map_to_execution(r: &RawTrade) -> Result<IbkrExecution, String> {
    let qty_signed: f64 = r
        .quantity
        .parse()
        .map_err(|e| format!("quantity {:?}: {e}", r.quantity))?;
    let qty = qty_signed.abs();
    let avg_price: f64 = r
        .trade_price
        .parse()
        .map_err(|e| format!("tradePrice {:?}: {e}", r.trade_price))?;
    let side = match r.buy_sell.as_str() {
        "BUY" => ExecutionSide::Bought,
        "SELL" => ExecutionSide::Sold,
        other => return Err(format!("unknown buy/sell {other:?}")),
    };
    // Flex `ibOrderID`s come from a different IBKR subsystem than the live
    // `reqExecutions` orderId space and routinely exceed `i32::MAX`. The
    // store/FIFO/event paths are all `i32`, and `order_id` is only load-
    // bearing for the `complex_strategy` heuristic (same order_id across
    // legs). Saturate overflows to 0 — the heuristic just won't fire on
    // those rows, which is acceptable for historical backfill.
    let order_id: i32 = r
        .ib_order_id
        .parse::<i64>()
        .map_err(|e| format!("ibOrderID {:?}: {e}", r.ib_order_id))?
        .try_into()
        .unwrap_or(0);
    let commission: Option<f64> = r
        .ib_commission
        .as_deref()
        .map(|s| {
            s.parse::<f64>()
                .map_err(|e| format!("ibCommission {s:?}: {e}"))
        })
        .transpose()?
        // Flex reports commissions as signed negative; flip to the
        // store's "magnitude" convention used by the live ingestor.
        .map(|c| -c);
    let realized_pnl: Option<f64> = r
        .fifo_pnl_realized
        .as_deref()
        .map(|s| {
            s.parse::<f64>()
                .map_err(|e| format!("fifoPnlRealized {s:?}: {e}"))
        })
        .transpose()?;
    let exec_time = parse_flex_datetime(&r.date_time)?;
    let symbol = base_symbol(&r.symbol, &r.asset_category);
    let (expiry, strike, right, multiplier) = if r.asset_category == "OPT" {
        (
            r.expiry
                .as_deref()
                .map(parse_date)
                .transpose()
                .map_err(|e| format!("expiry: {e}"))?,
            r.strike
                .as_deref()
                .map(|s| s.parse::<f64>().map_err(|e| format!("strike {s:?}: {e}")))
                .transpose()?,
            r.put_call.clone(),
            r.multiplier.clone(),
        )
    } else {
        (None, None, None, None)
    };

    Ok(IbkrExecution {
        symbol,
        side,
        qty,
        avg_price,
        exec_time,
        order_id,
        // Prefix backfill exec_ids so they can never collide with a
        // live `reqExecutions` execId.
        exec_id: format!("flex:{}", r.trade_id),
        account: r.account_id.clone(),
        contract_type: r.asset_category.clone(),
        expiry,
        strike,
        right,
        multiplier,
        commission,
        realized_pnl,
        currency: r.currency.clone(),
        commission_currency: r.ib_commission_currency.clone(),
    })
}

/// Flex `dateTime` ships as `yyyy-MM-dd;HH:mm:ss` (semicolon separator
/// per the Flex Query template). Treat the wall time as ET, convert
/// to UTC for storage.
fn parse_flex_datetime(s: &str) -> Result<DateTime<Utc>, String> {
    let normalised = s.replace(';', " ");
    let naive = NaiveDateTime::parse_from_str(&normalised, "%Y-%m-%d %H:%M:%S")
        .map_err(|e| format!("dateTime {s:?}: {e}"))?;
    let et = naive
        .and_local_timezone(New_York)
        .single()
        .ok_or_else(|| format!("ambiguous ET datetime {s:?} (DST)"))?;
    Ok(et.with_timezone(&Utc))
}

/// OPT symbols in Flex are OCC-padded (`"AMD   251010P00225000"`).
/// Take the leading non-space token; trust the explicit `expiry` /
/// `strike` / `putCall` / `multiplier` attributes for the rest.
fn base_symbol(raw: &str, asset_category: &str) -> String {
    if asset_category == "OPT" {
        raw.split_whitespace().next().unwrap_or(raw).to_string()
    } else {
        raw.to_string()
    }
}

// ---- Stats -------------------------------------------------------------

fn print_stats(rows: &[IbkrExecution]) {
    if rows.is_empty() {
        println!("0 mapped rows; nothing to insert.");
        return;
    }
    let mut by_currency: BTreeSet<String> = BTreeSet::new();
    let mut by_category: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_account: BTreeSet<String> = BTreeSet::new();
    let mut by_day: BTreeMap<NaiveDate, usize> = BTreeMap::new();
    let mut min_t = rows[0].exec_time;
    let mut max_t = rows[0].exec_time;
    for r in rows {
        if let Some(c) = &r.currency {
            by_currency.insert(c.clone());
        }
        *by_category.entry(r.contract_type.clone()).or_default() += 1;
        by_account.insert(r.account.clone());
        let day = r.exec_time.with_timezone(&New_York).date_naive();
        *by_day.entry(day).or_default() += 1;
        if r.exec_time < min_t {
            min_t = r.exec_time;
        }
        if r.exec_time > max_t {
            max_t = r.exec_time;
        }
    }
    println!("--- stats ---");
    println!("rows: {}", rows.len());
    println!(
        "ET range: {} .. {}",
        min_t.with_timezone(&New_York).date_naive(),
        max_t.with_timezone(&New_York).date_naive(),
    );
    println!(
        "accounts: {}",
        by_account.iter().cloned().collect::<Vec<_>>().join(", "),
    );
    println!(
        "currencies: {}",
        by_currency.iter().cloned().collect::<Vec<_>>().join(", "),
    );
    let cats: Vec<String> = by_category
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    println!("asset categories: {}", cats.join(", "));
    println!("trading days touched: {}", by_day.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<FlexQueryResponse>
  <FlexStatements>
    <FlexStatement>
      <Trades>
        <Trade accountId="U1" currency="USD" assetCategory="STK" subCategory="ADR"
               symbol="BILI" tradeID="100" multiplier="1" strike="" expiry=""
               dateTime="2026-01-20;13:16:00" putCall="" quantity="30" tradePrice="30.495"
               ibCommission="-1" ibCommissionCurrency="USD" notes=""
               fifoPnlRealized="0" buySell="BUY" ibOrderID="42" ibExecID="x.x.01.01" />
        <Trade accountId="U1" currency="USD" assetCategory="OPT" subCategory="P"
               symbol="AMD   251010P00225000" tradeID="200" multiplier="100" strike="225"
               expiry="2025-10-10" dateTime="2025-10-08;13:06:38" putCall="P"
               quantity="1" tradePrice="4.08" ibCommission="-1.0459"
               ibCommissionCurrency="USD" notes="P" fifoPnlRealized="0" buySell="BUY"
               ibOrderID="43" ibExecID="y.y.01.01" />
        <Trade accountId="U1" currency="USD" assetCategory="CASH" subCategory=""
               symbol="EUR.USD" tradeID="300" multiplier="1" strike="" expiry=""
               dateTime="2025-05-08;11:23:26" putCall="" quantity="-29.6"
               tradePrice="1.12665" ibCommission="0" ibCommissionCurrency="EUR"
               notes="AFx" fifoPnlRealized="0" buySell="SELL" ibOrderID="44" ibExecID="z" />
      </Trades>
    </FlexStatement>
  </FlexStatements>
</FlexQueryResponse>
"#;

    #[test]
    fn parses_three_trades() {
        let rows = parse_trades(SAMPLE_XML).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].symbol, "BILI");
        assert_eq!(rows[1].symbol, "AMD   251010P00225000");
        assert_eq!(rows[2].asset_category, "CASH");
    }

    #[test]
    fn cash_filter_drops_all_cash_rows() {
        let rows = parse_trades(SAMPLE_XML).unwrap();
        let kept: Vec<_> = rows.iter().filter(|r| r.asset_category != "CASH").collect();
        assert_eq!(kept.len(), 2);
        assert!(!kept.iter().any(|r| r.asset_category == "CASH"));
    }

    #[test]
    fn maps_stk_row_with_flipped_commission_sign() {
        let rows = parse_trades(SAMPLE_XML).unwrap();
        let exec = map_to_execution(&rows[0]).unwrap();
        assert_eq!(exec.symbol, "BILI");
        assert_eq!(exec.contract_type, "STK");
        assert!(matches!(exec.side, ExecutionSide::Bought));
        assert_eq!(exec.qty, 30.0);
        assert_eq!(exec.avg_price, 30.495);
        assert_eq!(exec.commission, Some(1.0)); // -(-1.0)
        assert_eq!(exec.exec_id, "flex:100");
        assert!(exec.expiry.is_none());
    }

    #[test]
    fn maps_opt_row_with_strike_expiry_and_base_symbol() {
        let rows = parse_trades(SAMPLE_XML).unwrap();
        let exec = map_to_execution(&rows[1]).unwrap();
        assert_eq!(exec.symbol, "AMD"); // OCC padding stripped
        assert_eq!(exec.contract_type, "OPT");
        assert_eq!(exec.strike, Some(225.0));
        assert_eq!(exec.right.as_deref(), Some("P"));
        assert_eq!(exec.multiplier.as_deref(), Some("100"));
        assert_eq!(
            exec.expiry,
            Some(NaiveDate::from_ymd_opt(2025, 10, 10).unwrap()),
        );
        assert_eq!(exec.exec_id, "flex:200");
    }

    #[test]
    fn parses_dt_as_et_then_converts_to_utc() {
        // 2026-01-20 13:16:00 ET (EST, UTC-5) → 18:16:00 UTC
        let dt = parse_flex_datetime("2026-01-20;13:16:00").unwrap();
        assert_eq!(
            dt.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            "2026-01-20T18:16:00Z"
        );
    }

    #[test]
    fn extract_tag_finds_simple_xml_text() {
        let xml = "<a><Status>Success</Status><ReferenceCode>123</ReferenceCode></a>";
        assert_eq!(extract_tag(xml, "Status").as_deref(), Some("Success"));
        assert_eq!(extract_tag(xml, "ReferenceCode").as_deref(), Some("123"));
        assert_eq!(extract_tag(xml, "Missing"), None);
    }
}
