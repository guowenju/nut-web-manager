use std::str::FromStr;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use nwm_common::{BindingId, ConfigRevisionId, HostId, HostRole, NutServerId, OperationId};
use serde::{Deserialize, Serialize};

use crate::{
    nut::{self, ConfigError, ConfigPreview, UsbScanCandidate, client_candidate, server_candidate},
    persistence::{BindingRecord, ServerRecord, ShutdownOptions, ShutdownTriggerMode},
    state::AppState,
};

use super::{ApiError, nut::OperationAccepted, ssh::map_ssh_error};

#[derive(Deserialize)]
pub struct SelectServerRequest {
    host_id: HostId,
    #[serde(default = "default_ups_name")]
    ups_name: String,
    candidate: UsbScanCandidate,
}

#[derive(Deserialize)]
pub struct CreateBindingRequest {
    server_id: NutServerId,
    client_host_id: HostId,
}

#[derive(Clone, Deserialize)]
pub struct ApplyRequest {
    confirmed: bool,
    #[serde(default)]
    takeover: bool,
    #[serde(default)]
    takeover_snapshots: Vec<TakeoverSnapshotRequest>,
}

#[derive(Clone, Deserialize)]
pub struct TakeoverSnapshotRequest {
    host_id: HostId,
    snapshot_hash: String,
}

#[derive(Deserialize)]
pub struct UpdateShutdownRequest {
    trigger_mode: ShutdownTriggerMode,
    battery_level_percent: u8,
    on_battery_seconds: u32,
    host_sync_seconds: u16,
    final_delay_seconds: u16,
    powerdown_enabled: bool,
}

#[derive(Serialize)]
pub struct BindingPreview {
    server: ConfigPreview,
    client: ConfigPreview,
}

#[derive(Serialize)]
struct AppliedConfigResult {
    #[serde(flatten)]
    outcome: nut::ConfigApplyOutcome,
    revision_id: ConfigRevisionId,
}

#[derive(Serialize)]
struct BindingApplyResult {
    server: AppliedConfigResult,
    client: AppliedConfigResult,
}

struct BindingTakeover {
    server: Option<String>,
    client: Option<String>,
}

pub async fn select_server(
    State(state): State<AppState>,
    Json(request): Json<SelectServerRequest>,
) -> Result<(StatusCode, Json<ServerRecord>), ApiError> {
    let host = state
        .database
        .hosts()
        .get(request.host_id)
        .await?
        .ok_or_else(|| ApiError::not_found("host"))?;
    if host.role != HostRole::Server {
        return Err(ApiError::unprocessable(
            "ServerRoleRequired",
            "the selected host does not have the Server role",
        ));
    }
    let server = state
        .database
        .topology()
        .select_server_device(
            host.id,
            &request.ups_name,
            &request.candidate.driver,
            &request.candidate.port,
            &request.candidate.selectors,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(server)))
}

pub async fn list_servers(
    State(state): State<AppState>,
) -> Result<Json<Vec<ServerRecord>>, ApiError> {
    Ok(Json(state.database.topology().list_servers().await?))
}

pub async fn get_server(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ServerRecord>, ApiError> {
    find_server(&state, &id).await.map(Json)
}

pub async fn update_shutdown(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<UpdateShutdownRequest>,
) -> Result<Json<ServerRecord>, ApiError> {
    let server = find_server(&state, &id).await?;
    let updated = state
        .database
        .topology()
        .update_shutdown_options(
            server.id,
            &ShutdownOptions {
                trigger_mode: request.trigger_mode,
                battery_level_percent: request.battery_level_percent,
                on_battery_seconds: request.on_battery_seconds,
                host_sync_seconds: request.host_sync_seconds,
                final_delay_seconds: request.final_delay_seconds,
                powerdown_enabled: request.powerdown_enabled,
            },
        )
        .await?;
    Ok(Json(updated))
}

pub async fn preview_server(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ConfigPreview>, ApiError> {
    let server = find_server(&state, &id).await?;
    let host = find_host(&state, server.host_id).await?;
    let credentials = state.database.topology().credentials(server.id).await?;
    let candidate = server_candidate(&server, &credentials);
    let latest = state.database.topology().latest_revision(host.id).await?;
    let mut preview = nut::preview_config(&state.ssh, &host, &candidate, latest.as_ref())
        .await
        .map_err(map_config_error)?;
    annotate_takeover_safety(&state, &host, &mut [&mut preview]).await;
    Ok(Json(preview))
}

pub async fn apply_server(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<ApplyRequest>,
) -> Result<(StatusCode, Json<OperationAccepted>), ApiError> {
    require_confirmation(request.confirmed)?;
    let server = find_server(&state, &id).await?;
    if state
        .database
        .topology()
        .list_servers()
        .await?
        .iter()
        .any(|existing| existing.enabled && existing.id != server.id)
    {
        return Err(ApiError::conflict(
            "MultipleServersNotSupportedInV1",
            "V1 already has another enabled NUT Server",
        ));
    }
    let host = find_host(&state, server.host_id).await?;
    let guard = state
        .host_operations
        .try_acquire(host.id)
        .ok_or_else(operation_conflict)?;
    let credentials = state.database.topology().credentials(server.id).await?;
    let candidate = server_candidate(&server, &credentials);
    let latest = state.database.topology().latest_revision(host.id).await?;
    let preview = nut::preview_config(&state.ssh, &host, &candidate, latest.as_ref())
        .await
        .map_err(map_config_error)?;
    let takeover_snapshot =
        requested_takeover_snapshot(&request, host.id, preview.takeover_required)?;
    let takeover =
        nut::validate_preview(&preview, takeover_snapshot.as_deref()).map_err(map_config_error)?;
    if takeover {
        nut::require_safe_power_state(&state.ssh, &host)
            .await
            .map_err(map_config_error)?;
    }
    let operation = state
        .database
        .operations()
        .create(
            Some(host.id),
            if takeover {
                "server_config_takeover"
            } else {
                "server_config_apply"
            },
        )
        .await?;
    let operation_id = operation.id;
    let task_state = state.clone();
    tokio::spawn(async move {
        let _guard = guard;
        let repository = task_state.database.operations();
        let _ = repository.set_running(operation_id, 10).await;
        let result = apply_server_inner(
            &task_state,
            &server,
            &host,
            operation_id,
            takeover_snapshot.as_deref(),
        )
        .await;
        if let Err(error) = result {
            let _ = task_state
                .database
                .topology()
                .set_server_failed(server.id)
                .await;
            record_config_failure(&repository, operation_id, &error).await;
        }
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(OperationAccepted { operation_id }),
    ))
}

pub async fn create_binding(
    State(state): State<AppState>,
    Json(request): Json<CreateBindingRequest>,
) -> Result<(StatusCode, Json<BindingRecord>), ApiError> {
    let server = state
        .database
        .topology()
        .get_server(request.server_id)
        .await?
        .ok_or_else(|| ApiError::not_found("server"))?;
    if !server.enabled {
        return Err(ApiError::conflict(
            "ServerNotApplied",
            "apply and verify the Server before adding a Client",
        ));
    }
    let host = find_host(&state, request.client_host_id).await?;
    if host.role != HostRole::Client {
        return Err(ApiError::unprocessable(
            "ClientRoleRequired",
            "the selected host does not have the Client role",
        ));
    }
    let binding = state
        .database
        .topology()
        .create_binding(server.id, host.id)
        .await?;
    Ok((StatusCode::CREATED, Json(binding)))
}

pub async fn list_bindings(
    State(state): State<AppState>,
) -> Result<Json<Vec<BindingRecord>>, ApiError> {
    Ok(Json(state.database.topology().list_bindings().await?))
}

pub async fn get_binding(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<BindingRecord>, ApiError> {
    find_binding(&state, &id).await.map(Json)
}

pub async fn preview_binding(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<BindingPreview>, ApiError> {
    let binding = find_binding(&state, &id).await?;
    let server = state
        .database
        .topology()
        .get_server(binding.server_id)
        .await?
        .ok_or_else(|| ApiError::not_found("server"))?;
    let server_host = find_host(&state, server.host_id).await?;
    let client_host = find_host(&state, binding.client_host_id).await?;
    let credentials = state.database.topology().credentials(server.id).await?;
    let binding_credential = state
        .database
        .topology()
        .binding_credential(binding.id)
        .await?
        .ok_or_else(|| ApiError::not_found("binding credential"))?;
    let server_files = server_candidate(&server, &credentials);
    let client_files = client_candidate(&server, &server_host, &binding_credential);
    let server_revision = state
        .database
        .topology()
        .latest_revision(server_host.id)
        .await?;
    let client_revision = state
        .database
        .topology()
        .latest_revision(client_host.id)
        .await?;
    let mut server_preview = nut::preview_config(
        &state.ssh,
        &server_host,
        &server_files,
        server_revision.as_ref(),
    )
    .await
    .map_err(map_config_error)?;
    let mut client_preview = nut::preview_config(
        &state.ssh,
        &client_host,
        &client_files,
        client_revision.as_ref(),
    )
    .await
    .map_err(map_config_error)?;
    annotate_takeover_safety(
        &state,
        &server_host,
        &mut [&mut server_preview, &mut client_preview],
    )
    .await;
    Ok(Json(BindingPreview {
        server: server_preview,
        client: client_preview,
    }))
}

pub async fn apply_binding(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<ApplyRequest>,
) -> Result<(StatusCode, Json<OperationAccepted>), ApiError> {
    require_confirmation(request.confirmed)?;
    let binding = find_binding(&state, &id).await?;
    let server = state
        .database
        .topology()
        .get_server(binding.server_id)
        .await?
        .ok_or_else(|| ApiError::not_found("server"))?;
    let server_host = find_host(&state, server.host_id).await?;
    let client_host = find_host(&state, binding.client_host_id).await?;
    let server_guard = state
        .host_operations
        .try_acquire(server_host.id)
        .ok_or_else(operation_conflict)?;
    let client_guard = state
        .host_operations
        .try_acquire(client_host.id)
        .ok_or_else(operation_conflict)?;
    let credentials = state.database.topology().credentials(server.id).await?;
    let binding_credential = state
        .database
        .topology()
        .binding_credential(binding.id)
        .await?
        .ok_or_else(|| ApiError::not_found("binding credential"))?;
    let server_files = server_candidate(&server, &credentials);
    let client_files = client_candidate(&server, &server_host, &binding_credential);
    let server_latest = state
        .database
        .topology()
        .latest_revision(server_host.id)
        .await?;
    let client_latest = state
        .database
        .topology()
        .latest_revision(client_host.id)
        .await?;
    let server_preview = nut::preview_config(
        &state.ssh,
        &server_host,
        &server_files,
        server_latest.as_ref(),
    )
    .await
    .map_err(map_config_error)?;
    let client_preview = nut::preview_config(
        &state.ssh,
        &client_host,
        &client_files,
        client_latest.as_ref(),
    )
    .await
    .map_err(map_config_error)?;
    let server_takeover =
        requested_takeover_snapshot(&request, server_host.id, server_preview.takeover_required)?;
    let client_takeover =
        requested_takeover_snapshot(&request, client_host.id, client_preview.takeover_required)?;
    let server_requires_takeover =
        nut::validate_preview(&server_preview, server_takeover.as_deref())
            .map_err(map_config_error)?;
    let client_requires_takeover =
        nut::validate_preview(&client_preview, client_takeover.as_deref())
            .map_err(map_config_error)?;
    if server_requires_takeover || client_requires_takeover {
        nut::require_safe_power_state(&state.ssh, &server_host)
            .await
            .map_err(map_config_error)?;
    }
    let operation = state
        .database
        .operations()
        .create(
            Some(client_host.id),
            if server_requires_takeover || client_requires_takeover {
                "client_binding_takeover"
            } else {
                "client_binding_apply"
            },
        )
        .await?;
    let operation_id = operation.id;
    let task_state = state.clone();
    tokio::spawn(async move {
        let (_server_guard, _client_guard) = (server_guard, client_guard);
        let repository = task_state.database.operations();
        let _ = repository.set_running(operation_id, 10).await;
        let result = apply_binding_inner(
            &task_state,
            &binding,
            &server,
            &server_host,
            &client_host,
            operation_id,
            BindingTakeover {
                server: server_takeover,
                client: client_takeover,
            },
        )
        .await;
        if let Err(error) = result {
            let _ = task_state
                .database
                .topology()
                .set_binding_failed(binding.id)
                .await;
            record_config_failure(&repository, operation_id, &error).await;
        }
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(OperationAccepted { operation_id }),
    ))
}

async fn apply_server_inner(
    state: &AppState,
    server: &ServerRecord,
    host: &nwm_common::Host,
    operation_id: OperationId,
    takeover_snapshot: Option<&str>,
) -> Result<(), ConfigError> {
    let credentials = state
        .database
        .topology()
        .credentials(server.id)
        .await
        .map_err(db_config)?;
    let candidate = server_candidate(server, &credentials);
    let latest = state
        .database
        .topology()
        .latest_revision(host.id)
        .await
        .map_err(db_config)?;
    if takeover_snapshot.is_some() {
        nut::require_safe_power_state(&state.ssh, host).await?;
    }
    let outcome = nut::apply_config(
        &state.ssh,
        &state.settings.data_dir,
        host,
        &candidate,
        latest.as_ref(),
        Some(server),
        takeover_snapshot,
    )
    .await?;
    let revision = state
        .database
        .topology()
        .create_revision(host.id, &outcome.manifest_json, &outcome.backup_path)
        .await
        .map_err(db_config)?;
    state
        .database
        .topology()
        .set_server_applied(server.id, revision)
        .await
        .map_err(db_config)?;
    state
        .database
        .operations()
        .succeed_with_result(
            operation_id,
            &AppliedConfigResult {
                outcome,
                revision_id: revision,
            },
        )
        .await
        .map_err(db_config)?;
    Ok(())
}

async fn apply_binding_inner(
    state: &AppState,
    binding: &BindingRecord,
    server: &ServerRecord,
    server_host: &nwm_common::Host,
    client_host: &nwm_common::Host,
    operation_id: OperationId,
    takeover: BindingTakeover,
) -> Result<(), ConfigError> {
    if takeover.server.is_some() || takeover.client.is_some() {
        nut::require_safe_power_state(&state.ssh, server_host).await?;
    }
    let credentials = state
        .database
        .topology()
        .credentials(server.id)
        .await
        .map_err(db_config)?;
    let server_files = server_candidate(server, &credentials);
    let server_latest = state
        .database
        .topology()
        .latest_revision(server_host.id)
        .await
        .map_err(db_config)?;
    let server_outcome = nut::apply_config(
        &state.ssh,
        &state.settings.data_dir,
        server_host,
        &server_files,
        server_latest.as_ref(),
        Some(server),
        takeover.server.as_deref(),
    )
    .await?;
    let _ = state
        .database
        .operations()
        .set_running(operation_id, 60)
        .await;

    let credential = state
        .database
        .topology()
        .binding_credential(binding.id)
        .await
        .map_err(db_config)?
        .ok_or_else(|| ConfigError::Inspection("binding credential is missing".into()))?;
    let client_files = client_candidate(server, server_host, &credential);
    let client_latest = state
        .database
        .topology()
        .latest_revision(client_host.id)
        .await
        .map_err(db_config)?;
    let client_outcome = nut::apply_config(
        &state.ssh,
        &state.settings.data_dir,
        client_host,
        &client_files,
        client_latest.as_ref(),
        None,
        takeover.client.as_deref(),
    )
    .await;
    let client_outcome = match client_outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            if let Err(rollback) = nut::restore_backup(
                &state.ssh,
                server_host,
                std::path::Path::new(&server_outcome.backup_path),
            )
            .await
            {
                return Err(ConfigError::RollbackFailed(format!(
                    "client apply failed: {error}; server rollback failed: {rollback}"
                )));
            }
            return Err(error);
        }
    };
    let server_revision = state
        .database
        .topology()
        .create_revision(
            server_host.id,
            &server_outcome.manifest_json,
            &server_outcome.backup_path,
        )
        .await
        .map_err(db_config)?;
    state
        .database
        .topology()
        .set_server_applied(server.id, server_revision)
        .await
        .map_err(db_config)?;
    let client_revision = state
        .database
        .topology()
        .create_revision(
            client_host.id,
            &client_outcome.manifest_json,
            &client_outcome.backup_path,
        )
        .await
        .map_err(db_config)?;
    state
        .database
        .topology()
        .set_binding_applied(binding.id, client_revision)
        .await
        .map_err(db_config)?;
    state
        .database
        .operations()
        .succeed_with_result(
            operation_id,
            &BindingApplyResult {
                server: AppliedConfigResult {
                    outcome: server_outcome,
                    revision_id: server_revision,
                },
                client: AppliedConfigResult {
                    outcome: client_outcome,
                    revision_id: client_revision,
                },
            },
        )
        .await
        .map_err(db_config)?;
    Ok(())
}

async fn find_server(state: &AppState, id: &str) -> Result<ServerRecord, ApiError> {
    let id = NutServerId::from_str(id)
        .map_err(|_| ApiError::bad_request("InvalidServerId", "server id must be a UUID"))?;
    state
        .database
        .topology()
        .get_server(id)
        .await?
        .ok_or_else(|| ApiError::not_found("server"))
}

async fn find_binding(state: &AppState, id: &str) -> Result<BindingRecord, ApiError> {
    let id = BindingId::from_str(id)
        .map_err(|_| ApiError::bad_request("InvalidBindingId", "binding id must be a UUID"))?;
    state
        .database
        .topology()
        .get_binding(id)
        .await?
        .ok_or_else(|| ApiError::not_found("binding"))
}

async fn find_host(state: &AppState, id: HostId) -> Result<nwm_common::Host, ApiError> {
    state
        .database
        .hosts()
        .get(id)
        .await?
        .ok_or_else(|| ApiError::not_found("host"))
}

async fn annotate_takeover_safety(
    state: &AppState,
    server_host: &nwm_common::Host,
    previews: &mut [&mut ConfigPreview],
) {
    if !previews
        .iter()
        .any(|preview| preview.takeover_required && !preview.role_transition_required)
    {
        return;
    }
    let safety = nut::require_safe_power_state(&state.ssh, server_host).await;
    for preview in previews
        .iter_mut()
        .filter(|preview| preview.takeover_required && !preview.role_transition_required)
    {
        match &safety {
            Ok(status) => {
                preview.takeover_allowed = true;
                preview.takeover_block_reason = None;
                preview.takeover_warning = status.warning.clone();
                tracing::debug!(host_id = %server_host.id, status = %status.summary, "configuration takeover is allowed");
            }
            Err(error) => {
                preview.takeover_allowed = false;
                preview.takeover_block_reason = Some(error.to_string());
                preview.takeover_warning = None;
            }
        }
    }
}

fn requested_takeover_snapshot(
    request: &ApplyRequest,
    host_id: HostId,
    required: bool,
) -> Result<Option<String>, ApiError> {
    if !required {
        return Ok(None);
    }
    if !request.takeover {
        return Ok(None);
    }
    request
        .takeover_snapshots
        .iter()
        .find(|snapshot| snapshot.host_id == host_id)
        .map(|snapshot| snapshot.snapshot_hash.clone())
        .map(Some)
        .ok_or_else(|| {
            ApiError::bad_request(
                "TakeoverSnapshotRequired",
                format!("a preview snapshot is required to take over host {host_id}"),
            )
        })
}

fn require_confirmation(confirmed: bool) -> Result<(), ApiError> {
    if confirmed {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "ConfirmationRequired",
            "configuration apply must be explicitly confirmed",
        ))
    }
}

fn operation_conflict() -> ApiError {
    ApiError::conflict(
        "HostOperationInProgress",
        "another operation is already running on one of these hosts",
    )
}

fn map_config_error(error: ConfigError) -> ApiError {
    match error {
        ConfigError::Ssh(error) => map_ssh_error(error),
        ConfigError::Conflict { .. } => {
            ApiError::conflict("ConfigurationConflict", error.to_string())
        }
        ConfigError::ChangedSincePreview => {
            ApiError::conflict("ConfigurationChangedSincePreview", error.to_string())
        }
        ConfigError::UnsafePowerState(_) => {
            ApiError::conflict("UnsafePowerState", error.to_string())
        }
        ConfigError::RoleTransitionRequired => {
            ApiError::conflict("RoleTransitionRequired", error.to_string())
        }
        ConfigError::ApplyFailed(_) => {
            ApiError::bad_gateway("ConfigApplyFailed", error.to_string())
        }
        ConfigError::RollbackFailed(_) => {
            ApiError::bad_gateway("ConfigRollbackFailed", error.to_string())
        }
        ConfigError::Backup(_) | ConfigError::Inspection(_) | ConfigError::InvalidManifest(_) => {
            ApiError::unprocessable("ConfigurationInvalid", error.to_string())
        }
    }
}

fn config_error_code(error: &ConfigError) -> &'static str {
    match error {
        ConfigError::Ssh(crate::ssh::SshError::HostKeyConfirmationRequired) => {
            "HostKeyConfirmationRequired"
        }
        ConfigError::Ssh(crate::ssh::SshError::HostKeyChanged) => "HostKeyChanged",
        ConfigError::Ssh(crate::ssh::SshError::Timeout { .. }) => "ConfigApplyTimedOut",
        ConfigError::Ssh(_) => "SshError",
        ConfigError::Conflict { .. } => "ConfigurationConflict",
        ConfigError::ChangedSincePreview => "ConfigurationChangedSincePreview",
        ConfigError::UnsafePowerState(_) => "UnsafePowerState",
        ConfigError::RoleTransitionRequired => "RoleTransitionRequired",
        ConfigError::ApplyFailed(_) => "ConfigApplyFailedRolledBack",
        ConfigError::RollbackFailed(_) => "ConfigRollbackFailed",
        ConfigError::Backup(_) => "BackupFailed",
        ConfigError::Inspection(_) => "ConfigurationInvalid",
        ConfigError::InvalidManifest(_) => "InvalidManifest",
    }
}

async fn record_config_failure(
    repository: &crate::persistence::OperationRepository,
    operation_id: OperationId,
    error: &ConfigError,
) {
    if let Err(persistence_error) = repository
        .fail(operation_id, config_error_code(error), &error.to_string())
        .await
    {
        tracing::error!(%persistence_error, %operation_id, "failed to record configuration failure");
    }
}

fn db_config(error: crate::persistence::PersistenceError) -> ConfigError {
    ConfigError::Inspection(format!("database operation failed: {error}"))
}

fn default_ups_name() -> String {
    "ups".into()
}
