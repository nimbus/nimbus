//! Remaining `nimbus_storage::ReadCapabilities` trait impls for
//! `TenantPersistence` / `TenantPersistenceSnapshot`: `DurableJournal`,
//! `ResourcePathScan`, `ResourcePathSnapshot`, and `MaterializedRebuild`.
//! `TenantPointRead` and `TenantRangeScan` (which `QueryReadStore` is a
//! blanket alias over) live in `persistence::query`.
//!
//! Every method here delegates to an inherent method that already exists on
//! `TenantPersistence` / `TenantPersistenceSnapshot` (see `persistence/tenant/*.rs`
//! and `persistence/snapshot.rs`), so this is pure type-surface lifting: no
//! new logic, no changed query results.

use nimbus_core::{
    CollectionName, CommitEntry, Document, ResourcePathBinding, Result, SequenceNumber, TableId,
    TableName, TenantEventRecord,
};
use nimbus_storage::{
    ChangefeedBootstrap, ChangefeedCursor, ChangefeedPage, DurableJournal, DurableJournalBootstrap,
    DurableJournalPage, JournalProgress, MaterializedRebuild, PointInTimeRestoreArchive,
    PointInTimeRestoreTarget, ResourcePathScan, ResourcePathSnapshot, RetentionGcConfig,
};

use super::{TenantPersistence, TenantPersistenceSnapshot};

impl DurableJournal for TenantPersistence {
    fn journal_progress(&self) -> Result<JournalProgress> {
        TenantPersistence::journal_progress(self)
    }

    fn read_durable_journal_from(
        &self,
        sequence: SequenceNumber,
    ) -> Result<Vec<TenantEventRecord>> {
        TenantPersistence::read_durable_journal_from(self, sequence)
    }

    fn stream_durable_journal(
        &self,
        after: SequenceNumber,
        limit: usize,
    ) -> Result<DurableJournalPage> {
        TenantPersistence::stream_durable_journal(self, after, limit)
    }

    fn export_durable_journal_bootstrap(&self) -> Result<DurableJournalBootstrap> {
        TenantPersistence::export_durable_journal_bootstrap(self)
    }

    fn latest_sequence(&self) -> Result<SequenceNumber> {
        TenantPersistence::latest_sequence(self)
    }

    fn applied_sequence(&self) -> Result<SequenceNumber> {
        TenantPersistence::applied_sequence(self)
    }

    fn read_commit_log_from(&self, sequence: SequenceNumber) -> Result<Vec<CommitEntry>> {
        TenantPersistence::read_commit_log_from(self, sequence)
    }

    fn recover_durable_journal(&self) -> Result<JournalProgress> {
        TenantPersistence::recover_durable_journal(self)
    }

    fn append_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        TenantPersistence::append_durable_records_batch(self, records)
    }

    fn apply_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        TenantPersistence::apply_durable_records_batch(self, records)
    }

    fn export_point_in_time_restore_archive(
        &self,
        target: PointInTimeRestoreTarget,
        retention_config: RetentionGcConfig,
    ) -> Result<PointInTimeRestoreArchive> {
        TenantPersistence::export_point_in_time_restore_archive(self, target, retention_config)
    }

    fn import_point_in_time_restore_archive(
        &self,
        archive: &PointInTimeRestoreArchive,
    ) -> Result<JournalProgress> {
        TenantPersistence::import_point_in_time_restore_archive(self, archive)
    }

    // Overridden (rather than left as the trait default) because the
    // inherent `TenantPersistence` methods apply changefeed-specific cursor
    // handling (retention-floor checks, cursor rotation) that the generic
    // trait default does not. Delegating here keeps behavior identical
    // whether a caller reaches these through the concrete type or through a
    // `DurableJournal`-bounded generic.
    fn export_changefeed_bootstrap(&self) -> Result<ChangefeedBootstrap> {
        TenantPersistence::export_changefeed_bootstrap(self)
    }

    fn stream_changefeed(&self, cursor: &ChangefeedCursor, limit: usize) -> Result<ChangefeedPage> {
        TenantPersistence::stream_changefeed(self, cursor, limit)
    }
}

impl ResourcePathSnapshot for TenantPersistenceSnapshot {
    fn scan_resource_path_bindings(&self) -> Result<Vec<ResourcePathBinding>> {
        TenantPersistenceSnapshot::scan_resource_path_bindings(self)
    }
}

impl ResourcePathScan for TenantPersistence {
    type Snapshot = TenantPersistenceSnapshot;

    fn read_snapshot(&self) -> Result<Self::Snapshot> {
        TenantPersistence::read_snapshot(self)
    }

    fn table_id(&self, table: &TableName) -> Result<Option<TableId>> {
        TenantPersistence::table_id(self, table)
    }

    fn scan_collection_group_bindings(
        &self,
        collection_group: &CollectionName,
    ) -> Result<Vec<ResourcePathBinding>> {
        TenantPersistence::scan_collection_group_bindings(self, collection_group)
    }
}

// Compile-time proof that the hand-rolled `TenantPersistence` enum-dispatch
// (SR7's future retirement target) already satisfies the full read-seam
// capability surface today.
const _: fn() = || {
    fn assert_read_capabilities<S: nimbus_storage::ReadCapabilities>() {}
    assert_read_capabilities::<TenantPersistence>();
};

impl MaterializedRebuild for TenantPersistence {
    fn scan_table_matching_cancellable<F>(
        &self,
        table: &TableName,
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        TenantPersistence::scan_table_matching_cancellable(
            self,
            table,
            check_cancel,
            include_document,
        )
    }
}
