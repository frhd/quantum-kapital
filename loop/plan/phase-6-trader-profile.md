# Phase 6 — `get_trader_profile` MCP tool + behavioral feedback wired into `morning_sweep.py`

> Part of [Behavioral assessment via MCP](master.md). See master for invariants.

**Status:** done (commit e60e993, 2026-05-05)

**Depends on:** 4 (`day_reviews` rows are the data source), 5 (`morning_sweep.py`'s playbook step is the consumer)

**Goal:** Ship `get_trader_profile(window_days?, account?)` — a pure SQL aggregate over the last N `day_reviews` returning `{tag_frequencies, pnl_by_tag, trendline, n_reviews, since_date}`. Then wire `morning_sweep.py` to load the profile at startup and pass it to the playbook generator's prompt, where it conditions today's `ranked_setups` and `skip_list` on the trader's behavioral history. **This is the moat.** A playbook that knows "the trader chased TSLA 0DTE 3 of the last 7 days" can put TSLA in `skip_list` proactively.

**Why this matters:** every other "AI trading assistant" gives generic setups. After Phase 6 ships, QK's playbook is **trader-aware**. After 60 trading days of `day_reviews`, the profile has statistical power; the playbook starts steering the trader away from their own worst patterns BEFORE they fire.

## End-state for this phase

- `mcp/tools/get_trader_profile.rs` ships a pure-SQL aggregator (no LLM, no IBKR).
- The tool returns:
  ```jsonc
  {
    "account": "U4393159",
    "window_days": 30,
    "since_date": "2026-04-05",
    "n_reviews": 22,
    "tag_frequencies": [
      { "tag": "flat_close", "count": 18, "pct_of_reviews": 0.82 },
      { "tag": "chase_own_exit", "count": 7, "pct_of_reviews": 0.32 },
      { "tag": "discipline_on_loser", "count": 14, "pct_of_reviews": 0.64 },
      // ...
    ],
    "pnl_by_tag": [
      { "tag": "discipline_on_loser", "n_days": 14, "net_pnl_total": 4250.0, "net_pnl_per_day_avg": 303.6 },
      { "tag": "chase_own_exit", "n_days": 7, "net_pnl_total": -1820.0, "net_pnl_per_day_avg": -260.0 },
      // ...
    ],
    "trendline": {
      "last_7d":   { "n_reviews": 5, "tag_counts": {"chase_own_exit": 3, "flat_close": 5, ...}, "net_pnl": 1240.0, "avg_grade_score": 12.4 },
      "prior_21d": { "n_reviews": 17, "tag_counts": {"chase_own_exit": 4, "flat_close": 13, ...}, "net_pnl": 3010.0, "avg_grade_score": 14.1 }
    },
    "recent_incidents": [
      { "date": "2026-05-04", "symbol": "TSLA", "tag": "chase_own_exit", "leg_observation": "Re-entered 395C at $2.50 within 2 min of selling at $2.45" },
      // ...
    ]
  }
  ```
- `morning_sweep.py` calls `get_trader_profile(window_days=30)` at start (between budget check and bundle gather). The result is passed into `format_playbook_prompt(bundles, trader_profile)` (Phase 5 left the placeholder in).
- The playbook system prompt (`agent/prompts/playbook.md`) gains an explicit instruction block referencing the profile and how to use it for `skip_list` decisions.
- Tracer-bullet test: seeded `day_reviews` with `chase_own_exit` on TSLA in 3 of last 7 days ⇒ the resulting playbook puts TSLA in `skip_list` with a reason citing the pattern.

## Files

**Create:**
- `src-tauri/src/services/trader_profile/mod.rs` — module root.
- `src-tauri/src/services/trader_profile/aggregator.rs` — pure SQL aggregator.
- `src-tauri/src/services/trader_profile/types.rs` — `TraderProfile`, `TagFrequency`, `PnlByTag`, `Trendline`, `RecentIncident`.
- `src-tauri/src/services/trader_profile/tests.rs` — unit tests with seeded reviews.
- `src-tauri/src/mcp/tools/get_trader_profile.rs` — read tool.

**Modify:**
- `src-tauri/src/services/mod.rs` — `pub mod trader_profile;`.
- `src-tauri/src/mcp/tools/mod.rs`, `src-tauri/src/mcp/handler.rs` — register router.
- `agent/morning_sweep.py` — fetch profile, pass to playbook prompt formatter.
- `agent/playbook.py` — `format_playbook_prompt` now interpolates the profile section.
- `agent/prompts/playbook.md` — add the "TRADER PROFILE" instruction block.
- `agent/mcp_client.py` — wrapper for `get_trader_profile`.
- `agent/tests/test_morning_sweep_playbook.py` — extended with the moat test.

## End-state types

```rust
// services/trader_profile/types.rs
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::services::trade_reviews::tags::BehavioralTag;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagFrequency {
    pub tag: BehavioralTag,
    pub count: i64,
    pub pct_of_reviews: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PnlByTag {
    pub tag: BehavioralTag,
    pub n_days: i64,
    pub net_pnl_total: f64,
    pub net_pnl_per_day_avg: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowSummary {
    pub n_reviews: i64,
    pub tag_counts: std::collections::BTreeMap<String, i64>,
    pub net_pnl: f64,
    pub avg_grade_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trendline {
    pub last_7d: WindowSummary,
    pub prior_21d: WindowSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentIncident {
    pub date: NaiveDate,
    pub symbol: String,
    pub tag: BehavioralTag,
    pub leg_observation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraderProfile {
    pub account: String,
    pub window_days: u32,
    pub since_date: NaiveDate,
    pub n_reviews: i64,
    pub tag_frequencies: Vec<TagFrequency>,
    pub pnl_by_tag: Vec<PnlByTag>,
    pub trendline: Trendline,
    pub recent_incidents: Vec<RecentIncident>,
}
```

## Tasks

### Task 1: Aggregator skeleton + types

**Files:** `services/trader_profile/{mod,types}.rs`, `services/mod.rs`

- [ ] Create the types per the block above.
- [ ] Module root: `pub mod aggregator; pub mod types; pub use aggregator::aggregate;`
- [ ] Wire `pub mod trader_profile;` into `services/mod.rs`.

### Task 2: Failing test for empty store

**Files:** `services/trader_profile/tests.rs`

- [ ] **Step 1: Write the test**

```rust
#[tokio::test]
async fn aggregator_empty_store_returns_zero_review_profile() {
    let (_tmp, db) = make_db();
    let p = aggregate(&db, "U1", 30).await.expect("ok");
    assert_eq!(p.n_reviews, 0);
    assert!(p.tag_frequencies.is_empty());
    assert!(p.pnl_by_tag.is_empty());
    assert_eq!(p.trendline.last_7d.n_reviews, 0);
    assert_eq!(p.trendline.prior_21d.n_reviews, 0);
    assert!(p.recent_incidents.is_empty());
}
```

- [ ] **Step 2: Run, verify it fails** (no `aggregate` function yet).

### Task 3: Implement `aggregate`

**Files:** `services/trader_profile/aggregator.rs`

- [ ] **Step 1: Implement** — pure SQL queries against `day_reviews`. Use a single connection acquisition with multiple statements; no LLM, no IBKR, no IO outside the DB pool.

```rust
//! `aggregate` — read the last N day_reviews and produce a TraderProfile.
//! Pure SQL aggregation over the day_reviews table.

use chrono::{Duration, NaiveDate, Utc};
use chrono_tz::America::New_York;
use rusqlite::params;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::services::trade_reviews::tags::BehavioralTag;
use crate::storage::Db;
use super::types::*;

pub async fn aggregate(
    db: &Arc<Db>,
    account: &str,
    window_days: u32,
) -> Result<TraderProfile, rusqlite::Error> {
    let today_et = Utc::now().with_timezone(&New_York).date_naive();
    let since = today_et - Duration::days(window_days as i64);
    let last_7d_since = today_et - Duration::days(7);
    let prior_21d_since = today_et - Duration::days(28);
    let prior_21d_until = today_et - Duration::days(7);

    let account = account.to_string();
    let db = Arc::clone(db);
    tokio::task::spawn_blocking(move || {
        let conn = db.get().expect("db conn");

        // Pull every relevant review row in one scan.
        let mut stmt = conn.prepare(
            "SELECT date, behavioral_tags, leg_observations, net_pnl, grade_score
             FROM day_reviews
             WHERE account = ?1 AND date >= ?2
             ORDER BY date DESC",
        )?;
        let rows = stmt
            .query_map(params![account, since.to_string()], |r| {
                Ok((
                    r.get::<_, String>(0)?,                  // date
                    r.get::<_, String>(1)?,                  // behavioral_tags JSON
                    r.get::<_, String>(2)?,                  // leg_observations JSON
                    r.get::<_, f64>(3)?,                     // net_pnl
                    r.get::<_, f64>(4)?,                     // grade_score
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let n_reviews = rows.len() as i64;

        // Tag frequencies.
        let mut tag_count: BTreeMap<String, i64> = BTreeMap::new();
        let mut pnl_per_tag: BTreeMap<String, (i64, f64)> = BTreeMap::new(); // (n_days, sum_pnl)
        let mut last_7d_pnl = 0.0_f64;
        let mut last_7d_score_sum = 0.0_f64;
        let mut last_7d_n = 0_i64;
        let mut prior_21d_pnl = 0.0_f64;
        let mut prior_21d_score_sum = 0.0_f64;
        let mut prior_21d_n = 0_i64;
        let mut last_7d_tags: BTreeMap<String, i64> = BTreeMap::new();
        let mut prior_21d_tags: BTreeMap<String, i64> = BTreeMap::new();
        let mut incidents: Vec<RecentIncident> = Vec::new();

        for (date_str, tags_json, legs_json, net_pnl, grade_score) in &rows {
            let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").expect("date");
            let tags: Vec<String> = serde_json::from_str(tags_json).unwrap_or_default();

            for t in &tags {
                *tag_count.entry(t.clone()).or_insert(0) += 1;
                let entry = pnl_per_tag.entry(t.clone()).or_insert((0, 0.0));
                entry.0 += 1;
                entry.1 += *net_pnl;
            }

            if date >= last_7d_since {
                last_7d_n += 1;
                last_7d_pnl += *net_pnl;
                last_7d_score_sum += *grade_score;
                for t in &tags {
                    *last_7d_tags.entry(t.clone()).or_insert(0) += 1;
                }
            } else if date >= prior_21d_since && date < prior_21d_until {
                prior_21d_n += 1;
                prior_21d_pnl += *net_pnl;
                prior_21d_score_sum += *grade_score;
                for t in &tags {
                    *prior_21d_tags.entry(t.clone()).or_insert(0) += 1;
                }
            }

            // Recent incidents — only from last 7d, only the per-leg observations
            // that carry a tag.
            if date >= last_7d_since && incidents.len() < 10 {
                if let Ok(legs) = serde_json::from_str::<serde_json::Value>(legs_json) {
                    if let Some(arr) = legs.as_array() {
                        for leg in arr {
                            if let (Some(symbol), Some(observation), Some(tag_str)) = (
                                leg.get("leg_id").and_then(|v| v.as_str()),
                                leg.get("observation_md").and_then(|v| v.as_str()),
                                leg.get("tag").and_then(|v| v.as_str()),
                            ) {
                                if let Ok(tag) = serde_json::from_value::<BehavioralTag>(serde_json::Value::String(tag_str.to_string())) {
                                    incidents.push(RecentIncident {
                                        date,
                                        symbol: symbol.to_string(),
                                        tag,
                                        leg_observation: observation.to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        let mut tag_frequencies: Vec<TagFrequency> = tag_count
            .iter()
            .filter_map(|(name, count)| {
                let parsed: Result<BehavioralTag, _> =
                    serde_json::from_value(serde_json::Value::String(name.clone()));
                parsed.ok().map(|tag| TagFrequency {
                    tag,
                    count: *count,
                    pct_of_reviews: if n_reviews > 0 {
                        *count as f64 / n_reviews as f64
                    } else {
                        0.0
                    },
                })
            })
            .collect();
        tag_frequencies.sort_by(|a, b| b.count.cmp(&a.count));

        let mut pnl_by_tag: Vec<PnlByTag> = pnl_per_tag
            .iter()
            .filter_map(|(name, (n, total))| {
                let parsed: Result<BehavioralTag, _> =
                    serde_json::from_value(serde_json::Value::String(name.clone()));
                parsed.ok().map(|tag| PnlByTag {
                    tag,
                    n_days: *n,
                    net_pnl_total: *total,
                    net_pnl_per_day_avg: if *n > 0 { *total / *n as f64 } else { 0.0 },
                })
            })
            .collect();
        pnl_by_tag.sort_by(|a, b| b.net_pnl_total.partial_cmp(&a.net_pnl_total).unwrap_or(std::cmp::Ordering::Equal));

        let trendline = Trendline {
            last_7d: WindowSummary {
                n_reviews: last_7d_n,
                tag_counts: last_7d_tags,
                net_pnl: last_7d_pnl,
                avg_grade_score: if last_7d_n > 0 { last_7d_score_sum / last_7d_n as f64 } else { 0.0 },
            },
            prior_21d: WindowSummary {
                n_reviews: prior_21d_n,
                tag_counts: prior_21d_tags,
                net_pnl: prior_21d_pnl,
                avg_grade_score: if prior_21d_n > 0 { prior_21d_score_sum / prior_21d_n as f64 } else { 0.0 },
            },
        };

        Ok(TraderProfile {
            account,
            window_days,
            since_date: since,
            n_reviews,
            tag_frequencies,
            pnl_by_tag,
            trendline,
            recent_incidents: incidents,
        })
    })
    .await
    .expect("blocking task")
}
```

> **Caveat on `recent_incidents`.** The leg_observations JSON may use `leg_id` as the symbol surrogate or store the symbol explicitly — depends on Phase 4's chosen shape. Adapt the field reads above accordingly. The Phase 4 spec stores `LegObservation { leg_id, observation_md, tag? }` — to surface symbols in incidents, either extend `LegObservation` to include `symbol` (the cleaner path) OR join via the `trade_legs` derivation when reading. v1 takes the former; bump Phase 4's `LegObservation` to include `symbol` if you haven't already.

- [ ] **Step 2: Run the empty-store test** — should pass now.

- [ ] **Step 3: Commit**: `feat(trader_profile): SQL aggregator over day_reviews`.

### Task 4: Tests with seeded reviews

**Files:** `services/trader_profile/tests.rs`

- [ ] **Step 1: Helper to seed a review row directly via SQL** (avoids depending on the full eod_review chain in tests).

```rust
async fn seed_review(
    db: &Arc<Db>,
    date: NaiveDate,
    account: &str,
    tags: &[BehavioralTag],
    net_pnl: f64,
    grade_score: f64,
) {
    let conn = db.get().unwrap();
    let tags_json = serde_json::to_string(tags).unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO day_reviews (
            date, account, prompt_version, generated_at, grade, grade_score,
            gross_pnl, net_pnl, commissions_total, n_round_trips, n_carryover,
            win_rate, behavioral_tags, leg_observations, narrative_md
         ) VALUES (?1, ?2, 1, ?3, 'C', ?4, ?5, ?5, 0.0, 0, 0, NULL, ?6, '[]', '')",
        params![date.to_string(), account, Utc::now().to_rfc3339(), grade_score, net_pnl, tags_json],
    ).unwrap();
}

#[tokio::test]
async fn aggregator_counts_tags_across_window() {
    let (_tmp, db) = make_db();
    let today = Utc::now().with_timezone(&chrono_tz::America::New_York).date_naive();
    seed_review(&db, today - Duration::days(1), "U1", &[BehavioralTag::FlatClose, BehavioralTag::ChaseOwnExit], 100.0, 5.0).await;
    seed_review(&db, today - Duration::days(2), "U1", &[BehavioralTag::FlatClose], 200.0, 10.0).await;
    seed_review(&db, today - Duration::days(3), "U1", &[BehavioralTag::ChaseOwnExit, BehavioralTag::DisciplineOnLoser], -50.0, -3.0).await;

    let p = aggregate(&db, "U1", 30).await.unwrap();
    assert_eq!(p.n_reviews, 3);

    let flat = p.tag_frequencies.iter().find(|f| matches!(f.tag, BehavioralTag::FlatClose)).expect("flat_close");
    assert_eq!(flat.count, 2);
    assert!((flat.pct_of_reviews - 2.0/3.0).abs() < 1e-9);

    let chase = p.tag_frequencies.iter().find(|f| matches!(f.tag, BehavioralTag::ChaseOwnExit)).expect("chase");
    assert_eq!(chase.count, 2);
}

#[tokio::test]
async fn aggregator_isolates_account_window() {
    let (_tmp, db) = make_db();
    let today = Utc::now().with_timezone(&chrono_tz::America::New_York).date_naive();
    seed_review(&db, today - Duration::days(1), "U1", &[BehavioralTag::FlatClose], 100.0, 5.0).await;
    seed_review(&db, today - Duration::days(1), "U2", &[BehavioralTag::ChaseOwnExit], -100.0, -10.0).await;

    let p1 = aggregate(&db, "U1", 30).await.unwrap();
    assert_eq!(p1.n_reviews, 1);
    assert!(p1.tag_frequencies.iter().all(|f| !matches!(f.tag, BehavioralTag::ChaseOwnExit)));
}

#[tokio::test]
async fn aggregator_trendline_splits_last_7_vs_prior_21() {
    let (_tmp, db) = make_db();
    let today = Utc::now().with_timezone(&chrono_tz::America::New_York).date_naive();
    // 5 reviews in last 7d
    for d in 1..=5 {
        seed_review(&db, today - Duration::days(d), "U1", &[BehavioralTag::FlatClose], 50.0, 5.0).await;
    }
    // 10 reviews in prior 21d (days 8–17)
    for d in 8..=17 {
        seed_review(&db, today - Duration::days(d), "U1", &[BehavioralTag::FlatClose], 30.0, 3.0).await;
    }
    let p = aggregate(&db, "U1", 30).await.unwrap();
    assert_eq!(p.trendline.last_7d.n_reviews, 5);
    assert_eq!(p.trendline.prior_21d.n_reviews, 10);
    assert!((p.trendline.last_7d.net_pnl - 250.0).abs() < 1e-9);
    assert!((p.trendline.prior_21d.net_pnl - 300.0).abs() < 1e-9);
}
```

- [ ] **Step 2: Run the suite**, fix any off-by-one in the date math.

- [ ] **Step 3: Commit**: `test(trader_profile): seeded aggregator coverage`.

### Task 5: MCP read tool — `get_trader_profile`

**Files:** `mcp/tools/get_trader_profile.rs`

- [ ] Mirror `mcp/tools/get_morning_pack.rs` shape. Args: `{window_days?: u32, account?: String}`. Default `window_days = 30`. Returns the `TraderProfile` envelope. Read-only; no audit row.
- [ ] Write tests: empty store ⇒ `n_reviews: 0`; with seeded reviews ⇒ counts match; multi-account error wording matches `resolve_account`.
- [ ] Wire router in `mcp/handler.rs`.
- [ ] Commit: `feat(mcp): get_trader_profile read tool`.

### Task 6: Python — wrapper + morning_sweep wire-up

**Files:** `agent/mcp_client.py`, `agent/morning_sweep.py`, `agent/playbook.py`, `agent/prompts/playbook.md`

- [ ] **Step 1: Wrapper**

```python
# agent/mcp_client.py
async def get_trader_profile(self, *, window_days: int = 30, account: str | None = None) -> Any:
    args: dict = {"window_days": window_days}
    if account: args["account"] = account
    return await self.call_tool("get_trader_profile", args)
```

- [ ] **Step 2: Fetch the profile in `run_sweep`**

In `morning_sweep.py`, after the budget check and BEFORE bundle gather:

```python
trader_profile: dict | None
try:
    trader_profile = await mcp.get_trader_profile(window_days=cfg.profile_window_days)
    if not isinstance(trader_profile, dict) or trader_profile.get("n_reviews", 0) == 0:
        log.info("trader profile empty (no reviews yet); proceeding without behavioral conditioning")
        trader_profile = None
except Exception:  # noqa: BLE001
    log.exception("get_trader_profile failed; proceeding without")
    trader_profile = None
```

`cfg.profile_window_days` defaults to 30 (add to `agent/config.toml` and `agent/config.py`).

- [ ] **Step 3: Pass into `format_playbook_prompt`**

```python
playbook_prompt = format_playbook_prompt(bundles=bundles, trader_profile=trader_profile)
```

In `agent/playbook.py`, implement `format_playbook_prompt`:

```python
def format_playbook_prompt(*, bundles, trader_profile) -> str:
    lines: list[str] = []
    if trader_profile:
        lines.append("## TRADER PROFILE")
        lines.append(f"Reviews considered: {trader_profile['n_reviews']} (since {trader_profile['since_date']})")
        if trader_profile.get("tag_frequencies"):
            lines.append("\nMost frequent behavioral tags (last {}d):".format(trader_profile["window_days"]))
            for tf in trader_profile["tag_frequencies"][:6]:
                lines.append(f"  - {tf['tag']}: {tf['count']} times ({tf['pct_of_reviews']*100:.0f}% of reviews)")
        last_7 = trader_profile["trendline"]["last_7d"]
        prior_21 = trader_profile["trendline"]["prior_21d"]
        lines.append(f"\nTrend: last 7d net P&L ${last_7['net_pnl']:.0f} (avg score {last_7['avg_grade_score']:.1f}); "
                     f"prior 21d net P&L ${prior_21['net_pnl']:.0f} (avg score {prior_21['avg_grade_score']:.1f}).")
        if trader_profile.get("recent_incidents"):
            lines.append("\nRecent behavioral incidents (last 7d):")
            for inc in trader_profile["recent_incidents"][:5]:
                lines.append(f"  - {inc['date']} {inc['symbol']}: {inc['tag']} — {inc['leg_observation']}")
        lines.append("")
    lines.append("## WATCHLIST BRIEFING")
    for b in bundles:
        # render bundle compactly — reuse data_summary helpers from the existing
        # synthesizer.py path.
        lines.append(f"### {b.symbol}")
        lines.append(b.daily_summary)
        if b.news_summary:
            lines.append(f"News: {b.news_summary}")
        # ...etc...
        lines.append("")
    return "\n".join(lines)
```

- [ ] **Step 4: Update `agent/prompts/playbook.md`** — add the trader-profile instruction block at the end:

```markdown
## USING THE TRADER PROFILE

If a `## TRADER PROFILE` section appears at the top of the prompt:

1. Read the recent behavioral incidents and tag frequencies. They tell you what the trader did wrong (and right) recently.
2. For any symbol with ≥3 occurrences of a negative-weight tag (`chase_own_exit`, `late_otm_lottery`, `post_loss_revenge`, `gamma_window_violation`, `position_sizing_ungraduated`, `scaled_in_loser`) in the last 7 days: PUT IT IN `skip_list` with a reason that names the pattern explicitly. Example: `{"symbol": "TSLA", "reason": "recent chase_own_exit pattern (3 of last 7 days)"}`.
3. For symbols with ≥2 occurrences of `thesis_match_executed` in the last 7 days: keep them in the candidate pool — the trader executes well on these.
4. The skip list is HOW the system protects the trader from their own worst tendencies. Use it.
```

- [ ] **Step 5: Commit**: `feat(agent): wire trader_profile into morning_sweep playbook step`.

### Task 7: The MOAT test (tracer-bullet)

**Files:** `agent/tests/test_morning_sweep_playbook.py`

- [ ] **Step 1: Write the test**

```python
"""The moat: with 3 chase_own_exit incidents on TSLA in last 7 days,
the playbook generator must surface TSLA in skip_list with a reason
referencing the pattern. Without those incidents, it must NOT."""

import pytest

from morning_sweep import run_sweep
# ... fakes/fixtures imports ...


@pytest.mark.asyncio
async def test_playbook_skips_tsla_when_recent_chase_pattern(fake_mcp, fake_llm, cfg):
    # Seed a trader_profile with 3 chase_own_exit on TSLA in last 7 days.
    fake_mcp.trader_profile = {
        "account": "U1",
        "window_days": 30,
        "since_date": "2026-04-05",
        "n_reviews": 7,
        "tag_frequencies": [
            {"tag": "chase_own_exit", "count": 3, "pct_of_reviews": 3/7},
            {"tag": "flat_close", "count": 5, "pct_of_reviews": 5/7},
        ],
        "pnl_by_tag": [],
        "trendline": {
            "last_7d": {"n_reviews": 5, "tag_counts": {"chase_own_exit": 3, "flat_close": 5}, "net_pnl": -200.0, "avg_grade_score": -2.0},
            "prior_21d": {"n_reviews": 0, "tag_counts": {}, "net_pnl": 0.0, "avg_grade_score": 0.0},
        },
        "recent_incidents": [
            {"date": "2026-05-04", "symbol": "TSLA", "tag": "chase_own_exit", "leg_observation": "..."},
            {"date": "2026-05-03", "symbol": "TSLA", "tag": "chase_own_exit", "leg_observation": "..."},
            {"date": "2026-05-02", "symbol": "TSLA", "tag": "chase_own_exit", "leg_observation": "..."},
        ],
    }
    # Seed watchlist with TSLA.
    fake_mcp.watchlist = [{"symbol": "TSLA"}, {"symbol": "AMD"}]
    # ... seed bundles ...

    # Capture the playbook prompt the LLM sees.
    captured_prompts = []
    fake_llm.capture_user_messages = captured_prompts

    # Pre-canned LLM response: a playbook that has TSLA in skip_list.
    fake_llm.set_playbook_response({
        "ranked_setups": [
            {"symbol": "AMD", "bias": "long", "trigger": "...", "entry": "...",
             "invalidation": "...", "target_1": "...", "conviction": "B",
             "rationale_md": "..."}
        ],
        "skip_list": [
            {"symbol": "TSLA", "reason": "recent chase_own_exit pattern (3 of last 7 days)"}
        ]
    })

    await run_sweep(mcp=fake_mcp, llm=fake_llm, cfg=cfg, today=date.today())

    # Assert the prompt the LLM saw included the trader profile section.
    last_prompt = captured_prompts[-1]
    assert "TRADER PROFILE" in last_prompt
    assert "chase_own_exit" in last_prompt
    assert "TSLA" in last_prompt

    # Assert write_playbook was called with TSLA in skip_list.
    written = fake_mcp.last_write_playbook_args
    skip_syms = {e["symbol"] for e in written["skip_list"]}
    assert "TSLA" in skip_syms
```

- [ ] **Step 2: Run** (`uv run pytest tests/test_morning_sweep_playbook.py::test_playbook_skips_tsla_when_recent_chase_pattern`).

- [ ] **Step 3: Commit**: `test(agent): moat — playbook conditions on trader profile`.

### Task 8: Tracer-bullet (live, on real reviews)

After Phases 4 and 5 have been running for at least 5 trading days and have seeded real `day_reviews`:

- [ ] Manually seed (or wait for) 3 `chase_own_exit` incidents on a real symbol over the last 7 days.
- [ ] Run `agent/morning_sweep.py --dry-run` and inspect the captured prompt body. Verify the TRADER PROFILE block is present and the recent incidents are listed.
- [ ] Run the same without `--dry-run`. Open the desktop UI (Phase 7) or call `get_today_playbook(today)` directly. Confirm the symbol appears in `skip_list` with a reason citing the pattern.
- [ ] Document the observation in the master plan's exit notes.

## Exit criteria

- [ ] `aggregate` returns the correct counts/PnL/trendline for all unit-test fixtures.
- [ ] `get_trader_profile` MCP tool serves the `TraderProfile` envelope; no audit; no LLM cost.
- [ ] `morning_sweep.py` fetches and passes the profile; gracefully degrades when empty.
- [ ] `format_playbook_prompt` interpolates the profile section.
- [ ] System prompt includes the explicit skip_list instruction.
- [ ] Moat test passes (with-profile playbook surfaces the pattern in skip_list; without-profile baseline does not).
- [ ] Tracer-bullet on real data after 5+ trading days: the cron-driven playbook surfaces a real skip_list entry citing a real recent incident.
- [ ] Update master Phase 6 row + this Status header.

## Gotchas

- **Empty profile is the default.** First-time install: `n_reviews = 0`. The morning_sweep MUST handle this without erroring; the playbook just runs without behavioral conditioning. Don't crash; don't refuse to write a playbook.
- **`recent_incidents` source.** This phase assumes Phase 4's `LegObservation` carries a `symbol`. If Phase 4 left it as `leg_id` only, you have two options: (a) bump Phase 4 to add `symbol` (cleaner — recommended), or (b) join from `leg_observations.leg_id` to a derived `trade_legs` view at read time (more code, same result). Pick (a) and bump Phase 4's `LegObservation` if you haven't already.
- **Window math + DST.** `today_et = Utc::now().with_timezone(&New_York).date_naive()`. The `Duration::days(7)` arithmetic is calendar days, not trading days — that's what we want (7 calendar days back covers the relevant trading window because `day_reviews` only have rows on trading days).
- **Tag-name ↔ enum drift.** The aggregator parses tag strings out of the JSON column and maps back to the `BehavioralTag` enum via `serde_json::from_value`. If a tag is added in v2 but the deserializer is older, those rows silently drop from frequency/PnL counts. Mitigation: bump `prompt_version` when the enum changes (Phase 4's existing rule); aggregator queries can optionally filter by version range.
- **Incidents > 10 are dropped.** v1 caps `recent_incidents` at 10 — enough to inform the LLM without bloating the prompt. If dogfooding shows that's too tight (e.g. very active trader generates 5+ incidents/day), bump to 20.
- **Concurrency.** SQLite single-writer means the aggregator's read coexists fine with concurrent writes from `eod_review.py`. The `spawn_blocking` discipline is the same as Phase 1.
- **Cost.** Zero LLM cost. The aggregator is pure SQL. Adding the profile section to the playbook prompt adds ~300 tokens to the morning_sweep call — well under cap.
- **Trendline windowing.** "last 7d" vs "prior 21d" are sensible defaults. v2 may make these configurable. Keep the field names stable so the UI panel (Phase 7) doesn't break when v2 ships.
- **Backfill.** Like every other artifact in this stack, the trader profile is forward-only. If you delete `day_reviews` rows manually, the aggregator reflects the deletion immediately (it's pure SQL — no cache).
