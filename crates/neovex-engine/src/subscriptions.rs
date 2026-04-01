use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

use neovex_core::{
    CommitEntry, DependencySet, Document, PrincipalContext, PrincipalSnapshot, Query, TableName,
    commit_intersects_dependency_set,
};
use serde_json::Value;
use tokio::sync::mpsc;

/// A subscription event emitted by the engine.
#[derive(Debug, Clone)]
pub enum SubscriptionUpdate {
    Result {
        subscription_id: u64,
        request_id: Option<String>,
        commit: Option<CommitEntry>,
        deleted_documents: Vec<Document>,
        data: Vec<Value>,
    },
    Error {
        subscription_id: u64,
        request_id: Option<String>,
        message: String,
    },
}

#[derive(Debug, Clone)]
struct Subscription {
    id: u64,
    query: Query,
    dependencies: DependencySet,
    principal: PrincipalContext,
    principal_snapshot: PrincipalSnapshot,
    policy_revision: String,
    sender: mpsc::UnboundedSender<SubscriptionUpdate>,
}

#[derive(Debug, Clone)]
pub struct SubscriptionDelivery {
    pub id: u64,
    pub query: Query,
    pub principal: PrincipalContext,
    pub sender: mpsc::UnboundedSender<SubscriptionUpdate>,
}

#[derive(Debug)]
pub struct SubscriptionCleanupHandle {
    registry: Arc<SubscriptionRegistryState>,
    id: u64,
}

impl SubscriptionCleanupHandle {
    pub fn subscription_id(&self) -> u64 {
        self.id
    }
}

impl Drop for SubscriptionCleanupHandle {
    fn drop(&mut self) {
        self.registry.remove(self.id);
    }
}

#[derive(Debug)]
pub struct SubscriptionRegistration {
    id: u64,
    cleanup: SubscriptionCleanupHandle,
}

impl SubscriptionRegistration {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn into_parts(self) -> (u64, SubscriptionCleanupHandle) {
        (self.id, self.cleanup)
    }
}

#[derive(Debug)]
struct SubscriptionRegistryState {
    next_id: AtomicU64,
    subscriptions: RwLock<HashMap<u64, Subscription>>,
}

impl SubscriptionRegistryState {
    fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            subscriptions: RwLock::new(HashMap::new()),
        }
    }

    fn remove(&self, id: u64) {
        self.subscriptions
            .write()
            .expect("subscription lock should not be poisoned")
            .remove(&id);
    }

    fn len(&self) -> usize {
        self.subscriptions
            .read()
            .expect("subscription lock should not be poisoned")
            .len()
    }
}

/// In-memory subscription registry for a tenant.
#[derive(Debug)]
pub struct SubscriptionRegistry {
    state: Arc<SubscriptionRegistryState>,
}

impl SubscriptionRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self {
            state: Arc::new(SubscriptionRegistryState::new()),
        }
    }

    /// Registers a subscription and returns its stable id plus cleanup handle.
    pub fn register(
        &self,
        query: Query,
        principal: PrincipalContext,
        principal_snapshot: PrincipalSnapshot,
        policy_revision: String,
        sender: mpsc::UnboundedSender<SubscriptionUpdate>,
    ) -> SubscriptionRegistration {
        let id = self.state.next_id.fetch_add(1, Ordering::SeqCst);
        let subscription = Subscription {
            id,
            dependencies: DependencySet::from_engine_query(&query),
            principal,
            principal_snapshot,
            policy_revision,
            query,
            sender,
        };
        self.state
            .subscriptions
            .write()
            .expect("subscription lock should not be poisoned")
            .insert(id, subscription);
        SubscriptionRegistration {
            id,
            cleanup: SubscriptionCleanupHandle {
                registry: self.state.clone(),
                id,
            },
        }
    }

    /// Removes a subscription if present.
    pub fn remove(&self, id: u64) {
        self.state.remove(id);
    }

    pub(crate) fn len(&self) -> usize {
        self.state.len()
    }

    /// Returns the subscriptions affected by the provided commit.
    pub fn affected(
        &self,
        commit: &CommitEntry,
        candidate_documents: &[Document],
    ) -> Vec<SubscriptionDelivery> {
        self.state
            .subscriptions
            .read()
            .expect("subscription lock should not be poisoned")
            .values()
            .filter(|subscription| {
                commit_intersects_dependency_set(
                    commit,
                    &subscription.dependencies,
                    candidate_documents,
                    |_, _| Ok(None),
                )
            })
            .map(|subscription| SubscriptionDelivery {
                id: subscription.id,
                query: subscription.query.clone(),
                principal: subscription.principal.clone(),
                sender: subscription.sender.clone(),
            })
            .collect()
    }

    /// Sends a terminal error to subscriptions on the provided table that were
    /// registered under an outdated access-policy revision, then removes them.
    pub fn terminate_policy_revision_mismatches(
        &self,
        table: &TableName,
        current_policy_revision: &str,
        message: impl Into<String>,
    ) {
        let message = message.into();
        let mut removed = Vec::new();
        {
            let mut subscriptions = self
                .state
                .subscriptions
                .write()
                .expect("subscription lock should not be poisoned");
            subscriptions.retain(|_, subscription| {
                let is_stale = &subscription.query.table == table
                    && subscription.policy_revision != current_policy_revision;
                if is_stale {
                    removed.push((
                        subscription.id,
                        subscription.principal_snapshot.clone(),
                        subscription.sender.clone(),
                    ));
                }
                !is_stale
            });
        }

        for (subscription_id, _principal_snapshot, sender) in removed {
            let _ = sender.send(SubscriptionUpdate::Error {
                subscription_id,
                request_id: None,
                message: message.clone(),
            });
        }
    }

    /// Sends a terminal error to all subscriptions and removes them.
    pub fn shutdown_all(&self, message: impl Into<String>) {
        let message = message.into();
        let subscriptions = std::mem::take(
            &mut *self
                .state
                .subscriptions
                .write()
                .expect("subscription lock should not be poisoned"),
        );

        for subscription in subscriptions.into_values() {
            let _ = subscription.sender.send(SubscriptionUpdate::Error {
                subscription_id: subscription.id,
                request_id: None,
                message: message.clone(),
            });
        }
    }
}

impl Default for SubscriptionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use neovex_core::TableName;

    use super::*;

    #[test]
    fn dropping_registration_unregisters_subscription() {
        let registry = SubscriptionRegistry::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let registration = registry.register(
            Query {
                table: TableName::new("tasks").expect("table name should be valid"),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            PrincipalContext::anonymous(),
            PrincipalContext::anonymous()
                .snapshot()
                .expect("anonymous principal should snapshot"),
            "policy-v1".to_string(),
            tx,
        );

        assert_eq!(registration.id(), 1);
        assert_eq!(registry.len(), 1);

        drop(registration);

        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn unfiltered_registrations_store_coarse_table_dependencies() {
        let registry = SubscriptionRegistry::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let query = Query {
            table: TableName::new("tasks").expect("table name should be valid"),
            filters: Vec::new(),
            order: None,
            limit: None,
        };

        let registration = registry.register(
            query.clone(),
            PrincipalContext::anonymous(),
            PrincipalContext::anonymous()
                .snapshot()
                .expect("anonymous principal should snapshot"),
            "policy-v1".to_string(),
            tx,
        );
        let stored = registry
            .state
            .subscriptions
            .read()
            .expect("subscription lock should not be poisoned")
            .get(&registration.id())
            .expect("subscription should be stored")
            .dependencies
            .clone();

        assert!(stored.tables.contains(&query.table));
        assert!(stored.predicates.is_empty());
    }
}
