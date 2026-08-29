use std::{env, fs, net::SocketAddr, path::PathBuf};

use anyhow::{Context, bail};

const DEFAULT_BIND_ADDRESS: &str = "0.0.0.0:8080";
const DEFAULT_DATA_DIR: &str = "./data";
const DEFAULT_ADMIN_USERNAME: &str = "admin";
const DEFAULT_ADMIN_PASSWORD: &str = "admin";

#[derive(Clone, Debug)]
pub struct Settings {
    pub bind_address: SocketAddr,
    pub data_dir: PathBuf,
    pub database_url: String,
    pub admin_username: String,
    admin_password: String,
}

impl Settings {
    pub fn from_env() -> anyhow::Result<Self> {
        let bind_address = env::var("NWM_BIND_ADDRESS")
            .unwrap_or_else(|_| DEFAULT_BIND_ADDRESS.into())
            .parse()
            .context("NWM_BIND_ADDRESS must be an IP:port pair")?;
        let data_dir =
            PathBuf::from(env::var("NWM_DATA_DIR").unwrap_or_else(|_| DEFAULT_DATA_DIR.into()));
        let admin_username =
            env::var("NWM_ADMIN_USERNAME").unwrap_or_else(|_| DEFAULT_ADMIN_USERNAME.into());
        let admin_password =
            env::var("NWM_ADMIN_PASSWORD").unwrap_or_else(|_| DEFAULT_ADMIN_PASSWORD.into());

        if admin_username.trim().is_empty() {
            bail!("NWM_ADMIN_USERNAME cannot be empty");
        }
        if admin_password.is_empty() {
            bail!("NWM_ADMIN_PASSWORD cannot be empty");
        }

        let database_url = env::var("NWM_DATABASE_URL").unwrap_or_else(|_| {
            let path = data_dir.join("nut-web-manager.db");
            format!("sqlite://{}?mode=rwc", path.display())
        });

        Ok(Self {
            bind_address,
            data_dir,
            database_url,
            admin_username,
            admin_password,
        })
    }

    pub fn prepare_data_dir(&self) -> anyhow::Result<()> {
        fs::create_dir_all(&self.data_dir).with_context(|| {
            format!(
                "cannot create or access data directory {}",
                self.data_dir.display()
            )
        })
    }

    pub fn verify_admin_credentials(&self, username: &str, password: &str) -> bool {
        username == self.admin_username && password == self.admin_password
    }

    pub fn uses_default_admin_credentials(&self) -> bool {
        self.admin_username == DEFAULT_ADMIN_USERNAME
            && self.admin_password == DEFAULT_ADMIN_PASSWORD
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_verification_is_exact() {
        let settings = Settings {
            bind_address: DEFAULT_BIND_ADDRESS.parse().unwrap(),
            data_dir: PathBuf::from(DEFAULT_DATA_DIR),
            database_url: "sqlite::memory:".into(),
            admin_username: DEFAULT_ADMIN_USERNAME.into(),
            admin_password: DEFAULT_ADMIN_PASSWORD.into(),
        };

        assert!(settings.verify_admin_credentials("admin", "admin"));
        assert!(!settings.verify_admin_credentials("Admin", "admin"));
        assert!(settings.uses_default_admin_credentials());
    }

    #[test]
    fn local_development_data_dir_is_the_default() {
        assert_eq!(PathBuf::from(DEFAULT_DATA_DIR), PathBuf::from("./data"));
    }
}
