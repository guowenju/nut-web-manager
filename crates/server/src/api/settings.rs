use axum::{Json, extract::State};
use serde::Deserialize;

use crate::{
    persistence::{AppSettings, DefaultPage},
    state::AppState,
};

use super::ApiError;

#[derive(Deserialize)]
pub struct UpdateSettingsRequest {
    default_page: DefaultPage,
}

pub async fn get(State(state): State<AppState>) -> Result<Json<AppSettings>, ApiError> {
    Ok(Json(state.database.settings().get().await?))
}

pub async fn update(
    State(state): State<AppState>,
    Json(request): Json<UpdateSettingsRequest>,
) -> Result<Json<AppSettings>, ApiError> {
    Ok(Json(
        state
            .database
            .settings()
            .update(request.default_page)
            .await?,
    ))
}
