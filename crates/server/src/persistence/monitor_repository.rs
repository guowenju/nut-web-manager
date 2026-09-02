use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::Serialize;
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use super::PersistenceError;
use crate::ups_monitor::protocol::DiscoveredUps;

#[derive(Clone, Debug, Serialize)]
pub struct MonitorSource {
    pub id: String,
    pub name: String,
    pub address: String,
    pub port: u16,
    pub enabled: bool,
    pub last_discovery_at: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct MonitorDevice {
    pub id: String,
    pub source_id: String,
    pub source_name: String,
    pub ups_name: String,
    pub description: Option<String>,
    pub online: bool,
    pub last_seen_at: Option<String>,
    pub last_error: Option<String>,
    pub observed_at: Option<String>,
    pub status_flags: Vec<String>,
    pub charge_percent: Option<f64>,
    pub runtime_seconds: Option<i64>,
    pub runtime_capped: bool,
    pub load_percent: Option<f64>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MonitorSnapshot {
    pub device: MonitorDevice,
    pub raw: BTreeMap<String, String>,
    pub input_voltage: Option<f64>,
    pub output_voltage: Option<f64>,
    pub battery_temperature: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MonitorSample {
    pub observed_at: String,
    pub status_flags: Vec<String>,
    pub charge_percent: Option<f64>,
    pub runtime_seconds: Option<i64>,
    pub runtime_capped: bool,
    pub load_percent: Option<f64>,
    pub input_voltage: Option<f64>,
    pub output_voltage: Option<f64>,
    pub battery_temperature: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MonitorEvent {
    pub id: i64,
    pub occurred_at: String,
    pub kind: String,
    pub severity: String,
    pub message: String,
    pub status_flags: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Metrics {
    pub status_flags: Vec<String>,
    pub charge_percent: Option<f64>,
    pub runtime_seconds: Option<i64>,
    pub runtime_capped: bool,
    pub load_percent: Option<f64>,
    pub input_voltage: Option<f64>,
    pub output_voltage: Option<f64>,
    pub battery_temperature: Option<f64>,
}

impl Metrics {
    pub fn from_raw(raw: &BTreeMap<String, String>) -> Self {
        let reported_runtime: Option<i64> = number(raw, "battery.runtime");
        let runtime_capped = reported_runtime == Some(65_535);
        Self {
            status_flags: raw
                .get("ups.status")
                .map(|value| value.split_whitespace().map(str::to_owned).collect())
                .unwrap_or_default(),
            charge_percent: number(raw, "battery.charge"),
            runtime_capped,
            runtime_seconds: if runtime_capped {
                None
            } else {
                reported_runtime
            },
            load_percent: number(raw, "ups.load"),
            input_voltage: number(raw, "input.voltage"),
            output_voltage: number(raw, "output.voltage"),
            battery_temperature: number(raw, "battery.temperature"),
        }
    }
}

fn number<T: std::str::FromStr>(raw: &BTreeMap<String, String>, key: &str) -> Option<T> {
    raw.get(key)?.parse().ok()
}

#[derive(Clone)]
pub struct MonitorRepository {
    pool: SqlitePool,
}

impl MonitorRepository {
    pub(super) fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn list_sources(&self) -> Result<Vec<MonitorSource>, PersistenceError> {
        let rows = sqlx::query("SELECT * FROM ups_monitor_sources ORDER BY name COLLATE NOCASE")
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(source_from_row).collect()
    }

    pub async fn enabled_sources(&self) -> Result<Vec<MonitorSource>, PersistenceError> {
        let rows =
            sqlx::query("SELECT * FROM ups_monitor_sources WHERE enabled = 1 ORDER BY created_at")
                .fetch_all(&self.pool)
                .await?;
        rows.iter().map(source_from_row).collect()
    }

    pub async fn get_source(&self, id: &str) -> Result<Option<MonitorSource>, PersistenceError> {
        let row = sqlx::query("SELECT * FROM ups_monitor_sources WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(source_from_row).transpose()
    }

    pub async fn create_source(
        &self,
        name: &str,
        address: &str,
        port: u16,
        enabled: bool,
    ) -> Result<MonitorSource, PersistenceError> {
        validate_source(name, address, port)?;
        let id = Uuid::new_v4().to_string();
        let now = timestamp(Utc::now());
        sqlx::query(
            "INSERT INTO ups_monitor_sources (id, name, address, port, enabled, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(name.trim())
        .bind(address.trim())
        .bind(i64::from(port))
        .bind(enabled)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(source_write_error)?;
        self.get_source(&id).await?.ok_or_else(|| {
            PersistenceError::InvalidData("created monitor source disappeared".into())
        })
    }

    pub async fn update_source(
        &self,
        id: &str,
        name: &str,
        address: &str,
        port: u16,
        enabled: bool,
        reset_devices: bool,
    ) -> Result<Option<MonitorSource>, PersistenceError> {
        validate_source(name, address, port)?;
        let mut transaction = self.pool.begin().await?;
        let previous = sqlx::query("SELECT * FROM ups_monitor_sources WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *transaction)
            .await?;
        let Some(previous) = previous.as_ref().map(source_from_row).transpose()? else {
            return Ok(None);
        };
        let endpoint_changed = previous.address != address.trim() || previous.port != port;
        if endpoint_changed && !reset_devices {
            let device_count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM ups_monitor_devices WHERE source_id = ?")
                    .bind(id)
                    .fetch_one(&mut *transaction)
                    .await?;
            if device_count > 0 {
                return Err(PersistenceError::Conflict(
                    "changing the source address or port requires confirming the deletion of its devices and history".into(),
                ));
            }
        }
        let result = sqlx::query(
            "UPDATE ups_monitor_sources SET name = ?, address = ?, port = ?, enabled = ?, last_discovery_at = NULL, last_error = NULL, updated_at = ? WHERE id = ?",
        )
        .bind(name.trim())
        .bind(address.trim())
        .bind(i64::from(port))
        .bind(enabled)
        .bind(timestamp(Utc::now()))
        .bind(id)
        .execute(&mut *transaction)
        .await
        .map_err(source_write_error)?;
        if result.rows_affected() == 0 {
            Ok(None)
        } else {
            if endpoint_changed {
                sqlx::query("DELETE FROM ups_monitor_devices WHERE source_id = ?")
                    .bind(id)
                    .execute(&mut *transaction)
                    .await?;
            }
            transaction.commit().await?;
            if !enabled {
                for device in self.devices_for_source(id).await? {
                    self.record_failure(&device.id, "monitor source is disabled", Utc::now())
                        .await?;
                }
            }
            self.get_source(id).await
        }
    }

    pub async fn delete_source(&self, id: &str) -> Result<bool, PersistenceError> {
        Ok(sqlx::query("DELETE FROM ups_monitor_sources WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?
            .rows_affected()
            > 0)
    }

    pub async fn devices_for_source(
        &self,
        source_id: &str,
    ) -> Result<Vec<MonitorDevice>, PersistenceError> {
        self.device_query("WHERE d.source_id = ?", source_id).await
    }

    pub async fn overview_devices(&self) -> Result<Vec<MonitorDevice>, PersistenceError> {
        self.device_query("", "").await
    }

    async fn device_query(
        &self,
        clause: &str,
        value: &str,
    ) -> Result<Vec<MonitorDevice>, PersistenceError> {
        let rows = match clause {
            "" => sqlx::query("SELECT d.*, s.name AS source_name, x.observed_at, x.status_flags_json, x.charge_percent, x.runtime_seconds, x.runtime_capped, x.load_percent, x.raw_json FROM ups_monitor_devices d JOIN ups_monitor_sources s ON s.id = d.source_id LEFT JOIN ups_monitor_snapshots x ON x.device_id = d.id ORDER BY s.name COLLATE NOCASE, d.ups_name COLLATE NOCASE")
                .fetch_all(&self.pool).await?,
            "WHERE d.source_id = ?" => sqlx::query("SELECT d.*, s.name AS source_name, x.observed_at, x.status_flags_json, x.charge_percent, x.runtime_seconds, x.runtime_capped, x.load_percent, x.raw_json FROM ups_monitor_devices d JOIN ups_monitor_sources s ON s.id = d.source_id LEFT JOIN ups_monitor_snapshots x ON x.device_id = d.id WHERE d.source_id = ? ORDER BY s.name COLLATE NOCASE, d.ups_name COLLATE NOCASE")
                .bind(value).fetch_all(&self.pool).await?,
            "WHERE d.id = ?" => sqlx::query("SELECT d.*, s.name AS source_name, x.observed_at, x.status_flags_json, x.charge_percent, x.runtime_seconds, x.runtime_capped, x.load_percent, x.raw_json FROM ups_monitor_devices d JOIN ups_monitor_sources s ON s.id = d.source_id LEFT JOIN ups_monitor_snapshots x ON x.device_id = d.id WHERE d.id = ? ORDER BY s.name COLLATE NOCASE, d.ups_name COLLATE NOCASE")
                .bind(value).fetch_all(&self.pool).await?,
            _ => unreachable!("device query clause is internal"),
        };
        rows.iter().map(device_from_row).collect()
    }

    pub async fn sync_discovery(
        &self,
        source_id: &str,
        discovered: &[DiscoveredUps],
    ) -> Result<Vec<MonitorDevice>, PersistenceError> {
        let now = timestamp(Utc::now());
        let mut transaction = self.pool.begin().await?;
        let names: HashSet<&str> = discovered
            .iter()
            .map(|device| device.name.as_str())
            .collect();
        for device in discovered {
            sqlx::query(
                "INSERT INTO ups_monitor_devices (id, source_id, ups_name, description, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(source_id, ups_name) DO UPDATE SET description = excluded.description, updated_at = excluded.updated_at",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(source_id)
            .bind(&device.name)
            .bind(&device.description)
            .bind(&now)
            .bind(&now)
            .execute(&mut *transaction)
            .await?;
        }
        let existing =
            sqlx::query("SELECT id, ups_name, online FROM ups_monitor_devices WHERE source_id = ?")
                .bind(source_id)
                .fetch_all(&mut *transaction)
                .await?;
        for row in existing {
            let ups_name: String = row.try_get("ups_name")?;
            if !names.contains(ups_name.as_str()) {
                let device_id: String = row.try_get("id")?;
                let online: bool = row.try_get("online")?;
                sqlx::query("UPDATE ups_monitor_devices SET online = 0, last_error = 'UPS was not returned by discovery', updated_at = ? WHERE id = ?")
                    .bind(&now).bind(&device_id).execute(&mut *transaction).await?;
                if online {
                    insert_event(
                        &mut transaction,
                        &device_id,
                        &now,
                        "disconnected",
                        "warning",
                        "UPS 已从数据源离线",
                        &[],
                    )
                    .await?;
                }
            }
        }
        sqlx::query("UPDATE ups_monitor_sources SET last_discovery_at = ?, last_error = NULL, updated_at = ? WHERE id = ?")
            .bind(&now).bind(&now).bind(source_id).execute(&mut *transaction).await?;
        transaction.commit().await?;
        self.devices_for_source(source_id).await
    }

    pub async fn discovery_due(&self, source: &MonitorSource) -> bool {
        source
            .last_discovery_at
            .as_deref()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .is_none_or(|last| last.with_timezone(&Utc) < Utc::now() - Duration::minutes(5))
    }

    pub async fn source_failure(
        &self,
        source_id: &str,
        error: &str,
    ) -> Result<(), PersistenceError> {
        let now = timestamp(Utc::now());
        sqlx::query("UPDATE ups_monitor_sources SET last_error = ?, updated_at = ? WHERE id = ?")
            .bind(error)
            .bind(&now)
            .bind(source_id)
            .execute(&self.pool)
            .await?;
        let devices = self.devices_for_source(source_id).await?;
        for device in devices {
            self.record_failure(&device.id, error, Utc::now()).await?;
        }
        Ok(())
    }

    pub async fn record_failure(
        &self,
        device_id: &str,
        error: &str,
        observed_at: DateTime<Utc>,
    ) -> Result<(), PersistenceError> {
        let now = timestamp(observed_at);
        let mut transaction = self.pool.begin().await?;
        let previous: Option<bool> =
            sqlx::query_scalar("SELECT online FROM ups_monitor_devices WHERE id = ?")
                .bind(device_id)
                .fetch_optional(&mut *transaction)
                .await?;
        if previous == Some(true) {
            insert_event(
                &mut transaction,
                device_id,
                &now,
                "disconnected",
                "warning",
                "UPS 数据连接已中断",
                &[],
            )
            .await?;
        }
        sqlx::query("UPDATE ups_monitor_devices SET online = 0, last_error = ?, updated_at = ? WHERE id = ?")
            .bind(error).bind(&now).bind(device_id).execute(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn record_success(
        &self,
        device_id: &str,
        raw: &BTreeMap<String, String>,
        observed_at: DateTime<Utc>,
    ) -> Result<(), PersistenceError> {
        let metrics = Metrics::from_raw(raw);
        let now = timestamp(observed_at);
        let bucket = observed_at.format("%Y-%m-%dT%H:%M:00Z").to_string();
        let flags_json = serde_json::to_string(&metrics.status_flags)
            .map_err(|error| PersistenceError::InvalidData(error.to_string()))?;
        let raw_json = serde_json::to_string(raw)
            .map_err(|error| PersistenceError::InvalidData(error.to_string()))?;
        let mut transaction = self.pool.begin().await?;
        let previous = sqlx::query(
            "SELECT d.online, s.enabled AS source_enabled, x.status_flags_json FROM ups_monitor_devices d JOIN ups_monitor_sources s ON s.id = d.source_id LEFT JOIN ups_monitor_snapshots x ON x.device_id = d.id WHERE d.id = ?",
        )
        .bind(device_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let (was_online, source_enabled, had_snapshot, previous_flags) = match previous {
            Some(row) => {
                let previous_flags_json = row.try_get::<Option<String>, _>("status_flags_json")?;
                (
                    row.try_get::<bool, _>("online")?,
                    row.try_get::<bool, _>("source_enabled")?,
                    previous_flags_json.is_some(),
                    parse_flags(previous_flags_json.as_deref()),
                )
            }
            None => {
                return Err(PersistenceError::InvalidData(
                    "monitor device was not found".into(),
                ));
            }
        };
        if !source_enabled {
            return Ok(());
        }
        sqlx::query("UPDATE ups_monitor_devices SET online = 1, last_seen_at = ?, last_error = NULL, updated_at = ? WHERE id = ?")
            .bind(&now).bind(&now).bind(device_id).execute(&mut *transaction).await?;
        sqlx::query(
            "INSERT INTO ups_monitor_snapshots (device_id, observed_at, status_flags_json, raw_json, charge_percent, runtime_seconds, runtime_capped, load_percent, input_voltage, output_voltage, battery_temperature) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(device_id) DO UPDATE SET observed_at=excluded.observed_at, status_flags_json=excluded.status_flags_json, raw_json=excluded.raw_json, charge_percent=excluded.charge_percent, runtime_seconds=excluded.runtime_seconds, runtime_capped=excluded.runtime_capped, load_percent=excluded.load_percent, input_voltage=excluded.input_voltage, output_voltage=excluded.output_voltage, battery_temperature=excluded.battery_temperature",
        )
        .bind(device_id).bind(&now).bind(&flags_json).bind(&raw_json)
        .bind(metrics.charge_percent).bind(metrics.runtime_seconds).bind(metrics.runtime_capped)
        .bind(metrics.load_percent).bind(metrics.input_voltage).bind(metrics.output_voltage).bind(metrics.battery_temperature)
        .execute(&mut *transaction).await?;
        sqlx::query(
            "INSERT INTO ups_monitor_samples (device_id, bucket_at, observed_at, status_flags_json, charge_percent, runtime_seconds, runtime_capped, load_percent, input_voltage, output_voltage, battery_temperature) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(device_id, bucket_at) DO NOTHING",
        )
        .bind(device_id).bind(&bucket).bind(&now).bind(&flags_json)
        .bind(metrics.charge_percent).bind(metrics.runtime_seconds).bind(metrics.runtime_capped)
        .bind(metrics.load_percent).bind(metrics.input_voltage).bind(metrics.output_voltage).bind(metrics.battery_temperature)
        .execute(&mut *transaction).await?;
        if had_snapshot && !was_online {
            insert_event(
                &mut transaction,
                device_id,
                &now,
                "connected",
                "info",
                "UPS 数据连接已恢复",
                &metrics.status_flags,
            )
            .await?;
        }
        if had_snapshot {
            insert_status_events(
                &mut transaction,
                device_id,
                &now,
                &previous_flags,
                &metrics.status_flags,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn snapshot(
        &self,
        device_id: &str,
    ) -> Result<Option<MonitorSnapshot>, PersistenceError> {
        let devices = self.device_query("WHERE d.id = ?", device_id).await?;
        let Some(device) = devices.into_iter().next() else {
            return Ok(None);
        };
        let row = sqlx::query("SELECT raw_json, input_voltage, output_voltage, battery_temperature FROM ups_monitor_snapshots WHERE device_id = ?")
            .bind(device_id).fetch_optional(&self.pool).await?;
        let Some(row) = row else {
            return Ok(Some(MonitorSnapshot {
                device,
                raw: BTreeMap::new(),
                input_voltage: None,
                output_voltage: None,
                battery_temperature: None,
            }));
        };
        Ok(Some(MonitorSnapshot {
            device,
            raw: serde_json::from_str(&row.try_get::<String, _>("raw_json")?).unwrap_or_default(),
            input_voltage: row.try_get("input_voltage")?,
            output_voltage: row.try_get("output_voltage")?,
            battery_temperature: row.try_get("battery_temperature")?,
        }))
    }

    pub async fn history(
        &self,
        device_id: &str,
        since: DateTime<Utc>,
    ) -> Result<Vec<MonitorSample>, PersistenceError> {
        let rows = sqlx::query("SELECT * FROM ups_monitor_samples WHERE device_id = ? AND observed_at >= ? ORDER BY observed_at")
            .bind(device_id).bind(timestamp(since)).fetch_all(&self.pool).await?;
        rows.iter()
            .map(|row| {
                Ok(MonitorSample {
                    observed_at: row.try_get("observed_at")?,
                    status_flags: parse_flags(Some(
                        &row.try_get::<String, _>("status_flags_json")?,
                    )),
                    charge_percent: row.try_get("charge_percent")?,
                    runtime_seconds: row.try_get("runtime_seconds")?,
                    runtime_capped: row.try_get("runtime_capped")?,
                    load_percent: row.try_get("load_percent")?,
                    input_voltage: row.try_get("input_voltage")?,
                    output_voltage: row.try_get("output_voltage")?,
                    battery_temperature: row.try_get("battery_temperature")?,
                })
            })
            .collect()
    }

    pub async fn events(
        &self,
        device_id: &str,
        limit: u16,
    ) -> Result<Vec<MonitorEvent>, PersistenceError> {
        let rows = sqlx::query("SELECT * FROM ups_monitor_events WHERE device_id = ? ORDER BY occurred_at DESC LIMIT ?")
            .bind(device_id).bind(i64::from(limit)).fetch_all(&self.pool).await?;
        rows.iter()
            .map(|row| {
                Ok(MonitorEvent {
                    id: row.try_get("id")?,
                    occurred_at: row.try_get("occurred_at")?,
                    kind: row.try_get("kind")?,
                    severity: row.try_get("severity")?,
                    message: row.try_get("message")?,
                    status_flags: parse_flags(Some(
                        &row.try_get::<String, _>("status_flags_json")?,
                    )),
                })
            })
            .collect()
    }

    pub async fn prune_history(&self) -> Result<u64, PersistenceError> {
        let cutoff = timestamp(Utc::now() - Duration::days(90));
        let samples = sqlx::query("DELETE FROM ups_monitor_samples WHERE observed_at < ?")
            .bind(&cutoff)
            .execute(&self.pool)
            .await?
            .rows_affected();
        let events = sqlx::query("DELETE FROM ups_monitor_events WHERE occurred_at < ?")
            .bind(&cutoff)
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(samples + events)
    }
}

fn validate_source(name: &str, address: &str, port: u16) -> Result<(), PersistenceError> {
    if name.trim().is_empty() || name.trim().chars().count() > 100 {
        return Err(PersistenceError::InvalidData(
            "source name must contain 1 to 100 characters".into(),
        ));
    }
    if address.trim().is_empty()
        || address.trim().len() > 255
        || address.chars().any(char::is_whitespace)
    {
        return Err(PersistenceError::InvalidData(
            "source address must be a hostname or IP address without whitespace".into(),
        ));
    }
    if port == 0 {
        return Err(PersistenceError::InvalidData(
            "source port must be between 1 and 65535".into(),
        ));
    }
    Ok(())
}

fn source_write_error(error: sqlx::Error) -> PersistenceError {
    match error {
        sqlx::Error::Database(ref inner) if inner.is_unique_violation() => {
            PersistenceError::Conflict(
                "a UPS monitor source with this address and port already exists".into(),
            )
        }
        error => PersistenceError::Sqlx(error),
    }
}

fn source_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<MonitorSource, PersistenceError> {
    Ok(MonitorSource {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        address: row.try_get("address")?,
        port: u16::try_from(row.try_get::<i64, _>("port")?)
            .map_err(|_| PersistenceError::InvalidData("invalid monitor source port".into()))?,
        enabled: row.try_get("enabled")?,
        last_discovery_at: row.try_get("last_discovery_at")?,
        last_error: row.try_get("last_error")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn device_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<MonitorDevice, PersistenceError> {
    let raw: BTreeMap<String, String> = row
        .try_get::<Option<String>, _>("raw_json")?
        .and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_default();
    Ok(MonitorDevice {
        id: row.try_get("id")?,
        source_id: row.try_get("source_id")?,
        source_name: row.try_get("source_name")?,
        ups_name: row.try_get("ups_name")?,
        description: row.try_get("description")?,
        online: row.try_get("online")?,
        last_seen_at: row.try_get("last_seen_at")?,
        last_error: row.try_get("last_error")?,
        observed_at: row.try_get("observed_at")?,
        status_flags: parse_flags(
            row.try_get::<Option<String>, _>("status_flags_json")?
                .as_deref(),
        ),
        charge_percent: row.try_get("charge_percent")?,
        runtime_seconds: row.try_get("runtime_seconds")?,
        runtime_capped: row
            .try_get::<Option<bool>, _>("runtime_capped")?
            .unwrap_or(false),
        load_percent: row.try_get("load_percent")?,
        manufacturer: raw
            .get("device.mfr")
            .or_else(|| raw.get("ups.mfr"))
            .cloned(),
        model: raw
            .get("device.model")
            .or_else(|| raw.get("ups.model"))
            .cloned(),
    })
}

fn parse_flags(value: Option<&str>) -> Vec<String> {
    value
        .and_then(|value| serde_json::from_str(value).ok())
        .unwrap_or_default()
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Secs, true)
}

async fn insert_event(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    device_id: &str,
    occurred_at: &str,
    kind: &str,
    severity: &str,
    message: &str,
    flags: &[String],
) -> Result<(), PersistenceError> {
    sqlx::query("INSERT INTO ups_monitor_events (device_id, occurred_at, kind, severity, message, status_flags_json) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(device_id).bind(occurred_at).bind(kind).bind(severity).bind(message).bind(serde_json::to_string(flags).unwrap_or_else(|_| "[]".into()))
        .execute(&mut **transaction).await?;
    Ok(())
}

async fn insert_status_events(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    device_id: &str,
    occurred_at: &str,
    previous: &[String],
    current: &[String],
) -> Result<(), PersistenceError> {
    let transitions = [
        (
            "OB",
            "on_battery",
            "warning",
            "UPS 已切换为电池供电",
            "mains_restored",
            "info",
            "市电已恢复",
        ),
        (
            "LB",
            "low_battery",
            "critical",
            "UPS 电池电量低",
            "low_battery_cleared",
            "info",
            "UPS 低电量状态已解除",
        ),
        (
            "RB",
            "replace_battery",
            "warning",
            "UPS 提示需要更换电池",
            "replace_battery_cleared",
            "info",
            "UPS 更换电池提示已解除",
        ),
        (
            "BYPASS",
            "bypass_started",
            "warning",
            "UPS 已进入旁路模式",
            "bypass_cleared",
            "info",
            "UPS 已退出旁路模式",
        ),
        (
            "OFF",
            "output_off",
            "critical",
            "UPS 输出已关闭",
            "output_restored",
            "info",
            "UPS 输出已恢复",
        ),
    ];
    for (
        flag,
        added_kind,
        added_severity,
        added_message,
        removed_kind,
        removed_severity,
        removed_message,
    ) in transitions
    {
        let was = previous.iter().any(|value| value == flag);
        let is = current.iter().any(|value| value == flag);
        if !was && is {
            insert_event(
                transaction,
                device_id,
                occurred_at,
                added_kind,
                added_severity,
                added_message,
                current,
            )
            .await?;
        }
        if was && !is {
            insert_event(
                transaction,
                device_id,
                occurred_at,
                removed_kind,
                removed_severity,
                removed_message,
                current,
            )
            .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::Database;

    #[tokio::test]
    async fn source_crud_discovery_and_cascade_work() {
        let database = Database::connect("sqlite::memory:").await.unwrap();
        database.migrate().await.unwrap();
        let repo = database.monitor();
        let source = repo
            .create_source("NAS", "192.168.1.1", 3493, true)
            .await
            .unwrap();
        let devices = repo
            .sync_discovery(
                &source.id,
                &[DiscoveredUps {
                    name: "ups0".into(),
                    description: Some("UGREEN".into()),
                }],
            )
            .await
            .unwrap();
        assert_eq!(devices.len(), 1);
        assert!(repo.delete_source(&source.id).await.unwrap());
        assert!(repo.overview_devices().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn changing_an_endpoint_does_not_reuse_devices_or_history() {
        let database = Database::connect("sqlite::memory:").await.unwrap();
        database.migrate().await.unwrap();
        let repo = database.monitor();
        let source = repo
            .create_source("NAS", "192.168.1.1", 3493, true)
            .await
            .unwrap();
        let device = repo
            .sync_discovery(
                &source.id,
                &[DiscoveredUps {
                    name: "ups0".into(),
                    description: None,
                }],
            )
            .await
            .unwrap()
            .remove(0);
        repo.record_success(
            &device.id,
            &BTreeMap::from([("ups.status".into(), "OL".into())]),
            Utc::now(),
        )
        .await
        .unwrap();

        let error = repo
            .update_source(&source.id, "NAS", "10.0.0.10", 3493, true, false)
            .await
            .unwrap_err();
        assert!(matches!(error, PersistenceError::Conflict(_)));
        assert_eq!(repo.devices_for_source(&source.id).await.unwrap().len(), 1);

        repo.update_source(&source.id, "NAS", "10.0.0.10", 3493, true, true)
            .await
            .unwrap();

        assert!(
            repo.devices_for_source(&source.id)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(repo.snapshot(&device.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn duplicate_source_endpoint_is_a_conflict() {
        let database = Database::connect("sqlite::memory:").await.unwrap();
        database.migrate().await.unwrap();
        let repo = database.monitor();
        repo.create_source("one", "192.168.1.1", 3493, true)
            .await
            .unwrap();
        let error = repo
            .create_source("two", "192.168.1.1", 3493, true)
            .await
            .unwrap_err();
        assert!(matches!(error, PersistenceError::Conflict(_)));
    }

    #[tokio::test]
    async fn samples_are_bucketed_and_events_are_not_duplicated() {
        let database = Database::connect("sqlite::memory:").await.unwrap();
        database.migrate().await.unwrap();
        let repo = database.monitor();
        let source = repo
            .create_source("NAS", "192.168.1.1", 3493, true)
            .await
            .unwrap();
        let device = repo
            .sync_discovery(
                &source.id,
                &[DiscoveredUps {
                    name: "ups0".into(),
                    description: None,
                }],
            )
            .await
            .unwrap()
            .remove(0);
        let mut raw = BTreeMap::from([
            ("ups.status".into(), "OL".into()),
            ("battery.runtime".into(), "65535".into()),
        ]);
        let now = Utc::now();
        repo.record_success(&device.id, &raw, now).await.unwrap();
        repo.record_success(&device.id, &raw, now + Duration::seconds(5))
            .await
            .unwrap();
        assert!(repo.events(&device.id, 100).await.unwrap().is_empty());
        raw.insert("ups.status".into(), "OB".into());
        repo.record_success(&device.id, &raw, now + Duration::seconds(10))
            .await
            .unwrap();
        repo.record_success(&device.id, &raw, now + Duration::seconds(15))
            .await
            .unwrap();
        assert_eq!(
            repo.history(&device.id, now - Duration::minutes(1))
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            repo.events(&device.id, 100)
                .await
                .unwrap()
                .iter()
                .filter(|event| event.kind == "on_battery")
                .count(),
            1
        );
        assert!(
            repo.snapshot(&device.id)
                .await
                .unwrap()
                .unwrap()
                .device
                .runtime_capped
        );
        assert_eq!(
            repo.snapshot(&device.id)
                .await
                .unwrap()
                .unwrap()
                .device
                .runtime_seconds,
            None
        );
    }

    #[tokio::test]
    async fn a_success_result_cannot_reactivate_a_disabled_source() {
        let database = Database::connect("sqlite::memory:").await.unwrap();
        database.migrate().await.unwrap();
        let repo = database.monitor();
        let source = repo
            .create_source("NAS", "192.168.1.1", 3493, true)
            .await
            .unwrap();
        let device = repo
            .sync_discovery(
                &source.id,
                &[DiscoveredUps {
                    name: "ups0".into(),
                    description: None,
                }],
            )
            .await
            .unwrap()
            .remove(0);
        let raw = BTreeMap::from([("ups.status".into(), "OL".into())]);
        repo.record_success(&device.id, &raw, Utc::now())
            .await
            .unwrap();
        repo.update_source(
            &source.id,
            &source.name,
            &source.address,
            source.port,
            false,
            false,
        )
        .await
        .unwrap();

        repo.record_success(&device.id, &raw, Utc::now())
            .await
            .unwrap();

        assert!(
            !repo
                .snapshot(&device.id)
                .await
                .unwrap()
                .unwrap()
                .device
                .online
        );
    }
}
