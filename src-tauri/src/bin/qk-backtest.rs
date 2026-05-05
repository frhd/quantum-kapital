//! `qk-backtest` — headless / overnight backtester.
//!
//! Reads a `BacktestSpec` from stdin (or a file via `--spec`),
//! opens the same `tracker.sqlite` the live app uses, replays bars
//! through the registered detectors, and writes the result back to
//! stdout (one-line JSON) plus persists into `backtest_runs` /
//! `backtest_trades`.
//!
//! Surveillance-only: never connects to IBKR. The bars must already
//! be cached by a prior live session or `tracker_fetch_bars` call.
//!
//! Usage:
//!   cargo run --bin qk-backtest -- --db /path/to/tracker.sqlite \
//!       --spec spec.json
//!   echo '{"date_from":"2025-01-01", ... }' | cargo run --bin qk-backtest

use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use quantum_kapital_lib::{
    registry_from_config, AttributionService, BacktestSpec, Backtester, CompositeEarningsCalendar,
    Db, DbBarsReader, DetectorsConfig, EarningsCacheStore, EarningsCalendar,
    EarningsOverridesStore, EventCalendarService, FomcCalendar, NoOpUpstream,
};

#[derive(Debug)]
struct Cli {
    db_path: PathBuf,
    spec_path: Option<PathBuf>,
    no_blackouts: bool,
    label: Option<String>,
}

fn parse_cli() -> Result<Cli, String> {
    let mut args = env::args().skip(1);
    let mut db_path: Option<PathBuf> = None;
    let mut spec_path: Option<PathBuf> = None;
    let mut no_blackouts = false;
    let mut label: Option<String> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--db" => {
                db_path = Some(PathBuf::from(args.next().ok_or("--db expects a path")?));
            }
            "--spec" => {
                spec_path = Some(PathBuf::from(args.next().ok_or("--spec expects a path")?));
            }
            "--no-blackouts" => no_blackouts = true,
            "--label" => {
                label = Some(args.next().ok_or("--label expects a string")?);
            }
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            other => return Err(format!("unknown arg: {other}")),
        }
    }
    let db_path = db_path.ok_or("--db is required (path to tracker.sqlite)")?;
    Ok(Cli {
        db_path,
        spec_path,
        no_blackouts,
        label,
    })
}

fn print_usage() {
    eprintln!(
        "qk-backtest --db <path> [--spec <path>] [--no-blackouts] [--label <str>]\n\n\
         Reads a BacktestSpec from --spec (or stdin), runs the backtest,\n\
         persists the result, and prints the result JSON on stdout."
    );
}

fn read_spec(spec_path: Option<&PathBuf>) -> Result<String, String> {
    match spec_path {
        Some(p) => fs::read_to_string(p).map_err(|e| format!("read {p:?}: {e}")),
        None => {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| format!("read stdin: {e}"))?;
            Ok(buf)
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt::init();

    let cli = match parse_cli() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            print_usage();
            return ExitCode::from(2);
        }
    };

    let spec_text = match read_spec(cli.spec_path.as_ref()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };

    let mut spec: BacktestSpec = match serde_json::from_str(&spec_text) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: parsing spec JSON: {e}");
            return ExitCode::from(2);
        }
    };
    if cli.no_blackouts {
        spec.event_blackouts_enabled = false;
    }
    if let Some(label) = cli.label {
        spec.label = Some(label);
    }

    let db = match Db::open(&cli.db_path) {
        Ok(db) => Arc::new(db),
        Err(e) => {
            eprintln!("error: open db {:?}: {e}", cli.db_path);
            return ExitCode::FAILURE;
        }
    };

    // Build the same dependency graph the live app does, minus the
    // IBKR client. The composite earnings calendar still works because
    // it's manual-overrides + cache only (NoOp upstream).
    let earnings_overrides = Arc::new(EarningsOverridesStore::new(Arc::clone(&db)));
    let earnings_cache = Arc::new(EarningsCacheStore::new(Arc::clone(&db)));
    let composite_earnings: Arc<dyn EarningsCalendar> = Arc::new(CompositeEarningsCalendar::new(
        Arc::clone(&earnings_overrides),
        Arc::clone(&earnings_cache),
        Arc::new(NoOpUpstream),
    ));
    let fomc_calendar = match FomcCalendar::from_embedded() {
        Ok(c) => Arc::new(c),
        Err(_) => Arc::new(FomcCalendar::from_dates(Vec::new())),
    };
    let event_calendar = Arc::new(
        EventCalendarService::new(composite_earnings, fomc_calendar)
            .with_cache(Arc::clone(&earnings_cache)),
    );

    let detectors_cfg = DetectorsConfig::default();
    let registry = Arc::new(registry_from_config(&detectors_cfg));
    let bt = Backtester::new(
        Arc::clone(&db),
        Arc::new(DbBarsReader::new(Arc::clone(&db))),
        registry,
        Arc::new(detectors_cfg),
    )
    .with_event_calendar(event_calendar)
    .with_tca_attribution(Arc::new(AttributionService::new(Arc::clone(&db))));

    match bt.run(spec).await {
        Ok(result) => {
            // result_json on stdout. trade list is verbose; the
            // operator usually wants the headline + the run_id to
            // re-query later via the Tauri command.
            let mut compact = result.clone();
            compact.trades.clear();
            match serde_json::to_string_pretty(&compact) {
                Ok(s) => println!("{s}"),
                Err(e) => eprintln!("warn: serialize result: {e}"),
            }
            eprintln!(
                "qk-backtest: run_id={} trades={} fired={} gated={} unsizable={}",
                result.run_id,
                result.trades.len(),
                result.n_setups_fired,
                result.n_setups_blackout_skipped,
                result.n_setups_unsizable,
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("qk-backtest: run failed: {e}");
            ExitCode::FAILURE
        }
    }
}
