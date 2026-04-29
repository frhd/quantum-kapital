use crate::storage::error::Result;

const SCHEMA_SQL: &str = include_str!("schema.sql");

pub fn run_migrations(conn: &mut rusqlite::Connection) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute_batch(SCHEMA_SQL)?;
    add_column_if_missing(&tx, "tracked_tickers", "cool_down_until", "INTEGER")?;
    tx.commit()?;
    Ok(())
}

/// Idempotent `ALTER TABLE ... ADD COLUMN` — silently no-ops when the column
/// already exists. SQLite has no `IF NOT EXISTS` for column adds, so this
/// inspects `PRAGMA table_info` first.
fn add_column_if_missing(
    conn: &rusqlite::Connection,
    table: &str,
    column: &str,
    decl: &str,
) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let exists = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(|r| r.ok())
        .any(|name| name == column);
    if !exists {
        conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl};"))?;
    }
    Ok(())
}
