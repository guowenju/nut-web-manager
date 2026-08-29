use std::str::FromStr;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use nwm_common::{Host, HostId, HostRole, OperationId};
use serde::{Deserialize, Serialize};

use crate::{
    nut::{self, NutDeactivateError, NutInstallError, NutInstallStatus, UsbScanError},
    state::AppState,
};

use super::{ApiError, ssh::map_ssh_error};

#[derive(Deserialize)]
pub struct InstallRequest {
    confirmed: bool,
}

#[derive(Serialize)]
pub struct OperationAccepted {
    pub(super) operation_id: OperationId,
}

pub async fn install_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<NutInstallStatus>, ApiError> {
    let host = find_host(&state, &id).await?;
    let status = nut::status(&state.ssh, &host)
        .await
        .map_err(map_install_error)?;
    state
        .database
        .hosts()
        .update_platform(host.id, &status.platform)
        .await?;
    Ok(Json(status))
}

pub async fn install(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<InstallRequest>,
) -> Result<(StatusCode, Json<OperationAccepted>), ApiError> {
    if !request.confirmed {
        return Err(map_install_error(NutInstallError::ConfirmationRequired));
    }
    let host = find_host(&state, &id).await?;
    let operation_guard = state
        .host_operations
        .try_acquire(host.id)
        .ok_or_else(|| map_install_error(NutInstallError::HostOperationInProgress))?;
    let operation = state
        .database
        .operations()
        .create(Some(host.id), "nut_install")
        .await?;
    let operation_id = operation.id;
    let task_state = state.clone();
    tokio::spawn(async move {
        let _operation_guard = operation_guard;
        let repository = task_state.database.operations();
        if let Err(error) = repository.set_running(operation_id, 10).await {
            tracing::error!(%error, %operation_id, "failed to start NUT install operation");
            let _ = repository
                .fail(
                    operation_id,
                    "DatabaseError",
                    "failed to mark the install operation as running",
                )
                .await;
            return;
        }

        match nut::install(&task_state.ssh, &host).await {
            Ok(status) => {
                if let Err(error) = task_state
                    .database
                    .hosts()
                    .update_platform(host.id, &status.platform)
                    .await
                {
                    tracing::error!(%error, %operation_id, "failed to persist detected platform");
                    let _ = repository
                        .fail(
                            operation_id,
                            "DatabaseError",
                            "failed to persist detected platform",
                        )
                        .await;
                    return;
                }
                if let Err(error) = repository.succeed(operation_id).await {
                    tracing::error!(%error, %operation_id, "failed to finish NUT install operation");
                }
            }
            Err(error) => {
                let (code, detail) = operation_error(&error);
                if let Err(persistence_error) = repository.fail(operation_id, code, &detail).await {
                    tracing::error!(%persistence_error, %operation_id, "failed to record NUT install failure");
                }
            }
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(OperationAccepted { operation_id }),
    ))
}

pub async fn deactivate(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<InstallRequest>,
) -> Result<(StatusCode, Json<OperationAccepted>), ApiError> {
    if !request.confirmed {
        return Err(ApiError::bad_request(
            "ConfirmationRequired",
            "deactivation must be explicitly confirmed",
        ));
    }
    let host = find_host(&state, &id).await?;
    let operation_guard = state.host_operations.try_acquire(host.id).ok_or_else(|| {
        ApiError::conflict(
            "HostOperationInProgress",
            "another modifying operation is already running on this host",
        )
    })?;
    let operation = state
        .database
        .operations()
        .create(Some(host.id), "nut_deactivate")
        .await?;
    let operation_id = operation.id;
    let task_state = state.clone();
    tokio::spawn(async move {
        let _operation_guard = operation_guard;
        let operations = task_state.database.operations();
        if operations.set_running(operation_id, 20).await.is_err() {
            return;
        }
        match nut::deactivate(&task_state.ssh, &host).await {
            Ok(outcome) => {
                if let Err(error) = task_state
                    .database
                    .topology()
                    .clear_host_configuration(host.id)
                    .await
                {
                    let _ = operations
                        .fail(operation_id, "DatabaseError", &error.to_string())
                        .await;
                    return;
                }
                if let Err(error) = operations.succeed_with_result(operation_id, &outcome).await {
                    tracing::error!(%error, %operation_id, "failed to finish NUT deactivation");
                }
            }
            Err(error) => {
                let (code, detail) = deactivate_operation_error(&error);
                let _ = operations.fail(operation_id, code, &detail).await;
            }
        }
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(OperationAccepted { operation_id }),
    ))
}

pub async fn scan_usb(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<OperationAccepted>), ApiError> {
    let host = find_host(&state, &id).await?;
    if host.role != HostRole::Server {
        return Err(ApiError::unprocessable(
            "ServerRoleRequired",
            "USB UPS scanning is only available for Server hosts",
        ));
    }
    let operation_guard = state.host_operations.try_acquire(host.id).ok_or_else(|| {
        ApiError::conflict(
            "HostOperationInProgress",
            "another operation is already running on this host",
        )
    })?;
    let operation = state
        .database
        .operations()
        .create(Some(host.id), "usb_ups_scan")
        .await?;
    let operation_id = operation.id;
    let task_state = state.clone();
    tokio::spawn(async move {
        let _operation_guard = operation_guard;
        let repository = task_state.database.operations();
        if let Err(error) = repository.set_running(operation_id, 20).await {
            tracing::error!(%error, %operation_id, "failed to start USB scan operation");
            let _ = repository
                .fail(
                    operation_id,
                    "DatabaseError",
                    "failed to mark the USB scan operation as running",
                )
                .await;
            return;
        }

        match nut::scan(&task_state.ssh, &host).await {
            Ok(result) => {
                if let Err(error) = repository.succeed_with_result(operation_id, &result).await {
                    tracing::error!(%error, %operation_id, "failed to persist USB scan result");
                    let _ = repository
                        .fail(
                            operation_id,
                            "DatabaseError",
                            "failed to persist USB scan result",
                        )
                        .await;
                }
            }
            Err(error) => {
                let (code, detail) = scan_operation_error(&error);
                if let Err(persistence_error) = repository.fail(operation_id, code, &detail).await {
                    tracing::error!(%persistence_error, %operation_id, "failed to record USB scan failure");
                }
            }
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(OperationAccepted { operation_id }),
    ))
}

async fn find_host(state: &AppState, value: &str) -> Result<Host, ApiError> {
    let id = HostId::from_str(value)
        .map_err(|_| ApiError::bad_request("InvalidHostId", "host id must be a UUID"))?;
    state
        .database
        .hosts()
        .get(id)
        .await?
        .ok_or_else(|| ApiError::not_found("host"))
}

fn map_install_error(error: NutInstallError) -> ApiError {
    match error {
        NutInstallError::Ssh(error) => map_ssh_error(error),
        NutInstallError::UnsupportedPlatform => ApiError::unprocessable(
            "UnsupportedPlatform",
            "automatic installation requires Debian 13, PVE 9, or PBS 4 with systemd",
        ),
        NutInstallError::HostOperationInProgress => ApiError::conflict(
            "HostOperationInProgress",
            "another modifying operation is already running on this host",
        ),
        NutInstallError::ConfirmationRequired => ApiError::bad_request(
            "ConfirmationRequired",
            "automatic installation must be explicitly confirmed",
        ),
        NutInstallError::InstallTimedOut => ApiError::gateway_timeout(
            "InstallTimedOut",
            "APT did not finish within 10 minutes; inspect the host before retrying",
        ),
        NutInstallError::InstallFailed { code, detail } => {
            ApiError::bad_gateway(code.as_str(), detail)
        }
        NutInstallError::VerificationFailed { package } => ApiError::bad_gateway(
            "InstallVerificationFailed",
            format!("APT completed but {package} is still not installed"),
        ),
    }
}

fn operation_error(error: &NutInstallError) -> (&'static str, String) {
    let code = match error {
        NutInstallError::Ssh(crate::ssh::SshError::HostKeyConfirmationRequired) => {
            "HostKeyConfirmationRequired"
        }
        NutInstallError::Ssh(crate::ssh::SshError::HostKeyChanged) => "HostKeyChanged",
        NutInstallError::Ssh(crate::ssh::SshError::FingerprintMismatch) => "FingerprintMismatch",
        NutInstallError::Ssh(crate::ssh::SshError::Timeout { .. }) => "SshTimeout",
        NutInstallError::Ssh(crate::ssh::SshError::Command { .. }) => "SshUnavailable",
        NutInstallError::Ssh(_) => "SshError",
        NutInstallError::UnsupportedPlatform => "UnsupportedPlatform",
        NutInstallError::HostOperationInProgress => "HostOperationInProgress",
        NutInstallError::ConfirmationRequired => "ConfirmationRequired",
        NutInstallError::InstallTimedOut => "InstallTimedOut",
        NutInstallError::InstallFailed { code, .. } => code.as_str(),
        NutInstallError::VerificationFailed { .. } => "InstallVerificationFailed",
    };
    (code, error.to_string())
}

fn deactivate_operation_error(error: &NutDeactivateError) -> (&'static str, String) {
    let code = match error {
        NutDeactivateError::Ssh(crate::ssh::SshError::HostKeyConfirmationRequired) => {
            "HostKeyConfirmationRequired"
        }
        NutDeactivateError::Ssh(crate::ssh::SshError::HostKeyChanged) => "HostKeyChanged",
        NutDeactivateError::Ssh(crate::ssh::SshError::Timeout { .. }) => "SshTimeout",
        NutDeactivateError::Ssh(crate::ssh::SshError::Command { .. }) => "DeactivateFailed",
        NutDeactivateError::Ssh(_) => "SshError",
    };
    (code, error.to_string())
}

fn scan_operation_error(error: &UsbScanError) -> (&'static str, String) {
    let code = match error {
        UsbScanError::Ssh(crate::ssh::SshError::HostKeyConfirmationRequired) => {
            "HostKeyConfirmationRequired"
        }
        UsbScanError::Ssh(crate::ssh::SshError::HostKeyChanged) => "HostKeyChanged",
        UsbScanError::Ssh(crate::ssh::SshError::FingerprintMismatch) => "FingerprintMismatch",
        UsbScanError::Ssh(crate::ssh::SshError::Timeout { .. }) => "UsbScanTimedOut",
        UsbScanError::Ssh(crate::ssh::SshError::Command { .. }) => "SshUnavailable",
        UsbScanError::Ssh(_) => "SshError",
        UsbScanError::NutServerNotInstalled => "NutServerNotInstalled",
        UsbScanError::ScannerUnavailable => "ScannerUnavailable",
        UsbScanError::UsbScanUnavailable => "UsbScanUnavailable",
        UsbScanError::UsbScanFailed(_) => "UsbScanFailed",
        UsbScanError::InvalidOutput(_) => "InvalidScannerOutput",
    };
    (code, error.to_string())
}
