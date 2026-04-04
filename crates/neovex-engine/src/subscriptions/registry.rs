use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

use neovex_core::{DependencySet, PrincipalContext, Query, SequenceNumber};
use tokio::sync::mpsc;

use super::delivery::{SubscriptionDelivery, SubscriptionUpdate};

#[derive(Debug, Clone)]
pub(super) struct Subscription {
    pub(super) id: u64,
    pub(super) active: bool,
    pub(super) query: Query,
    pub(super) dependencies: DependencySet,
    pub(super) principal: PrincipalContext,
    pub(super) policy_revision: String,
    pub(super) sender: mpsc::Sender<SubscriptionUpdate>,
    pub(super) last_delivered_sequence: Arc<AtomicU64>,
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
pub(super) struct SubscriptionRegistryState {
    pub(super) next_id: AtomicU64,
    pub(super) subscriptions: RwLock<HashMap<u64, Subscription>>,
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
    pub(super) state: Arc<SubscriptionRegistryState>,
}

impl SubscriptionRegistry {
    fn update_subscription(&self, id: u64, update: impl FnOnce(&mut Subscription)) {
        if let Some(subscription) = self
            .state
            .subscriptions
            .write()
            .expect("subscription lock should not be poisoned")
            .get_mut(&id)
        {
            update(subscription);
        }
    }

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
        policy_revision: String,
        sender: mpsc::Sender<SubscriptionUpdate>,
        active: bool,
    ) -> SubscriptionRegistration {
        let id = self.state.next_id.fetch_add(1, Ordering::SeqCst);
        let subscription = Subscription {
            id,
            active,
            dependencies: DependencySet::from_engine_query(&query),
            principal,
            policy_revision,
            query,
            sender,
            last_delivered_sequence: Arc::new(AtomicU64::new(0)),
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

    #[cfg(test)]
    pub fn activate(&self, id: u64, delivered_sequence: SequenceNumber) {
        self.update_subscription(id, |subscription| {
            subscription.active = true;
            subscription
                .last_delivered_sequence
                .store(delivered_sequence.0, Ordering::Release);
        });
    }

    pub fn activate_with_dependencies(
        &self,
        id: u64,
        delivered_sequence: SequenceNumber,
        dependencies: DependencySet,
    ) {
        self.update_subscription(id, |subscription| {
            subscription.active = true;
            subscription.dependencies = dependencies;
            subscription
                .last_delivered_sequence
                .store(delivered_sequence.0, Ordering::Release);
        });
    }

    pub(crate) fn len(&self) -> usize {
        self.state.len()
    }

    pub(super) fn delivery(&self, subscription_id: u64) -> Option<SubscriptionDelivery> {
        let subscription = self
            .state
            .subscriptions
            .read()
            .expect("subscription lock should not be poisoned")
            .get(&subscription_id)
            .cloned()?;
        subscription.active.then(|| SubscriptionDelivery {
            id: subscription.id,
            query: subscription.query,
            principal: subscription.principal,
            sender: subscription.sender,
            last_delivered_sequence: subscription.last_delivered_sequence,
        })
    }

    pub(crate) fn record_delivery(
        &self,
        subscription_id: u64,
        delivered_sequence: SequenceNumber,
        dependencies: DependencySet,
    ) {
        self.update_subscription(subscription_id, |subscription| {
            subscription.dependencies = dependencies;
            subscription
                .last_delivered_sequence
                .store(delivered_sequence.0, Ordering::Release);
        });
    }
}

impl Default for SubscriptionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use neovex_core::{PrincipalContext, Query, SequenceNumber, TableName};
    use tokio::sync::mpsc;

    use super::SubscriptionRegistry;
    use crate::subscriptions::DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY;

    #[test]
    fn dropping_registration_unregisters_subscription() {
        let registry = SubscriptionRegistry::new();
        let (tx, _rx) = mpsc::channel(DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY);
        let registration = registry.register(
            Query {
                table: TableName::new("tasks").expect("table name should be valid"),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            PrincipalContext::anonymous(),
            "policy-v1".to_string(),
            tx,
            true,
        );

        assert_eq!(registration.id(), 1);
        assert_eq!(registry.len(), 1);

        drop(registration);

        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn activation_marks_bootstrap_sequence_as_already_delivered() {
        let registry = SubscriptionRegistry::new();
        let (tx, _rx) = mpsc::channel(DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY);
        let registration = registry.register(
            Query {
                table: TableName::new("tasks").expect("table name should be valid"),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            PrincipalContext::anonymous(),
            "policy-v1".to_string(),
            tx,
            false,
        );

        registry.activate(registration.id(), SequenceNumber(7));

        let delivery = registry
            .delivery(registration.id())
            .expect("activated subscription should be available for delivery");
        assert!(delivery.is_stale_for_sequence(SequenceNumber(7)));
        assert!(!delivery.is_stale_for_sequence(SequenceNumber(8)));
    }
}
