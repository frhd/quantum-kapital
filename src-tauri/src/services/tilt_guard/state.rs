//! Phase 11 — `tilt_episodes` row store. One open row per account at
//! any time (enforced by `open_for_account` returning the latest
//! `released_at IS NULL` row); release flips `released_at`,
//! `release_kind`, and `release_reason`.

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::{params, OptionalExtension};

use crate::storage::error::Result as StorageResult;
use crate::storage::Db;
use crate::utils::market_calendar::et_offset;

use super::triggers::TriggerKind;

/// One row in `tilt_episodes`. Open rows have `released_at_unix IS NULL`.
#[derive(Debug, Clone, PartialEq)]
pub struct TiltEpisode {
    pub id: i64,
    pub account: String,
    pub triggered_at: DateTime<Utc>,
    pub trigger_kind: TriggerKind,
    /// Cumulative R at activation, scaled ×1000 (integer math). Read
    /// back as `r = cumulative_r_milli as f64 / 1000.0`.
    pub cumulative_r_milli: i64,
    pub consecutive_losses: u32,
    pub auto_reset_at: DateTime<Utc>,
    pub released_at: Option<DateTime<Utc>>,
    pub release_kind: Option<ReleaseKind>,
    pub release_reason: Option<String>,
}

/// How a tilt episode was released. Stored on
/// `tilt_episodes.release_kind` as `as_str()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseKind {
    /// Auto-reset at next session open (calendar-aware).
    Auto,
    /// Trader hit dismiss with a reason. Mirrored into
    /// `gate_overrides` with `gate_kind = 'tilt'`.
    ManualOverride,
    /// Defensive — when a stale open row from a prior session is
    /// closed by the next activation. Doesn't fire a fresh emit.
    SessionEnd,
}

impl ReleaseKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReleaseKind::Auto => "auto",
            ReleaseKind::ManualOverride => "manual_override",
            ReleaseKind::SessionEnd => "session_end",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(ReleaseKind::Auto),
            "manual_override" => Some(ReleaseKind::ManualOverride),
            "session_end" => Some(ReleaseKind::SessionEnd),
            _ => None,
        }
    }
}

/// Insert payload — id is auto-assigned by SQLite.
#[derive(Debug, Clone)]
pub struct NewTiltEpisode {
    pub account: String,
    pub triggered_at: DateTime<Utc>,
    pub trigger_kind: TriggerKind,
    pub cumulative_r_milli: i64,
    pub consecutive_losses: u32,
    pub auto_reset_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct TiltEpisodeStore {
    db: Arc<Db>,
}

impl TiltEpisodeStore {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    /// Latest open episode for `account` (released_at IS NULL).
    pub async fn open_for_account(&self, account: &str) -> StorageResult<Option<TiltEpisode>> {
        let account = account.to_string();
        self.db
            .with_conn(move |conn| {
                conn.query_row(SELECT_COLUMNS_OPEN, params![account], map_row)
                    .optional()
                    .map_err(Into::into)
            })
            .await
    }

    /// Latest released-or-open episode whose `released_at` falls on
    /// the given ET date. Used for the day-N+1 stricter-threshold
    /// lookup. Returns `None` when no episode released on that date.
    pub async fn last_release_on_et_date(
        &self,
        account: &str,
        et_date: NaiveDate,
    ) -> StorageResult<Option<TiltEpisode>> {
        let account = account.to_string();
        let (start_utc, end_utc) = et_day_bounds_utc(et_date);
        let start_unix = start_utc.timestamp();
        let end_unix = end_utc.timestamp();
        self.db
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT id, account, triggered_at_unix, trigger_kind,
                            cumulative_r_milli, consecutive_losses,
                            auto_reset_at_unix, released_at_unix,
                            release_kind, release_reason
                     FROM tilt_episodes
                     WHERE account = ?1
                       AND released_at_unix IS NOT NULL
                       AND released_at_unix >= ?2
                       AND released_at_unix < ?3
                     ORDER BY released_at_unix DESC
                     LIMIT 1",
                    params![account, start_unix, end_unix],
                    map_row,
                )
                .optional()
                .map_err(Into::into)
            })
            .await
    }

    /// Most recent `released_at` for `account`, regardless of release
    /// kind. Used as the evaluation watermark so a re-run of
    /// `evaluate` after override doesn't re-pause from trades the
    /// trader already acknowledged with the override.
    pub async fn last_released_at(&self, account: &str) -> StorageResult<Option<DateTime<Utc>>> {
        let account = account.to_string();
        let row: Option<i64> = self
            .db
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT released_at_unix FROM tilt_episodes
                     WHERE account = ?1 AND released_at_unix IS NOT NULL
                     ORDER BY released_at_unix DESC
                     LIMIT 1",
                    params![account],
                    |row| row.get(0),
                )
                .optional()
                .map_err(Into::into)
            })
            .await?;
        Ok(row.and_then(|u| DateTime::from_timestamp(u, 0)))
    }

    /// History — most-recent first. `since` filters by triggered_at.
    pub async fn history_since(
        &self,
        account: &str,
        since: DateTime<Utc>,
    ) -> StorageResult<Vec<TiltEpisode>> {
        let account = account.to_string();
        let since_unix = since.timestamp();
        self.db
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, account, triggered_at_unix, trigger_kind,
                            cumulative_r_milli, consecutive_losses,
                            auto_reset_at_unix, released_at_unix,
                            release_kind, release_reason
                     FROM tilt_episodes
                     WHERE account = ?1 AND triggered_at_unix >= ?2
                     ORDER BY triggered_at_unix DESC",
                )?;
                let rows = stmt
                    .query_map(params![account, since_unix], map_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await
    }

    /// Insert and return the materialized row (with auto-assigned id).
    pub async fn insert(&self, row: NewTiltEpisode) -> StorageResult<TiltEpisode> {
        let triggered_unix = row.triggered_at.timestamp();
        let auto_reset_unix = row.auto_reset_at.timestamp();
        let trigger_str = row.trigger_kind.as_str().to_string();
        let cum_r = row.cumulative_r_milli;
        let consec = row.consecutive_losses as i64;
        let account = row.account.clone();
        let id: i64 = self
            .db
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO tilt_episodes (
                        account, triggered_at_unix, trigger_kind,
                        cumulative_r_milli, consecutive_losses,
                        auto_reset_at_unix
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        account,
                        triggered_unix,
                        trigger_str,
                        cum_r,
                        consec,
                        auto_reset_unix
                    ],
                )?;
                Ok(conn.last_insert_rowid())
            })
            .await?;
        Ok(TiltEpisode {
            id,
            account: row.account,
            triggered_at: row.triggered_at,
            trigger_kind: row.trigger_kind,
            cumulative_r_milli: row.cumulative_r_milli,
            consecutive_losses: row.consecutive_losses,
            auto_reset_at: row.auto_reset_at,
            released_at: None,
            release_kind: None,
            release_reason: None,
        })
    }

    /// Mark an open episode released. Idempotent — calling on an
    /// already-released row returns Ok(false). Returns Ok(true) when a
    /// row was flipped.
    pub async fn release(
        &self,
        id: i64,
        kind: ReleaseKind,
        reason: Option<String>,
        at: DateTime<Utc>,
    ) -> StorageResult<bool> {
        let at_unix = at.timestamp();
        let kind_str = kind.as_str().to_string();
        self.db
            .with_conn(move |conn| {
                let n = conn.execute(
                    "UPDATE tilt_episodes
                     SET released_at_unix = ?1, release_kind = ?2, release_reason = ?3
                     WHERE id = ?4 AND released_at_unix IS NULL",
                    params![at_unix, kind_str, reason, id],
                )?;
                Ok(n > 0)
            })
            .await
    }
}

const SELECT_COLUMNS_OPEN: &str = "SELECT
    id, account, triggered_at_unix, trigger_kind,
    cumulative_r_milli, consecutive_losses,
    auto_reset_at_unix, released_at_unix,
    release_kind, release_reason
 FROM tilt_episodes
 WHERE account = ?1 AND released_at_unix IS NULL
 ORDER BY triggered_at_unix DESC
 LIMIT 1";

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TiltEpisode> {
    let id: i64 = row.get(0)?;
    let account: String = row.get(1)?;
    let triggered_unix: i64 = row.get(2)?;
    let trigger_str: String = row.get(3)?;
    let cum_r_milli: i64 = row.get(4)?;
    let consec_i: i64 = row.get(5)?;
    let auto_reset_unix: i64 = row.get(6)?;
    let released_unix: Option<i64> = row.get(7)?;
    let release_kind_str: Option<String> = row.get(8)?;
    let release_reason: Option<String> = row.get(9)?;

    let trigger_kind = TriggerKind::parse(&trigger_str).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            3,
            rusqlite::types::Type::Text,
            format!("unknown tilt_episodes.trigger_kind '{trigger_str}'").into(),
        )
    })?;
    let release_kind = match release_kind_str.as_deref() {
        Some(s) => Some(ReleaseKind::parse(s).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                8,
                rusqlite::types::Type::Text,
                format!("unknown tilt_episodes.release_kind '{s}'").into(),
            )
        })?),
        None => None,
    };
    Ok(TiltEpisode {
        id,
        account,
        triggered_at: DateTime::from_timestamp(triggered_unix, 0).unwrap_or_else(Utc::now),
        trigger_kind,
        cumulative_r_milli: cum_r_milli,
        consecutive_losses: consec_i.max(0) as u32,
        auto_reset_at: DateTime::from_timestamp(auto_reset_unix, 0).unwrap_or_else(Utc::now),
        released_at: released_unix.and_then(|u| DateTime::from_timestamp(u, 0)),
        release_kind,
        release_reason,
    })
}

/// Same shape as `executions::store::et_day_bounds_utc` but local to
/// the tilt module so we don't pull a private dep.
fn et_day_bounds_utc(date: NaiveDate) -> (DateTime<Utc>, DateTime<Utc>) {
    use chrono::NaiveTime;
    use chrono::TimeZone;
    let offset = et_offset();
    let start_naive = date.and_time(NaiveTime::from_hms_opt(0, 0, 0).expect("valid time"));
    let start = offset
        .from_local_datetime(&start_naive)
        .single()
        .expect("ET is fixed offset")
        .with_timezone(&Utc);
    let end_naive = date
        .succ_opt()
        .expect("date arithmetic does not overflow")
        .and_time(NaiveTime::from_hms_opt(0, 0, 0).expect("valid time"));
    let end = offset
        .from_local_datetime(&end_naive)
        .single()
        .expect("ET is fixed offset")
        .with_timezone(&Utc);
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn fresh_store() -> (NamedTempFile, TiltEpisodeStore) {
        let tmp = NamedTempFile::new().unwrap();
        let db = Arc::new(Db::open(tmp.path()).unwrap());
        (tmp, TiltEpisodeStore::new(db))
    }

    fn sample(account: &str, triggered: DateTime<Utc>) -> NewTiltEpisode {
        NewTiltEpisode {
            account: account.to_string(),
            triggered_at: triggered,
            trigger_kind: TriggerKind::CumRNegative,
            cumulative_r_milli: -3200,
            consecutive_losses: 2,
            auto_reset_at: triggered + chrono::Duration::hours(15),
        }
    }

    #[tokio::test]
    async fn insert_and_open_round_trips() {
        let (_tmp, store) = fresh_store();
        let now = Utc::now();
        let row = store.insert(sample("DU1", now)).await.unwrap();
        assert!(row.id > 0);
        let open = store.open_for_account("DU1").await.unwrap().unwrap();
        assert_eq!(open.id, row.id);
        assert!(open.released_at.is_none());
    }

    #[tokio::test]
    async fn release_marks_row_closed() {
        let (_tmp, store) = fresh_store();
        let row = store.insert(sample("DU1", Utc::now())).await.unwrap();
        let later = Utc::now();
        let flipped = store
            .release(
                row.id,
                ReleaseKind::ManualOverride,
                Some("test".into()),
                later,
            )
            .await
            .unwrap();
        assert!(flipped);
        assert!(store.open_for_account("DU1").await.unwrap().is_none());
        // Second release is a no-op.
        let again = store
            .release(row.id, ReleaseKind::Auto, None, later)
            .await
            .unwrap();
        assert!(!again);
    }

    #[tokio::test]
    async fn open_returns_only_unreleased_row() {
        let (_tmp, store) = fresh_store();
        let earlier = Utc::now() - chrono::Duration::hours(2);
        let earlier_row = store.insert(sample("DU1", earlier)).await.unwrap();
        store
            .release(
                earlier_row.id,
                ReleaseKind::Auto,
                None,
                earlier + chrono::Duration::hours(1),
            )
            .await
            .unwrap();
        let later_row = store.insert(sample("DU1", Utc::now())).await.unwrap();
        let open = store.open_for_account("DU1").await.unwrap().unwrap();
        assert_eq!(open.id, later_row.id);
    }

    #[tokio::test]
    async fn last_release_on_et_date_finds_yesterdays_override() {
        let (_tmp, store) = fresh_store();
        let triggered = Utc::now() - chrono::Duration::hours(20);
        let row = store.insert(sample("DU1", triggered)).await.unwrap();
        let release_at = triggered + chrono::Duration::hours(1);
        store
            .release(
                row.id,
                ReleaseKind::ManualOverride,
                Some("test".into()),
                release_at,
            )
            .await
            .unwrap();
        let et_today = crate::utils::market_calendar::et_date(release_at);
        let found = store
            .last_release_on_et_date("DU1", et_today)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.id, row.id);
        assert_eq!(found.release_kind, Some(ReleaseKind::ManualOverride));
    }
}
