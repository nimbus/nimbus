mod backend;
#[cfg(test)]
mod pause;
mod snapshot;
mod stats;
mod warm_load;

#[cfg(test)]
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};

use neovex_core::{CommitEntry, Result, SequenceNumber, TableName};
use neovex_storage::TenantStore;

use self::backend::MaterializedServingBackend;
#[cfg(test)]
pub(crate) use self::pause::MaterializedReadPublishPauseHandle;
pub(crate) use self::snapshot::ServingSnapshot;
use self::snapshot::ServingSnapshotManager;
#[cfg(test)]
pub(crate) use self::stats::MaterializedTablePublicationStats;
pub use self::stats::{MaterializedReadSurfaceStats, ServingSnapshotManagerStats};

pub(super) const DEFAULT_MATERIALIZED_SURFACE_TABLE_CAPACITY: usize = 8;
pub(super) const DEFAULT_MATERIALIZED_SURFACE_BYTE_CAPACITY: usize = 16 * 1024 * 1024;
pub(super) const DEFAULT_MATERIALIZED_SURFACE_VERSION_CAPACITY: usize = 4;

pub(super) struct TenantMaterializedReadSurface {
    backend: MaterializedServingBackend,
    snapshots: ServingSnapshotManager,
    evaluation_count: AtomicU64,
    paginated_count: AtomicU64,
    get_hit_count: AtomicU64,
}

impl TenantMaterializedReadSurface {
    pub(super) fn new() -> Self {
        Self {
            backend: MaterializedServingBackend::new(),
            snapshots: ServingSnapshotManager::new(),
            evaluation_count: AtomicU64::new(0),
            paginated_count: AtomicU64::new(0),
            get_hit_count: AtomicU64::new(0),
        }
    }

    #[cfg(test)]
    pub(super) fn serving_snapshot_covering(
        &self,
        required_sequence: SequenceNumber,
    ) -> Option<ServingSnapshot> {
        self.snapshots.snapshot_covering(required_sequence)
    }

    pub(super) fn serving_snapshot_for_table_with_mode(
        &self,
        table: &TableName,
        required_sequence: SequenceNumber,
        count_bypass: bool,
    ) -> Option<ServingSnapshot> {
        self.backend.serving_snapshot_for_table_with_mode(
            &self.snapshots,
            table,
            required_sequence,
            count_bypass,
        )
    }

    pub(super) fn load_serving_snapshot_cancellable(
        &self,
        store: &TenantStore,
        table: &TableName,
        required_sequence: SequenceNumber,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<ServingSnapshot> {
        self.backend.load_serving_snapshot_cancellable(
            &self.snapshots,
            store,
            table,
            required_sequence,
            check_cancel,
        )
    }

    #[cfg(test)]
    pub(super) async fn wait_for_snapshot_covering_cancellable<Fut>(
        &self,
        required_sequence: SequenceNumber,
        cancel_wait: Fut,
    ) -> Result<ServingSnapshot>
    where
        Fut: Future<Output = ()>,
    {
        self.snapshots
            .wait_for_snapshot_covering_cancellable(required_sequence, cancel_wait)
            .await
    }

    pub(super) fn apply_commit(&self, commit: &CommitEntry) {
        self.backend.apply_commit(&self.snapshots, commit);
    }

    pub(super) fn apply_commits<'a>(&self, commits: impl IntoIterator<Item = &'a CommitEntry>) {
        self.backend.apply_commits(&self.snapshots, commits);
    }

    pub(super) fn record_evaluation(&self) {
        self.evaluation_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn record_paginated(&self) {
        self.paginated_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn record_get_hit(&self) {
        self.get_hit_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn clear(&self) {
        self.backend.clear_publications();
        self.snapshots.clear();
    }

    pub(super) fn stats(&self) -> MaterializedReadSurfaceStats {
        let backend = self.backend.stats();
        MaterializedReadSurfaceStats {
            loaded_table_count: backend.loaded_table_count,
            resident_document_count: backend.resident_document_count,
            resident_estimated_bytes: backend.resident_estimated_bytes,
            retained_version_count: backend.retained_version_count,
            retained_estimated_bytes: backend.retained_estimated_bytes,
            table_capacity: backend.table_capacity,
            byte_capacity: backend.byte_capacity,
            version_capacity: backend.version_capacity,
            table_load_count: backend.table_load_count,
            evaluation_count: self.evaluation_count.load(Ordering::Relaxed),
            paginated_count: self.paginated_count.load(Ordering::Relaxed),
            get_hit_count: self.get_hit_count.load(Ordering::Relaxed),
            bypass_count: backend.bypass_count,
            eviction_count: backend.eviction_count,
            in_flight_load_count: backend.in_flight_load_count,
            earliest_covered_sequence: backend.earliest_covered_sequence,
            latest_covered_sequence: backend.latest_covered_sequence,
            earliest_retained_sequence: backend.earliest_retained_sequence,
            latest_retained_sequence: backend.latest_retained_sequence,
        }
    }

    pub(super) fn serving_snapshot_manager_stats(&self) -> ServingSnapshotManagerStats {
        self.snapshots.stats()
    }

    #[cfg(test)]
    pub(super) fn table_publication_stats(
        &self,
        table: &TableName,
    ) -> Option<MaterializedTablePublicationStats> {
        self.backend.table_publication_stats(table)
    }

    #[cfg(test)]
    pub(super) fn publish_pause_handle(&self) -> MaterializedReadPublishPauseHandle {
        self.backend.publish_pause_handle()
    }

    #[cfg(test)]
    pub(super) fn set_limits_for_testing(&self, table_capacity: usize, byte_capacity: usize) {
        self.backend
            .set_limits_for_testing(table_capacity, byte_capacity);
    }

    #[cfg(test)]
    pub(super) fn set_version_capacity_for_testing(&self, version_capacity: usize) {
        self.backend
            .set_version_capacity_for_testing(version_capacity);
    }
}
