use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use uuid::Uuid;

pub const SESSION_COOKIE_NAME: &str = "nwm_session";
pub const SESSION_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Default)]
pub struct SessionStore {
    sessions: Arc<Mutex<HashMap<String, Instant>>>,
}

impl SessionStore {
    pub fn create(&self) -> String {
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        self.sessions
            .lock()
            .expect("session store lock poisoned")
            .insert(token.clone(), Instant::now() + SESSION_MAX_AGE);
        token
    }

    pub fn validate(&self, token: &str) -> bool {
        let mut sessions = self.sessions.lock().expect("session store lock poisoned");
        let now = Instant::now();
        sessions.retain(|_, expires_at| *expires_at > now);
        sessions.contains_key(token)
    }

    pub fn remove(&self, token: &str) {
        self.sessions
            .lock()
            .expect("session store lock poisoned")
            .remove(token);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_can_be_created_validated_and_removed() {
        let sessions = SessionStore::default();
        let token = sessions.create();

        assert!(sessions.validate(&token));
        assert!(!sessions.validate("unknown"));

        sessions.remove(&token);
        assert!(!sessions.validate(&token));
    }
}
