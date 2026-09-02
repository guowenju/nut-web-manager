use axum::{
    Router, middleware,
    routing::{get, post, put},
};

use crate::{api, state::AppState};

pub fn router(state: AppState) -> Router {
    let public = Router::new()
        .route("/api/v1/health", get(api::health))
        .route("/api/v1/auth/login", post(api::login));

    let authenticated = Router::new()
        .route("/api/v1/auth/session", get(api::session))
        .route("/api/v1/auth/logout", post(api::logout))
        .route("/api/v1/dashboard", get(api::dashboard))
        .route("/api/v1/dashboard/history", get(api::dashboard_history))
        .route(
            "/api/v1/ups-monitor/sources",
            get(api::list_monitor_sources).post(api::create_monitor_source),
        )
        .route(
            "/api/v1/ups-monitor/sources/{source_id}",
            put(api::update_monitor_source).delete(api::delete_monitor_source),
        )
        .route(
            "/api/v1/ups-monitor/sources/{source_id}/test",
            post(api::test_monitor_source),
        )
        .route(
            "/api/v1/ups-monitor/sources/{source_id}/discover",
            post(api::discover_monitor_source),
        )
        .route("/api/v1/ups-monitor/overview", get(api::monitor_overview))
        .route(
            "/api/v1/ups-monitor/devices/{device_id}/snapshot",
            get(api::monitor_snapshot),
        )
        .route(
            "/api/v1/ups-monitor/devices/{device_id}/history",
            get(api::monitor_history),
        )
        .route(
            "/api/v1/ups-monitor/devices/{device_id}/events",
            get(api::monitor_events),
        )
        .route("/api/v1/ssh/public-key", get(api::ssh_public_key))
        .route("/api/v1/operations/{operation_id}", get(api::get_operation))
        .route("/api/v1/hosts", get(api::list_hosts).post(api::create_host))
        .route(
            "/api/v1/hosts/{host_id}",
            get(api::get_host).delete(api::delete_host),
        )
        .route("/api/v1/hosts/{host_id}/ssh/test", post(api::test_ssh))
        .route("/api/v1/hosts/{host_id}/ssh/trust", post(api::trust_ssh))
        .route(
            "/api/v1/hosts/{host_id}/environment",
            get(api::host_environment),
        )
        .route(
            "/api/v1/hosts/{host_id}/nut/install",
            get(api::nut_install_status).post(api::install_nut),
        )
        .route(
            "/api/v1/hosts/{host_id}/nut/deactivate",
            post(api::deactivate_nut),
        )
        .route("/api/v1/hosts/{host_id}/nut/scan", post(api::scan_usb))
        .route(
            "/api/v1/servers",
            get(api::list_servers).post(api::select_server),
        )
        .route("/api/v1/servers/{server_id}", get(api::get_server))
        .route(
            "/api/v1/servers/{server_id}/shutdown",
            post(api::update_shutdown),
        )
        .route(
            "/api/v1/servers/{server_id}/config/preview",
            get(api::preview_server),
        )
        .route(
            "/api/v1/servers/{server_id}/config/apply",
            post(api::apply_server),
        )
        .route(
            "/api/v1/bindings",
            get(api::list_bindings).post(api::create_binding),
        )
        .route("/api/v1/bindings/{binding_id}", get(api::get_binding))
        .route(
            "/api/v1/bindings/{binding_id}/config/preview",
            get(api::preview_binding),
        )
        .route(
            "/api/v1/bindings/{binding_id}/config/apply",
            post(api::apply_binding),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            api::require_session,
        ));

    Router::new()
        .merge(public)
        .merge(authenticated)
        .fallback(api::web_asset)
        .with_state(state)
}
