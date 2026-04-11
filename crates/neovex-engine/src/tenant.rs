use std::sync::Arc;
#[cfg(test)]
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use neovex_core::{Result, Schema, TenantId};
use neovex_storage::LibsqlReplicaFreshnessStats;
use serde::Serialize;

use crate::persistence::{TenantPersistence, TenantPersistenceExecutor};
use crate::subscriptions::SubscriptionRegistry;

mod document_cache;
mod document_cache_facade;
mod lifecycle;
mod materialized_reads;
mod materialized_reads_facade;
mod mutation;
mod mutation_facade;
mod query_planning;
mod query_planning_facade;
mod subscription_delivery;
mod subscription_delivery_facade;

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
    pub store: TenantPersistence,
    pub read_storage: TenantPersistenceExecutor,
    pub subscriptions: SubscriptionRegistry,
    pub schema: ArcSwap<Schema>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantEngineDiagnosticsSnapshot {
    pub mutation_admission: MutationAdmissionStats,
    pub mutation_journal: MutationJournalStats,
    pub subscription_delivery: SubscriptionDeliveryStats,
    pub materialized_read_surface: MaterializedReadSurfaceStats,
    pub serving_snapshot_manager: ServingSnapshotManagerStats,
    pub query_planning: QueryPlanningStats,
    pub libsql_replica_freshness: Option<LibsqlReplicaFreshnessStats>,
}

impl Drop for TenantOperationGuard {
    fn drop(&mut self) {
        self.lifecycle.release_operation();
    }
}

impl TenantRuntime {
    fn from_initialized_parts(
        store: TenantPersistence,
        read_storage: TenantPersistenceExecutor,
        schema: Schema,
        progress: neovex_storage::JournalProgress,
    ) -> Self {
        Self {
            store,
            read_storage,
            subscriptions: SubscriptionRegistry::new(),
            schema: ArcSwap::new(Arc::new(schema)),
            document_cache: TenantDocumentCache::new(),
            materialized_reads: TenantMaterializedReadSurface::new(),
            query_planning: QueryPlanningMetrics::new(),
            subscription_delivery: SubscriptionDeliveryQueue::new(),
            lifecycle: Arc::new(TenantLifecycle::new()),
            mutation_admission: Arc::new(MutationAdmissionGate::new()),
            mutation_journal: Arc::new(MutationJournalState::new(progress)),
            #[cfg(any(test, feature = "test-hooks"))]
            subscription_bootstrap_pause: Arc::new(MutationJournalPauseState::default()),
        }
    }

    /// Creates a tenant runtime from a store.
    pub fn from_parts(
        store: TenantPersistence,
        read_storage: TenantPersistenceExecutor,
    ) -> Result<Self> {
        let schema = store.load_schema()?;
        let progress = store.journal_progress()?;
        Ok(Self::from_initialized_parts(
            store,
            read_storage,
            schema,
            progress,
        ))
    }

    /// Creates a tenant runtime asynchronously from a store.
    pub async fn from_parts_async(
        store: TenantPersistence,
        read_storage: TenantPersistenceExecutor,
    ) -> Result<Self> {
        let (schema, progress) = match &store {
            TenantPersistence::Postgres(store) => {
                let schema = store.load_schema_async().await?;
                let progress = store.journal_progress_async().await?;
                (schema, progress)
            }
            TenantPersistence::Redb(_)
            | TenantPersistence::Sqlite(_)
            | TenantPersistence::LibsqlReplica(_)
            | TenantPersistence::MySql(_) => {
                read_storage
                    .execute(|store| Ok((store.load_schema()?, store.journal_progress()?)))
                    .await?
            }
        };
        Ok(Self::from_initialized_parts(
            store,
            read_storage,
            schema,
            progress,
        ))
    }

    /// Returns the current schema snapshot.
    pub fn schema(&self) -> Arc<Schema> {
        self.schema.load_full()
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

    pub(crate) fn engine_diagnostics_snapshot(&self) -> TenantEngineDiagnosticsSnapshot {
        TenantEngineDiagnosticsSnapshot {
            mutation_admission: self.mutation_admission_stats(),
            mutation_journal: self.mutation_journal_stats(),
            subscription_delivery: self.subscription_delivery_stats(),
            materialized_read_surface: self.materialized_read_surface_stats(),
            serving_snapshot_manager: self.serving_snapshot_manager_stats(),
            query_planning: self.query_planning_stats(),
            libsql_replica_freshness: self.store.libsql_replica_freshness_stats(),
        }
    }
}

#[cfg(test)]
mod mutation_admission_tests;
