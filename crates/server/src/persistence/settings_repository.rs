use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use super::PersistenceError;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultPage {
    Overview,
    UpsMonitor,
}

impl DefaultPage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::UpsMonitor => "ups_monitor",
        }
    }
}

impl Default for DefaultPage {
    fn default() -> Self {
        Self::Overview
    }
}

impl fmt::Display for DefaultPage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for DefaultPage {
    type Err = PersistenceError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "overview" => Ok(Self::Overview),
            "ups_monitor" => Ok(Self::UpsMonitor),
            value => Err(PersistenceError::InvalidData(format!(
                "unsupported default page: {value}"
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct AppSettings {
    pub default_page: DefaultPage,
}

#[derive(Clone)]
pub struct SettingsRepository {
    pool: SqlitePool,
}

impl SettingsRepository {
    pub(super) fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get(&self) -> Result<AppSettings, PersistenceError> {
        let row = sqlx::query("SELECT default_page FROM app_settings WHERE id = 1")
            .fetch_one(&self.pool)
            .await?;
        Ok(AppSettings {
            default_page: row.try_get::<String, _>("default_page")?.parse()?,
        })
    }

    pub async fn update(&self, default_page: DefaultPage) -> Result<AppSettings, PersistenceError> {
        sqlx::query("UPDATE app_settings SET default_page = ? WHERE id = 1")
            .bind(default_page.as_str())
            .execute(&self.pool)
            .await?;
        self.get().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::Database;

    #[tokio::test]
    async fn defaults_to_overview_and_persists_updates() {
        let database = Database::connect("sqlite::memory:").await.unwrap();
        database.migrate().await.unwrap();

        assert_eq!(
            database.settings().get().await.unwrap().default_page,
            DefaultPage::Overview
        );

        database
            .settings()
            .update(DefaultPage::UpsMonitor)
            .await
            .unwrap();
        assert_eq!(
            database.settings().get().await.unwrap().default_page,
            DefaultPage::UpsMonitor
        );
    }
}
