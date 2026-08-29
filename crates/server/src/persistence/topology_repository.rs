use std::{collections::BTreeMap, fmt, str::FromStr};

use chrono::Utc;
use nwm_common::{
    ApplyState, BindingId, ConfigRevisionId, CredentialId, HostId, NutServerId, UpsId,
};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use super::PersistenceError;

pub const SHARED_CLIENT_USERNAME: &str = "nwm";
pub const SHARED_CLIENT_PASSWORD: &str = "nwm";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ServerRecord {
    pub id: NutServerId,
    pub host_id: HostId,
    pub ups_name: String,
    pub listen_address: String,
    pub listen_port: u16,
    pub enabled: bool,
    pub apply_state: ApplyState,
    pub applied_revision_id: Option<ConfigRevisionId>,
    pub shutdown: ShutdownOptions,
    pub device: DeviceRecord,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ShutdownOptions {
    pub trigger_mode: ShutdownTriggerMode,
    pub battery_level_percent: u8,
    pub on_battery_seconds: u32,
    pub host_sync_seconds: u16,
    pub final_delay_seconds: u16,
    pub powerdown_enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownTriggerMode {
    BatteryLevel,
    OnBatteryTimer,
}

impl fmt::Display for ShutdownTriggerMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::BatteryLevel => "battery_level",
            Self::OnBatteryTimer => "on_battery_timer",
        })
    }
}

impl FromStr for ShutdownTriggerMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "battery_level" => Ok(Self::BatteryLevel),
            "on_battery_timer" => Ok(Self::OnBatteryTimer),
            value => Err(format!("unsupported shutdown trigger mode: {value}")),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeviceRecord {
    pub id: UpsId,
    pub name: String,
    pub driver: String,
    pub port: String,
    pub selectors: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct CredentialRecord {
    pub id: CredentialId,
    pub username: String,
    pub secret: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BindingRecord {
    pub id: BindingId,
    pub server_id: NutServerId,
    pub client_host_id: HostId,
    pub username: String,
    pub apply_state: ApplyState,
    pub applied_revision_id: Option<ConfigRevisionId>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug)]
pub struct RevisionRecord {
    pub id: ConfigRevisionId,
    pub revision_number: i64,
    pub manifest_json: String,
    pub backup_path: String,
}

/// USB bus addresses are assigned by the kernel and may change after a reboot,
/// reconnect, or hypervisor USB reattachment. Keep them in scan results for
/// diagnostics, but never make them part of a durable device configuration.
pub(crate) fn is_durable_usb_selector(key: &str) -> bool {
    !matches!(
        key.to_ascii_lowercase().as_str(),
        "bus" | "device" | "busport"
    )
}

#[derive(Clone)]
pub struct TopologyRepository {
    pool: SqlitePool,
}

impl TopologyRepository {
    pub(super) fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn select_server_device(
        &self,
        host_id: HostId,
        ups_name: &str,
        driver: &str,
        port: &str,
        selectors: &BTreeMap<String, String>,
    ) -> Result<ServerRecord, PersistenceError> {
        let ups_name = required("ups_name", ups_name)?;
        if !ups_name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        {
            return Err(PersistenceError::InvalidData(
                "ups_name may contain only ASCII letters, numbers, '_' and '-'".into(),
            ));
        }
        let driver = required("driver", driver)?;
        let port = required("port", port)?;
        let mut transaction = self.pool.begin().await?;
        let other_enabled: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM nut_servers WHERE enabled = 1 AND host_id != ?",
        )
        .bind(host_id.to_string())
        .fetch_one(&mut *transaction)
        .await?;
        if other_enabled > 0 {
            return Err(PersistenceError::Conflict(
                "V1 supports only one enabled NUT Server".into(),
            ));
        }

        let existing: Option<String> =
            sqlx::query_scalar("SELECT id FROM nut_servers WHERE host_id = ?")
                .bind(host_id.to_string())
                .fetch_optional(&mut *transaction)
                .await?;
        let server_id: NutServerId = existing
            .as_deref()
            .map(|value| parse(value, "server id"))
            .transpose()?
            .unwrap_or_default();
        if existing.is_some() {
            sqlx::query(
                "UPDATE nut_servers SET ups_name = ?, apply_state = 'unconfigured' WHERE id = ?",
            )
            .bind(&ups_name)
            .bind(server_id.to_string())
            .execute(&mut *transaction)
            .await?;
        } else {
            sqlx::query(
                "INSERT INTO nut_servers (id, host_id, ups_name, listen_address, listen_port, enabled, apply_state) \
                 VALUES (?, ?, ?, '0.0.0.0', 3493, 0, 'unconfigured')",
            )
            .bind(server_id.to_string())
            .bind(host_id.to_string())
            .bind(&ups_name)
            .execute(&mut *transaction)
            .await?;
            let credential_id = CredentialId::new();
            sqlx::query(
                "INSERT INTO nut_credentials (id, server_id, username, secret_ciphertext, created_at) \
                 VALUES (?, ?, 'nwm_primary', ?, ?)",
            )
            .bind(credential_id.to_string())
            .bind(server_id.to_string())
            .bind(generate_secret().into_bytes())
            .bind(Utc::now().to_rfc3339())
            .execute(&mut *transaction)
            .await?;
            let credential_id = CredentialId::new();
            sqlx::query(
                "INSERT INTO nut_credentials (id, server_id, username, secret_ciphertext, created_at) \
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(credential_id.to_string())
            .bind(server_id.to_string())
            .bind(SHARED_CLIENT_USERNAME)
            .bind(SHARED_CLIENT_PASSWORD.as_bytes())
            .bind(Utc::now().to_rfc3339())
            .execute(&mut *transaction)
            .await?;
        }

        let device_id: Option<String> =
            sqlx::query_scalar("SELECT id FROM ups_devices WHERE server_id = ?")
                .bind(server_id.to_string())
                .fetch_optional(&mut *transaction)
                .await?;
        let durable_selectors = selectors
            .iter()
            .filter(|(key, _)| is_durable_usb_selector(key))
            .collect::<BTreeMap<_, _>>();
        let selectors_json = serde_json::to_string(&durable_selectors)
            .map_err(|error| PersistenceError::InvalidData(error.to_string()))?;
        if let Some(device_id) = device_id {
            sqlx::query(
                "UPDATE ups_devices SET name = ?, driver = ?, port = ?, selectors_json = ? WHERE id = ?",
            )
            .bind(&ups_name)
            .bind(&driver)
            .bind(&port)
            .bind(selectors_json)
            .bind(device_id)
            .execute(&mut *transaction)
            .await?;
        } else {
            sqlx::query(
                "INSERT INTO ups_devices (id, server_id, name, driver, port, selectors_json) VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(UpsId::new().to_string())
            .bind(server_id.to_string())
            .bind(&ups_name)
            .bind(&driver)
            .bind(&port)
            .bind(selectors_json)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        self.get_server(server_id)
            .await?
            .ok_or_else(|| PersistenceError::InvalidData("created server disappeared".into()))
    }

    pub async fn get_server(
        &self,
        id: NutServerId,
    ) -> Result<Option<ServerRecord>, PersistenceError> {
        sqlx::query(
            "SELECT s.*, d.id AS device_id, d.name AS device_name, d.driver, d.port, d.selectors_json \
             FROM nut_servers s JOIN ups_devices d ON d.server_id = s.id WHERE s.id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .map(decode_server)
        .transpose()
    }

    pub async fn get_server_by_host(
        &self,
        host_id: HostId,
    ) -> Result<Option<ServerRecord>, PersistenceError> {
        sqlx::query(
            "SELECT s.*, d.id AS device_id, d.name AS device_name, d.driver, d.port, d.selectors_json \
             FROM nut_servers s JOIN ups_devices d ON d.server_id = s.id WHERE s.host_id = ?",
        )
        .bind(host_id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .map(decode_server)
        .transpose()
    }

    pub async fn list_servers(&self) -> Result<Vec<ServerRecord>, PersistenceError> {
        let rows = sqlx::query(
            "SELECT s.*, d.id AS device_id, d.name AS device_name, d.driver, d.port, d.selectors_json \
             FROM nut_servers s JOIN ups_devices d ON d.server_id = s.id ORDER BY s.id",
        )
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter().map(decode_server).collect()
    }

    pub async fn credentials(
        &self,
        server_id: NutServerId,
    ) -> Result<Vec<CredentialRecord>, PersistenceError> {
        let rows = sqlx::query(
            "SELECT id, username, secret_ciphertext FROM nut_credentials WHERE server_id = ? ORDER BY created_at, id",
        )
        .bind(server_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let secret = String::from_utf8(row.try_get::<Vec<u8>, _>("secret_ciphertext")?)
                    .map_err(|error| PersistenceError::InvalidData(error.to_string()))?;
                Ok(CredentialRecord {
                    id: parse(&row.try_get::<String, _>("id")?, "credential id")?,
                    username: row.try_get("username")?,
                    secret,
                })
            })
            .collect()
    }

    pub async fn set_server_applied(
        &self,
        id: NutServerId,
        revision: ConfigRevisionId,
    ) -> Result<(), PersistenceError> {
        sqlx::query(
            "UPDATE nut_servers SET enabled = 1, apply_state = 'applied', applied_revision_id = ? WHERE id = ?",
        )
        .bind(revision.to_string())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_server_failed(&self, id: NutServerId) -> Result<(), PersistenceError> {
        sqlx::query("UPDATE nut_servers SET apply_state = 'failed' WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_shutdown_options(
        &self,
        id: NutServerId,
        options: &ShutdownOptions,
    ) -> Result<ServerRecord, PersistenceError> {
        if !(5..=300).contains(&options.host_sync_seconds) {
            return Err(PersistenceError::InvalidData(
                "host_sync_seconds must be between 5 and 300".into(),
            ));
        }
        if options.final_delay_seconds > 120 {
            return Err(PersistenceError::InvalidData(
                "final_delay_seconds must be between 0 and 120".into(),
            ));
        }
        if options.final_delay_seconds > options.host_sync_seconds {
            return Err(PersistenceError::InvalidData(
                "final_delay_seconds must not exceed host_sync_seconds".into(),
            ));
        }
        if !(5..=50).contains(&options.battery_level_percent) {
            return Err(PersistenceError::InvalidData(
                "battery_level_percent must be between 5 and 50".into(),
            ));
        }
        if !(60..=7200).contains(&options.on_battery_seconds) {
            return Err(PersistenceError::InvalidData(
                "on_battery_seconds must be between 60 and 7200".into(),
            ));
        }
        let result = sqlx::query(
            "UPDATE nut_servers SET shutdown_host_sync_seconds = ?, shutdown_final_delay_seconds = ?, \
             shutdown_powerdown_enabled = ?, shutdown_trigger_mode = ?, \
             shutdown_battery_level_percent = ?, shutdown_on_battery_seconds = ?, \
             apply_state = 'unconfigured' WHERE id = ?",
        )
        .bind(i64::from(options.host_sync_seconds))
        .bind(i64::from(options.final_delay_seconds))
        .bind(i64::from(options.powerdown_enabled))
        .bind(options.trigger_mode.to_string())
        .bind(i64::from(options.battery_level_percent))
        .bind(i64::from(options.on_battery_seconds))
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(PersistenceError::InvalidData(
                "server does not exist".into(),
            ));
        }
        self.get_server(id)
            .await?
            .ok_or_else(|| PersistenceError::InvalidData("updated server disappeared".into()))
    }

    pub async fn create_binding(
        &self,
        server_id: NutServerId,
        client_host_id: HostId,
    ) -> Result<BindingRecord, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let binding_id = BindingId::new();
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO nut_client_bindings (id, server_id, client_host_id, apply_state, created_at, updated_at) \
             VALUES (?, ?, ?, 'unconfigured', ?, ?)",
        )
        .bind(binding_id.to_string())
        .bind(server_id.to_string())
        .bind(client_host_id.to_string())
        .bind(&now)
        .bind(&now)
        .execute(&mut *transaction)
        .await
        .map_err(|error| match error {
            sqlx::Error::Database(ref inner) if inner.is_unique_violation() => {
                PersistenceError::Conflict("client already has a binding".into())
            }
            error => error.into(),
        })?;
        transaction.commit().await?;
        self.get_binding(binding_id)
            .await?
            .ok_or_else(|| PersistenceError::InvalidData("created binding disappeared".into()))
    }

    pub async fn get_binding(
        &self,
        id: BindingId,
    ) -> Result<Option<BindingRecord>, PersistenceError> {
        sqlx::query("SELECT b.*, ? AS username FROM nut_client_bindings b WHERE b.id = ?")
            .bind(SHARED_CLIENT_USERNAME)
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?
            .map(decode_binding)
            .transpose()
    }

    pub async fn list_bindings(&self) -> Result<Vec<BindingRecord>, PersistenceError> {
        sqlx::query(
            "SELECT b.*, ? AS username FROM nut_client_bindings b ORDER BY b.created_at, b.id",
        )
        .bind(SHARED_CLIENT_USERNAME)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(decode_binding)
        .collect()
    }

    pub async fn clear_host_configuration(&self, host_id: HostId) -> Result<(), PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("DELETE FROM nut_client_bindings WHERE client_host_id = ?")
            .bind(host_id.to_string())
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM nut_servers WHERE host_id = ?")
            .bind(host_id.to_string())
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM config_revisions WHERE host_id = ?")
            .bind(host_id.to_string())
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn binding_credential(
        &self,
        id: BindingId,
    ) -> Result<Option<CredentialRecord>, PersistenceError> {
        sqlx::query(
            "SELECT c.id, c.username, c.secret_ciphertext \
             FROM nut_client_bindings b \
             JOIN nut_credentials c ON c.server_id = b.server_id AND c.username = ? \
             WHERE b.id = ?",
        )
        .bind(SHARED_CLIENT_USERNAME)
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .map(|row| {
            Ok(CredentialRecord {
                id: parse(&row.try_get::<String, _>("id")?, "credential id")?,
                username: row.try_get("username")?,
                secret: String::from_utf8(row.try_get::<Vec<u8>, _>("secret_ciphertext")?)
                    .map_err(|error| PersistenceError::InvalidData(error.to_string()))?,
            })
        })
        .transpose()
    }

    pub async fn set_binding_applied(
        &self,
        id: BindingId,
        revision: ConfigRevisionId,
    ) -> Result<(), PersistenceError> {
        sqlx::query(
            "UPDATE nut_client_bindings SET apply_state = 'applied', applied_revision_id = ?, updated_at = ? WHERE id = ?",
        )
        .bind(revision.to_string())
        .bind(Utc::now().to_rfc3339())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_binding_failed(&self, id: BindingId) -> Result<(), PersistenceError> {
        sqlx::query(
            "UPDATE nut_client_bindings SET apply_state = 'failed', updated_at = ? WHERE id = ?",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn latest_revision(
        &self,
        host_id: HostId,
    ) -> Result<Option<RevisionRecord>, PersistenceError> {
        sqlx::query(
            "SELECT * FROM config_revisions WHERE host_id = ? ORDER BY revision_number DESC LIMIT 1",
        )
        .bind(host_id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .map(|row| {
            Ok(RevisionRecord {
                id: parse(&row.try_get::<String, _>("id")?, "revision id")?,
                revision_number: row.try_get("revision_number")?,
                manifest_json: row.try_get("manifest_json")?,
                backup_path: row.try_get("backup_path")?,
            })
        })
        .transpose()
    }

    pub async fn create_revision(
        &self,
        host_id: HostId,
        manifest_json: &str,
        backup_path: &str,
    ) -> Result<ConfigRevisionId, PersistenceError> {
        let id = ConfigRevisionId::new();
        sqlx::query(
            "INSERT INTO config_revisions (id, host_id, revision_number, manifest_json, backup_path, created_at) \
             VALUES (?, ?, COALESCE((SELECT MAX(revision_number) + 1 FROM config_revisions WHERE host_id = ?), 1), ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(host_id.to_string())
        .bind(host_id.to_string())
        .bind(manifest_json)
        .bind(backup_path)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(id)
    }
}

fn decode_server(row: sqlx::sqlite::SqliteRow) -> Result<ServerRecord, PersistenceError> {
    Ok(ServerRecord {
        id: parse(&row.try_get::<String, _>("id")?, "server id")?,
        host_id: parse(&row.try_get::<String, _>("host_id")?, "host id")?,
        ups_name: row.try_get("ups_name")?,
        listen_address: row.try_get("listen_address")?,
        listen_port: u16::try_from(row.try_get::<i64, _>("listen_port")?)
            .map_err(|error| PersistenceError::InvalidData(error.to_string()))?,
        enabled: row.try_get::<i64, _>("enabled")? != 0,
        apply_state: ApplyState::from_str(&row.try_get::<String, _>("apply_state")?)
            .map_err(PersistenceError::InvalidData)?,
        applied_revision_id: row
            .try_get::<Option<String>, _>("applied_revision_id")?
            .map(|value| parse(&value, "revision id"))
            .transpose()?,
        shutdown: ShutdownOptions {
            trigger_mode: ShutdownTriggerMode::from_str(
                &row.try_get::<String, _>("shutdown_trigger_mode")?,
            )
            .map_err(PersistenceError::InvalidData)?,
            battery_level_percent: u8::try_from(
                row.try_get::<i64, _>("shutdown_battery_level_percent")?,
            )
            .map_err(|error| PersistenceError::InvalidData(error.to_string()))?,
            on_battery_seconds: u32::try_from(
                row.try_get::<i64, _>("shutdown_on_battery_seconds")?,
            )
            .map_err(|error| PersistenceError::InvalidData(error.to_string()))?,
            host_sync_seconds: u16::try_from(row.try_get::<i64, _>("shutdown_host_sync_seconds")?)
                .map_err(|error| PersistenceError::InvalidData(error.to_string()))?,
            final_delay_seconds: u16::try_from(
                row.try_get::<i64, _>("shutdown_final_delay_seconds")?,
            )
            .map_err(|error| PersistenceError::InvalidData(error.to_string()))?,
            powerdown_enabled: row.try_get::<i64, _>("shutdown_powerdown_enabled")? != 0,
        },
        device: DeviceRecord {
            id: parse(&row.try_get::<String, _>("device_id")?, "device id")?,
            name: row.try_get("device_name")?,
            driver: row.try_get("driver")?,
            port: row.try_get("port")?,
            selectors: serde_json::from_str(&row.try_get::<String, _>("selectors_json")?)
                .map_err(|error| PersistenceError::InvalidData(error.to_string()))?,
        },
    })
}

fn decode_binding(row: sqlx::sqlite::SqliteRow) -> Result<BindingRecord, PersistenceError> {
    Ok(BindingRecord {
        id: parse(&row.try_get::<String, _>("id")?, "binding id")?,
        server_id: parse(&row.try_get::<String, _>("server_id")?, "server id")?,
        client_host_id: parse(
            &row.try_get::<String, _>("client_host_id")?,
            "client host id",
        )?,
        username: row.try_get("username")?,
        apply_state: ApplyState::from_str(&row.try_get::<String, _>("apply_state")?)
            .map_err(PersistenceError::InvalidData)?,
        applied_revision_id: row
            .try_get::<Option<String>, _>("applied_revision_id")?
            .map(|value| parse(&value, "revision id"))
            .transpose()?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn required(label: &str, value: &str) -> Result<String, PersistenceError> {
    let value = value.trim();
    if value.is_empty() || value.len() > 128 {
        return Err(PersistenceError::InvalidData(format!(
            "{label} must contain 1 to 128 characters"
        )));
    }
    if value.contains(['\n', '\r', '[', ']']) {
        return Err(PersistenceError::InvalidData(format!(
            "{label} contains unsupported characters"
        )));
    }
    Ok(value.to_owned())
}

fn generate_secret() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
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

#[cfg(test)]
mod tests {
    use nwm_common::HostRole;

    use crate::persistence::{CreateHost, Database};

    use super::*;

    #[tokio::test]
    async fn server_and_client_topology_round_trip_without_exposing_secrets() {
        let database = Database::connect("sqlite::memory:").await.unwrap();
        database.migrate().await.unwrap();
        let server_host = database
            .hosts()
            .create(CreateHost {
                name: "server".into(),
                address: "192.0.2.10".into(),
                ssh_port: 22,
                username: "root".into(),
                role: HostRole::Server,
            })
            .await
            .unwrap();
        let client_host = database
            .hosts()
            .create(CreateHost {
                name: "client".into(),
                address: "192.0.2.11".into(),
                ssh_port: 22,
                username: "root".into(),
                role: HostRole::Client,
            })
            .await
            .unwrap();
        let repository = database.topology();
        let server = repository
            .select_server_device(
                server_host.id,
                "ups",
                "usbhid-ups",
                "auto",
                &BTreeMap::from([
                    ("vendorid".into(), "051d".into()),
                    ("productid".into(), "0002".into()),
                    ("serial".into(), "ABC".into()),
                    ("bus".into(), "001".into()),
                    ("device".into(), "002".into()),
                    ("busport".into(), "003".into()),
                ]),
            )
            .await
            .unwrap();
        assert_eq!(
            server.device.selectors,
            BTreeMap::from([
                ("productid".into(), "0002".into()),
                ("serial".into(), "ABC".into()),
                ("vendorid".into(), "051d".into()),
            ])
        );
        let credentials = repository.credentials(server.id).await.unwrap();
        assert!(!server.shutdown.powerdown_enabled);
        assert_eq!(credentials.len(), 2);
        let primary = credentials
            .iter()
            .find(|credential| credential.username == "nwm_primary")
            .unwrap();
        assert_eq!(primary.secret.len(), 64);
        let shared_client = credentials
            .iter()
            .find(|credential| credential.username == SHARED_CLIENT_USERNAME)
            .unwrap();
        assert_eq!(shared_client.secret, SHARED_CLIENT_PASSWORD);

        let binding = repository
            .create_binding(server.id, client_host.id)
            .await
            .unwrap();
        assert_eq!(binding.username, SHARED_CLIENT_USERNAME);
        let binding_credential = repository
            .binding_credential(binding.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(binding_credential.username, SHARED_CLIENT_USERNAME);
        assert_eq!(binding_credential.secret, SHARED_CLIENT_PASSWORD);
        assert_eq!(repository.list_bindings().await.unwrap(), vec![binding]);
        assert_eq!(repository.credentials(server.id).await.unwrap().len(), 2);
        repository
            .clear_host_configuration(client_host.id)
            .await
            .unwrap();
        assert!(repository.list_bindings().await.unwrap().is_empty());
        assert_eq!(repository.credentials(server.id).await.unwrap().len(), 2);
        repository
            .clear_host_configuration(server_host.id)
            .await
            .unwrap();
        assert!(repository.list_servers().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn invalid_ups_name_is_rejected_before_persistence() {
        let database = Database::connect("sqlite::memory:").await.unwrap();
        database.migrate().await.unwrap();
        let host = database
            .hosts()
            .create(CreateHost {
                name: "server".into(),
                address: "192.0.2.10".into(),
                ssh_port: 22,
                username: "root".into(),
                role: HostRole::Server,
            })
            .await
            .unwrap();
        let result = database
            .topology()
            .select_server_device(host.id, "bad name", "usbhid-ups", "auto", &BTreeMap::new())
            .await;
        assert!(matches!(result, Err(PersistenceError::InvalidData(_))));
    }

    #[tokio::test]
    async fn shutdown_options_are_validated_and_persisted() {
        let database = Database::connect("sqlite::memory:").await.unwrap();
        database.migrate().await.unwrap();
        let host = database
            .hosts()
            .create(CreateHost {
                name: "server".into(),
                address: "192.0.2.10".into(),
                ssh_port: 22,
                username: "root".into(),
                role: HostRole::Server,
            })
            .await
            .unwrap();
        let repository = database.topology();
        let server = repository
            .select_server_device(host.id, "ups", "usbhid-ups", "auto", &BTreeMap::new())
            .await
            .unwrap();
        let updated = repository
            .update_shutdown_options(
                server.id,
                &ShutdownOptions {
                    trigger_mode: ShutdownTriggerMode::OnBatteryTimer,
                    battery_level_percent: 25,
                    on_battery_seconds: 600,
                    host_sync_seconds: 45,
                    final_delay_seconds: 10,
                    powerdown_enabled: false,
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.shutdown.host_sync_seconds, 45);
        assert_eq!(
            updated.shutdown.trigger_mode,
            ShutdownTriggerMode::OnBatteryTimer
        );
        assert_eq!(updated.shutdown.battery_level_percent, 25);
        assert_eq!(updated.shutdown.on_battery_seconds, 600);
        assert_eq!(updated.shutdown.final_delay_seconds, 10);
        assert!(!updated.shutdown.powerdown_enabled);
        assert_eq!(updated.apply_state, ApplyState::Unconfigured);

        let invalid = repository
            .update_shutdown_options(
                server.id,
                &ShutdownOptions {
                    trigger_mode: ShutdownTriggerMode::BatteryLevel,
                    battery_level_percent: 4,
                    on_battery_seconds: 59,
                    host_sync_seconds: 15,
                    final_delay_seconds: 5,
                    powerdown_enabled: true,
                },
            )
            .await;
        assert!(matches!(invalid, Err(PersistenceError::InvalidData(_))));
    }
}
