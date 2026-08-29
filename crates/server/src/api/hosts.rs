use std::str::FromStr;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use nwm_common::{Host, HostId, HostRole};
use serde::{Deserialize, Serialize};

use crate::{persistence::CreateHost, state::AppState};

use super::ApiError;

#[derive(Deserialize)]
pub struct CreateHostRequest {
    name: String,
    address: String,
    #[serde(default = "default_ssh_port")]
    ssh_port: u16,
    #[serde(default = "default_username")]
    username: String,
    role: HostRole,
}

#[derive(Serialize)]
pub struct DeleteHostResponse {
    remote_modified: bool,
    public_key_removed: bool,
    nut_uninstalled: bool,
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<Host>>, ApiError> {
    Ok(Json(state.database.hosts().list().await?))
}

pub async fn create(
    State(state): State<AppState>,
    Json(request): Json<CreateHostRequest>,
) -> Result<(StatusCode, Json<Host>), ApiError> {
    let host = state
        .database
        .hosts()
        .create(CreateHost {
            name: request.name,
            address: request.address,
            ssh_port: request.ssh_port,
            username: request.username,
            role: request.role,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(host)))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Host>, ApiError> {
    let id = parse_host_id(&id)?;
    state
        .database
        .hosts()
        .get(id)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::not_found("host"))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DeleteHostResponse>, ApiError> {
    let id = parse_host_id(&id)?;
    if !state.database.hosts().delete(id).await? {
        return Err(ApiError::not_found("host"));
    }

    Ok(Json(DeleteHostResponse {
        remote_modified: false,
        public_key_removed: false,
        nut_uninstalled: false,
    }))
}

fn parse_host_id(value: &str) -> Result<HostId, ApiError> {
    HostId::from_str(value)
        .map_err(|_| ApiError::bad_request("InvalidHostId", "host id must be a UUID"))
}

fn default_ssh_port() -> u16 {
    22
}

fn default_username() -> String {
    "root".into()
}
