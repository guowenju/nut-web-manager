use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use chrono::{DateTime, Utc};
use nwm_common::NutServerId;

use crate::{
    auth::SessionStore, config::Settings, operation::HostOperationRegistry, persistence::Database,
    ssh::SshManager,
};

#[derive(Clone)]
pub struct AppState {
    pub database: Database,
    pub settings: Arc<Settings>,
    pub sessions: SessionStore,
    pub ssh: SshManager,
    pub host_operations: HostOperationRegistry,
    pub last_verified_at: Arc<Mutex<HashMap<NutServerId, DateTime<Utc>>>>,
}

impl AppState {
    pub fn new(database: Database, settings: Settings, ssh: SshManager) -> Self {
        Self {
            database,
            settings: Arc::new(settings),
            sessions: SessionStore::default(),
            ssh,
            host_operations: HostOperationRegistry::default(),
            last_verified_at: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}
