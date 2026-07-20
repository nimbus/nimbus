use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use nimbus_core::{Error, Result, Schema, TableId, TableName, TenantId, Timestamp};
use nimbus_storage::LibsqlReplicaFreshnessStats;
use serde::Serialize;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::engine::{
    CommitPhaseDurations, CommitPhaseMetrics, CommitPhaseMetricsSnapshot, CommitTraceSample,
    WriteLog, WriteLogConfig, maybe_emit_commit_trace,
};
use crate::persistence::{TenantPersistence, TenantPersistenceExecutor};
use crate::subscriptions::SubscriptionRegistry;
use crate::triggers::TriggerRegistration;
use crate::triggers::TriggerRegistry;
use crate::triggers::execution::SharedTriggerInvocationExecutor;
use nimbus_storage::Clock;

mod background;
mod committer_lease;
mod document_cache;
mod document_cache_facade;
mod lifecycle;
mod materialized_reads;
mod materialized_reads_facade;
mod mutation;
mod mutation_facade;
#[cfg(test)]
pub(crate) mod pause_barrier;
mod query_planning;
mod query_planning_facade;
mod subscription_delivery;
mod subscription_delivery_facade;
mod trigger_candidates;
mod trigger_execution;
mod write_rate;

use self::committer_lease::CommitterLeaseLifecycle;
#[cfg(test)]
pub(crate) use self::document_cache::DocumentCacheStats;
use self::document_cache::TenantDocumentCache;
#[cfg(test)]
pub(crate) use self::document_cache::{
    DOCUMENT_CACHE_CAPACITY, DocumentCacheInvalidationPauseHandle,
};
use self::lifecycle::TenantLifecycle;
#[cfg(test)]
pub(crate) use self::materialized_reads::MaterializedReadPublishPauseHandle;
#[cfg(test)]
pub(crate) use self::materialized_reads::MaterializedTablePublicationStats;
pub(crate) use self::materialized_reads::ServingSnapshot;
use self::materialized_reads::TenantMaterializedReadSurface;
pub use self::materialized_reads::{
    MaterializedReadSurfaceStats, PinnedServingReadSnapshot, ServingSnapshotManagerStats,
};
#[cfg(test)]
pub(crate) use self::mutation::DEFAULT_MUTATION_ADMISSION_QUEUE_CAPACITY;
#[cfg(test)]
pub(crate) use self::mutation::DEFAULT_MUTATION_JOURNAL_QUEUE_CAPACITY;
pub(crate) use self::mutation::MutationIsolateAdmissionPermit;
#[cfg(any(test, feature = "test-hooks"))]
pub use self::mutation::MutationJournalPauseHandle;
#[cfg(any(test, feature = "test-hooks"))]
use self::mutation::MutationJournalPauseState;
#[cfg(test)]
pub(crate) use self::mutation::configure_committer_limits_for_testing;
pub(crate) use self::mutation::{
    AssignedPublisherBatch, DeferredPublisherResponse, MutationResponseSender,
    PendingPublisherResponse, PreparedPayloadAccounting, PublisherErrorCounts, PublisherMessage,
    PublisherQueueError, QueuedMutationRequest, QueuedMutationResult,
};
pub(crate) use self::mutation::{
    CommitterActor, CommitterJob, CommitterMessage, assign_and_validate, run_committer_actor,
    run_job, validate_append_sequences,
};
pub use self::mutation::{
    CommitterPipelineMode, MutationAdmissionPhase, MutationAdmissionStats,
    MutationIsolateAdmissionStats, MutationJournalStats,
};
use self::mutation::{
    MutationAdmissionDecision, MutationAdmissionGate, MutationIsolateAdmission,
    MutationJournalState, ObserverHandoff, PublisherHandoff,
};
#[cfg(test)]
pub(crate) use self::mutation::{
    configure_observer_drain_blocking_timeout_for_testing, configure_observer_limits_for_testing,
    configure_publisher_limits_for_testing,
};
use self::query_planning::QueryPlanningMetrics;
pub use self::query_planning::QueryPlanningStats;
pub(crate) use self::query_planning::{QueryPlanMetricKind, QueryPlanMetricOperation};
#[cfg(test)]
pub(crate) use self::subscription_delivery::DEFAULT_SUBSCRIPTION_WORK_QUEUE_CAPACITY;
pub(crate) use self::subscription_delivery::SubscriptionDeliveryMetrics;
#[cfg(test)]
pub(crate) use self::subscription_delivery::SubscriptionDeliveryPauseHandle;
use self::subscription_delivery::SubscriptionDeliveryQueue;
pub use self::subscription_delivery::SubscriptionDeliveryStats;
use self::trigger_candidates::TriggerCandidateFeed;
#[cfg(test)]
pub(crate) use self::trigger_candidates::TriggerCandidatePauseHandle;
use self::trigger_execution::TriggerExecutionQueue;
use self::write_rate::TenantWriteRateLimiter;
pub use self::write_rate::TenantWriteRateStats;
#[cfg(test)]
pub(crate) use crate::subscriptions::SubscriptionDeliveryPublishPauseHandle;

/// Runtime state for a loaded tenant.
///
/// Ownership is intentionally grouped by subsystem:
/// - persistence reads/writes go through `store()` / `read_storage()`
/// - the live schema snapshot goes through `schema()` / `replace_schema_snapshot()`
/// - subscription coordination goes through `subscription_registry()`
/// - mutation, materialized-read, and trigger coordination each use their
///   concept-owned facade modules under `tenant/`
pub struct TenantRuntime {
    tenant_id: TenantId,
    pub store: TenantPersistence,
    pub read_storage: TenantPersistenceExecutor,
    pub subscriptions: SubscriptionRegistry,
    pub schema: ArcSwap<Schema>,
    document_cache: TenantDocumentCache,
    materialized_reads: TenantMaterializedReadSurface,
    query_planning: QueryPlanningMetrics,
    commit_phases: CommitPhaseMetrics,
    pub(crate) write_log: WriteLog,
    subscription_delivery: SubscriptionDeliveryQueue,
    trigger_candidates: TriggerCandidateFeed,
    trigger_execution: TriggerExecutionQueue,
    trigger_registry: TriggerRegistry,
    lifecycle: Arc<TenantLifecycle>,
    mutation_admission: Arc<MutationAdmissionGate>,
    mutation_isolate_admission: Arc<MutationIsolateAdmission>,
    mutation_journal: Arc<MutationJournalState>,
    committer: Arc<CommitterActor>,
    committer_lease: Option<Arc<CommitterLeaseLifecycle>>,
    publisher: Arc<PublisherHandoff>,
    observer_dispatch: Arc<ObserverHandoff>,
    observer_lifetime: Arc<()>,
    write_rate: TenantWriteRateLimiter,
    last_assigned_commit_timestamp: AtomicU64,
    prepared_table_ids: Mutex<HashMap<TableName, TableId>>,
    prepare_permits: Arc<Semaphore>,
    #[cfg(any(test, feature = "test-hooks"))]
    subscription_bootstrap_pause: Arc<MutationJournalPauseState>,
}

pub struct TenantOperationGuard {
    lifecycle: Arc<TenantLifecycle>,
}

pub struct TenantDeletionGuard;

pub(crate) struct TenantRuntimeInitialState {
    pub schema: Schema,
    pub progress: nimbus_storage::JournalProgress,
    pub last_commit_timestamp: Timestamp,
}

pub(crate) struct TenantRuntimeInitialStateProfile {
    pub schema_load: Duration,
    pub journal_progress: Duration,
    pub total: Duration,
}

fn prepare_concurrency() -> usize {
    std::env::var_os("NIMBUS_PREPARE_CONCURRENCY")
        .and_then(|value| value.into_string().ok())
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(std::num::NonZeroUsize::get)
                .unwrap_or(4)
                // SQLite's read-snapshot pool is deliberately small. Four
                // callers overlap CPU and serialization without turning pool
                // polling into the dominant prepare cost.
                .min(4)
        })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantEngineDiagnosticsSnapshot {
    pub mutation_admission: MutationAdmissionStats,
    pub mutation_isolate_admission: MutationIsolateAdmissionStats,
    pub mutation_journal: MutationJournalStats,
    pub subscription_delivery: SubscriptionDeliveryStats,
    pub materialized_read_surface: MaterializedReadSurfaceStats,
    pub serving_snapshot_manager: ServingSnapshotManagerStats,
    pub query_planning: QueryPlanningStats,
    pub commit_phases: CommitPhaseMetricsSnapshot,
    pub tenant_write_rate: TenantWriteRateStats,
    pub libsql_replica_freshness: Option<LibsqlReplicaFreshnessStats>,
}

impl Drop for TenantOperationGuard {
    fn drop(&mut self) {
        self.lifecycle.release_operation();
    }
}

impl TenantRuntime {
    fn from_initialized_parts(
        tenant_id: TenantId,
        store: TenantPersistence,
        read_storage: TenantPersistenceExecutor,
        schema: Schema,
        progress: nimbus_storage::JournalProgress,
        last_commit_timestamp: Timestamp,
        clock: Arc<dyn Clock>,
        committer_owner_id: Option<String>,
    ) -> Self {
        let publisher_pipeline_capable = store.has_process_local_sequence_authority();
        let committer = Arc::new(CommitterActor::new(tenant_id.clone()));
        let publisher = Arc::new(PublisherHandoff::new(
            publisher_pipeline_capable,
            &tenant_id,
        ));
        let observer_dispatch = Arc::new(ObserverHandoff::new(&tenant_id));
        Self {
            tenant_id,
            store,
            read_storage,
            subscriptions: SubscriptionRegistry::new(),
            schema: ArcSwap::new(Arc::new(schema)),
            document_cache: TenantDocumentCache::new(),
            materialized_reads: TenantMaterializedReadSurface::new(),
            query_planning: QueryPlanningMetrics::new(),
            commit_phases: CommitPhaseMetrics::new(),
            write_log: WriteLog::new(
                WriteLogConfig::from_env(),
                progress.applied_head,
                progress.durable_head,
            ),
            subscription_delivery: SubscriptionDeliveryQueue::new(),
            trigger_candidates: TriggerCandidateFeed::new(),
            trigger_execution: TriggerExecutionQueue::new(),
            trigger_registry: TriggerRegistry::new(),
            lifecycle: Arc::new(TenantLifecycle::new()),
            mutation_admission: Arc::new(MutationAdmissionGate::new()),
            mutation_isolate_admission: Arc::new(MutationIsolateAdmission::from_env()),
            mutation_journal: Arc::new(MutationJournalState::new(progress)),
            committer,
            committer_lease: committer_owner_id
                .map(|owner_id| Arc::new(CommitterLeaseLifecycle::new(owner_id, clock))),
            publisher,
            observer_dispatch,
            observer_lifetime: Arc::new(()),
            write_rate: TenantWriteRateLimiter::new(),
            last_assigned_commit_timestamp: AtomicU64::new(last_commit_timestamp.0),
            prepared_table_ids: Mutex::new(HashMap::new()),
            prepare_permits: Arc::new(Semaphore::new(prepare_concurrency())),
            #[cfg(any(test, feature = "test-hooks"))]
            subscription_bootstrap_pause: Arc::new(MutationJournalPauseState::default()),
        }
    }

    pub(crate) fn from_loaded_state(
        tenant_id: TenantId,
        store: TenantPersistence,
        read_storage: TenantPersistenceExecutor,
        initial_state: TenantRuntimeInitialState,
        clock: Arc<dyn Clock>,
        committer_owner_id: Option<String>,
    ) -> Self {
        Self::from_initialized_parts(
            tenant_id,
            store,
            read_storage,
            initial_state.schema,
            initial_state.progress,
            initial_state.last_commit_timestamp,
            clock,
            committer_owner_id,
        )
    }

    pub(crate) async fn load_initial_state_async(
        store: &TenantPersistence,
        read_storage: &TenantPersistenceExecutor,
    ) -> Result<(TenantRuntimeInitialState, TenantRuntimeInitialStateProfile)> {
        let total_started = Instant::now();
        let schema_started = Instant::now();
        let schema = store.load_schema_async(read_storage).await?;
        let schema_load = schema_started.elapsed();
        let progress_started = Instant::now();
        let progress = store.journal_progress_async(read_storage).await?;
        let last_commit_timestamp = if progress.durable_head.0 == 0 {
            Timestamp(0)
        } else {
            store
                .read_durable_journal_from_async(read_storage, progress.durable_head)
                .await?
                .into_iter()
                .find(|record| record.sequence == progress.durable_head)
                .map_or(Timestamp(0), |record| record.timestamp)
        };
        let journal_progress = progress_started.elapsed();
        Ok((
            TenantRuntimeInitialState {
                schema,
                progress,
                last_commit_timestamp,
            },
            TenantRuntimeInitialStateProfile {
                schema_load,
                journal_progress,
                total: total_started.elapsed(),
            },
        ))
    }

    /// Creates a tenant runtime from a store.
    pub fn from_parts(
        tenant_id: TenantId,
        store: TenantPersistence,
        read_storage: TenantPersistenceExecutor,
        clock: Arc<dyn Clock>,
        committer_owner_id: Option<String>,
    ) -> Result<Self> {
        let schema = store.load_schema()?;
        let progress = store.journal_progress()?;
        let last_commit_timestamp = if progress.durable_head.0 == 0 {
            Timestamp(0)
        } else {
            store
                .read_durable_journal_from(progress.durable_head)?
                .into_iter()
                .find(|record| record.sequence == progress.durable_head)
                .map_or(Timestamp(0), |record| record.timestamp)
        };
        Ok(Self::from_initialized_parts(
            tenant_id,
            store,
            read_storage,
            schema,
            progress,
            last_commit_timestamp,
            clock,
            committer_owner_id,
        ))
    }

    /// Creates a tenant runtime asynchronously from a store.
    pub async fn from_parts_async(
        tenant_id: TenantId,
        store: TenantPersistence,
        read_storage: TenantPersistenceExecutor,
        clock: Arc<dyn Clock>,
        committer_owner_id: Option<String>,
    ) -> Result<Self> {
        let (initial_state, _) = Self::load_initial_state_async(&store, &read_storage).await?;
        Ok(Self::from_loaded_state(
            tenant_id,
            store,
            read_storage,
            initial_state,
            clock,
            committer_owner_id,
        ))
    }

    /// Returns the current schema snapshot.
    pub fn schema(&self) -> Arc<Schema> {
        self.schema.load_full()
    }

    pub(crate) fn store(&self) -> &TenantPersistence {
        &self.store
    }

    pub(crate) fn observer_identity(&self) -> crate::engine::TenantRuntimeObserverIdentity {
        crate::engine::TenantRuntimeObserverIdentity::new(&self.observer_lifetime)
    }

    /// Reserves one stable identity while concurrent prepares race to create a
    /// schemaless table. The durable table identity wins after the first apply.
    pub(crate) fn prepared_table_id(&self, table: &TableName, durable: Option<TableId>) -> TableId {
        if let Some(table_id) = durable {
            self.prepared_table_ids
                .lock()
                .expect("prepared table-id lock should not be poisoned")
                .insert(table.clone(), table_id.clone());
            return table_id;
        }
        self.prepared_table_ids
            .lock()
            .expect("prepared table-id lock should not be poisoned")
            .entry(table.clone())
            .or_default()
            .clone()
    }

    pub(crate) fn prepared_table_id_if_known(&self, table: &TableName) -> Option<TableId> {
        self.prepared_table_ids
            .lock()
            .expect("prepared table-id lock should not be poisoned")
            .get(table)
            .cloned()
    }

    pub(crate) async fn acquire_prepare_permit(&self) -> Result<OwnedSemaphorePermit> {
        self.prepare_permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| nimbus_core::Error::Internal("tenant prepare pool closed".to_string()))
    }

    pub(crate) async fn acquire_mutation_isolate_permit(
        &self,
    ) -> Result<MutationIsolateAdmissionPermit> {
        self.mutation_isolate_admission.acquire().await
    }

    pub(crate) fn acquire_prepare_permit_blocking(&self) -> Result<OwnedSemaphorePermit> {
        loop {
            match Arc::clone(&self.prepare_permits).try_acquire_owned() {
                Ok(permit) => return Ok(permit),
                Err(tokio::sync::TryAcquireError::Closed) => {
                    return Err(nimbus_core::Error::Internal(
                        "tenant prepare pool closed before accepting work".to_string(),
                    ));
                }
                Err(tokio::sync::TryAcquireError::NoPermits) => {
                    std::thread::park_timeout(Duration::from_millis(1));
                }
            }
        }
    }

    pub(crate) fn read_storage(&self) -> &TenantPersistenceExecutor {
        &self.read_storage
    }

    pub(crate) fn subscription_registry(&self) -> &SubscriptionRegistry {
        &self.subscriptions
    }

    pub(crate) fn replace_schema_snapshot(&self, schema: Arc<Schema>) {
        self.prepared_table_ids
            .lock()
            .expect("prepared table-id lock should not be poisoned")
            .clear();
        self.schema.store(schema);
    }

    pub(crate) fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub(crate) fn commit_phase_metrics(&self) -> &CommitPhaseMetrics {
        &self.commit_phases
    }

    pub(crate) fn record_commit_phase_sample(
        &self,
        path: &'static str,
        commit_count: u64,
        phases: CommitPhaseDurations,
        total: Duration,
    ) {
        self.commit_phases
            .record_sample(commit_count, phases, total);
        maybe_emit_commit_trace(CommitTraceSample {
            tenant_id: &self.tenant_id,
            path,
            commit_count,
            phases,
            total,
        });
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

    pub(crate) fn mark_deleting_for_eviction(&self) {
        self.lifecycle.begin_eviction();
        self.shutdown_committer_lease_renewal();
        self.mutation_journal.fail_applied_waiters(Error::storage(
            nimbus_core::StorageErrorKind::Unavailable,
            format!(
                "tenant {} runtime was evicted before the required sequence became applied",
                self.tenant_id
            ),
        ));
    }

    pub(crate) async fn wait_for_operation_drain_for_eviction(&self) {
        self.lifecycle.wait_for_operations_async().await;
    }

    pub(crate) fn wait_for_operation_drain_for_eviction_blocking(&self) {
        self.lifecycle.wait_for_operations_blocking();
    }

    pub(crate) fn eviction_started(&self) -> bool {
        self.lifecycle.eviction_started()
    }

    pub(crate) fn durable_recovery_eviction_error(&self) -> Error {
        Error::storage(
            nimbus_core::StorageErrorKind::Unavailable,
            format!(
                "tenant {} runtime is restarting after durable recovery",
                self.tenant_id
            ),
        )
    }

    pub(crate) async fn wait_for_eviction_complete(&self) {
        self.lifecycle.wait_for_eviction_complete().await;
    }

    pub(crate) fn finish_eviction(&self) {
        self.lifecycle.finish_eviction();
    }

    pub(crate) fn trigger_registry(&self) -> &TriggerRegistry {
        &self.trigger_registry
    }

    pub(crate) fn ensure_trigger_execution_worker_started(
        self: &Arc<Self>,
        clock: Arc<dyn Clock>,
        executor: SharedTriggerInvocationExecutor,
    ) {
        self.trigger_execution.start_worker(self, clock, executor);
    }

    pub(crate) fn enqueue_trigger_invocation_keys(
        &self,
        keys: Vec<nimbus_core::TriggerInvocationKey>,
    ) {
        self.trigger_execution.enqueue(keys);
    }

    pub(crate) fn enqueue_trigger_invocation_scheduled(
        &self,
        entries: Vec<(nimbus_core::TriggerInvocationKey, Timestamp)>,
    ) {
        self.trigger_execution.enqueue_scheduled(entries);
    }

    pub(crate) fn shutdown_trigger_execution(&self) {
        self.trigger_execution.shutdown();
    }

    pub(crate) fn replace_trigger_registrations(
        &self,
        registrations: Vec<TriggerRegistration>,
    ) -> Result<()> {
        self.trigger_registry.replace(registrations)
    }

    pub(crate) fn engine_diagnostics_snapshot(&self) -> TenantEngineDiagnosticsSnapshot {
        let mutation_journal = self.mutation_journal_stats();
        let mut commit_phases = self.commit_phases.snapshot();
        commit_phases.committer_inbox_depth =
            u64::try_from(mutation_journal.committer_inbox_depth).unwrap_or(u64::MAX);
        commit_phases.committer_send_timeout_total = mutation_journal.committer_send_timeout_count;
        TenantEngineDiagnosticsSnapshot {
            mutation_admission: self.mutation_admission_stats(),
            mutation_isolate_admission: self.mutation_isolate_admission.stats(),
            mutation_journal,
            subscription_delivery: self.subscription_delivery_stats(),
            materialized_read_surface: self.materialized_read_surface_stats(),
            serving_snapshot_manager: self.serving_snapshot_manager_stats(),
            query_planning: self.query_planning_stats(),
            commit_phases,
            tenant_write_rate: self.write_rate.stats(),
            libsql_replica_freshness: self.store.libsql_replica_freshness_stats(),
        }
    }
}

#[cfg(test)]
mod mutation_admission_tests;
