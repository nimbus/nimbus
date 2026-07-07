//! Core storage capability traits.

use nimbus_core::{
    CollectionName, CommitEntry, Document, DocumentId, Filter, ResourcePathBinding, Result,
    SequenceNumber, TableId, TableName, TenantEventRecord, TenantId, Timestamp,
};
use nimbus_crypto::LocalKeyProvider;
use serde_json::{Map, Value};

use crate::IndexRangeBound;
use crate::async_storage::UsageStorage;
use crate::changefeed::{ChangefeedBootstrap, ChangefeedCursor, ChangefeedPage};
use crate::retention::RetentionGcConfig;
use crate::store::{
    DurableJournalBootstrap, DurableJournalPage, JournalProgress, PointInTimeRestoreArchive,
    PointInTimeRestoreTarget,
};

use super::object_metadata::ObjectMetaStore;

pub trait TenantLifecycle {
    type OpenedTenant;

    async fn list_tenants(&self) -> Result<Vec<TenantId>>;
    async fn tenant_exists(&self, tenant_id: &TenantId) -> Result<bool>;
    async fn create_tenant(&self, tenant_id: &TenantId) -> Result<Self::OpenedTenant>;
    async fn open_existing_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<Self::OpenedTenant>>;
    async fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()>;
}

/// Point document reads by table and document ID.
pub trait TenantPointRead {
    fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>>;
}

/// Point document writes that commit through the backend's durable write path.
pub trait TenantPointWrite {
    fn insert_document(&self, document: &Document) -> Result<CommitEntry>;

    fn update_document_validated<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &Map<String, Value>,
        validate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static;

    fn delete_document_validated<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        validate: F,
    ) -> Result<(CommitEntry, Document)>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static;
}

/// Table and index range reads used by the query planner.
pub trait TenantRangeScan {
    fn scan_table_matching_with_filters_cancellable<F>(
        &self,
        table: &TableName,
        filters: &[Filter],
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>;

    fn scan_table_id_prefix_cancellable(
        &self,
        table: &TableName,
        id_prefix: &str,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>>;

    fn scan_table_id_starting_at_cancellable(
        &self,
        table: &TableName,
        start_id: &str,
        limit: usize,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>>;

    fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>>;

    fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>>;

    fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>>;

    fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>>;
}

/// Durable journal access used for recovery, subscriptions, and replication.
pub trait DurableJournal {
    fn journal_progress(&self) -> Result<JournalProgress>;
    fn read_durable_journal_from(&self, sequence: SequenceNumber)
    -> Result<Vec<TenantEventRecord>>;
    fn stream_durable_journal(
        &self,
        after: SequenceNumber,
        limit: usize,
    ) -> Result<DurableJournalPage>;
    fn export_durable_journal_bootstrap(&self) -> Result<DurableJournalBootstrap>;

    /// Latest sequence number durably appended to the journal (may be ahead
    /// of `applied_sequence` while replay/apply is still catching up).
    fn latest_sequence(&self) -> Result<SequenceNumber>;
    /// Latest sequence number whose effects are visible to reads.
    fn applied_sequence(&self) -> Result<SequenceNumber>;
    /// Reads applied commit-log entries (document-level write effects),
    /// starting at `sequence`, in contrast to `read_durable_journal_from`'s
    /// raw event records.
    fn read_commit_log_from(&self, sequence: SequenceNumber) -> Result<Vec<CommitEntry>>;
    /// Replays any durable records not yet applied, bringing
    /// `applied_sequence` back up to `latest_sequence`.
    fn recover_durable_journal(&self) -> Result<JournalProgress>;
    /// Appends records to the durable journal without applying them.
    fn append_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()>;
    /// Appends and applies records to the durable journal in one step.
    fn apply_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()>;
    fn export_point_in_time_restore_archive(
        &self,
        target: PointInTimeRestoreTarget,
        retention_config: RetentionGcConfig,
    ) -> Result<PointInTimeRestoreArchive>;
    fn import_point_in_time_restore_archive(
        &self,
        archive: &PointInTimeRestoreArchive,
    ) -> Result<JournalProgress>;

    fn export_changefeed_bootstrap(&self) -> Result<ChangefeedBootstrap> {
        ChangefeedBootstrap::from_durable_bootstrap(self.export_durable_journal_bootstrap()?)
    }

    fn stream_changefeed(&self, cursor: &ChangefeedCursor, limit: usize) -> Result<ChangefeedPage> {
        cursor.rotate_handle(cursor.handle.clone())?;
        let page = self
            .stream_durable_journal(cursor.after, limit)
            .map_err(crate::changefeed::map_changefeed_journal_error)?;
        ChangefeedPage::from_durable_page(cursor.handle.clone(), page)
    }
}

/// Scheduler inspection capability for stores that own scheduled work.
pub trait SchedulerStore {
    fn scheduled_execution_exists(&self, execution_id: &str) -> Result<bool>;
    fn has_scheduled_work(&self) -> Result<bool>;
    fn next_scheduled_work_at(&self) -> Result<Option<Timestamp>>;
}

/// Control-plane usage storage.
pub trait ControlPlaneUsage: UsageStorage {}

/// Local database key-provider capability.
pub trait KeyProviderSurface: LocalKeyProvider {}

/// Resource-path binding lookups backing document-path <-> locator
/// resolution (collection listing, path uniqueness). `Snapshot` is the
/// point-in-time read surface returned by `read_snapshot`, which owns the
/// bulk `scan_resource_path_bindings` listing.
pub trait ResourcePathScan {
    type Snapshot: ResourcePathSnapshot;

    fn read_snapshot(&self) -> Result<Self::Snapshot>;
    fn table_id(&self, table: &TableName) -> Result<Option<TableId>>;
    fn scan_collection_group_bindings(
        &self,
        collection_group: &CollectionName,
    ) -> Result<Vec<ResourcePathBinding>>;
}

/// Snapshot-scoped resource-path surface obtained via `ResourcePathScan::read_snapshot`.
pub trait ResourcePathSnapshot {
    fn scan_resource_path_bindings(&self) -> Result<Vec<ResourcePathBinding>>;
}

/// Materialized read-surface rebuild triad: the minimal surface
/// `tenant/materialized_reads` needs to load or catch up a table's
/// in-memory serving snapshot from the durable commit log. `applied_sequence`
/// and `read_commit_log_from` come from the `DurableJournal` supertrait
/// (declared once there to avoid an ambiguous two-trait method call); this
/// trait adds the one method `DurableJournal` doesn't cover: an unfiltered,
/// cancellable full-table scan.
pub trait MaterializedRebuild: DurableJournal {
    fn scan_table_matching_cancellable<F>(
        &self,
        table: &TableName,
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>;
}

/// Composite capability bound for the async read seam: everything an
/// `engine/queries/` read closure may need from a tenant store, without
/// naming a concrete backend type. See `TenantReadStorage`.
pub trait ReadCapabilities:
    TenantPointRead + TenantRangeScan + DurableJournal + ResourcePathScan + MaterializedRebuild
{
}

impl<T> ReadCapabilities for T where
    T: TenantPointRead + TenantRangeScan + DurableJournal + ResourcePathScan + MaterializedRebuild
{
}

/// Composite convenience trait for tenant data stores that support the core
/// engine read, write, journal, and scheduler capabilities.
pub trait StorageEngine:
    TenantPointRead
    + TenantPointWrite
    + TenantRangeScan
    + DurableJournal
    + SchedulerStore
    + ObjectMetaStore
{
}
