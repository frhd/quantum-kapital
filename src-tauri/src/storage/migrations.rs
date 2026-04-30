use crate::storage::error::{Result, StorageError};

mod embedded {
    use refinery::embed_migrations;
    embed_migrations!("./src/storage/migrations");
}

/// Run all pending refinery migrations against `conn`. Idempotent: refinery
/// records each applied version in `refinery_schema_history` and skips ones
/// already present.
pub fn run_migrations(conn: &mut rusqlite::Connection) -> Result<()> {
    embedded::migrations::runner()
        .run(conn)
        .map(|_| ())
        .map_err(|e| StorageError::Migration(e.to_string()))
}
