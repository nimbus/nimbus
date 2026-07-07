use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(test)]
use std::time::Duration;

use nimbus_core::{DependencySet, PrincipalContext, Query, SequenceNumber};
use tokio::sync::mpsc;

use super::delivery::{SubscriptionDelivery, SubscriptionUpdate};
#[cfg(test)]
use crate::tenant::pause_barrier::{PauseBarrier, PauseBarrierHandle};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SubscriptionPublishResult {
    Delivered,
    Stale,
    Missing,
}

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
    #[cfg(test)]
    delivery_publish_pause: Arc<SubscriptionDeliveryPublishPauseState>,
}

#[cfg(test)]
pub(super) type SubscriptionDeliveryPublishPauseState = PauseBarrier<SequenceNumber>;

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct SubscriptionDeliveryPublishPauseHandle {
    inner: PauseBarrierHandle<SequenceNumber>,
}

#[cfg(test)]
impl SubscriptionDeliveryPublishPauseHandle {
    fn new(state: Arc<SubscriptionDeliveryPublishPauseState>) -> Self {
        Self {
            inner: PauseBarrierHandle::new(state),
        }
    }

    pub(crate) fn arm_next_publish(&self) {
        self.inner.arm();
    }

    pub(crate) fn wait_until_entered(&self, timeout: Duration) -> Option<SequenceNumber> {
        self.inner.wait_until_entered(timeout)
    }

    pub(crate) fn release(&self) {
        self.inner.release();
    }
}

impl SubscriptionRegistryState {
    fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            subscriptions: RwLock::new(HashMap::new()),
            #[cfg(test)]
            delivery_publish_pause: Arc::new(SubscriptionDeliveryPublishPauseState::default()),
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
            dependencies: DependencySet::from_engine_query(&query, None),
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
            last_delivered_sequence: subscription.last_delivered_sequence,
        })
    }

    #[cfg(test)]
    pub(crate) fn delivery_publish_pause_handle(&self) -> SubscriptionDeliveryPublishPauseHandle {
        SubscriptionDeliveryPublishPauseHandle::new(self.state.delivery_publish_pause.clone())
    }

    #[cfg(test)]
    pub(super) fn wait_before_delivery_publish_for_testing(&self, sequence: SequenceNumber) {
        self.state.delivery_publish_pause.wait_if_armed(sequence);
    }

    pub(super) fn publish_delivery_update(
        &self,
        subscription_id: u64,
        delivered_sequence: SequenceNumber,
        update: SubscriptionUpdate,
        dependencies: Option<DependencySet>,
    ) -> SubscriptionPublishResult {
        let mut subscriptions = self
            .state
            .subscriptions
            .write()
            .expect("subscription lock should not be poisoned");

        let Some(subscription) = subscriptions.get_mut(&subscription_id) else {
            return SubscriptionPublishResult::Missing;
        };
        if !subscription.active {
            return SubscriptionPublishResult::Missing;
        }
        if subscription.last_delivered_sequence.load(Ordering::Acquire) >= delivered_sequence.0 {
            return SubscriptionPublishResult::Stale;
        }

        if subscription.sender.try_send(update).is_ok() {
            if let Some(dependencies) = dependencies {
                subscription.dependencies = dependencies;
            }
            subscription
                .last_delivered_sequence
                .store(delivered_sequence.0, Ordering::Release);
            SubscriptionPublishResult::Delivered
        } else {
            subscriptions.remove(&subscription_id);
            SubscriptionPublishResult::Missing
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
    use nimbus_core::{PrincipalContext, Query, SequenceNumber, TableName};
    use tokio::sync::mpsc;

    use super::{SubscriptionPublishResult, SubscriptionRegistry};
    use crate::subscriptions::DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY;
    use crate::subscriptions::SubscriptionUpdate;

    fn query(table: &str) -> Query {
        Query {
            table: TableName::new(table).expect("table name should be valid"),
            filters: Vec::new(),
            order: None,
            limit: None,
        }
    }

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

    #[test]
    fn publishing_rechecks_staleness_before_sending_update() {
        let registry = SubscriptionRegistry::new();
        let (tx, mut rx) = mpsc::channel(DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY);
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

        assert_eq!(
            registry.publish_delivery_update(
                registration.id(),
                SequenceNumber(9),
                SubscriptionUpdate::Error {
                    subscription_id: registration.id(),
                    request_id: None,
                    message: "newer".to_string(),
                },
                None,
            ),
            SubscriptionPublishResult::Delivered
        );
        assert_eq!(
            registry.publish_delivery_update(
                registration.id(),
                SequenceNumber(8),
                SubscriptionUpdate::Error {
                    subscription_id: registration.id(),
                    request_id: None,
                    message: "older".to_string(),
                },
                None,
            ),
            SubscriptionPublishResult::Stale
        );

        let delivered = rx.try_recv().expect("newer update should be delivered");
        assert!(matches!(
            delivered,
            SubscriptionUpdate::Error { message, .. } if message == "newer"
        ));
        assert!(
            rx.try_recv().is_err(),
            "stale older update must not reach the receiver"
        );
    }

    #[test]
    fn pending_policy_revision_mismatch_marks_and_restores_subscription() {
        let registry = SubscriptionRegistry::new();
        let table = TableName::new("tasks").expect("table name should be valid");
        let (tx, _rx) = mpsc::channel(DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY);
        let registration = registry.register(
            query("tasks"),
            PrincipalContext::anonymous(),
            "policy-v1".to_string(),
            tx,
            true,
        );

        assert_eq!(
            registry.publish_delivery_update(
                registration.id(),
                SequenceNumber(5),
                SubscriptionUpdate::Error {
                    subscription_id: registration.id(),
                    request_id: None,
                    message: "delivered".to_string(),
                },
                None,
            ),
            SubscriptionPublishResult::Delivered
        );

        let pending = registry.begin_policy_revision_mismatches(&table, "policy-v2");
        assert!(!pending.is_empty());
        assert!(
            registry.delivery(registration.id()).is_none(),
            "pre-marked subscriptions should not be visible for delivery"
        );
        assert_eq!(
            registry.publish_delivery_update(
                registration.id(),
                SequenceNumber(6),
                SubscriptionUpdate::Error {
                    subscription_id: registration.id(),
                    request_id: None,
                    message: "stale".to_string(),
                },
                None,
            ),
            SubscriptionPublishResult::Missing
        );

        registry.restore_policy_revision_mismatches(pending);

        let delivery = registry
            .delivery(registration.id())
            .expect("restored subscription should be active");
        assert!(delivery.is_stale_for_sequence(SequenceNumber(5)));
        assert!(!delivery.is_stale_for_sequence(SequenceNumber(6)));
    }

    #[test]
    fn finishing_policy_revision_mismatch_removes_only_marked_subscriptions() {
        let registry = SubscriptionRegistry::new();
        let table = TableName::new("tasks").expect("table name should be valid");
        let (stale_tx, mut stale_rx) = mpsc::channel(DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY);
        let (current_tx, mut current_rx) = mpsc::channel(DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY);
        let (other_tx, mut other_rx) = mpsc::channel(DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY);

        let stale = registry.register(
            query("tasks"),
            PrincipalContext::anonymous(),
            "policy-v1".to_string(),
            stale_tx,
            true,
        );
        let current = registry.register(
            query("tasks"),
            PrincipalContext::anonymous(),
            "policy-v2".to_string(),
            current_tx,
            true,
        );
        let other_table = registry.register(
            query("notes"),
            PrincipalContext::anonymous(),
            "policy-v1".to_string(),
            other_tx,
            true,
        );

        let pending = registry.begin_policy_revision_mismatches(&table, "policy-v2");
        registry.finish_policy_revision_mismatches(pending, "authorization policy changed");

        assert!(registry.delivery(stale.id()).is_none());
        assert!(registry.delivery(current.id()).is_some());
        assert!(registry.delivery(other_table.id()).is_some());
        assert_eq!(registry.len(), 2);

        let update = stale_rx
            .try_recv()
            .expect("stale subscription should receive terminal error");
        assert!(matches!(
            update,
            SubscriptionUpdate::Error { message, .. } if message == "authorization policy changed"
        ));
        assert!(current_rx.try_recv().is_err());
        assert!(other_rx.try_recv().is_err());
    }
}
