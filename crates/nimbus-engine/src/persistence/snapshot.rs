use std::sync::{Arc, Mutex, MutexGuard};

use nimbus_core::{
    CollectionName, Document, DocumentId, ResourcePathBinding, Result, SequenceNumber, TableId,
    TableName,
};
#[cfg(any(test, feature = "test-hooks"))]
use nimbus_storage::MemoryTenantSnapshot;
#[cfg(feature = "mysql")]
use nimbus_storage::MySqlReadSnapshot;
#[cfg(feature = "postgres")]
use nimbus_storage::PostgresReadSnapshot;
use nimbus_storage::{
    SqliteReadSnapshot, TableIdentitySnapshotEntry, TenantReadSnapshot as RedbReadSnapshot,
};

pub(crate) enum TenantPersistenceSnapshot {
    Redb(RedbReadSnapshot),
    Sqlite(Arc<Mutex<SqliteReadSnapshot>>),
    #[cfg(feature = "libsql")]
    LibsqlReplica(Arc<Mutex<SqliteReadSnapshot>>),
    #[cfg(feature = "postgres")]
    Postgres(PostgresReadSnapshot),
    #[cfg(feature = "mysql")]
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
            Self::Sqlite(snapshot) => lock_sqlite_snapshot(snapshot).table_identities(),
            #[cfg(feature = "libsql")]
            Self::LibsqlReplica(snapshot) => lock_sqlite_snapshot(snapshot).table_identities(),
            #[cfg(feature = "postgres")]
            Self::Postgres(snapshot) => snapshot.table_identities(),
            #[cfg(feature = "mysql")]
            Self::MySql(snapshot) => snapshot.table_identities(),
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(snapshot) => snapshot.table_identities(),
        }
    }

    pub(crate) fn scan_resource_path_bindings(&self) -> Result<Vec<ResourcePathBinding>> {
        match self {
            Self::Redb(snapshot) => snapshot.scan_resource_path_bindings(),
            Self::Sqlite(snapshot) => lock_sqlite_snapshot(snapshot).scan_resource_path_bindings(),
            #[cfg(feature = "libsql")]
            Self::LibsqlReplica(snapshot) => {
                lock_sqlite_snapshot(snapshot).scan_resource_path_bindings()
            }
            #[cfg(feature = "postgres")]
            Self::Postgres(snapshot) => snapshot.scan_resource_path_bindings(),
            #[cfg(feature = "mysql")]
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
            Self::Sqlite(snapshot) => lock_sqlite_snapshot(snapshot).resource_path_binding(locator),
            #[cfg(feature = "libsql")]
            Self::LibsqlReplica(snapshot) => {
                lock_sqlite_snapshot(snapshot).resource_path_binding(locator)
            }
            #[cfg(feature = "postgres")]
            Self::Postgres(snapshot) => snapshot.resource_path_binding(locator),
            #[cfg(feature = "mysql")]
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
            Self::Sqlite(snapshot) => {
                lock_sqlite_snapshot(snapshot).scan_collection_group_bindings(collection_group)
            }
            #[cfg(feature = "libsql")]
            Self::LibsqlReplica(snapshot) => {
                lock_sqlite_snapshot(snapshot).scan_collection_group_bindings(collection_group)
            }
            #[cfg(feature = "postgres")]
            Self::Postgres(snapshot) => snapshot.scan_collection_group_bindings(collection_group),
            #[cfg(feature = "mysql")]
            Self::MySql(snapshot) => snapshot.scan_collection_group_bindings(collection_group),
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(snapshot) => snapshot.scan_collection_group_bindings(collection_group),
        }
    }
}

/// The libSQL replica serves reads from a local SQLite snapshot, so its arms
/// share every SQLite body above. They are written as separate match arms
/// rather than one or-pattern because `cfg` cannot gate half of an or-pattern,
/// and this helper keeps the duplication to the arm head.
fn lock_sqlite_snapshot(
    snapshot: &Arc<Mutex<SqliteReadSnapshot>>,
) -> MutexGuard<'_, SqliteReadSnapshot> {
    snapshot
        .lock()
        .expect("sqlite read snapshot lock should not be poisoned")
}
