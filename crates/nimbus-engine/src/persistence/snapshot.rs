use std::sync::{Arc, Mutex};

use nimbus_core::{
    CollectionName, Document, DocumentId, ResourcePathBinding, Result, SequenceNumber, TableId,
    TableName,
};
#[cfg(any(test, feature = "test-hooks"))]
use nimbus_storage::MemoryTenantSnapshot;
use nimbus_storage::{
    MySqlReadSnapshot, PostgresReadSnapshot, SqliteReadSnapshot, TableIdentitySnapshotEntry,
    TenantReadSnapshot as RedbReadSnapshot,
};

pub(crate) enum TenantPersistenceSnapshot {
    Redb(RedbReadSnapshot),
    Sqlite(Arc<Mutex<SqliteReadSnapshot>>),
    LibsqlReplica(Arc<Mutex<SqliteReadSnapshot>>),
    Postgres(PostgresReadSnapshot),
    MySql(MySqlReadSnapshot),
    #[cfg(any(test, feature = "test-hooks"))]
    Memory(MemoryTenantSnapshot),
}

impl TenantPersistenceSnapshot {
    pub(crate) fn applied_sequence(&self) -> Result<SequenceNumber> {
        match_tenant_persistence_snapshot!(self, |snapshot| snapshot.applied_sequence())
    }

    pub(crate) fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        match_tenant_persistence_snapshot!(self, |snapshot| snapshot.get(table, id))
    }

    pub(crate) fn table_id(&self, table: &TableName) -> Result<Option<TableId>> {
        match_tenant_persistence_snapshot!(self, |snapshot| snapshot.table_id(table))
    }

    pub(crate) fn table_identities(&self) -> Result<Vec<TableIdentitySnapshotEntry>> {
        match self {
            Self::Redb(snapshot) => snapshot.table_identities(),
            Self::Sqlite(snapshot) | Self::LibsqlReplica(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .table_identities(),
            Self::Postgres(snapshot) => snapshot.table_identities(),
            Self::MySql(snapshot) => snapshot.table_identities(),
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(snapshot) => snapshot.table_identities(),
        }
    }

    pub(crate) fn scan_resource_path_bindings(&self) -> Result<Vec<ResourcePathBinding>> {
        match self {
            Self::Redb(snapshot) => snapshot.scan_resource_path_bindings(),
            Self::Sqlite(snapshot) | Self::LibsqlReplica(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .scan_resource_path_bindings(),
            Self::Postgres(snapshot) => snapshot.scan_resource_path_bindings(),
            Self::MySql(snapshot) => snapshot.scan_resource_path_bindings(),
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(snapshot) => snapshot.scan_resource_path_bindings(),
        }
    }

    pub(crate) fn resource_path_binding(
        &self,
        locator: &nimbus_core::DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        match self {
            Self::Redb(snapshot) => snapshot.resource_path_binding(locator),
            Self::Sqlite(snapshot) | Self::LibsqlReplica(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .resource_path_binding(locator),
            Self::Postgres(snapshot) => snapshot.resource_path_binding(locator),
            Self::MySql(snapshot) => snapshot.resource_path_binding(locator),
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(snapshot) => snapshot.resource_path_binding(locator),
        }
    }

    pub(crate) fn scan_table_matching_cancellable<F>(
        &self,
        table: &TableName,
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        match_tenant_persistence_snapshot!(self, |snapshot| {
            snapshot.scan_table_matching_with_filters_cancellable(
                table,
                &[],
                check_cancel,
                include_document,
            )
        })
    }

    pub(crate) fn scan_collection_group_bindings(
        &self,
        collection_group: &CollectionName,
    ) -> Result<Vec<ResourcePathBinding>> {
        match self {
            Self::Redb(snapshot) => snapshot.scan_collection_group_bindings(collection_group),
            Self::Sqlite(snapshot) | Self::LibsqlReplica(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .scan_collection_group_bindings(collection_group),
            Self::Postgres(snapshot) => snapshot.scan_collection_group_bindings(collection_group),
            Self::MySql(snapshot) => snapshot.scan_collection_group_bindings(collection_group),
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(snapshot) => snapshot.scan_collection_group_bindings(collection_group),
        }
    }
}
