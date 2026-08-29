use std::str::FromStr;

use chrono::{DateTime, Utc};
use nwm_common::{Host, HostId, HostRole, PlatformInfo, PlatformKind};
use sqlx::{Row, SqlitePool, sqlite::SqliteRow};

use super::PersistenceError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateHost {
    pub name: String,
    pub address: String,
    pub ssh_port: u16,
    pub username: String,
    pub role: HostRole,
}

#[derive(Clone)]
pub struct HostRepository {
    pool: SqlitePool,
}

impl HostRepository {
    pub(super) fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, input: CreateHost) -> Result<Host, PersistenceError> {
        let name = required("name", input.name)?;
        let address = required("address", input.address)?;
        let username = required("username", input.username)?;
        validate_address(&address)?;
        validate_username(&username)?;
        if input.ssh_port == 0 {
            return Err(PersistenceError::InvalidData(
                "ssh_port must be between 1 and 65535".into(),
            ));
        }
        let now = Utc::now();
        let host = Host {
            id: HostId::new(),
            name,
            address,
            ssh_port: input.ssh_port,
            username,
            role: input.role,
            platform: None,
            created_at: now,
            updated_at: now,
        };

        let result = sqlx::query(
            r#"
            INSERT INTO hosts (
                id, name, address, ssh_port, username, role, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(host.id.to_string())
        .bind(&host.name)
        .bind(&host.address)
        .bind(i64::from(host.ssh_port))
        .bind(&host.username)
        .bind(host.role.as_str())
        .bind(host.created_at.to_rfc3339())
        .bind(host.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => Ok(host),
            Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
                Err(PersistenceError::Conflict(format!(
                    "host {}:{} already exists",
                    host.address, host.ssh_port
                )))
            }
            Err(error) => Err(error.into()),
        }
    }

    pub async fn get(&self, id: HostId) -> Result<Option<Host>, PersistenceError> {
        sqlx::query("SELECT * FROM hosts WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?
            .map(decode_host)
            .transpose()
    }

    pub async fn list(&self) -> Result<Vec<Host>, PersistenceError> {
        sqlx::query("SELECT * FROM hosts ORDER BY created_at, id")
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(decode_host)
            .collect()
    }

    pub async fn update_platform(
        &self,
        id: HostId,
        platform: &PlatformInfo,
    ) -> Result<bool, PersistenceError> {
        let result = sqlx::query(
            r#"
            UPDATE hosts SET
                platform_kind = ?, os_version = ?, product_version = ?, hostname = ?,
                nut_version = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(platform.kind.as_str())
        .bind(&platform.os_version)
        .bind(&platform.product_version)
        .bind(&platform.hostname)
        .bind(&platform.nut_version)
        .bind(Utc::now().to_rfc3339())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn delete(&self, id: HostId) -> Result<bool, PersistenceError> {
        let result = sqlx::query("DELETE FROM hosts WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() == 1)
    }
}

fn required(field: &str, value: String) -> Result<String, PersistenceError> {
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Err(PersistenceError::InvalidData(format!(
            "{field} cannot be empty"
        )));
    }
    Ok(value)
}

fn validate_address(value: &str) -> Result<(), PersistenceError> {
    let valid = !value.starts_with('-')
        && value.len() <= 255
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '.' | ':' | '-' | '_' | '[' | ']' | '%')
        });
    if !valid {
        return Err(PersistenceError::InvalidData(
            "address must be an IPv4, IPv6, or DNS host name".into(),
        ));
    }
    Ok(())
}

fn validate_username(value: &str) -> Result<(), PersistenceError> {
    let valid = !value.starts_with('-')
        && value.len() <= 64
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'));
    if !valid {
        return Err(PersistenceError::InvalidData(
            "username contains unsupported characters".into(),
        ));
    }
    Ok(())
}

fn decode_host(row: SqliteRow) -> Result<Host, PersistenceError> {
    let platform_kind = row.try_get::<Option<String>, _>("platform_kind")?;
    let platform = platform_kind
        .map(|kind| {
            Ok::<PlatformInfo, PersistenceError>(PlatformInfo {
                kind: PlatformKind::from_str(&kind).map_err(PersistenceError::InvalidData)?,
                os_version: required_column(&row, "os_version")?,
                product_version: row.try_get("product_version")?,
                hostname: required_column(&row, "hostname")?,
                nut_version: row.try_get("nut_version")?,
            })
        })
        .transpose()?;

    Ok(Host {
        id: parse(&row.try_get::<String, _>("id")?, "host id")?,
        name: row.try_get("name")?,
        address: row.try_get("address")?,
        ssh_port: u16::try_from(row.try_get::<i64, _>("ssh_port")?)
            .map_err(|error| PersistenceError::InvalidData(error.to_string()))?,
        username: row.try_get("username")?,
        role: HostRole::from_str(&row.try_get::<String, _>("role")?)
            .map_err(PersistenceError::InvalidData)?,
        platform,
        created_at: parse_datetime(&row.try_get::<String, _>("created_at")?)?,
        updated_at: parse_datetime(&row.try_get::<String, _>("updated_at")?)?,
    })
}

fn required_column(row: &SqliteRow, column: &str) -> Result<String, PersistenceError> {
    row.try_get::<Option<String>, _>(column)?
        .ok_or_else(|| PersistenceError::InvalidData(format!("{column} is null")))
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
    use super::*;
    use crate::persistence::Database;

    async fn repository() -> HostRepository {
        let database = Database::connect("sqlite::memory:").await.unwrap();
        database.migrate().await.unwrap();
        database.hosts()
    }

    fn input(address: &str) -> CreateHost {
        CreateHost {
            name: "pve-01".into(),
            address: address.into(),
            ssh_port: 22,
            username: "root".into(),
            role: HostRole::Server,
        }
    }

    #[tokio::test]
    async fn host_round_trip_and_platform_update() {
        let repository = repository().await;
        let created = repository.create(input("192.168.1.10")).await.unwrap();
        let platform = PlatformInfo {
            kind: PlatformKind::ProxmoxVe,
            os_version: "13".into(),
            product_version: Some("9.0".into()),
            hostname: "pve-01".into(),
            nut_version: Some("2.8.1".into()),
        };

        assert!(
            repository
                .update_platform(created.id, &platform)
                .await
                .unwrap()
        );
        let loaded = repository.get(created.id).await.unwrap().unwrap();
        assert_eq!(loaded.platform, Some(platform));
        assert_eq!(repository.list().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn duplicate_endpoint_is_a_conflict() {
        let repository = repository().await;
        repository.create(input("192.168.1.10")).await.unwrap();
        let error = repository.create(input("192.168.1.10")).await.unwrap_err();
        assert!(matches!(error, PersistenceError::Conflict(_)));
    }

    #[tokio::test]
    async fn deleting_a_host_only_changes_local_persistence() {
        let repository = repository().await;
        let created = repository.create(input("192.168.1.10")).await.unwrap();
        assert!(repository.delete(created.id).await.unwrap());
        assert!(repository.get(created.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn ssh_destination_fields_are_validated() {
        let repository = repository().await;
        let mut invalid = input("-oProxyCommand=bad");
        assert!(matches!(
            repository.create(invalid.clone()).await.unwrap_err(),
            PersistenceError::InvalidData(_)
        ));

        invalid.address = "192.168.1.10".into();
        invalid.username = "root@example".into();
        assert!(matches!(
            repository.create(invalid).await.unwrap_err(),
            PersistenceError::InvalidData(_)
        ));
    }
}
