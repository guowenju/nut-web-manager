use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::Serialize;
use sqlx::{Row, SqlitePool};

use super::PersistenceError;

#[derive(Clone, Debug, Serialize)]
pub struct DashboardHistorySample {
    pub observed_at: String,
    pub load_percent: Option<f64>,
    pub runtime_seconds: Option<i64>,
    pub realpower_watts: Option<f64>,
}

#[derive(Clone)]
pub struct DashboardHistoryRepository {
    pool: SqlitePool,
}

impl DashboardHistoryRepository {
    pub(super) fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn record(
        &self,
        server_id: &str,
        observed_at: DateTime<Utc>,
        load_percent: Option<f64>,
        runtime_seconds: Option<i64>,
        realpower_watts: Option<f64>,
    ) -> Result<(), PersistenceError> {
        let observed_at = observed_at.to_rfc3339_opts(SecondsFormat::Secs, true);
        let bucket_at = observed_at[..16].to_owned() + ":00Z";
        sqlx::query(
            "INSERT INTO dashboard_ups_samples (server_id, bucket_at, observed_at, load_percent, runtime_seconds, realpower_watts) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(server_id, bucket_at) DO NOTHING",
        )
        .bind(server_id)
        .bind(bucket_at)
        .bind(observed_at)
        .bind(load_percent)
        .bind(runtime_seconds)
        .bind(realpower_watts)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn history(
        &self,
        server_id: &str,
        since: DateTime<Utc>,
    ) -> Result<Vec<DashboardHistorySample>, PersistenceError> {
        let rows = sqlx::query("SELECT observed_at, load_percent, runtime_seconds, realpower_watts FROM dashboard_ups_samples WHERE server_id = ? AND observed_at >= ? ORDER BY observed_at")
            .bind(server_id)
            .bind(since.to_rfc3339_opts(SecondsFormat::Secs, true))
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|row| {
                Ok(DashboardHistorySample {
                    observed_at: row.try_get("observed_at")?,
                    load_percent: row.try_get("load_percent")?,
                    runtime_seconds: row.try_get("runtime_seconds")?,
                    realpower_watts: row.try_get("realpower_watts")?,
                })
            })
            .collect()
    }

    pub async fn prune(&self) -> Result<u64, PersistenceError> {
        let cutoff = (Utc::now() - Duration::days(90)).to_rfc3339_opts(SecondsFormat::Secs, true);
        Ok(
            sqlx::query("DELETE FROM dashboard_ups_samples WHERE observed_at < ?")
                .bind(cutoff)
                .execute(&self.pool)
                .await?
                .rows_affected(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::{CreateHost, Database};
    use nwm_common::HostRole;

    #[tokio::test]
    async fn records_only_one_sample_per_minute() {
        let database = Database::connect("sqlite::memory:").await.unwrap();
        database.migrate().await.unwrap();
        let host = database
            .hosts()
            .create(CreateHost {
                name: "server".into(),
                address: "127.0.0.1".into(),
                ssh_port: 22,
                username: "root".into(),
                role: HostRole::Server,
            })
            .await
            .unwrap();
        let server_id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO nut_servers (id, host_id, ups_name, enabled, apply_state) VALUES (?, ?, 'ups', 1, 'applied')")
            .bind(&server_id).bind(host.id.to_string()).execute(&database.pool).await.unwrap();
        let repository = database.dashboard_history();
        let now = Utc::now();
        repository
            .record(&server_id, now, Some(10.0), Some(600), Some(100.0))
            .await
            .unwrap();
        repository
            .record(
                &server_id,
                now + Duration::seconds(5),
                Some(20.0),
                Some(500),
                Some(200.0),
            )
            .await
            .unwrap();
        let samples = repository
            .history(&server_id, now - Duration::minutes(1))
            .await
            .unwrap();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].load_percent, Some(10.0));
    }
}
