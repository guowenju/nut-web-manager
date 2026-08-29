mod auth;
mod dashboard;
mod error;
mod health;
mod hosts;
mod nut;
mod operations;
mod ssh;
mod topology;
mod web;

pub use auth::{login, logout, require_session, session};
pub use dashboard::dashboard;
pub use error::ApiError;
pub use health::health;
pub use hosts::{
    create as create_host, delete as delete_host, get as get_host, list as list_hosts,
};
pub use nut::{
    deactivate as deactivate_nut, install as install_nut, install_status as nut_install_status,
    scan_usb,
};
pub use operations::get as get_operation;
pub use ssh::{environment as host_environment, public_key as ssh_public_key};
pub use ssh::{test as test_ssh, trust as trust_ssh};
pub use topology::{
    apply_binding, apply_server, create_binding, get_binding, get_server, list_bindings,
    list_servers, preview_binding, preview_server, select_server, update_shutdown,
};
pub use web::asset as web_asset;
