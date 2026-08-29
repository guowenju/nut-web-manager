use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value).map(Self)
            }
        }
    };
}

id_type!(HostId);
id_type!(NutServerId);
id_type!(BindingId);
id_type!(UpsId);
id_type!(CredentialId);
id_type!(ConfigRevisionId);
id_type!(OperationId);

macro_rules! string_enum {
    ($name:ident { $($variant:ident => $value:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $value),+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = String;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($value => Ok(Self::$variant)),+,
                    _ => Err(format!("invalid {}: {value}", stringify!($name))),
                }
            }
        }
    };
}

string_enum!(HostRole {
    Server => "server",
    Client => "client",
});

string_enum!(PlatformKind {
    Debian => "debian",
    ProxmoxVe => "proxmox_ve",
    ProxmoxBackupServer => "proxmox_backup_server",
    Unsupported => "unsupported",
});

string_enum!(ApplyState {
    Unconfigured => "unconfigured",
    Pending => "pending",
    Applying => "applying",
    Applied => "applied",
    Removing => "removing",
    Failed => "failed",
});

string_enum!(OperationState {
    Pending => "pending",
    Running => "running",
    Succeeded => "succeeded",
    Failed => "failed",
});

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlatformInfo {
    pub kind: PlatformKind,
    pub os_version: String,
    pub product_version: Option<String>,
    pub hostname: String,
    pub nut_version: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Host {
    pub id: HostId,
    pub name: String,
    pub address: String,
    pub ssh_port: u16,
    pub username: String,
    pub role: HostRole,
    pub platform: Option<PlatformInfo>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NutServer {
    pub id: NutServerId,
    pub host_id: HostId,
    pub ups_name: String,
    pub listen_address: String,
    pub listen_port: u16,
    pub enabled: bool,
    pub apply_state: ApplyState,
    pub applied_revision: Option<ConfigRevisionId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UpsDevice {
    pub id: UpsId,
    pub server_id: NutServerId,
    pub name: String,
    pub driver: String,
    pub port: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NutClientBinding {
    pub id: BindingId,
    pub server_id: NutServerId,
    pub client_host_id: HostId,
    pub apply_state: ApplyState,
    pub applied_revision: Option<ConfigRevisionId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Operation {
    pub id: OperationId,
    pub host_id: Option<HostId>,
    pub kind: String,
    pub state: OperationState,
    pub progress: u8,
    pub error_code: Option<String>,
    pub error_detail: Option<String>,
    pub result: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_as_strings() {
        let id = HostId::new();
        assert_eq!(id.to_string().parse::<HostId>().unwrap(), id);
    }

    #[test]
    fn enums_have_stable_storage_values() {
        assert_eq!(HostRole::Server.as_str(), "server");
        assert_eq!(
            "proxmox_ve".parse::<PlatformKind>().unwrap(),
            PlatformKind::ProxmoxVe
        );
        assert!("primary".parse::<HostRole>().is_err());
    }

    #[test]
    fn serde_uses_public_snake_case_values() {
        assert_eq!(
            serde_json::to_string(&ApplyState::Applied).unwrap(),
            r#""applied""#
        );
    }
}
