use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use nwm_common::HostId;

#[derive(Clone, Default)]
pub struct HostOperationRegistry {
    active: Arc<Mutex<HashSet<HostId>>>,
}

impl HostOperationRegistry {
    pub fn try_acquire(&self, host_id: HostId) -> Option<HostOperationGuard> {
        let mut active = self.active.lock().expect("host operation lock poisoned");
        if !active.insert(host_id) {
            return None;
        }
        Some(HostOperationGuard {
            host_id,
            active: self.active.clone(),
        })
    }
}

pub struct HostOperationGuard {
    host_id: HostId,
    active: Arc<Mutex<HashSet<HostId>>>,
}

impl Drop for HostOperationGuard {
    fn drop(&mut self) {
        self.active
            .lock()
            .expect("host operation lock poisoned")
            .remove(&self.host_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_mutation_can_hold_a_host_lock() {
        let registry = HostOperationRegistry::default();
        let host_id = HostId::new();
        let first = registry.try_acquire(host_id).unwrap();
        assert!(registry.try_acquire(host_id).is_none());
        drop(first);
        assert!(registry.try_acquire(host_id).is_some());
    }
}
