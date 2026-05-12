use std::sync::{Arc, Mutex};

use nimbus_core::{
    CollectionName, Document, DocumentId, ResourcePathBinding, Result, SequenceNumber, TableName,
};
use nimbus_storage::{
    MySqlReadSnapshot, PostgresReadSnapshot, SqliteReadSnapshot,
    TenantReadSnapshot as RedbReadSnapshot,
};

pub(crate) enum TenantPersistenceSnapshot {
    Redb(RedbReadSnapshot),
    Sqlite(Arc<Mutex<SqliteReadSnapshot>>),
    LibsqlReplica(Arc<Mutex<SqliteReadSnapshot>>),
    Postgres(PostgresReadSnapshot),
    MySql(MySqlReadSnapshot),
}

impl TenantPersistenceSnapshot {
    pub(crate) fn applied_sequence(&self) -> Result<SequenceNumber> {
        match_tenant_persistence_snapshot!(self, |snapshot| snapshot.applied_sequence())
    }

    pub(crate) fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        match_tenant_persistence_snapshot!(self, |snapshot| snapshot.get(table, id))
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
        }
    }
}
