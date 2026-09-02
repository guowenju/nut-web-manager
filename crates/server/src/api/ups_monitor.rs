use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    persistence::{MonitorDevice, MonitorEvent, MonitorSample, MonitorSnapshot, MonitorSource},
    state::AppState,
    ups_monitor::protocol::{DiscoveredUps, list_ups},
};

use super::ApiError;

#[derive(Deserialize)]
pub struct SourceRequest {
    name: String,
    address: String,
    #[serde(default = "default_port")]
    port: u16,
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default)]
    reset_devices: bool,
}

#[derive(Serialize)]
pub struct OverviewResponse {
    sources: Vec<MonitorSource>,
    devices: Vec<MonitorDevice>,
    observed_at: chrono::DateTime<Utc>,
}

#[derive(Serialize)]
pub struct ConnectionTestResponse {
    reachable: bool,
    devices: Vec<DiscoveredUps>,
    error: Option<String>,
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    #[serde(default = "default_range")]
    range: String,
}

#[derive(Deserialize)]
pub struct EventsQuery {
    #[serde(default = "default_limit")]
    limit: u16,
}

pub async fn list_sources(
    State(state): State<AppState>,
) -> Result<Json<Vec<MonitorSource>>, ApiError> {
    Ok(Json(state.database.monitor().list_sources().await?))
}

pub async fn create_source(
    State(state): State<AppState>,
    Json(request): Json<SourceRequest>,
) -> Result<(StatusCode, Json<MonitorSource>), ApiError> {
    let source = state
        .database
        .monitor()
        .create_source(
            &request.name,
            &request.address,
            request.port,
            request.enabled,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(source)))
}

pub async fn update_source(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<SourceRequest>,
) -> Result<Json<MonitorSource>, ApiError> {
    validate_id(&id)?;
    state
        .database
        .monitor()
        .update_source(
            &id,
            &request.name,
            &request.address,
            request.port,
            request.enabled,
            request.reset_devices,
        )
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::not_found("UPS monitor source"))
}

pub async fn delete_source(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    validate_id(&id)?;
    if state.database.monitor().delete_source(&id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("UPS monitor source"))
    }
}

pub async fn test_source(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ConnectionTestResponse>, ApiError> {
    validate_id(&id)?;
    let source = state
        .database
        .monitor()
        .get_source(&id)
        .await?
        .ok_or_else(|| ApiError::not_found("UPS monitor source"))?;
    Ok(Json(match list_ups(&source.address, source.port).await {
        Ok(devices) => ConnectionTestResponse {
            reachable: true,
            devices,
            error: None,
        },
        Err(error) => ConnectionTestResponse {
            reachable: false,
            devices: Vec::new(),
            error: Some(error),
        },
    }))
}

pub async fn discover_source(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<MonitorDevice>>, ApiError> {
    validate_id(&id)?;
    let source = state
        .database
        .monitor()
        .get_source(&id)
        .await?
        .ok_or_else(|| ApiError::not_found("UPS monitor source"))?;
    let devices = list_ups(&source.address, source.port)
        .await
        .map_err(|error| ApiError::bad_gateway("NutDiscoveryFailed", error))?;
    Ok(Json(
        state
            .database
            .monitor()
            .sync_discovery(&id, &devices)
            .await?,
    ))
}

pub async fn overview(State(state): State<AppState>) -> Result<Json<OverviewResponse>, ApiError> {
    let source_repository = state.database.monitor();
    let device_repository = state.database.monitor();
    let (sources, devices) = tokio::try_join!(
        source_repository.list_sources(),
        device_repository.overview_devices(),
    )?;
    Ok(Json(OverviewResponse {
        sources,
        devices,
        observed_at: Utc::now(),
    }))
}

pub async fn snapshot(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<MonitorSnapshot>, ApiError> {
    validate_id(&id)?;
    state
        .database
        .monitor()
        .snapshot(&id)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::not_found("UPS monitor device"))
}

pub async fn history(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<Vec<MonitorSample>>, ApiError> {
    validate_id(&id)?;
    let duration = match query.range.as_str() {
        "24h" => Duration::hours(24),
        "7d" => Duration::days(7),
        "30d" => Duration::days(30),
        "90d" => Duration::days(90),
        _ => {
            return Err(ApiError::bad_request(
                "InvalidHistoryRange",
                "range must be one of 24h, 7d, 30d, or 90d",
            ));
        }
    };
    let samples = state
        .database
        .monitor()
        .history(&id, Utc::now() - duration)
        .await?;
    let gap_markers = history_gap_markers(&samples);
    let mut samples = downsample_history(samples, 180);
    samples.extend(gap_markers);
    samples.sort_by(|left, right| left.observed_at.cmp(&right.observed_at));
    samples.dedup_by(|left, right| left.observed_at == right.observed_at);
    Ok(Json(samples))
}

pub async fn events(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<EventsQuery>,
) -> Result<Json<Vec<MonitorEvent>>, ApiError> {
    validate_id(&id)?;
    if query.limit == 0 || query.limit > 500 {
        return Err(ApiError::bad_request(
            "InvalidEventLimit",
            "limit must be between 1 and 500",
        ));
    }
    Ok(Json(
        state.database.monitor().events(&id, query.limit).await?,
    ))
}

fn validate_id(value: &str) -> Result<(), ApiError> {
    uuid::Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| ApiError::bad_request("InvalidMonitorId", "UPS monitor id must be a UUID"))
}

fn default_port() -> u16 {
    3493
}
fn default_enabled() -> bool {
    true
}
fn default_range() -> String {
    "24h".into()
}
fn default_limit() -> u16 {
    100
}

fn downsample_history(values: Vec<MonitorSample>, bucket_count: usize) -> Vec<MonitorSample> {
    if values.len() <= 1_440 || bucket_count == 0 {
        return values;
    }
    let chunk_size = values.len().div_ceil(bucket_count);
    let mut selected = std::collections::BTreeSet::new();
    for chunk_start in (0..values.len()).step_by(chunk_size) {
        let chunk_end = (chunk_start + chunk_size).min(values.len());
        selected.insert(chunk_start);
        selected.insert(chunk_end - 1);
        for metric in 0..6 {
            let mut minimum: Option<(usize, f64)> = None;
            let mut maximum: Option<(usize, f64)> = None;
            for (index, sample) in values[chunk_start..chunk_end].iter().enumerate() {
                let absolute = chunk_start + index;
                let value = sample_metric(sample, metric);
                if let Some(value) = value {
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
        for index in (chunk_start + 1)..chunk_end {
            if values[index - 1].status_flags != values[index].status_flags {
                selected.insert(index - 1);
                selected.insert(index);
            }
        }
    }
    selected
        .into_iter()
        .map(|index| values[index].clone())
        .collect()
}

fn history_gap_markers(values: &[MonitorSample]) -> Vec<MonitorSample> {
    values
        .windows(2)
        .filter_map(|pair| {
            let previous = chrono::DateTime::parse_from_rfc3339(&pair[0].observed_at).ok()?;
            let current = chrono::DateTime::parse_from_rfc3339(&pair[1].observed_at).ok()?;
            if current - previous <= Duration::seconds(150) {
                return None;
            }
            Some(MonitorSample {
                observed_at: (previous + Duration::seconds(60)).to_rfc3339(),
                status_flags: Vec::new(),
                charge_percent: None,
                runtime_seconds: None,
                runtime_capped: false,
                load_percent: None,
                input_voltage: None,
                output_voltage: None,
                battery_temperature: None,
            })
        })
        .collect()
}

fn sample_metric(sample: &MonitorSample, index: usize) -> Option<f64> {
    match index {
        0 => sample.charge_percent,
        1 => sample.runtime_seconds.map(|value| value as f64),
        2 => sample.load_percent,
        3 => sample.input_voltage,
        4 => sample.output_voltage,
        5 => sample.battery_temperature,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{downsample_history, history_gap_markers};
    use crate::persistence::MonitorSample;

    #[test]
    fn history_downsampling_preserves_metric_extrema() {
        let mut values = (0..10_000)
            .map(|index| MonitorSample {
                observed_at: format!("2026-01-01T00:{index:04}:00Z"),
                status_flags: vec!["OL".into()],
                charge_percent: Some(100.0),
                runtime_seconds: None,
                runtime_capped: false,
                load_percent: Some(10.0),
                input_voltage: Some(220.0),
                output_voltage: Some(220.0),
                battery_temperature: Some(30.0),
            })
            .collect::<Vec<_>>();
        values[5_555].load_percent = Some(100.0);
        values[7_777].battery_temperature = Some(80.0);
        let sampled = downsample_history(values, 180);
        assert!(
            sampled
                .iter()
                .any(|sample| sample.load_percent == Some(100.0))
        );
        assert!(
            sampled
                .iter()
                .any(|sample| sample.battery_temperature == Some(80.0))
        );
        assert_eq!(
            sampled.first().unwrap().observed_at,
            "2026-01-01T00:0000:00Z"
        );
        assert_eq!(
            sampled.last().unwrap().observed_at,
            "2026-01-01T00:9999:00Z"
        );
    }

    #[test]
    fn inserts_a_null_sample_across_collection_gaps() {
        let sample = |observed_at: &str| MonitorSample {
            observed_at: observed_at.into(),
            status_flags: vec!["OL".into()],
            charge_percent: Some(100.0),
            runtime_seconds: Some(600),
            runtime_capped: false,
            load_percent: Some(10.0),
            input_voltage: Some(220.0),
            output_voltage: Some(220.0),
            battery_temperature: Some(30.0),
        };
        let markers = history_gap_markers(&[
            sample("2026-01-01T00:00:00Z"),
            sample("2026-01-01T00:05:00Z"),
        ]);
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].observed_at, "2026-01-01T00:01:00+00:00");
        assert_eq!(markers[0].load_percent, None);
    }
}
