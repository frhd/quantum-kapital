use super::*;
use tempfile::NamedTempFile;

fn temp_db_path() -> NamedTempFile {
    NamedTempFile::new().expect("create tempfile")
}

#[tokio::test]
async fn db_open_creates_file_and_runs_migrations() {
    let tmp = temp_db_path();
    let path = tmp.path().to_path_buf();

    let db = Db::open(&path).expect("open db");

    assert!(path.exists(), "sqlite file should exist after Db::open");

    db.with_conn(|conn| {
        let fk: i64 = conn.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
        assert_eq!(fk, 1, "foreign_keys must be ON");

        let mode: String = conn.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
        assert_eq!(mode.to_lowercase(), "wal", "journal mode must be WAL");

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN \
             ('tracked_tickers','setups','alerts','bars_cache','news_cache','llm_calls')",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 6, "all six baseline tables must exist");
        Ok(())
    })
    .await
    .expect("with_conn ok");
}

#[tokio::test]
async fn db_open_is_idempotent() {
    let tmp = temp_db_path();
    let path = tmp.path().to_path_buf();

    let _db1 = Db::open(&path).expect("first open");
    let _db2 = Db::open(&path).expect("second open should not error");
}

#[tokio::test]
async fn db_with_conn_round_trips_value() {
    let tmp = temp_db_path();
    let db = Db::open(tmp.path()).expect("open db");

    let value = db
        .with_conn(|conn| {
            conn.execute(
                "INSERT INTO tracked_tickers (symbol, source, added_at) VALUES (?1, ?2, ?3)",
                rusqlite::params!["AAPL", "manual", 1_700_000_000_i64],
            )?;
            let symbol: String = conn.query_row(
                "SELECT symbol FROM tracked_tickers WHERE symbol = 'AAPL'",
                [],
                |row| row.get(0),
            )?;
            Ok(symbol)
        })
        .await
        .expect("with_conn ok");

    assert_eq!(value, "AAPL");
}

#[tokio::test]
async fn db_with_conn_propagates_rusqlite_error() {
    let tmp = temp_db_path();
    let db = Db::open(tmp.path()).expect("open db");

    let result: Result<i64> = db
        .with_conn(|conn| {
            let v: i64 =
                conn.query_row("SELECT no_such_column FROM tracked_tickers", [], |row| {
                    row.get(0)
                })?;
            Ok(v)
        })
        .await;

    match result {
        Err(StorageError::Sqlite(_)) => {}
        other => panic!("expected StorageError::Sqlite, got {other:?}"),
    }
}

#[tokio::test]
async fn migration_creates_all_baseline_tables() {
    let tmp = temp_db_path();
    let db = Db::open(tmp.path()).expect("open db");

    let names: Vec<String> = db
        .with_conn(|conn| {
            let mut stmt =
                conn.prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
        })
        .await
        .expect("with_conn ok");

    for required in [
        "tracked_tickers",
        "setups",
        "alerts",
        "bars_cache",
        "news_cache",
        "llm_calls",
    ] {
        assert!(
            names.iter().any(|n| n == required),
            "missing table {required}; have {names:?}"
        );
    }
}

#[tokio::test]
async fn migration_history_records_every_version() {
    let tmp = temp_db_path();
    let db = Db::open(tmp.path()).expect("open db");

    let versions: Vec<i32> = db
        .with_conn(|conn| {
            let mut stmt =
                conn.prepare("SELECT version FROM refinery_schema_history ORDER BY version")?;
            let rows = stmt.query_map([], |row| row.get::<_, i32>(0))?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
        })
        .await
        .expect("with_conn ok");

    assert!(
        versions.contains(&1),
        "V01 must be recorded; have {versions:?}"
    );
    assert!(
        versions.contains(&2),
        "V02 must be recorded; have {versions:?}"
    );
}

#[tokio::test]
async fn migration_v03_creates_research_artifact_tables() {
    let tmp = temp_db_path();
    let db = Db::open(tmp.path()).expect("open db");

    db.with_conn(|conn| {
        for table in ["research_notes", "mcp_audit", "agent_morning_packs"] {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?1",
                rusqlite::params![table],
                |row| row.get(0),
            )?;
            assert_eq!(count, 1, "{table} must exist after V03");
        }

        // alerts gained the ack_alert decision rail.
        for col in ["decision", "decision_note_id", "decided_at"] {
            let has_col: bool = conn
                .prepare("PRAGMA table_info(alerts)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .filter_map(|r| r.ok())
                .any(|name| name == col);
            assert!(has_col, "alerts.{col} must exist after V03");
        }

        // research_notes columns we'll rely on.
        let names: Vec<String> = conn
            .prepare("PRAGMA table_info(research_notes)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .collect();
        for col in [
            "id",
            "symbol",
            "body_md",
            "conviction",
            "evidence_refs",
            "written_by",
            "written_at",
            "setup_id",
            "alert_id",
        ] {
            assert!(
                names.iter().any(|n| n == col),
                "research_notes.{col} expected; have {names:?}"
            );
        }

        Ok(())
    })
    .await
    .expect("with_conn ok");
}

#[tokio::test]
async fn migration_v05_creates_candidate_universe() {
    let tmp = temp_db_path();
    let db = Db::open(tmp.path()).expect("open db");

    db.with_conn(|conn| {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='candidate_universe'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 1, "candidate_universe must exist after V05");

        let names: Vec<String> = conn
            .prepare("PRAGMA table_info(candidate_universe)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .collect();
        for col in [
            "symbol",
            "score",
            "sources",
            "reason_md",
            "first_seen",
            "last_seen",
            "decay_at",
            "promoted_at",
        ] {
            assert!(
                names.iter().any(|n| n == col),
                "candidate_universe.{col} expected; have {names:?}"
            );
        }

        // Indexes from V05.
        let indexes: Vec<String> = conn
            .prepare(
                "SELECT name FROM sqlite_master \
                 WHERE type='index' AND tbl_name='candidate_universe' \
                 ORDER BY name",
            )?
            .query_map([], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();
        for required in [
            "idx_candidate_universe_decay",
            "idx_candidate_universe_score_desc",
            "idx_candidate_universe_last_seen_desc",
            "idx_candidate_universe_promoted_at",
        ] {
            assert!(
                indexes.iter().any(|n| n == required),
                "missing index {required}; have {indexes:?}"
            );
        }

        Ok(())
    })
    .await
    .expect("with_conn ok");
}

#[tokio::test]
async fn migration_v02_adds_archived_at_columns() {
    let tmp = temp_db_path();
    let db = Db::open(tmp.path()).expect("open db");

    db.with_conn(|conn| {
        for table in ["tracked_tickers", "setups"] {
            let has_col: bool = conn
                .prepare(&format!("PRAGMA table_info({table})"))?
                .query_map([], |row| row.get::<_, String>(1))?
                .filter_map(|r| r.ok())
                .any(|name| name == "archived_at");
            assert!(has_col, "{table}.archived_at must exist after V02");
        }
        Ok(())
    })
    .await
    .expect("with_conn ok");
}

#[tokio::test]
async fn migration_creates_required_indexes() {
    let tmp = temp_db_path();
    let db = Db::open(tmp.path()).expect("open db");

    let names: Vec<String> = db
        .with_conn(|conn| {
            let mut stmt =
                conn.prepare("SELECT name FROM sqlite_master WHERE type='index' ORDER BY name")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
        })
        .await
        .expect("with_conn ok");

    for required in ["idx_setups_symbol", "idx_setups_status_detected"] {
        assert!(
            names.iter().any(|n| n == required),
            "missing index {required}; have {names:?}"
        );
    }
}

#[tokio::test]
async fn tracked_tickers_pk_rejects_duplicate_symbol() {
    let tmp = temp_db_path();
    let db = Db::open(tmp.path()).expect("open db");

    let result: Result<()> = db
        .with_conn(|conn| {
            conn.execute(
                "INSERT INTO tracked_tickers (symbol, source, added_at) VALUES ('AAPL','manual',1)",
                [],
            )?;
            conn.execute(
                "INSERT INTO tracked_tickers (symbol, source, added_at) VALUES ('AAPL','manual',2)",
                [],
            )?;
            Ok(())
        })
        .await;

    match result {
        Err(StorageError::Sqlite(rusqlite::Error::SqliteFailure(_, _))) => {}
        other => panic!("expected sqlite constraint error, got {other:?}"),
    }
}

#[tokio::test]
async fn setups_fk_cascades_on_ticker_delete() {
    let tmp = temp_db_path();
    let db = Db::open(tmp.path()).expect("open db");

    let (setup_count, alert_count) = db
        .with_conn(|conn| {
            conn.execute(
                "INSERT INTO tracked_tickers (symbol, source, added_at) VALUES ('AAPL','manual',1)",
                [],
            )?;
            conn.execute(
                "INSERT INTO setups (symbol, strategy, direction, detected_at, trigger_price, stop_price, targets, raw_signals) \
                 VALUES ('AAPL','breakout','long',1, 100.0, 95.0, '[]', '{}')",
                [],
            )?;
            let setup_id: i64 = conn.query_row("SELECT id FROM setups", [], |row| row.get(0))?;
            conn.execute(
                "INSERT INTO alerts (setup_id, kind, fired_at, payload) VALUES (?1, 'detected', 1, '{}')",
                rusqlite::params![setup_id],
            )?;

            conn.execute("DELETE FROM tracked_tickers WHERE symbol = 'AAPL'", [])?;

            let setups: i64 = conn.query_row("SELECT COUNT(*) FROM setups", [], |row| row.get(0))?;
            let alerts: i64 = conn.query_row("SELECT COUNT(*) FROM alerts", [], |row| row.get(0))?;
            Ok((setups, alerts))
        })
        .await
        .expect("with_conn ok");

    assert_eq!(setup_count, 0, "setups should cascade-delete");
    assert_eq!(alert_count, 0, "alerts should cascade-delete via setup fk");
}

#[tokio::test]
async fn bars_cache_pk_dedup() {
    let tmp = temp_db_path();
    let db = Db::open(tmp.path()).expect("open db");

    let close = db
        .with_conn(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO bars_cache \
                 (symbol, bar_size, bar_time, open, high, low, close, volume) \
                 VALUES ('AAPL','1day',1714435200, 100.0, 101.0, 99.0, 100.5, 1000)",
                [],
            )?;
            conn.execute(
                "INSERT OR REPLACE INTO bars_cache \
                 (symbol, bar_size, bar_time, open, high, low, close, volume) \
                 VALUES ('AAPL','1day',1714435200, 100.0, 102.0, 99.0, 101.5, 2000)",
                [],
            )?;

            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM bars_cache WHERE symbol='AAPL' AND bar_size='1day' AND bar_time=1714435200",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(count, 1, "upsert must dedup");

            let close: f64 = conn.query_row(
                "SELECT close FROM bars_cache WHERE symbol='AAPL' AND bar_size='1day' AND bar_time=1714435200",
                [],
                |row| row.get(0),
            )?;
            Ok(close)
        })
        .await
        .expect("with_conn ok");

    assert!(
        (close - 101.5).abs() < f64::EPSILON,
        "second row must overwrite first"
    );
}
