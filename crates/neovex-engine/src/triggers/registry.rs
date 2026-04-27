use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use neovex_core::{
    DocumentPath, DocumentTriggerMatch, DocumentTriggerPattern, Error, FirestoreCloudEventType,
    Result,
};

#[derive(Debug)]
struct TriggerRegistryState {
    registrations: RwLock<HashMap<String, TriggerRegistration>>,
    ready: AtomicBool,
}

impl TriggerRegistryState {
    fn new() -> Self {
        Self {
            registrations: RwLock::new(HashMap::new()),
            ready: AtomicBool::new(false),
        }
    }
}

/// Stable handler registration stored in one tenant-scoped trigger registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerRegistration {
    id: String,
    event_type: FirestoreCloudEventType,
    pattern: DocumentTriggerPattern,
    enabled: bool,
}

impl TriggerRegistration {
    pub fn new(
        id: impl Into<String>,
        event_type: FirestoreCloudEventType,
        pattern: DocumentTriggerPattern,
    ) -> Result<Self> {
        let id = id.into();
        if id.trim().is_empty() {
            return Err(Error::InvalidInput(
                "trigger registration id cannot be empty".to_string(),
            ));
        }
        Ok(Self {
            id,
            event_type,
            pattern,
            enabled: true,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn event_type(&self) -> FirestoreCloudEventType {
        self.event_type
    }

    pub fn pattern(&self) -> &DocumentTriggerPattern {
        &self.pattern
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }
}

/// Successful pattern lookup result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerLookupMatch {
    pub registration: TriggerRegistration,
    pub path_match: DocumentTriggerMatch,
}

/// Thread-safe, tenant-scoped trigger registry.
#[derive(Debug, Clone)]
pub struct TriggerRegistry {
    state: Arc<TriggerRegistryState>,
}

impl TriggerRegistry {
    pub fn new() -> Self {
        Self {
            state: Arc::new(TriggerRegistryState::new()),
        }
    }

    pub fn register(&self, registration: TriggerRegistration) -> Result<()> {
        let mut registrations = self
            .state
            .registrations
            .write()
            .expect("trigger registry lock should not be poisoned");
        if registrations.contains_key(registration.id()) {
            return Err(Error::InvalidInput(format!(
                "trigger registration `{}` already exists",
                registration.id()
            )));
        }
        registrations.insert(registration.id.clone(), registration);
        self.state.ready.store(true, Ordering::Release);
        Ok(())
    }

    pub fn deregister(&self, id: &str) -> bool {
        self.state
            .registrations
            .write()
            .expect("trigger registry lock should not be poisoned")
            .remove(id)
            .is_some()
    }

    pub fn enable(&self, id: &str) -> bool {
        self.set_enabled(id, true)
    }

    pub fn disable(&self, id: &str) -> bool {
        self.set_enabled(id, false)
    }

    pub fn list(&self) -> Vec<TriggerRegistration> {
        let mut registrations = self
            .state
            .registrations
            .read()
            .expect("trigger registry lock should not be poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        registrations.sort_by(|left, right| left.id.cmp(&right.id));
        registrations
    }

    pub fn replace(&self, registrations: Vec<TriggerRegistration>) -> Result<()> {
        let mut next = HashMap::with_capacity(registrations.len());
        for registration in registrations {
            if next.insert(registration.id.clone(), registration).is_some() {
                return Err(Error::InvalidInput(
                    "trigger registration ids must be unique".to_string(),
                ));
            }
        }
        let mut state = self
            .state
            .registrations
            .write()
            .expect("trigger registry lock should not be poisoned");
        *state = next;
        self.state.ready.store(true, Ordering::Release);
        Ok(())
    }

    pub fn is_ready(&self) -> bool {
        self.state.ready.load(Ordering::Acquire)
    }

    pub fn lookup(
        &self,
        event_type: FirestoreCloudEventType,
        document_path: &DocumentPath,
    ) -> Vec<TriggerLookupMatch> {
        let mut matches = self
            .state
            .registrations
            .read()
            .expect("trigger registry lock should not be poisoned")
            .values()
            .filter(|registration| registration.enabled && registration.event_type == event_type)
            .filter_map(|registration| {
                registration
                    .pattern
                    .matches(document_path)
                    .map(|path_match| TriggerLookupMatch {
                        registration: registration.clone(),
                        path_match,
                    })
            })
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| left.registration.id.cmp(&right.registration.id));
        matches
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.state
            .registrations
            .read()
            .expect("trigger registry lock should not be poisoned")
            .len()
    }

    fn set_enabled(&self, id: &str, enabled: bool) -> bool {
        let mut registrations = self
            .state
            .registrations
            .write()
            .expect("trigger registry lock should not be poisoned");
        let Some(registration) = registrations.get_mut(id) else {
            return false;
        };
        registration.enabled = enabled;
        true
    }
}

impl Default for TriggerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;

    use neovex_core::{DocumentPath, DocumentTriggerPattern, FirestoreCloudEventType};

    use super::{TriggerRegistration, TriggerRegistry};

    #[test]
    fn registry_registers_lists_and_deregisters_triggers() {
        let registry = TriggerRegistry::new();
        let registration = TriggerRegistration::new(
            "firebase:syncUser",
            FirestoreCloudEventType::Written,
            DocumentTriggerPattern::from_segments(["users", "{userId}"])
                .expect("pattern should parse"),
        )
        .expect("registration should parse");

        registry
            .register(registration.clone())
            .expect("registration should insert");
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.list(), vec![registration]);
        assert!(registry.deregister("firebase:syncUser"));
        assert_eq!(registry.len(), 0);
        assert!(!registry.deregister("firebase:syncUser"));
    }

    #[test]
    fn registry_lookup_respects_event_type_and_wildcard_params() {
        let registry = TriggerRegistry::new();
        registry
            .register(
                TriggerRegistration::new(
                    "firebase:userWritten",
                    FirestoreCloudEventType::Written,
                    DocumentTriggerPattern::from_segments(["users", "{userId}"])
                        .expect("pattern should parse"),
                )
                .expect("registration should parse"),
            )
            .expect("registration should insert");
        registry
            .register(
                TriggerRegistration::new(
                    "firebase:messageCreated",
                    FirestoreCloudEventType::Created,
                    DocumentTriggerPattern::from_segments([
                        "users",
                        "{userId}",
                        "messages",
                        "{messageId}",
                    ])
                    .expect("pattern should parse"),
                )
                .expect("registration should parse"),
            )
            .expect("registration should insert");

        let user_path = DocumentPath::from_segments(["users", "alice"]).expect("path should parse");
        let message_path = DocumentPath::from_segments(["users", "alice", "messages", "m1"])
            .expect("path should parse");

        let written_matches = registry.lookup(FirestoreCloudEventType::Written, &user_path);
        assert_eq!(written_matches.len(), 1);
        assert_eq!(written_matches[0].registration.id(), "firebase:userWritten");
        assert_eq!(
            written_matches[0]
                .path_match
                .params()
                .get("userId")
                .map(String::as_str),
            Some("alice")
        );

        let created_matches = registry.lookup(FirestoreCloudEventType::Created, &message_path);
        assert_eq!(created_matches.len(), 1);
        assert_eq!(
            created_matches[0].registration.id(),
            "firebase:messageCreated"
        );
        assert_eq!(
            created_matches[0]
                .path_match
                .params()
                .get("messageId")
                .map(String::as_str),
            Some("m1")
        );

        assert!(
            registry
                .lookup(FirestoreCloudEventType::Deleted, &message_path)
                .is_empty()
        );
    }

    #[test]
    fn registry_enable_disable_controls_lookup() {
        let registry = TriggerRegistry::new();
        registry
            .register(
                TriggerRegistration::new(
                    "firebase:userDeleted",
                    FirestoreCloudEventType::Deleted,
                    DocumentTriggerPattern::from_segments(["users", "{userId}"])
                        .expect("pattern should parse"),
                )
                .expect("registration should parse"),
            )
            .expect("registration should insert");
        let path = DocumentPath::from_segments(["users", "alice"]).expect("path should parse");

        assert_eq!(
            registry
                .lookup(FirestoreCloudEventType::Deleted, &path)
                .len(),
            1
        );
        assert!(registry.disable("firebase:userDeleted"));
        assert!(
            registry
                .lookup(FirestoreCloudEventType::Deleted, &path)
                .is_empty()
        );
        assert!(registry.enable("firebase:userDeleted"));
        assert_eq!(
            registry
                .lookup(FirestoreCloudEventType::Deleted, &path)
                .len(),
            1
        );
    }

    #[test]
    fn registry_rejects_duplicate_ids_and_keeps_tenant_isolation() {
        let left = TriggerRegistry::new();
        let right = TriggerRegistry::new();
        let registration = TriggerRegistration::new(
            "shared-id",
            FirestoreCloudEventType::Written,
            DocumentTriggerPattern::from_segments(["users", "{userId}"])
                .expect("pattern should parse"),
        )
        .expect("registration should parse");
        left.register(registration.clone())
            .expect("left registry should insert");
        right
            .register(registration)
            .expect("right registry should allow same id in another tenant");

        let duplicate = left
            .register(
                TriggerRegistration::new(
                    "shared-id",
                    FirestoreCloudEventType::Created,
                    DocumentTriggerPattern::from_segments(["tasks", "{taskId}"])
                        .expect("pattern should parse"),
                )
                .expect("registration should parse"),
            )
            .expect_err("duplicate ids should be rejected");
        assert!(duplicate.to_string().contains("already exists"));
        assert_eq!(left.len(), 1);
        assert_eq!(right.len(), 1);
    }

    #[test]
    fn registry_supports_concurrent_registration() {
        let registry = Arc::new(TriggerRegistry::new());
        let mut workers = Vec::new();
        for index in 0..8 {
            let registry = registry.clone();
            workers.push(thread::spawn(move || {
                let registration = TriggerRegistration::new(
                    format!("trigger-{index}"),
                    FirestoreCloudEventType::Written,
                    DocumentTriggerPattern::from_segments(["users", "{userId}"])
                        .expect("pattern should parse"),
                )
                .expect("registration should parse");
                registry
                    .register(registration)
                    .expect("concurrent insert should succeed");
            }));
        }
        for worker in workers {
            worker.join().expect("worker should finish");
        }

        assert_eq!(registry.len(), 8);
        assert_eq!(registry.list().len(), 8);
    }
}
