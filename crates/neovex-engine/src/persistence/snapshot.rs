use std::sync::{Arc, Mutex};

use neovex_core::{Document, DocumentId, Result, SequenceNumber, TableName};
use neovex_storage::{
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
        match self {
            Self::Redb(snapshot) => snapshot.applied_sequence(),
            Self::Sqlite(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .applied_sequence(),
            Self::LibsqlReplica(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .applied_sequence(),
            Self::Postgres(snapshot) => snapshot.applied_sequence(),
            Self::MySql(snapshot) => snapshot.applied_sequence(),
        }
    }

    pub(crate) fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        match self {
            Self::Redb(snapshot) => snapshot.get(table, id),
            Self::Sqlite(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .get(table, id),
            Self::LibsqlReplica(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .get(table, id),
            Self::Postgres(snapshot) => snapshot.get(table, id),
            Self::MySql(snapshot) => snapshot.get(table, id),
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
        match self {
            Self::Redb(snapshot) => {
                snapshot.scan_table_matching_cancellable(table, check_cancel, include_document)
            }
            Self::Sqlite(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .scan_table_matching_with_filters_cancellable(
                    table,
                    &[],
                    check_cancel,
                    include_document,
                ),
            Self::LibsqlReplica(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .scan_table_matching_with_filters_cancellable(
                    table,
                    &[],
                    check_cancel,
                    include_document,
                ),
            Self::Postgres(snapshot) => snapshot.scan_table_matching_with_filters_cancellable(
                table,
                &[],
                check_cancel,
                include_document,
            ),
            Self::MySql(snapshot) => snapshot.scan_table_matching_with_filters_cancellable(
                table,
                &[],
                check_cancel,
                include_document,
            ),
        }
    }
}
