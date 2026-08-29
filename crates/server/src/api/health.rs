use axum::{Json, extract::State, http::StatusCode};
use serde::Serialize;

use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    status: &'static str,
    version: &'static str,
    database: &'static str,
    default_credentials: bool,
}

pub async fn health(State(state): State<AppState>) -> (StatusCode, Json<HealthResponse>) {
    match state.database.health().await {
        Ok(()) => (
            StatusCode::OK,
            Json(HealthResponse {
                status: "ok",
                version: env!("CARGO_PKG_VERSION"),
                database: "ok",
                default_credentials: state.settings.uses_default_admin_credentials(),
            }),
        ),
        Err(error) => {
            tracing::error!(%error, "database health check failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(HealthResponse {
                    status: "degraded",
                    version: env!("CARGO_PKG_VERSION"),
                    database: "unavailable",
                    default_credentials: state.settings.uses_default_admin_credentials(),
                }),
            )
        }
    }
}
