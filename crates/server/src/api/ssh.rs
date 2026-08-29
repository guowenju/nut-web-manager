use std::str::FromStr;

use axum::{
    Json,
    extract::{Path, State},
};
use nwm_common::{Host, HostId};
use serde::{Deserialize, Serialize};

use crate::{
    ssh::{EnvironmentReport, HostKeyInspection, SshError, SshTestReport},
    state::AppState,
};

use super::ApiError;

#[derive(Serialize)]
pub struct PublicKeyResponse {
    algorithm: &'static str,
    public_key: String,
}

#[derive(Deserialize)]
pub struct TrustHostKeyRequest {
    fingerprint: String,
}

pub async fn public_key(State(state): State<AppState>) -> Json<PublicKeyResponse> {
    Json(PublicKeyResponse {
        algorithm: "ssh-ed25519",
        public_key: state.ssh.public_key().to_owned(),
    })
}

pub async fn test(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<SshTestReport>, ApiError> {
    let host = find_host(&state, &id).await?;
    state.ssh.test(&host).await.map(Json).map_err(map_ssh_error)
}

pub async fn trust(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<TrustHostKeyRequest>,
) -> Result<Json<HostKeyInspection>, ApiError> {
    let host = find_host(&state, &id).await?;
    let fingerprint = request.fingerprint.trim();
    if !fingerprint.starts_with("SHA256:") || fingerprint.len() > 128 {
        return Err(ApiError::bad_request(
            "InvalidFingerprint",
            "fingerprint must be a SHA256 SSH fingerprint",
        ));
    }
    state
        .ssh
        .trust_host_key(&host, fingerprint)
        .await
        .map(Json)
        .map_err(map_ssh_error)
}

pub async fn environment(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<EnvironmentReport>, ApiError> {
    let host = find_host(&state, &id).await?;
    let report = state.ssh.environment(&host).await.map_err(map_ssh_error)?;
    state
        .database
        .hosts()
        .update_platform(host.id, &report.platform)
        .await?;
    Ok(Json(report))
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

pub(super) fn map_ssh_error(error: SshError) -> ApiError {
    match error {
        SshError::HostKeyConfirmationRequired => ApiError::conflict(
            "HostKeyConfirmationRequired",
            "confirm the SSH host key before connecting",
        ),
        SshError::HostKeyChanged => ApiError::conflict(
            "HostKeyChanged",
            "the SSH host key differs from the trusted key",
        ),
        SshError::FingerprintMismatch => ApiError::conflict(
            "FingerprintMismatch",
            "the host key changed after it was displayed; scan it again",
        ),
        SshError::Timeout { .. } | SshError::Command { .. } => {
            ApiError::bad_gateway("SshUnavailable", error.to_string())
        }
        SshError::Io(_) | SshError::InvalidOutput(_) => {
            tracing::error!(%error, "SSH management operation failed");
            ApiError::internal("SshError", "the SSH management operation failed")
        }
    }
}
