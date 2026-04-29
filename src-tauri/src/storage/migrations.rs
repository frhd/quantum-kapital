use crate::storage::error::Result;

const SCHEMA_SQL: &str = include_str!("schema.sql");

pub fn run_migrations(conn: &mut rusqlite::Connection) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute_batch(SCHEMA_SQL)?;
    tx.commit()?;
    Ok(())
}
