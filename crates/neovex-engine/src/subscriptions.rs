use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use neovex_core::{
    CommitEntry, DependencySet, Document, PaginatedWindowDependency, PrincipalContext,
    PrincipalSnapshot, Query, SequenceNumber, TableName, commit_intersects_dependency_set,
};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::service::evaluate_with_index_cancellable_for_principal;
use crate::tenant::TenantRuntime;

pub const DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY: usize = 256;

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
    active: bool,
    query: Query,
    dependencies: DependencySet,
    principal: PrincipalContext,
    principal_snapshot: PrincipalSnapshot,
    policy_revision: String,
    sender: mpsc::Sender<SubscriptionUpdate>,
    last_delivered_sequence: Arc<AtomicU64>,
}

#[derive(Debug, Clone)]
pub struct SubscriptionDelivery {
    pub id: u64,
    pub query: Query,
    pub principal: PrincipalContext,
    pub sender: mpsc::Sender<SubscriptionUpdate>,
    last_delivered_sequence: Arc<AtomicU64>,
}

impl SubscriptionDelivery {
    fn is_stale_for_sequence(&self, sequence: SequenceNumber) -> bool {
        self.last_delivered_sequence.load(Ordering::Acquire) >= sequence.0
    }

    fn mark_delivered(&self, sequence: SequenceNumber) {
        let mut current = self.last_delivered_sequence.load(Ordering::Acquire);
        while current < sequence.0 {
            match self.last_delivered_sequence.compare_exchange_weak(
                current,
                sequence.0,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(observed) => current = observed,
            }
        }
    }
}

pub(crate) fn subscription_dependencies(query: &Query, documents: &[Document]) -> DependencySet {
    let Some(page_size) = query.limit else {
        return DependencySet::from_engine_query(query);
    };

    let (end_sort_values, end_doc_id) = documents
        .last()
        .map(|document| {
            (
                query.order.as_ref().map_or_else(Vec::new, |order| {
                    vec![document.get_field(&order.field).cloned()]
                }),
                Some(document.id),
            )
        })
        .unwrap_or_else(|| (Vec::new(), None));

    let mut dependencies = DependencySet::default();
    dependencies.record_paginated_window(PaginatedWindowDependency {
        table: query.table.clone(),
        filters: query.filters.clone(),
        order: query.order.clone(),
        start_sort_values: Vec::new(),
        start_doc_id: None,
        end_sort_values,
        end_doc_id,
        result_count: documents.len(),
        page_size,
    });
    dependencies
}

#[derive(Debug, Clone)]
pub(crate) struct QueuedSubscriptionWork {
    pub subscription_ids: Vec<u64>,
    pub delivery_sequence: SequenceNumber,
    pub commit: Option<CommitEntry>,
    pub deleted_documents: Vec<Document>,
    pub enqueued_at: Instant,
}

impl QueuedSubscriptionWork {
    pub(crate) fn new_single(
        subscription_ids: Vec<u64>,
        commit: CommitEntry,
        deleted_documents: Vec<Document>,
    ) -> Self {
        Self {
            subscription_ids,
            delivery_sequence: commit.sequence,
            commit: Some(commit),
            deleted_documents,
            enqueued_at: Instant::now(),
        }
    }

    pub(crate) fn new_coalesced(
        subscription_ids: Vec<u64>,
        delivery_sequence: SequenceNumber,
        commit: Option<CommitEntry>,
        deleted_documents: Vec<Document>,
    ) -> Self {
        Self {
            subscription_ids,
            delivery_sequence,
            commit,
            deleted_documents,
            enqueued_at: Instant::now(),
        }
    }
}

pub(crate) fn merge_queued_subscription_work(
    batch: Vec<QueuedSubscriptionWork>,
) -> (QueuedSubscriptionWork, u64) {
    let mut batch_iter = batch.into_iter();
    let first = batch_iter
        .next()
        .expect("queued subscription merge requires at least one work item");
    let mut merged_subscription_ids = first
        .subscription_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut delivery_sequence = first.delivery_sequence;
    let mut deleted_documents = first
        .deleted_documents
        .into_iter()
        .map(|document| (document.id, document))
        .collect::<BTreeMap<_, _>>();
    let mut earliest_enqueued_at = first.enqueued_at;
    let mut merged_count = 0_u64;

    for work in batch_iter {
        merged_count = merged_count.saturating_add(1);
        delivery_sequence = delivery_sequence.max(work.delivery_sequence);
        earliest_enqueued_at = earliest_enqueued_at.min(work.enqueued_at);
        merged_subscription_ids.extend(work.subscription_ids);
        for document in work.deleted_documents {
            deleted_documents.insert(document.id, document);
        }
    }

    let commit = (merged_count == 0).then_some(first.commit).flatten();
    (
        QueuedSubscriptionWork {
            subscription_ids: merged_subscription_ids.into_iter().collect(),
            delivery_sequence,
            commit,
            deleted_documents: deleted_documents.into_values().collect(),
            enqueued_at: earliest_enqueued_at,
        },
        merged_count,
    )
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SubscriptionDispatchStats {
    pub reevaluation_count: u64,
    pub total_reevaluation_nanos: u64,
    pub coalesced_work_count: u64,
}

pub(crate) struct SubscriptionBatchCandidate<'a> {
    pub commit: &'a CommitEntry,
    pub candidate_documents: &'a [Document],
}

pub(crate) struct BatchAffectedSubscriptions {
    pub subscription_ids: Vec<u64>,
    pub merged_wakeup_count: u64,
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
        sender: mpsc::Sender<SubscriptionUpdate>,
        active: bool,
    ) -> SubscriptionRegistration {
        let id = self.state.next_id.fetch_add(1, Ordering::SeqCst);
        let subscription = Subscription {
            id,
            active,
            dependencies: DependencySet::from_engine_query(&query),
            principal,
            principal_snapshot,
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
        if let Some(subscription) = self
            .state
            .subscriptions
            .write()
            .expect("subscription lock should not be poisoned")
            .get_mut(&id)
        {
            subscription.active = true;
            subscription
                .last_delivered_sequence
                .store(delivered_sequence.0, Ordering::Release);
        }
    }

    pub fn activate_with_dependencies(
        &self,
        id: u64,
        delivered_sequence: SequenceNumber,
        dependencies: DependencySet,
    ) {
        if let Some(subscription) = self
            .state
            .subscriptions
            .write()
            .expect("subscription lock should not be poisoned")
            .get_mut(&id)
        {
            subscription.active = true;
            subscription.dependencies = dependencies;
            subscription
                .last_delivered_sequence
                .store(delivered_sequence.0, Ordering::Release);
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.state.len()
    }

    /// Returns the ids of subscriptions affected by the provided commit.
    pub fn affected_subscription_ids(
        &self,
        commit: &CommitEntry,
        candidate_documents: &[Document],
    ) -> Vec<u64> {
        self.state
            .subscriptions
            .read()
            .expect("subscription lock should not be poisoned")
            .values()
            .filter(|subscription| subscription.active)
            .filter(|subscription| {
                commit_intersects_dependency_set(
                    commit,
                    &subscription.dependencies,
                    candidate_documents,
                    |_, _| Ok(None),
                )
            })
            .map(|subscription| subscription.id)
            .collect()
    }

    pub(crate) fn affected_subscription_ids_for_batch(
        &self,
        batch: &[SubscriptionBatchCandidate<'_>],
    ) -> BatchAffectedSubscriptions {
        let mut subscription_ids = Vec::new();
        let mut merged_wakeup_count = 0_u64;
        for subscription in self
            .state
            .subscriptions
            .read()
            .expect("subscription lock should not be poisoned")
            .values()
            .filter(|subscription| subscription.active)
        {
            let mut match_count = 0_u64;
            for candidate in batch {
                if commit_intersects_dependency_set(
                    candidate.commit,
                    &subscription.dependencies,
                    candidate.candidate_documents,
                    |_, _| Ok(None),
                ) {
                    match_count += 1;
                }
            }
            if match_count != 0 {
                subscription_ids.push(subscription.id);
                merged_wakeup_count += match_count.saturating_sub(1);
            }
        }
        BatchAffectedSubscriptions {
            subscription_ids,
            merged_wakeup_count,
        }
    }

    pub(crate) fn delivery(&self, subscription_id: u64) -> Option<SubscriptionDelivery> {
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
        if let Some(subscription) = self
            .state
            .subscriptions
            .write()
            .expect("subscription lock should not be poisoned")
            .get_mut(&subscription_id)
        {
            subscription.dependencies = dependencies;
            subscription
                .last_delivered_sequence
                .store(delivered_sequence.0, Ordering::Release);
        }
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
            let _ = sender.try_send(SubscriptionUpdate::Error {
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
            let _ = subscription.sender.try_send(SubscriptionUpdate::Error {
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

pub(crate) fn dispatch_subscription_work(
    runtime: &Arc<TenantRuntime>,
    work: &QueuedSubscriptionWork,
) -> SubscriptionDispatchStats {
    let sequence = work.delivery_sequence;
    let mut stats = SubscriptionDispatchStats::default();

    for subscription_id in &work.subscription_ids {
        let Some(subscription) = runtime.subscriptions.delivery(*subscription_id) else {
            continue;
        };
        if subscription.is_stale_for_sequence(sequence) {
            stats.coalesced_work_count += 1;
            continue;
        }

        let started_at = Instant::now();
        let mut check_cancel = || -> neovex_core::Result<()> { Ok(()) };
        let result = evaluate_with_index_cancellable_for_principal(
            runtime,
            &subscription.query,
            &subscription.principal,
            sequence,
            &mut check_cancel,
        );
        stats.reevaluation_count += 1;
        let elapsed_nanos = started_at.elapsed().as_nanos();
        stats.total_reevaluation_nanos += elapsed_nanos.min(u128::from(u64::MAX)) as u64;

        if subscription.is_stale_for_sequence(sequence) {
            stats.coalesced_work_count += 1;
            continue;
        }

        match result {
            Ok(documents) => {
                let dependencies = subscription_dependencies(&subscription.query, &documents);
                let update = SubscriptionUpdate::Result {
                    subscription_id: subscription.id,
                    request_id: None,
                    commit: work.commit.clone(),
                    deleted_documents: work.deleted_documents.clone(),
                    data: documents.into_iter().map(Document::into_json).collect(),
                };
                if subscription.sender.try_send(update).is_ok() {
                    runtime
                        .subscriptions
                        .record_delivery(subscription.id, sequence, dependencies);
                } else {
                    runtime.subscriptions.remove(subscription.id);
                }
            }
            Err(error) => {
                tracing::warn!(
                    subscription_id = subscription.id,
                    error = %error,
                    "subscription re-evaluation failed"
                );
                if subscription
                    .sender
                    .try_send(SubscriptionUpdate::Error {
                        subscription_id: subscription.id,
                        request_id: None,
                        message: error.to_string(),
                    })
                    .is_ok()
                {
                    subscription.mark_delivered(sequence);
                } else {
                    runtime.subscriptions.remove(subscription.id);
                }
            }
        }
    }

    stats
}

#[cfg(test)]
mod tests {
    use neovex_core::TableName;

    use super::*;

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
            PrincipalContext::anonymous()
                .snapshot()
                .expect("anonymous principal should snapshot"),
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
    fn unfiltered_registrations_store_coarse_table_dependencies() {
        let registry = SubscriptionRegistry::new();
        let (tx, _rx) = mpsc::channel(DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY);
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
            true,
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
            PrincipalContext::anonymous()
                .snapshot()
                .expect("anonymous principal should snapshot"),
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
