use std::str::FromStr;

use axum::{
    Json,
    extract::{Path, State},
};
use nwm_common::{Operation, OperationId};

use crate::state::AppState;

use super::ApiError;

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Operation>, ApiError> {
    let id = OperationId::from_str(&id)
        .map_err(|_| ApiError::bad_request("InvalidOperationId", "operation id must be a UUID"))?;
    state
        .database
        .operations()
        .get(id)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::not_found("operation"))
}
