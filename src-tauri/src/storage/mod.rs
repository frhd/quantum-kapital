pub mod error;
mod migrations;

#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::Arc;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;

pub use error::{Result, StorageError};

pub type SqlitePool = Pool<SqliteConnectionManager>;

#[derive(Clone)]
pub struct Db {
    pool: Arc<SqlitePool>,
}

impl Db {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let manager = SqliteConnectionManager::file(path.as_ref()).with_init(|conn| {
            conn.execute_batch(
                "PRAGMA journal_mode = WAL;\n\
                 PRAGMA foreign_keys = ON;\n\
                 PRAGMA synchronous = NORMAL;",
            )
        });

        let pool = Pool::builder().build(manager).map_err(StorageError::from)?;

        let mut conn = pool.get().map_err(StorageError::from)?;
        migrations::run_migrations(&mut conn)?;

        Ok(Self {
            pool: Arc::new(pool),
        })
    }

    pub async fn with_conn<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = pool.get().map_err(StorageError::from)?;
            f(&mut conn)
        })
        .await
        .map_err(|e| StorageError::Join(e.to_string()))?
    }
}
