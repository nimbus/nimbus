//! Core storage capability traits.

use nimbus_core::{
    CommitEntry, Document, DocumentId, Filter, Result, SequenceNumber, TableName,
    TenantEventRecord, TenantId, Timestamp,
};
use nimbus_crypto::LocalKeyProvider;
use serde_json::{Map, Value};

use crate::IndexRangeBound;
use crate::async_storage::UsageStorage;
use crate::changefeed::{ChangefeedBootstrap, ChangefeedCursor, ChangefeedPage};
use crate::store::{DurableJournalBootstrap, DurableJournalPage, JournalProgress};

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
