use nimbus_core::{Result, SequenceNumber, TableName};

use super::*;
use crate::persistence::TenantPersistence;

impl TenantRuntime {
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
        store: &TenantPersistence,
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

    #[cfg(test)]
    pub(crate) fn materialized_read_publish_pause_handle_for_testing(
        &self,
    ) -> MaterializedReadPublishPauseHandle {
        self.materialized_reads.publish_pause_handle()
    }
}
