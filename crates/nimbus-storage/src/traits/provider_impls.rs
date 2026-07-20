//! Concrete provider implementations for the storage capability traits.

use nimbus_core::{
    CollectionName, CommitEntry, Document, DocumentId, Filter, ResourcePathBinding, Result,
    SequenceNumber, TableId, TableName, TenantEventRecord, TenantId, Timestamp,
};
use serde_json::{Map, Value};

use crate::IndexRangeBound;
use crate::async_storage::{
    EmbeddedRedbProvider, EmbeddedSqliteProvider, OpenedEmbeddedRedbTenant,
    OpenedEmbeddedSqliteTenant,
};
use crate::libsql::OpenedLibsqlReplicaTenant;
use crate::mysql::OpenedMySqlTenant;
use crate::postgres::OpenedPostgresTenant;
use crate::retention::RetentionGcConfig;
use crate::store::{
    DurableJournalBootstrap, DurableJournalPage, JournalProgress, PointInTimeRestoreArchive,
    PointInTimeRestoreTarget, TenantReadSnapshot,
};
use crate::{
    LibsqlReplicaProvider, LibsqlReplicaTenantStore, MemoryTenantProvider, MemoryTenantSnapshot,
    MemoryTenantStore, MySqlProvider, MySqlReadSnapshot, MySqlTenantStore, OpenedMemoryTenant,
    PostgresProvider, PostgresReadSnapshot, PostgresTenantStore, RedbUsageStorage,
    SqliteReadSnapshot, SqliteTenantStore, TenantStore,
};

use super::object_metadata::{
    delete_multipart_upload_for_store, delete_object_manifest_for_store,
    get_multipart_upload_for_store, get_object_manifest_for_store,
    list_multipart_uploads_for_store, list_object_manifests_for_store,
    put_multipart_upload_for_store, put_object_manifest_for_store,
};
use super::{
    CommitterLease, CommitterLeaseResult, CommitterLeaseStore, ControlPlaneUsage, DurableJournal,
    KeyProviderSurface, MaterializedRebuild, ObjectManifest, ObjectMetaStore,
    ObjectMultipartUpload, ResourcePathScan, ResourcePathSnapshot, SchedulerStore, StorageEngine,
    TenantLifecycle, TenantPointRead, TenantPointWrite, TenantRangeScan,
};

macro_rules! impl_committer_lease_store {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl CommitterLeaseStore for $ty {
                fn read_committer_lease(&self) -> Result<Option<CommitterLease>> {
                    <$ty>::read_committer_lease(self)
                }

                fn acquire_committer_lease(
                    &self,
                    owner_id: &str,
                    lease_duration: std::time::Duration,
                ) -> CommitterLeaseResult<CommitterLease> {
                    <$ty>::acquire_committer_lease(self, owner_id, lease_duration)
                }

                fn renew_committer_lease(
                    &self,
                    owner_id: &str,
                    epoch: u64,
                    lease_duration: std::time::Duration,
                ) -> CommitterLeaseResult<CommitterLease> {
                    <$ty>::renew_committer_lease(self, owner_id, epoch, lease_duration)
                }
            }
        )+
    };
}

impl_committer_lease_store!(
    PostgresTenantStore,
    MySqlTenantStore,
    LibsqlReplicaTenantStore
);

trait FencedDurableApply {
    fn fenced_append_and_apply_durable_records_batch(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_previous: SequenceNumber,
        records: &[TenantEventRecord],
    ) -> CommitterLeaseResult<()>;
}

impl FencedDurableApply for PostgresTenantStore {
    fn fenced_append_and_apply_durable_records_batch(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_previous: SequenceNumber,
        records: &[TenantEventRecord],
    ) -> CommitterLeaseResult<()> {
        PostgresTenantStore::fenced_append_and_apply_durable_records_batch(
            self,
            owner_id,
            epoch,
            expected_previous,
            records,
        )
    }
}

impl FencedDurableApply for LibsqlReplicaTenantStore {
    fn fenced_append_and_apply_durable_records_batch(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_previous: SequenceNumber,
        records: &[TenantEventRecord],
    ) -> CommitterLeaseResult<()> {
        LibsqlReplicaTenantStore::fenced_append_and_apply_durable_records_batch(
            self,
            owner_id,
            epoch,
            expected_previous,
            records,
        )
    }
}

macro_rules! impl_unsupported_fenced_durable_apply {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl FencedDurableApply for $ty {
                fn fenced_append_and_apply_durable_records_batch(
                    &self,
                    _owner_id: &str,
                    _epoch: u64,
                    _expected_previous: SequenceNumber,
                    _records: &[TenantEventRecord],
                ) -> CommitterLeaseResult<()> {
                    Err(super::CommitterLeaseError::Unsupported)
                }
            }
        )+
    };
}

impl_unsupported_fenced_durable_apply!(
    TenantStore,
    SqliteTenantStore,
    MySqlTenantStore,
    MemoryTenantStore,
);

impl TenantLifecycle for EmbeddedRedbProvider {
    type OpenedTenant = OpenedEmbeddedRedbTenant;

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
impl_provider_lifecycle!(MemoryTenantProvider, OpenedMemoryTenant);

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
    MemoryTenantStore,
    TenantReadSnapshot,
    SqliteReadSnapshot,
    PostgresReadSnapshot,
    MySqlReadSnapshot,
    MemoryTenantSnapshot,
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
    MemoryTenantStore,
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
    MemoryTenantStore,
    TenantReadSnapshot,
    SqliteReadSnapshot,
    PostgresReadSnapshot,
    MySqlReadSnapshot,
    MemoryTenantSnapshot,
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

                  fn latest_sequence(&self) -> Result<SequenceNumber> {
                      <$ty>::latest_sequence(self)
                  }

                  fn applied_sequence(&self) -> Result<SequenceNumber> {
                      <$ty>::applied_sequence(self)
                  }

                  fn read_commit_log_from(&self, sequence: SequenceNumber) -> Result<Vec<CommitEntry>> {
                      <$ty>::read_commit_log_from(self, sequence)
                  }

                  fn recover_durable_journal(&self) -> Result<JournalProgress> {
                      <$ty>::recover_durable_journal(self)
                  }

                  fn append_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
                      <$ty>::append_durable_records_batch(self, records)
                  }

                  fn apply_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
                      <$ty>::apply_durable_records_batch(self, records)
                  }

                  fn fenced_append_and_apply_durable_records_batch(
                      &self,
                      owner_id: &str,
                      epoch: u64,
                      expected_previous: SequenceNumber,
                      records: &[TenantEventRecord],
                  ) -> CommitterLeaseResult<()> {
                      FencedDurableApply::fenced_append_and_apply_durable_records_batch(
                          self,
                          owner_id,
                          epoch,
                          expected_previous,
                          records,
                      )
                  }

                  fn export_point_in_time_restore_archive(
                      &self,
                      target: PointInTimeRestoreTarget,
                      retention_config: RetentionGcConfig,
                  ) -> Result<PointInTimeRestoreArchive> {
                      <$ty>::export_point_in_time_restore_archive(self, target, retention_config)
                  }

                  fn import_point_in_time_restore_archive(
                      &self,
                      archive: &PointInTimeRestoreArchive,
                  ) -> Result<JournalProgress> {
                      <$ty>::import_point_in_time_restore_archive(self, archive)
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
    MemoryTenantStore,
);

macro_rules! impl_resource_path_snapshot {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl ResourcePathSnapshot for $ty {
                fn scan_resource_path_bindings(&self) -> Result<Vec<ResourcePathBinding>> {
                    <$ty>::scan_resource_path_bindings(self)
                }
            }
        )+
    };
}

impl_resource_path_snapshot!(
    TenantReadSnapshot,
    SqliteReadSnapshot,
    PostgresReadSnapshot,
    MySqlReadSnapshot,
    MemoryTenantSnapshot,
);

macro_rules! impl_resource_path_scan {
    ($(($ty:ty, $snapshot:ty)),+ $(,)?) => {
        $(
            impl ResourcePathScan for $ty {
                type Snapshot = $snapshot;

                fn read_snapshot(&self) -> Result<Self::Snapshot> {
                    <$ty>::read_snapshot(self)
                }

                fn table_id(&self, table: &TableName) -> Result<Option<TableId>> {
                    <$ty>::table_id(self, table)
                }

                fn scan_collection_group_bindings(
                    &self,
                    collection_group: &CollectionName,
                ) -> Result<Vec<ResourcePathBinding>> {
                    <$ty>::scan_collection_group_bindings(self, collection_group)
                }
            }
        )+
    };
}

impl_resource_path_scan!(
    (TenantStore, TenantReadSnapshot),
    (SqliteTenantStore, SqliteReadSnapshot),
    (PostgresTenantStore, PostgresReadSnapshot),
    (MySqlTenantStore, MySqlReadSnapshot),
    // libsql's local read replica cache is backed by a SQLite store, so its
    // read snapshot is a `SqliteReadSnapshot` rather than a distinct type.
    (LibsqlReplicaTenantStore, SqliteReadSnapshot),
    (MemoryTenantStore, MemoryTenantSnapshot),
);

macro_rules! impl_materialized_rebuild {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl MaterializedRebuild for $ty {
                fn scan_table_matching_cancellable<F>(
                    &self,
                    table: &TableName,
                    check_cancel: &mut dyn FnMut() -> Result<()>,
                    include_document: F,
                ) -> Result<Vec<Document>>
                where
                    F: FnMut(&Document) -> Result<bool>,
                {
                    <$ty>::scan_table_matching_cancellable(self, table, check_cancel, include_document)
                }
            }
        )+
    };
}

impl_materialized_rebuild!(
    TenantStore,
    SqliteTenantStore,
    PostgresTenantStore,
    MySqlTenantStore,
    LibsqlReplicaTenantStore,
    MemoryTenantStore,
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
    MemoryTenantStore,
);

macro_rules! impl_object_meta_store {
      ($($ty:ty),+ $(,)?) => {
          $(
              impl ObjectMetaStore for $ty {
                  fn put_object_manifest(&self, manifest: &ObjectManifest) -> Result<CommitEntry> {
                      put_object_manifest_for_store(self, manifest)
                  }

                  fn get_object_manifest(
                      &self,
                      bucket: &str,
                      key: &str,
                  ) -> Result<Option<ObjectManifest>> {
                      get_object_manifest_for_store(self, bucket, key)
                  }

                  fn delete_object_manifest(
                      &self,
                      bucket: &str,
                      key: &str,
                  ) -> Result<Option<(CommitEntry, ObjectManifest)>> {
                      delete_object_manifest_for_store(self, bucket, key)
                  }

                  fn list_object_manifests(
                      &self,
                      bucket: &str,
                      prefix: &str,
                      limit: usize,
                  ) -> Result<Vec<ObjectManifest>> {
                      list_object_manifests_for_store(self, bucket, prefix, limit)
                  }

                  fn put_multipart_upload(
                      &self,
                      upload: &ObjectMultipartUpload,
                  ) -> Result<CommitEntry> {
                      put_multipart_upload_for_store(self, upload)
                  }

                  fn get_multipart_upload(
                      &self,
                      upload_id: &str,
                  ) -> Result<Option<ObjectMultipartUpload>> {
                      get_multipart_upload_for_store(self, upload_id)
                  }

                  fn delete_multipart_upload(
                      &self,
                      upload_id: &str,
                  ) -> Result<Option<(CommitEntry, ObjectMultipartUpload)>> {
                      delete_multipart_upload_for_store(self, upload_id)
                  }

                  fn list_multipart_uploads(
                      &self,
                      bucket: &str,
                      prefix: &str,
                      limit: usize,
                  ) -> Result<Vec<ObjectMultipartUpload>> {
                      list_multipart_uploads_for_store(self, bucket, prefix, limit)
                  }
              }
          )+
      };
  }

impl_object_meta_store!(
    TenantStore,
    SqliteTenantStore,
    PostgresTenantStore,
    MySqlTenantStore,
    LibsqlReplicaTenantStore,
    MemoryTenantStore,
);

impl ControlPlaneUsage for RedbUsageStorage {}

impl KeyProviderSurface for nimbus_crypto::MasterKeyFileProvider {}
impl KeyProviderSurface for nimbus_crypto::KeyDirectoryProvider {}
#[cfg(feature = "aws-kms")]
impl KeyProviderSurface for nimbus_crypto::AwsKmsKeyProvider {}

/// StorageEngine includes ObjectMetaStore so object manifests use the same stores.
impl<T> StorageEngine for T where
    T: TenantPointRead
        + TenantPointWrite
        + TenantRangeScan
        + DurableJournal
        + SchedulerStore
        + ObjectMetaStore
{
}
