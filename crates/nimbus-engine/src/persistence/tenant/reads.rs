use super::*;

impl TenantPersistence {
    pub(crate) fn check_fault(&self, point: FaultPoint) -> Result<()> {
        match self {
            Self::Redb(store) => store.check_fault(point),
            Self::Sqlite(store) => store.check_fault(point),
            Self::LibsqlReplica(store) => store.check_fault(point),
            Self::Postgres(store) => store.check_fault(point),
            Self::MySql(store) => store.check_fault(point),
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(store) => store.check_fault(point),
        }
    }

    pub(crate) fn read_snapshot(&self) -> Result<TenantPersistenceSnapshot> {
        match self {
            Self::Redb(store) => store.read_snapshot().map(TenantPersistenceSnapshot::Redb),
            Self::Sqlite(store) => store
                .read_snapshot()
                .map(|snapshot| TenantPersistenceSnapshot::Sqlite(Arc::new(Mutex::new(snapshot)))),
            Self::LibsqlReplica(store) => store.read_snapshot().map(|snapshot| {
                TenantPersistenceSnapshot::LibsqlReplica(Arc::new(Mutex::new(snapshot)))
            }),
            Self::Postgres(store) => store
                .read_snapshot()
                .map(TenantPersistenceSnapshot::Postgres),
            Self::MySql(store) => store.read_snapshot().map(TenantPersistenceSnapshot::MySql),
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(store) => store.read_snapshot().map(TenantPersistenceSnapshot::Memory),
        }
    }

    pub(crate) fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        match_tenant_persistence!(self, |store| store.get(table, id))
    }

    pub(crate) fn table_id(&self, table: &TableName) -> Result<Option<TableId>> {
        match_tenant_persistence!(self, |store| store.table_id(table))
    }

    pub(crate) fn libsql_replica_freshness_stats(&self) -> Option<LibsqlReplicaFreshnessStats> {
        match self {
            Self::LibsqlReplica(store) => store.replica_freshness_stats().ok(),
            Self::Redb(_) | Self::Sqlite(_) | Self::Postgres(_) | Self::MySql(_) => None,
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(_) => None,
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
        match_tenant_persistence!(self, |store| {
            store.scan_table_matching_cancellable(table, check_cancel, include_document)
        })
    }

    pub(crate) fn scan_table_id_prefix_cancellable(
        &self,
        table: &TableName,
        id_prefix: &str,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match_tenant_persistence!(self, |store| {
            store.scan_table_id_prefix_cancellable(table, id_prefix, check_cancel)
        })
    }

    pub(crate) fn scan_table_id_starting_at_cancellable(
        &self,
        table: &TableName,
        start_id: &str,
        limit: usize,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match_tenant_persistence!(self, |store| {
            store.scan_table_id_starting_at_cancellable(table, start_id, limit, check_cancel)
        })
    }
}
