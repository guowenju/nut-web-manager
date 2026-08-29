use std::str::FromStr;

use chrono::{DateTime, Utc};
use nwm_common::{HostId, Operation, OperationId, OperationState};
use sqlx::{Row, SqlitePool, sqlite::SqliteRow};

use super::PersistenceError;

#[derive(Clone)]
pub struct OperationRepository {
    pool: SqlitePool,
}

impl OperationRepository {
    pub(super) fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create(
        &self,
        host_id: Option<HostId>,
        kind: &str,
    ) -> Result<Operation, PersistenceError> {
        let now = Utc::now();
        let operation = Operation {
            id: OperationId::new(),
            host_id,
            kind: kind.to_owned(),
            state: OperationState::Pending,
            progress: 0,
            error_code: None,
            error_detail: None,
            result: None,
            created_at: now,
            updated_at: now,
        };
        sqlx::query(
            "INSERT INTO operations (id, host_id, kind, state, progress, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(operation.id.to_string())
        .bind(operation.host_id.map(|id| id.to_string()))
        .bind(&operation.kind)
        .bind(operation.state.as_str())
        .bind(i64::from(operation.progress))
        .bind(operation.created_at.to_rfc3339())
        .bind(operation.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(operation)
    }

    pub async fn get(&self, id: OperationId) -> Result<Option<Operation>, PersistenceError> {
        sqlx::query("SELECT * FROM operations WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?
            .map(decode_operation)
            .transpose()
    }

    pub async fn set_running(&self, id: OperationId, progress: u8) -> Result<(), PersistenceError> {
        self.update(id, OperationState::Running, progress, None, None)
            .await
    }

    pub async fn succeed(&self, id: OperationId) -> Result<(), PersistenceError> {
        self.update(id, OperationState::Succeeded, 100, None, None)
            .await
    }

    pub async fn succeed_with_result(
        &self,
        id: OperationId,
        result: &impl serde::Serialize,
    ) -> Result<(), PersistenceError> {
        let result = serde_json::to_string(result)
            .map_err(|error| PersistenceError::InvalidData(error.to_string()))?;
        sqlx::query(
            "UPDATE operations SET state = 'succeeded', progress = 100, error_code = NULL, \
             error_detail = NULL, result_json = ?, updated_at = ? WHERE id = ?",
        )
        .bind(result)
        .bind(Utc::now().to_rfc3339())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn fail(
        &self,
        id: OperationId,
        code: &str,
        detail: &str,
    ) -> Result<(), PersistenceError> {
        self.update(id, OperationState::Failed, 100, Some(code), Some(detail))
            .await
    }

    pub async fn fail_incomplete_after_restart(&self) -> Result<u64, PersistenceError> {
        let now = Utc::now().to_rfc3339();
        let result = sqlx::query(
            "UPDATE operations SET state = 'failed', progress = 100, \
             error_code = 'ServiceRestarted', \
             error_detail = 'the operation was interrupted by an application restart', \
             updated_at = ? WHERE state IN ('pending', 'running')",
        )
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    async fn update(
        &self,
        id: OperationId,
        state: OperationState,
        progress: u8,
        error_code: Option<&str>,
        error_detail: Option<&str>,
    ) -> Result<(), PersistenceError> {
        sqlx::query(
            "UPDATE operations SET state = ?, progress = ?, error_code = ?, error_detail = ?, \
             updated_at = ? WHERE id = ?",
        )
        .bind(state.as_str())
        .bind(i64::from(progress))
        .bind(error_code)
        .bind(error_detail)
        .bind(Utc::now().to_rfc3339())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

fn decode_operation(row: SqliteRow) -> Result<Operation, PersistenceError> {
    Ok(Operation {
        id: parse(&row.try_get::<String, _>("id")?, "operation id")?,
        host_id: row
            .try_get::<Option<String>, _>("host_id")?
            .map(|id| parse(&id, "host id"))
            .transpose()?,
        kind: row.try_get("kind")?,
        state: OperationState::from_str(&row.try_get::<String, _>("state")?)
            .map_err(PersistenceError::InvalidData)?,
        progress: u8::try_from(row.try_get::<i64, _>("progress")?)
            .map_err(|error| PersistenceError::InvalidData(error.to_string()))?,
        error_code: row.try_get("error_code")?,
        error_detail: row.try_get("error_detail")?,
        result: row
            .try_get::<Option<String>, _>("result_json")?
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(|error| {
                PersistenceError::InvalidData(format!("invalid operation result: {error}"))
            })?,
        created_at: parse_datetime(&row.try_get::<String, _>("created_at")?)?,
        updated_at: parse_datetime(&row.try_get::<String, _>("updated_at")?)?,
    })
}

fn parse<T>(value: &str, label: &str) -> Result<T, PersistenceError>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    value
        .parse()
        .map_err(|error| PersistenceError::InvalidData(format!("invalid {label}: {error}")))
}

fn parse_datetime(value: &str) -> Result<DateTime<Utc>, PersistenceError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| PersistenceError::InvalidData(format!("invalid timestamp: {error}")))
}

#[cfg(test)]
mod tests {
    use nwm_common::HostRole;

    use crate::persistence::{CreateHost, Database};

    use super::*;

    #[tokio::test]
    async fn operation_lifecycle_is_persisted() {
        let database = Database::connect("sqlite::memory:").await.unwrap();
        database.migrate().await.unwrap();
        let repository = database.operations();
        let host = database
            .hosts()
            .create(CreateHost {
                name: "pve".into(),
                address: "192.168.1.10".into(),
                ssh_port: 22,
                username: "root".into(),
                role: HostRole::Server,
            })
            .await
            .unwrap();
        let operation = repository
            .create(Some(host.id), "nut_install")
            .await
            .unwrap();
        repository.set_running(operation.id, 10).await.unwrap();
        repository.succeed(operation.id).await.unwrap();
        let stored = repository.get(operation.id).await.unwrap().unwrap();
        assert_eq!(stored.state, OperationState::Succeeded);
        assert_eq!(stored.progress, 100);
        assert!(stored.result.is_none());
    }

    #[tokio::test]
    async fn operation_result_is_persisted() {
        let database = Database::connect("sqlite::memory:").await.unwrap();
        database.migrate().await.unwrap();
        let repository = database.operations();
        let operation = repository.create(None, "usb_scan").await.unwrap();
        repository
            .succeed_with_result(operation.id, &serde_json::json!({"devices": 1}))
            .await
            .unwrap();

        let stored = repository.get(operation.id).await.unwrap().unwrap();
        assert_eq!(stored.result, Some(serde_json::json!({"devices": 1})));
    }

    #[tokio::test]
    async fn incomplete_operations_are_failed_after_restart() {
        let database = Database::connect("sqlite::memory:").await.unwrap();
        database.migrate().await.unwrap();
        let repository = database.operations();
        let operation = repository.create(None, "nut_install").await.unwrap();
        repository.set_running(operation.id, 10).await.unwrap();

        assert_eq!(repository.fail_incomplete_after_restart().await.unwrap(), 1);
        let stored = repository.get(operation.id).await.unwrap().unwrap();
        assert_eq!(stored.state, OperationState::Failed);
        assert_eq!(stored.error_code.as_deref(), Some("ServiceRestarted"));
    }
}
