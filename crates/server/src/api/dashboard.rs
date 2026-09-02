use std::{collections::HashMap, time::Duration};

use axum::{
    Json,
    extract::{Query, State},
};
use chrono::{DateTime, Utc};
use nwm_common::{
    BatteryCondition, ManagementConnectivity, ObservationError, PowerSource, ProtectionHealth,
    UpsObservation,
};
use serde::{Deserialize, Serialize};

use crate::{
    nut,
    persistence::{DashboardHistorySample, ServerRecord},
    state::AppState,
};

use super::ApiError;

const STATUS_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Serialize)]
pub struct DashboardSnapshot {
    server: Option<ServerRecord>,
    ups: Option<UpsObservation>,
    management: ManagementConnectivity,
    protection: ProtectionHealth,
    services: Option<NutServiceStatus>,
    observed_at: DateTime<Utc>,
    last_verified_at: Option<DateTime<Utc>>,
    error: Option<ObservationError>,
}

#[derive(Clone, Debug, Serialize)]
pub struct NutServiceStatus {
    driver_active: bool,
    server_active: bool,
    monitor_active: bool,
}

#[derive(Deserialize)]
pub struct DashboardHistoryQuery {
    #[serde(default = "default_history_range")]
    range: String,
}

pub async fn dashboard(State(state): State<AppState>) -> Result<Json<DashboardSnapshot>, ApiError> {
    let server = state
        .database
        .topology()
        .list_servers()
        .await?
        .into_iter()
        .find(|server| server.enabled);
    let Some(server) = server else {
        return Ok(Json(DashboardSnapshot {
            server: None,
            ups: None,
            management: ManagementConnectivity::Unknown,
            protection: ProtectionHealth::Unconfigured,
            services: None,
            observed_at: Utc::now(),
            last_verified_at: None,
            error: None,
        }));
    };
    let host = state
        .database
        .hosts()
        .get(server.host_id)
        .await?
        .ok_or_else(|| ApiError::not_found("server host"))?;

    let variables = nut::query_ups_variables(&host.address, server.listen_port, &server.ups_name);
    let service_script = format!(
        "printf 'driver=%s\\n' \"$(systemctl is-active 'nut-driver@{}.service' 2>/dev/null || true)\"\nprintf 'server=%s\\n' \"$(systemctl is-active nut-server.service 2>/dev/null || true)\"\nprintf 'monitor=%s\\n' \"$(systemctl is-active nut-monitor.service 2>/dev/null || true)\"\n",
        server.ups_name
    );
    let services = state
        .ssh
        .execute_script(&host, &service_script, STATUS_TIMEOUT);
    let (variables, services) = tokio::join!(variables, services);
    let observed_at = Utc::now();
    let (ups, tcp_error) = match variables {
        Ok(raw) => (Some(observation(&server, raw, observed_at)), None),
        Err(error) => (
            None,
            Some(if error.contains("timed out") {
                ObservationError::Timeout
            } else if error.contains("refused") {
                ObservationError::ConnectionRefused
            } else {
                ObservationError::InvalidResponse
            }),
        ),
    };
    let (management, services, ssh_error) = match services {
        Ok(output) => (
            ManagementConnectivity::Connected,
            Some(parse_services(&output)),
            None,
        ),
        Err(crate::ssh::SshError::HostKeyChanged) => (
            ManagementConnectivity::HostKeyMismatch,
            None,
            Some(ObservationError::SshUnavailable),
        ),
        Err(crate::ssh::SshError::HostKeyConfirmationRequired) => (
            ManagementConnectivity::Unknown,
            None,
            Some(ObservationError::SshUnavailable),
        ),
        Err(_) => (
            ManagementConnectivity::Disconnected,
            None,
            Some(ObservationError::SshUnavailable),
        ),
    };
    let protection = if ups.is_some()
        && services.as_ref().is_some_and(|services| {
            services.driver_active && services.server_active && services.monitor_active
        })
        && server.apply_state == nwm_common::ApplyState::Applied
    {
        ProtectionHealth::Active
    } else if ups.is_some() && services.is_none() {
        ProtectionHealth::Unknown
    } else {
        ProtectionHealth::Degraded
    };
    let last_verified_at = remember_last_verified_at(
        &state,
        server.id,
        protection == ProtectionHealth::Active,
        observed_at,
    );

    Ok(Json(DashboardSnapshot {
        server: Some(server),
        ups,
        management,
        protection,
        services,
        observed_at,
        last_verified_at,
        error: tcp_error.or(ssh_error),
    }))
}

pub async fn dashboard_history(
    State(state): State<AppState>,
    Query(query): Query<DashboardHistoryQuery>,
) -> Result<Json<Vec<DashboardHistorySample>>, ApiError> {
    let duration = match query.range.as_str() {
        "24h" => chrono::Duration::hours(24),
        "7d" => chrono::Duration::days(7),
        "30d" => chrono::Duration::days(30),
        "90d" => chrono::Duration::days(90),
        _ => {
            return Err(ApiError::bad_request(
                "InvalidHistoryRange",
                "range must be one of 24h, 7d, 30d, or 90d",
            ));
        }
    };
    let Some(server) = state
        .database
        .topology()
        .list_servers()
        .await?
        .into_iter()
        .find(|server| server.enabled)
    else {
        return Ok(Json(Vec::new()));
    };
    let samples = state
        .database
        .dashboard_history()
        .history(&server.id.to_string(), Utc::now() - duration)
        .await?;
    let gap_markers = dashboard_gap_markers(&samples);
    let mut samples = downsample_dashboard_history(samples, 240);
    samples.extend(gap_markers);
    samples.sort_by(|left, right| left.observed_at.cmp(&right.observed_at));
    samples.dedup_by(|left, right| left.observed_at == right.observed_at);
    Ok(Json(samples))
}

fn remember_last_verified_at(
    state: &AppState,
    server_id: nwm_common::NutServerId,
    verified: bool,
    observed_at: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let mut values = state
        .last_verified_at
        .lock()
        .expect("last verified state lock poisoned");
    if verified {
        values.insert(server_id, observed_at);
        Some(observed_at)
    } else {
        values.get(&server_id).copied()
    }
}

fn observation(
    server: &ServerRecord,
    raw: HashMap<String, String>,
    observed_at: DateTime<Utc>,
) -> UpsObservation {
    let status_flags: Vec<String> = raw
        .get("ups.status")
        .map(|status| status.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_default();
    UpsObservation {
        ups_id: server.device.id,
        reachable: true,
        power_source: power_source(&status_flags),
        battery_condition: battery_condition(&status_flags),
        status_flags,
        charge_percent: number(&raw, "battery.charge"),
        runtime_seconds: number(&raw, "battery.runtime")
            .map(|value: f32| value as u64)
            .filter(|value| *value != 65_535),
        load_percent: number(&raw, "ups.load"),
        manufacturer: raw
            .get("device.mfr")
            .or_else(|| raw.get("ups.mfr"))
            .cloned(),
        model: raw
            .get("device.model")
            .or_else(|| raw.get("ups.model"))
            .cloned(),
        serial: raw
            .get("device.serial")
            .or_else(|| raw.get("ups.serial"))
            .cloned(),
        raw,
        observed_at,
        error: None,
    }
}

fn number<T: std::str::FromStr>(raw: &HashMap<String, String>, key: &str) -> Option<T> {
    raw.get(key)?.parse().ok()
}

fn power_source(flags: &[String]) -> PowerSource {
    if flags.iter().any(|flag| flag == "OB") {
        PowerSource::Battery
    } else if flags.iter().any(|flag| flag == "OL") {
        PowerSource::Mains
    } else if flags.iter().any(|flag| flag == "BYPASS") {
        PowerSource::Bypass
    } else if flags.iter().any(|flag| flag == "OFF") {
        PowerSource::Off
    } else {
        PowerSource::Unknown
    }
}

fn battery_condition(flags: &[String]) -> BatteryCondition {
    if flags.iter().any(|flag| flag == "RB") {
        BatteryCondition::Replace
    } else if flags.iter().any(|flag| flag == "LB") {
        BatteryCondition::Low
    } else {
        BatteryCondition::Normal
    }
}

fn parse_services(output: &str) -> NutServiceStatus {
    let values: HashMap<_, _> = output
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect();
    NutServiceStatus {
        driver_active: values.get("driver") == Some(&"active"),
        server_active: values.get("server") == Some(&"active"),
        monitor_active: values.get("monitor") == Some(&"active"),
    }
}

fn default_history_range() -> String {
    "24h".into()
}

fn downsample_dashboard_history(
    values: Vec<DashboardHistorySample>,
    bucket_count: usize,
) -> Vec<DashboardHistorySample> {
    if values.len() <= 1_440 || bucket_count == 0 {
        return values;
    }
    let chunk_size = values.len().div_ceil(bucket_count);
    let mut selected = std::collections::BTreeSet::new();
    for chunk_start in (0..values.len()).step_by(chunk_size) {
        let chunk_end = (chunk_start + chunk_size).min(values.len());
        selected.insert(chunk_start);
        selected.insert(chunk_end - 1);
        for metric in 0..3 {
            let mut minimum: Option<(usize, f64)> = None;
            let mut maximum: Option<(usize, f64)> = None;
            for (index, sample) in values[chunk_start..chunk_end].iter().enumerate() {
                let absolute = chunk_start + index;
                if let Some(value) = dashboard_metric(sample, metric) {
                    if minimum.is_none_or(|(_, current)| value < current) {
                        minimum = Some((absolute, value));
                    }
                    if maximum.is_none_or(|(_, current)| value > current) {
                        maximum = Some((absolute, value));
                    }
                }
            }
            if let Some((index, _)) = minimum {
                selected.insert(index);
            }
            if let Some((index, _)) = maximum {
                selected.insert(index);
            }
        }
    }
    selected
        .into_iter()
        .map(|index| values[index].clone())
        .collect()
}

fn dashboard_metric(sample: &DashboardHistorySample, index: usize) -> Option<f64> {
    match index {
        0 => sample.load_percent,
        1 => sample.runtime_seconds.map(|value| value as f64),
        2 => sample.realpower_watts,
        _ => None,
    }
}

fn dashboard_gap_markers(values: &[DashboardHistorySample]) -> Vec<DashboardHistorySample> {
    values
        .windows(2)
        .filter_map(|pair| {
            let previous = chrono::DateTime::parse_from_rfc3339(&pair[0].observed_at).ok()?;
            let current = chrono::DateTime::parse_from_rfc3339(&pair[1].observed_at).ok()?;
            if current - previous <= chrono::Duration::seconds(150) {
                return None;
            }
            Some(DashboardHistorySample {
                observed_at: (previous + chrono::Duration::seconds(60)).to_rfc3339(),
                load_percent: None,
                runtime_seconds: None,
                realpower_watts: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_service_states() {
        let status = parse_services("driver=active\nserver=active\nmonitor=inactive\n");
        assert!(status.driver_active);
        assert!(status.server_active);
        assert!(!status.monitor_active);
    }

    #[test]
    fn dashboard_downsampling_keeps_power_spikes() {
        let mut values = (0..10_000)
            .map(|index| DashboardHistorySample {
                observed_at: format!("2026-01-01T00:{index:04}:00Z"),
                load_percent: Some(10.0),
                runtime_seconds: Some(600),
                realpower_watts: Some(100.0),
            })
            .collect::<Vec<_>>();
        values[5_555].realpower_watts = Some(900.0);
        let sampled = downsample_dashboard_history(values, 240);
        assert!(
            sampled
                .iter()
                .any(|sample| sample.realpower_watts == Some(900.0))
        );
    }
}
