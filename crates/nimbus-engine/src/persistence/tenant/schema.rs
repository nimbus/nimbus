use super::*;

impl TenantPersistence {
    delegate_store_method!(fn load_schema(&self) -> Result<Schema>);

    pub(crate) fn replace_table_schema(&self, table_schema: &TableSchema) -> Result<()> {
        match_tenant_persistence!(self, |store| store.replace_table_schema(table_schema))
    }

    pub(crate) fn delete_table_schema(&self, table: &TableName) -> Result<()> {
        match_tenant_persistence!(self, |store| store.delete_table_schema(table))
    }

    pub(crate) fn invalidate_schema_cache(&self) {
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres(store) => store.invalidate_schema_cache(),
            #[cfg(feature = "mysql")]
            Self::MySql(store) => store.invalidate_schema_cache(),
            Self::Redb(_) | Self::Sqlite(_) => {}
            #[cfg(feature = "libsql")]
            Self::LibsqlReplica(_) => {}
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(_) => {}
        }
    }
}
