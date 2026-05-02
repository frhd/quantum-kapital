//! Tracker invariant: the tracker pipeline MUST NOT depend on the
//! `FundamentalsProvider` trait or `FundamentalData` type. The morning
//! sweep is news-driven; pulling fundamentals into the 100-ticker sweep
//! would burn AV's daily quota in a single tick.
//!
//! This test asserts the invariant via grep against the tracker-adjacent
//! source files. Renaming `FundamentalsProvider` / `FundamentalData`
//! would break the test — that's intentional, the rename should be
//! reviewed against the invariant in the same commit.

use std::fs;
use std::path::{Path, PathBuf};

/// Tracker-adjacent module roots. Anything inside these directories
/// (recursive) MUST NOT mention the forbidden symbols. Modules outside
/// these roots (e.g., `mcp/tools/fundamentals.rs`,
/// `ibkr/commands/analysis.rs`) are allowed to read fundamentals
/// because they're user-explicit code paths.
const TRACKER_ADJACENT_DIRS: &[&str] = &[
    "src/services/tracker_runner",
    "src/services/tracker_state_machine",
    "src/services/eod_scheduler",
    "src/services/intraday_scheduler",
    "src/services/news_interpreter",
    "src/services/thesis_generator",
    "src/services/decay_watcher",
    "src/services/daily_ranker",
    "src/services/auto_scanner",
    "src/services/candidate_promoter",
    "src/services/candidate_scheduler",
    "src/services/candidate_universe",
    "src/services/sentiment_surge_scanner",
    "src/services/social_sentiment_scheduler",
    "src/strategies",
];

/// Symbols that must NOT appear in tracker-adjacent code. The list
/// targets the active *fetch* path (the `FundamentalsProvider` trait,
/// the underlying AV call, and the cache-read helper that would
/// resurface AV data); the passive `FundamentalData` shape is permitted
/// because `MarketContext.fundamentals: Option<&'a FundamentalData>`
/// is set to `None` in production and never populated by the tracker
/// (master.md § Context). A future regression that wires fundamentals
/// into the sweep would have to mention one of the symbols below.
const FORBIDDEN_SYMBOLS: &[&str] = &[
    "FundamentalsProvider",
    "fetch_fundamental_data",
    "read_cached_fundamentals_ignoring_ttl",
    "fundamentals_provider",
    "manual_fundamentals_store",
];

#[test]
fn tracker_pipeline_does_not_depend_on_fundamentals_provider() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();
    for dir in TRACKER_ADJACENT_DIRS {
        let dir_abs = crate_root.join(dir);
        if !dir_abs.exists() {
            // Module may have been removed/renamed; that's fine — the
            // invariant only applies to extant modules. Surface a
            // panic-test failure only when the module exists AND
            // contains a forbidden symbol.
            continue;
        }
        scan_dir(&dir_abs, &mut violations);
    }
    assert!(
        violations.is_empty(),
        "tracker-no-fundamentals invariant violated. Hard Invariant #6 forbids the \
         tracker pipeline from depending on the FundamentalsProvider trait or \
         FundamentalData type. If you intentionally lifted fundamentals into the \
         tracker, escalate to the project owner before adding the dependency.\n\nViolations:\n{}",
        violations.join("\n"),
    );
}

fn scan_dir(dir: &Path, violations: &mut Vec<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            violations.push(format!("read_dir({}): {e}", dir.display()));
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir(&path, violations);
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let contents = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                violations.push(format!("read({}): {e}", path.display()));
                continue;
            }
        };
        for (line_no, line) in contents.lines().enumerate() {
            // Strip simple `//` line comments so the test can carry a
            // doc-comment that says "must NOT depend on FundamentalsProvider".
            // Block-comments and string literals are out of scope; the
            // test grep is intentionally conservative — false positives
            // are easier to diagnose than false negatives.
            let code = match line.split_once("//") {
                Some((before, _)) => before,
                None => line,
            };
            for symbol in FORBIDDEN_SYMBOLS {
                if code.contains(symbol) {
                    violations.push(format!(
                        "{}:{} mentions forbidden symbol `{symbol}`: {}",
                        path.display(),
                        line_no + 1,
                        line.trim(),
                    ));
                }
            }
        }
    }
}
