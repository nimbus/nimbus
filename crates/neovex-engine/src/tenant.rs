use std::future::Future;
use std::sync::{Arc, RwLock};
#[cfg(test)]
use std::time::Duration;
use std::time::Instant;

use neovex_core::{
    CommitEntry, Document, DocumentId, Result, Schema, SequenceNumber, TableName, TenantId,
};
use neovex_storage::{JournalProgress, RedbTenantStorage, TenantStore};
use serde::Serialize;

use crate::subscriptions::{
    QueuedSubscriptionWork, SubscriptionDispatchStats, SubscriptionRegistry,
};

mod document_cache;
mod lifecycle;
mod materialized_reads;
mod mutation;
mod query_planning;
mod subscription_delivery;

#[cfg(test)]
pub(crate) use self::document_cache::DOCUMENT_CACHE_CAPACITY;
#[cfg(test)]
pub(crate) use self::document_cache::DocumentCacheStats;
use self::document_cache::TenantDocumentCache;
use self::lifecycle::TenantLifecycle;
#[cfg(test)]
pub(crate) use self::materialized_reads::MaterializedReadPublishPauseHandle;
#[cfg(test)]
pub(crate) use self::materialized_reads::MaterializedTablePublicationStats;
pub(crate) use self::materialized_reads::ServingSnapshot;
use self::materialized_reads::TenantMaterializedReadSurface;
pub use self::materialized_reads::{MaterializedReadSurfaceStats, ServingSnapshotManagerStats};
#[cfg(test)]
pub(crate) use self::mutation::DEFAULT_MUTATION_ADMISSION_QUEUE_CAPACITY;
#[cfg(test)]
pub(crate) use self::mutation::DEFAULT_MUTATION_JOURNAL_QUEUE_CAPACITY;
#[cfg(any(test, feature = "test-hooks"))]
pub(crate) use self::mutation::MutationJournalPauseHandle;
#[cfg(any(test, feature = "test-hooks"))]
use self::mutation::MutationJournalPauseState;
use self::mutation::{MutationAdmissionDecision, MutationAdmissionGate, MutationJournalState};
pub use self::mutation::{MutationAdmissionPhase, MutationAdmissionStats, MutationJournalStats};
pub(crate) use self::mutation::{QueuedMutationRequest, QueuedMutationResult};
use self::query_planning::QueryPlanningMetrics;
pub use self::query_planning::QueryPlanningStats;
pub(crate) use self::query_planning::{QueryPlanMetricKind, QueryPlanMetricOperation};
#[cfg(test)]
pub(crate) use self::subscription_delivery::DEFAULT_SUBSCRIPTION_WORK_QUEUE_CAPACITY;
#[cfg(test)]
pub(crate) use self::subscription_delivery::SubscriptionDeliveryPauseHandle;
use self::subscription_delivery::SubscriptionDeliveryQueue;
pub use self::subscription_delivery::SubscriptionDeliveryStats;

/// Runtime state for a loaded tenant.
pub struct TenantRuntime {
    pub store: Arc<TenantStore>,
    pub read_storage: Arc<RedbTenantStorage>,
    pub subscriptions: SubscriptionRegistry,
    pub schema: RwLock<Arc<Schema>>,
    document_cache: TenantDocumentCache,
    materialized_reads: TenantMaterializedReadSurface,
    query_planning: QueryPlanningMetrics,
    subscription_delivery: SubscriptionDeliveryQueue,
    lifecycle: Arc<TenantLifecycle>,
    mutation_admission: Arc<MutationAdmissionGate>,
    mutation_journal: Arc<MutationJournalState>,
    #[cfg(any(test, feature = "test-hooks"))]
    subscription_bootstrap_pause: Arc<MutationJournalPauseState>,
}

pub struct TenantOperationGuard {
    lifecycle: Arc<TenantLifecycle>,
}

pub struct TenantDeletionGuard;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TenantEngineDiagnosticsSnapshot {
    pub mutation_admission: MutationAdmissionStats,
    pub mutation_journal: MutationJournalStats,
    pub subscription_delivery: SubscriptionDeliveryStats,
    pub materialized_read_surface: MaterializedReadSurfaceStats,
    pub serving_snapshot_manager: ServingSnapshotManagerStats,
    pub query_planning: QueryPlanningStats,
}

impl Drop for TenantOperationGuard {
    fn drop(&mut self) {
        self.lifecycle.release_operation();
    }
}

impl TenantRuntime {
    /// Creates a tenant runtime from a store.
    pub fn from_parts(
        store: Arc<TenantStore>,
        read_storage: Arc<RedbTenantStorage>,
    ) -> Result<Self> {
        let schema = store.load_schema()?;
        let progress = store.journal_progress()?;
        Ok(Self {
            store,
            read_storage,
            subscriptions: SubscriptionRegistry::new(),
            schema: RwLock::new(Arc::new(schema)),
            document_cache: TenantDocumentCache::new(),
            materialized_reads: TenantMaterializedReadSurface::new(),
            query_planning: QueryPlanningMetrics::new(),
            subscription_delivery: SubscriptionDeliveryQueue::new(),
            lifecycle: Arc::new(TenantLifecycle::new()),
            mutation_admission: Arc::new(MutationAdmissionGate::new()),
            mutation_journal: Arc::new(MutationJournalState::new(progress)),
            #[cfg(any(test, feature = "test-hooks"))]
            subscription_bootstrap_pause: Arc::new(MutationJournalPauseState::default()),
        })
    }

    /// Returns the current schema snapshot.
    pub fn schema(&self) -> Arc<Schema> {
        self.schema
            .read()
            .expect("schema lock should not be poisoned")
            .clone()
    }

    /// Enters a tenant operation, preventing deletion while the operation is active.
    pub fn enter_operation(&self, tenant_id: &TenantId) -> Result<TenantOperationGuard> {
        self.lifecycle.enter_operation(tenant_id)?;
        Ok(TenantOperationGuard {
            lifecycle: self.lifecycle.clone(),
        })
    }

    /// Begins tenant deletion and blocks until all in-flight operations complete.
    pub fn begin_delete(&self) -> TenantDeletionGuard {
        self.lifecycle.begin_delete_blocking();
        TenantDeletionGuard
    }

    /// Begins tenant deletion asynchronously and waits until all in-flight operations complete.
    pub async fn begin_delete_async(&self) -> TenantDeletionGuard {
        self.lifecycle.begin_delete_async().await;
        TenantDeletionGuard
    }

    pub(crate) fn get_cached_document(
        &self,
        table: &TableName,
        document_id: DocumentId,
    ) -> Option<Document> {
        self.document_cache.get(table, document_id)
    }

    pub(crate) fn cache_document(&self, document: &Document) {
        self.document_cache.insert(document);
    }

    pub(crate) fn cache_documents<'a>(&self, documents: impl IntoIterator<Item = &'a Document>) {
        self.document_cache.insert_documents(documents);
    }

    #[cfg(test)]
    pub(crate) fn materialized_serving_snapshot(
        &self,
        required_sequence: SequenceNumber,
    ) -> Option<ServingSnapshot> {
        self.materialized_reads
            .serving_snapshot_covering(required_sequence)
    }

    pub(crate) fn materialized_serving_snapshot_for_table(
        &self,
        table: &TableName,
        required_sequence: SequenceNumber,
    ) -> Option<ServingSnapshot> {
        self.materialized_reads
            .serving_snapshot_for_table_with_mode(table, required_sequence, true)
    }

    pub(crate) fn load_materialized_serving_snapshot_cancellable(
        &self,
        store: &TenantStore,
        table: &TableName,
        required_sequence: SequenceNumber,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<ServingSnapshot> {
        self.materialized_reads.load_serving_snapshot_cancellable(
            store,
            table,
            required_sequence,
            check_cancel,
        )
    }

    #[cfg(test)]
    pub(crate) async fn wait_for_materialized_serving_snapshot_cancellable<Fut>(
        &self,
        required_sequence: SequenceNumber,
        cancel_wait: Fut,
    ) -> Result<ServingSnapshot>
    where
        Fut: Future<Output = ()>,
    {
        self.materialized_reads
            .wait_for_snapshot_covering_cancellable(required_sequence, cancel_wait)
            .await
    }

    pub(crate) fn record_materialized_query_evaluation(&self) {
        self.materialized_reads.record_evaluation();
    }

    pub(crate) fn record_materialized_paginated_evaluation(&self) {
        self.materialized_reads.record_paginated();
    }

    pub(crate) fn record_materialized_get_hit(&self) {
        self.materialized_reads.record_get_hit();
    }

    pub(crate) fn invalidate_document_cache_for_commit(&self, commit: &CommitEntry) {
        self.document_cache.invalidate_commit(commit);
        self.materialized_reads.apply_commit(commit);
    }

    pub(crate) fn invalidate_document_cache_for_commits<'a>(
        &self,
        commits: impl IntoIterator<Item = &'a CommitEntry>,
    ) {
        let commits = commits.into_iter().collect::<Vec<_>>();
        self.document_cache
            .invalidate_commits(commits.iter().copied());
        self.materialized_reads.apply_commits(commits);
    }

    pub(crate) fn clear_document_cache(&self) {
        self.document_cache.clear();
        self.materialized_reads.clear();
    }

    pub(crate) fn record_query_plan_metric(
        &self,
        operation: QueryPlanMetricOperation,
        kind: QueryPlanMetricKind,
    ) {
        self.query_planning.record(operation, kind);
    }

    pub(crate) fn ensure_subscription_delivery_worker_started(self: &Arc<Self>) {
        self.subscription_delivery.start_worker(self);
    }

    pub(crate) fn enqueue_subscription_work(
        &self,
        work: QueuedSubscriptionWork,
    ) -> std::result::Result<(), QueuedSubscriptionWork> {
        self.subscription_delivery.enqueue(work)
    }

    pub(crate) fn record_subscription_dispatch_stats(&self, stats: SubscriptionDispatchStats) {
        self.subscription_delivery.record_dispatch_stats(stats);
    }

    pub(crate) fn record_subscription_overflow_sync_fallback(&self) {
        self.subscription_delivery.record_overflow_sync_fallback();
    }

    pub(crate) fn record_subscription_coalesced_batch(
        &self,
        commit_count: u64,
        merged_subscription_wakeup_count: u64,
    ) {
        self.subscription_delivery
            .record_coalesced_batch(commit_count, merged_subscription_wakeup_count);
    }

    pub(crate) fn shutdown_subscription_delivery(&self) {
        self.subscription_delivery.shutdown();
    }

    pub(crate) fn enqueue_mutation_admission_request(
        &self,
        request: QueuedMutationRequest,
    ) -> Result<bool> {
        self.mutation_admission.enqueue(request)?;
        Ok(self.mutation_journal.try_start_worker())
    }

    pub(crate) fn drain_mutation_admission_queue(&self) {
        loop {
            match self.mutation_admission.pop_next_at(Instant::now()) {
                MutationAdmissionDecision::Admit(request) => {
                    if let Err(enqueue_error) = self.mutation_journal.enqueue(request) {
                        let (request, error) = *enqueue_error;
                        let _ = request.response.send(Err(error));
                    }
                }
                MutationAdmissionDecision::Reject { request, error } => {
                    let _ = request.response.send(Err(error));
                }
                MutationAdmissionDecision::Empty => break,
            }
        }
    }

    pub(crate) async fn drain_mutation_batch(
        &self,
        max_batch_size: usize,
    ) -> Vec<QueuedMutationRequest> {
        #[cfg(test)]
        self.mutation_journal.wait_before_drain().await;
        let mut batch = self.mutation_journal.drain_batch(max_batch_size).await;
        let batch_limit = max_batch_size.max(1);
        while batch.len() < batch_limit {
            match self.mutation_admission.pop_next_at(Instant::now()) {
                MutationAdmissionDecision::Admit(request) => batch.push(request),
                MutationAdmissionDecision::Reject { request, error } => {
                    let _ = request.response.send(Err(error));
                }
                MutationAdmissionDecision::Empty => break,
            }
        }
        batch
    }

    pub(crate) fn release_mutation_worker(&self) -> bool {
        self.mutation_journal
            .release_worker(self.mutation_admission.has_pending())
    }

    pub(crate) fn record_mutation_worker_start(&self) {
        self.mutation_journal.record_worker_start();
    }

    pub(crate) fn record_mutation_worker_failure(&self) {
        self.mutation_journal.record_worker_failure();
    }

    pub(crate) fn begin_pending_mutation_response(&self) {
        self.mutation_journal.begin_pending_response();
    }

    pub(crate) fn finish_pending_mutation_response(&self) {
        self.mutation_journal.finish_pending_response();
    }

    pub(crate) fn durable_head(&self) -> SequenceNumber {
        self.mutation_journal.durable_head()
    }

    pub(crate) fn applied_head(&self) -> SequenceNumber {
        self.mutation_journal.applied_head()
    }

    pub(crate) fn lock_mutation_sequence(&self) -> std::sync::MutexGuard<'_, ()> {
        self.mutation_journal.lock_sequence_gate()
    }

    pub(crate) fn mark_durable_head(&self, sequence: SequenceNumber) {
        self.mutation_journal.mark_durable_head(sequence);
    }

    pub(crate) fn mark_applied_head(&self, sequence: SequenceNumber) {
        self.mutation_journal.mark_applied_head(sequence);
    }

    pub(crate) async fn wait_for_applied_sequence_cancellable<Fut>(
        &self,
        sequence: SequenceNumber,
        cancel_wait: Fut,
    ) -> Result<()>
    where
        Fut: Future<Output = ()>,
    {
        self.mutation_journal
            .wait_for_applied_sequence_cancellable(sequence, cancel_wait)
            .await
    }

    pub(crate) fn wait_for_applied_sequence_blocking(&self, sequence: SequenceNumber) {
        self.mutation_journal
            .wait_for_applied_sequence_blocking(sequence);
    }

    pub(crate) fn sync_mutation_journal_progress(&self, progress: JournalProgress) {
        self.mark_durable_head(progress.durable_head);
        self.mark_applied_head(progress.applied_head);
    }

    #[cfg(test)]
    pub(crate) fn document_cache_stats(&self) -> DocumentCacheStats {
        self.document_cache.stats()
    }

    pub(crate) fn materialized_read_surface_stats(&self) -> MaterializedReadSurfaceStats {
        self.materialized_reads.stats()
    }

    #[cfg(test)]
    pub(crate) fn materialized_table_publication_stats(
        &self,
        table: &TableName,
    ) -> Option<MaterializedTablePublicationStats> {
        self.materialized_reads.table_publication_stats(table)
    }

    #[cfg(test)]
    pub(crate) fn materialized_serving_snapshot_for_testing(
        &self,
        required_sequence: SequenceNumber,
    ) -> Option<ServingSnapshot> {
        self.materialized_serving_snapshot(required_sequence)
    }

    pub(crate) fn serving_snapshot_manager_stats(&self) -> ServingSnapshotManagerStats {
        self.materialized_reads.serving_snapshot_manager_stats()
    }

    #[cfg(test)]
    pub(crate) fn set_materialized_read_surface_limits_for_testing(
        &self,
        table_capacity: usize,
        byte_capacity: usize,
    ) {
        self.materialized_reads
            .set_limits_for_testing(table_capacity, byte_capacity);
    }

    #[cfg(test)]
    pub(crate) fn set_materialized_read_surface_version_capacity_for_testing(
        &self,
        version_capacity: usize,
    ) {
        self.materialized_reads
            .set_version_capacity_for_testing(version_capacity);
    }

    pub(crate) fn mutation_admission_stats(&self) -> MutationAdmissionStats {
        self.mutation_admission.stats()
    }

    pub(crate) fn mutation_journal_stats(&self) -> MutationJournalStats {
        self.mutation_journal.stats()
    }

    pub(crate) fn subscription_delivery_stats(&self) -> SubscriptionDeliveryStats {
        self.subscription_delivery.stats()
    }

    pub(crate) fn query_planning_stats(&self) -> QueryPlanningStats {
        self.query_planning.stats()
    }

    pub(crate) fn engine_diagnostics_snapshot(&self) -> TenantEngineDiagnosticsSnapshot {
        TenantEngineDiagnosticsSnapshot {
            mutation_admission: self.mutation_admission_stats(),
            mutation_journal: self.mutation_journal_stats(),
            subscription_delivery: self.subscription_delivery_stats(),
            materialized_read_surface: self.materialized_read_surface_stats(),
            serving_snapshot_manager: self.serving_snapshot_manager_stats(),
            query_planning: self.query_planning_stats(),
        }
    }

    #[cfg(test)]
    pub(crate) fn set_subscription_delivery_queue_capacity_for_testing(&self, capacity: usize) {
        self.subscription_delivery
            .set_capacity_for_testing(capacity);
    }

    #[cfg(test)]
    pub(crate) fn set_mutation_journal_queue_capacity_for_testing(&self, capacity: usize) {
        self.mutation_journal.set_capacity_for_testing(capacity);
    }

    #[cfg(test)]
    pub(crate) fn set_mutation_admission_codel_for_testing(
        &self,
        target: Duration,
        interval: Duration,
    ) {
        self.mutation_admission
            .set_codel_for_testing(target, interval);
    }

    #[cfg(test)]
    pub(crate) fn subscription_delivery_pause_handle_for_testing(
        &self,
    ) -> SubscriptionDeliveryPauseHandle {
        self.subscription_delivery.pause_handle()
    }

    #[cfg(test)]
    pub(crate) fn materialized_read_publish_pause_handle_for_testing(
        &self,
    ) -> MaterializedReadPublishPauseHandle {
        self.materialized_reads.publish_pause_handle()
    }

    #[cfg(test)]
    pub(crate) fn mutation_journal_pause_handle_for_testing(&self) -> MutationJournalPauseHandle {
        self.mutation_journal.pause_handle()
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn subscription_bootstrap_pause_handle_for_testing(
        &self,
    ) -> MutationJournalPauseHandle {
        MutationJournalPauseHandle::from_state(self.subscription_bootstrap_pause.clone())
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) async fn wait_if_subscription_bootstrap_pause_armed(&self) {
        self.subscription_bootstrap_pause.wait_if_armed().await;
    }
}

#[cfg(test)]
mod mutation_admission_tests;
