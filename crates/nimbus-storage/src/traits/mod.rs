//! Focused storage capability traits.
//!
//! These traits are the MBA2 capability split over the current concrete store
//! families. They do not replace the async executor seam in `async_storage`;
//! that seam still owns blocking work and cancellation. The traits here make
//! backend support explicit so future providers can implement only the
//! capability families they actually support.
#![allow(async_fn_in_trait)]

mod kv;

use nimbus_core::{
    CommitEntry, Document, DocumentId, Filter, Result, SequenceNumber, TableName,
    TenantEventRecord, TenantId, Timestamp,
};
use nimbus_crypto::LocalKeyProvider;
use serde_json::{Map, Value};

use crate::async_storage::{
    EmbeddedPersistenceProvider, EmbeddedRedbProvider, EmbeddedSqliteProvider,
    OpenedEmbeddedRedbTenant, OpenedEmbeddedSqliteTenant, UsageStorage,
};
use crate::changefeed::{ChangefeedBootstrap, ChangefeedCursor, ChangefeedPage};
use crate::libsql::OpenedLibsqlReplicaTenant;
use crate::mysql::OpenedMySqlTenant;
use crate::postgres::OpenedPostgresTenant;
use crate::store::{DurableJournalBootstrap, DurableJournalPage, JournalProgress};
use crate::{
    IndexRangeBound, LibsqlReplicaProvider, LibsqlReplicaTenantStore, MySqlProvider,
    MySqlTenantStore, PostgresProvider, PostgresTenantStore, RedbUsageStorage, SqliteTenantStore,
    TenantStore,
};

pub use kv::{
    KvBatchOp, KvBatchOutcome, KvEntry, KvMutation, KvPut, KvScanPage, KvStorageEngine,
    KvSweepOutcome, TenantKvStore,
};

/// Tenant lifecycle and discovery for provider families that can own tenants.
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
    TenantPointRead + TenantPointWrite + TenantRangeScan + DurableJournal + SchedulerStore
{
}

impl TenantLifecycle for EmbeddedRedbProvider {
    type OpenedTenant = OpenedEmbeddedRedbTenant;

    async fn list_tenants(&self) -> Result<Vec<TenantId>> {
        <Self as EmbeddedPersistenceProvider>::list_tenants(self).await
    }

    async fn tenant_exists(&self, tenant_id: &TenantId) -> Result<bool> {
        Self::tenant_exists(self, tenant_id).await
    }

    async fn create_tenant(&self, tenant_id: &TenantId) -> Result<Self::OpenedTenant> {
        Self::create_tenant(self, tenant_id).await
    }

    async fn open_existing_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<Self::OpenedTenant>> {
        Self::open_existing_tenant(self, tenant_id).await
    }

    async fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
        Self::delete_tenant(self, tenant_id).await
    }
}

impl TenantLifecycle for EmbeddedSqliteProvider {
    type OpenedTenant = OpenedEmbeddedSqliteTenant;

    async fn list_tenants(&self) -> Result<Vec<TenantId>> {
        Self::list_tenants(self).await
    }

    async fn tenant_exists(&self, tenant_id: &TenantId) -> Result<bool> {
        Self::tenant_exists(self, tenant_id).await
    }

    async fn create_tenant(&self, tenant_id: &TenantId) -> Result<Self::OpenedTenant> {
        Self::create_tenant(self, tenant_id).await
    }

    async fn open_existing_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<Self::OpenedTenant>> {
        Self::open_existing_tenant(self, tenant_id).await
    }

    async fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
        Self::delete_tenant(self, tenant_id).await
    }
}

macro_rules! impl_provider_lifecycle {
    ($provider:ty, $opened:ty) => {
        impl TenantLifecycle for $provider {
            type OpenedTenant = $opened;

            async fn list_tenants(&self) -> Result<Vec<TenantId>> {
                <$provider>::list_tenants(self).await
            }

            async fn tenant_exists(&self, tenant_id: &TenantId) -> Result<bool> {
                <$provider>::tenant_exists(self, tenant_id).await
            }

            async fn create_tenant(&self, tenant_id: &TenantId) -> Result<Self::OpenedTenant> {
                <$provider>::create_opened_tenant(self, tenant_id).await
            }

            async fn open_existing_tenant(
                &self,
                tenant_id: &TenantId,
            ) -> Result<Option<Self::OpenedTenant>> {
                <$provider>::open_existing_opened_tenant(self, tenant_id).await
            }

            async fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
                <$provider>::delete_tenant(self, tenant_id).await
            }
        }
    };
}

impl_provider_lifecycle!(PostgresProvider, OpenedPostgresTenant);
impl_provider_lifecycle!(MySqlProvider, OpenedMySqlTenant);
impl_provider_lifecycle!(LibsqlReplicaProvider, OpenedLibsqlReplicaTenant);

macro_rules! impl_point_read {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl TenantPointRead for $ty {
                fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
                    <$ty>::get(self, table, id)
                }
            }
        )+
    };
}

impl_point_read!(
    TenantStore,
    SqliteTenantStore,
    PostgresTenantStore,
    MySqlTenantStore,
    LibsqlReplicaTenantStore,
);

macro_rules! impl_point_write {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl TenantPointWrite for $ty {
                fn insert_document(&self, document: &Document) -> Result<CommitEntry> {
                    <$ty>::insert(self, document)
                }

                fn update_document_validated<F>(
                    &self,
                    table: &TableName,
                    id: &DocumentId,
                    patch: &Map<String, Value>,
                    validate: F,
                ) -> Result<CommitEntry>
                where
                    F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
                {
                    <$ty>::update_validated(self, table, id, patch, validate)
                }

                fn delete_document_validated<F>(
                    &self,
                    table: &TableName,
                    id: &DocumentId,
                    validate: F,
                ) -> Result<(CommitEntry, Document)>
                where
                    F: FnOnce(&Document) -> Result<()> + Send + 'static,
                {
                    <$ty>::delete_validated_returning_document(self, table, id, validate)
                }
            }
        )+
    };
}

impl_point_write!(
    TenantStore,
    SqliteTenantStore,
    PostgresTenantStore,
    MySqlTenantStore,
    LibsqlReplicaTenantStore,
);

macro_rules! impl_range_scan {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl TenantRangeScan for $ty {
                fn scan_table_matching_with_filters_cancellable<F>(
                    &self,
                    table: &TableName,
                    filters: &[Filter],
                    check_cancel: &mut dyn FnMut() -> Result<()>,
                    include_document: F,
                ) -> Result<Vec<Document>>
                where
                    F: FnMut(&Document) -> Result<bool>,
                {
                    <$ty>::scan_table_matching_with_filters_cancellable(
                        self,
                        table,
                        filters,
                        check_cancel,
                        include_document,
                    )
                }

                fn scan_table_id_prefix_cancellable(
                    &self,
                    table: &TableName,
                    id_prefix: &str,
                    check_cancel: &mut dyn FnMut() -> Result<()>,
                ) -> Result<Vec<Document>> {
                    <$ty>::scan_table_id_prefix_cancellable(self, table, id_prefix, check_cancel)
                }

                fn scan_table_id_starting_at_cancellable(
                    &self,
                    table: &TableName,
                    start_id: &str,
                    limit: usize,
                    check_cancel: &mut dyn FnMut() -> Result<()>,
                ) -> Result<Vec<Document>> {
                    <$ty>::scan_table_id_starting_at_cancellable(
                        self,
                        table,
                        start_id,
                        limit,
                        check_cancel,
                    )
                }

                fn index_scan_eq_cancellable(
                    &self,
                    table: &TableName,
                    index_name: &str,
                    value: &Value,
                    check_cancel: &mut dyn FnMut() -> Result<()>,
                ) -> Result<Vec<Document>> {
                    <$ty>::index_scan_eq_cancellable(self, table, index_name, value, check_cancel)
                }

                fn index_scan_prefix_cancellable(
                    &self,
                    table: &TableName,
                    index_name: &str,
                    prefix_values: &[Value],
                    check_cancel: &mut dyn FnMut() -> Result<()>,
                ) -> Result<Vec<Document>> {
                    <$ty>::index_scan_prefix_cancellable(
                        self,
                        table,
                        index_name,
                        prefix_values,
                        check_cancel,
                    )
                }

                fn index_scan_range_cancellable(
                    &self,
                    table: &TableName,
                    index_name: &str,
                    start: IndexRangeBound<'_>,
                    end: IndexRangeBound<'_>,
                    check_cancel: &mut dyn FnMut() -> Result<()>,
                ) -> Result<Vec<Document>> {
                    <$ty>::index_scan_range_cancellable(
                        self,
                        table,
                        index_name,
                        start,
                        end,
                        check_cancel,
                    )
                }

                fn index_scan_composite_range_cancellable(
                    &self,
                    table: &TableName,
                    index_name: &str,
                    exact_prefix: &[Value],
                    start: IndexRangeBound<'_>,
                    end: IndexRangeBound<'_>,
                    check_cancel: &mut dyn FnMut() -> Result<()>,
                ) -> Result<Vec<Document>> {
                    <$ty>::index_scan_composite_range_cancellable(
                        self,
                        table,
                        index_name,
                        exact_prefix,
                        start,
                        end,
                        check_cancel,
                    )
                }
            }
        )+
    };
}

impl_range_scan!(
    TenantStore,
    SqliteTenantStore,
    PostgresTenantStore,
    MySqlTenantStore,
    LibsqlReplicaTenantStore,
);

macro_rules! impl_durable_journal {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl DurableJournal for $ty {
                fn journal_progress(&self) -> Result<JournalProgress> {
                    <$ty>::journal_progress(self)
                }

                fn read_durable_journal_from(
                    &self,
                    sequence: SequenceNumber,
                ) -> Result<Vec<TenantEventRecord>> {
                    <$ty>::read_durable_journal_from(self, sequence)
                }

                fn stream_durable_journal(
                    &self,
                    after: SequenceNumber,
                    limit: usize,
                ) -> Result<DurableJournalPage> {
                    <$ty>::stream_durable_journal(self, after, limit)
                }

                fn export_durable_journal_bootstrap(&self) -> Result<DurableJournalBootstrap> {
                    <$ty>::export_durable_journal_bootstrap(self)
                }
            }
        )+
    };
}

impl_durable_journal!(
    TenantStore,
    SqliteTenantStore,
    PostgresTenantStore,
    MySqlTenantStore,
    LibsqlReplicaTenantStore,
);

macro_rules! impl_scheduler_store {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl SchedulerStore for $ty {
                fn scheduled_execution_exists(&self, execution_id: &str) -> Result<bool> {
                    <$ty>::scheduled_execution_exists(self, execution_id)
                }

                fn has_scheduled_work(&self) -> Result<bool> {
                    <$ty>::has_scheduled_work(self)
                }

                fn next_scheduled_work_at(&self) -> Result<Option<Timestamp>> {
                    <$ty>::next_scheduled_work_at(self)
                }
            }
        )+
    };
}

impl_scheduler_store!(
    TenantStore,
    SqliteTenantStore,
    PostgresTenantStore,
    MySqlTenantStore,
    LibsqlReplicaTenantStore,
);

impl ControlPlaneUsage for RedbUsageStorage {}

impl KeyProviderSurface for nimbus_crypto::MasterKeyFileProvider {}
impl KeyProviderSurface for nimbus_crypto::KeyDirectoryProvider {}
#[cfg(feature = "aws-kms")]
impl KeyProviderSurface for nimbus_crypto::AwsKmsKeyProvider {}

impl<T> StorageEngine for T where
    T: TenantPointRead + TenantPointWrite + TenantRangeScan + DurableJournal + SchedulerStore
{
}
