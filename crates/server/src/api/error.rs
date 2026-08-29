use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Serialize;

use crate::persistence::PersistenceError;

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

#[derive(Serialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
}

impl ApiError {
    pub fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "Unauthorized",
            "authentication is required",
        )
    }

    pub fn invalid_credentials() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "InvalidCredentials",
            "invalid username or password",
        )
    }

    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    pub fn not_found(entity: &'static str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "NotFound",
            format!("{entity} was not found"),
        )
    }

    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }

    pub fn bad_gateway(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, code, message)
    }

    pub fn unprocessable(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, code, message)
    }

    pub fn gateway_timeout(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::GATEWAY_TIMEOUT, code, message)
    }

    pub fn internal(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, code, message)
    }

    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
}

impl From<PersistenceError> for ApiError {
    fn from(error: PersistenceError) -> Self {
        match error {
            PersistenceError::Conflict(message) => {
                Self::new(StatusCode::CONFLICT, "Conflict", message)
            }
            PersistenceError::InvalidData(message) => {
                Self::new(StatusCode::UNPROCESSABLE_ENTITY, "InvalidData", message)
            }
            error => {
                tracing::error!(%error, "database operation failed");
                Self::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DatabaseError",
                    "the database operation failed",
                )
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            self.status,
            Json(ErrorEnvelope {
                error: ErrorBody {
                    code: self.code,
                    message: self.message,
                },
            }),
        )
            .into_response()
    }
}
