use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{BindingId, HostId, UpsId};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagementConnectivity {
    Connected,
    Disconnected,
    HostKeyMismatch,
    AuthenticationFailed,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtectionHealth {
    Active,
    Degraded,
    Unknown,
    Unconfigured,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerSource {
    Mains,
    Battery,
    Bypass,
    Off,
    Other,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatteryCondition {
    Normal,
    Low,
    Depleted,
    Replace,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "code", content = "detail", rename_all = "snake_case")]
pub enum ObservationError {
    Timeout,
    ConnectionRefused,
    DnsFailure,
    AccessDenied,
    UnknownUps,
    DataStale,
    DriverNotConnected,
    InvalidResponse,
    SshUnavailable,
    ServiceInactive,
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UpsObservation {
    pub ups_id: UpsId,
    pub reachable: bool,
    pub power_source: PowerSource,
    pub battery_condition: BatteryCondition,
    pub status_flags: Vec<String>,
    pub charge_percent: Option<f32>,
    pub runtime_seconds: Option<u64>,
    pub load_percent: Option<f32>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub serial: Option<String>,
    pub raw: HashMap<String, String>,
    pub observed_at: DateTime<Utc>,
    pub error: Option<ObservationError>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HostObservation {
    pub host_id: HostId,
    pub management: ManagementConnectivity,
    pub protection: ProtectionHealth,
    pub reasons: Vec<String>,
    pub observed_at: DateTime<Utc>,
    pub stale: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BindingObservation {
    pub binding_id: BindingId,
    pub health: ProtectionHealth,
    pub tcp_reachable: Option<bool>,
    pub monitor_running: Option<bool>,
    pub upsc_responding: Option<bool>,
    pub observed_at: DateTime<Utc>,
    pub error: Option<ObservationError>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observation_errors_have_machine_readable_codes() {
        let value = serde_json::to_value(ObservationError::DataStale).unwrap();
        assert_eq!(value["code"], "data_stale");
    }
}
