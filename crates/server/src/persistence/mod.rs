mod host_repository;
mod operation_repository;
mod topology_repository;

use std::{str::FromStr, time::Duration};

pub use host_repository::{CreateHost, HostRepository};
pub use operation_repository::OperationRepository;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use thiserror::Error;
pub(crate) use topology_repository::is_durable_usb_selector;
pub use topology_repository::{
    BindingRecord, CredentialRecord, DeviceRecord, RevisionRecord, ServerRecord, ShutdownOptions,
    ShutdownTriggerMode, TopologyRepository,
};

#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn connect(database_url: &str) -> Result<Self, PersistenceError> {
        let options = SqliteConnectOptions::from_str(database_url)?
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5));
        let max_connections = if database_url.contains(":memory:") {
            1
        } else {
            5
        };
        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
            .connect_with(options)
            .await?;
        Ok(Self { pool })
    }

    pub async fn migrate(&self) -> Result<(), PersistenceError> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }

    pub async fn health(&self) -> Result<(), PersistenceError> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    pub fn hosts(&self) -> HostRepository {
        HostRepository::new(self.pool.clone())
    }

    pub fn operations(&self) -> OperationRepository {
        OperationRepository::new(self.pool.clone())
    }

    pub fn topology(&self) -> TopologyRepository {
        TopologyRepository::new(self.pool.clone())
    }
}

#[derive(Debug, Error)]
pub enum PersistenceError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("database migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("invalid persisted data: {0}")]
    InvalidData(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn migrations_create_the_initial_schema() {
        let database = Database::connect("sqlite::memory:").await.unwrap();
        database.migrate().await.unwrap();
        database.health().await.unwrap();

        let tables: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'hosts'",
        )
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert_eq!(tables, 1);
    }
}
