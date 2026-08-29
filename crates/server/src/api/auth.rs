use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use crate::{
    auth::{SESSION_COOKIE_NAME, SESSION_MAX_AGE},
    state::AppState,
};

use super::ApiError;

#[derive(Deserialize)]
pub struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Serialize)]
pub struct SessionResponse {
    authenticated: bool,
    username: String,
    default_credentials: bool,
}

pub async fn login(
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if !state
        .settings
        .verify_admin_credentials(&request.username, &request.password)
    {
        return Err(ApiError::invalid_credentials());
    }

    let token = state.sessions.create();
    let cookie = format!(
        "{SESSION_COOKIE_NAME}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}",
        SESSION_MAX_AGE.as_secs()
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie).expect("generated session cookie must be valid"),
    );

    Ok((headers, Json(session_response(&state))))
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Some(token) = session_token(&headers) {
        state.sessions.remove(token);
    }

    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        header::SET_COOKIE,
        HeaderValue::from_static("nwm_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"),
    );
    (
        response_headers,
        Json(SessionResponse {
            authenticated: false,
            username: state.settings.admin_username.clone(),
            default_credentials: state.settings.uses_default_admin_credentials(),
        }),
    )
}

pub async fn session(State(state): State<AppState>) -> Json<SessionResponse> {
    Json(session_response(&state))
}

pub async fn require_session(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let authenticated =
        session_token(request.headers()).is_some_and(|token| state.sessions.validate(token));

    if !authenticated {
        return ApiError::unauthorized().into_response();
    }

    next.run(request).await
}

fn session_response(state: &AppState) -> SessionResponse {
    SessionResponse {
        authenticated: true,
        username: state.settings.admin_username.clone(),
        default_credentials: state.settings.uses_default_admin_credentials(),
    }
}

fn session_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|cookie| cookie.trim().split_once('='))
        .find_map(|(name, value)| (name == SESSION_COOKIE_NAME).then_some(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_cookie_is_found_among_other_cookies() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("theme=dark; nwm_session=secret; locale=zh"),
        );
        assert_eq!(session_token(&headers), Some("secret"));
    }
}
