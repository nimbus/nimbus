use super::*;

impl TenantPersistence {
    delegate_store_method!(fn load_schema(&self) -> Result<Schema>);

    pub(crate) fn replace_table_schema(&self, table_schema: &TableSchema) -> Result<()> {
        match self {
            Self::Redb(store) => store.replace_table_schema(table_schema),
            Self::Sqlite(store) => store.replace_table_schema(table_schema),
            Self::LibsqlReplica(store) => store.replace_table_schema(table_schema),
            Self::Postgres(store) => store.replace_table_schema(table_schema),
            Self::MySql(store) => store.replace_table_schema(table_schema),
        }
    }

    pub(crate) fn delete_table_schema(&self, table: &TableName) -> Result<()> {
        match self {
            Self::Redb(store) => store.delete_table_schema(table),
            Self::Sqlite(store) => store.delete_table_schema(table),
            Self::LibsqlReplica(store) => store.delete_table_schema(table),
            Self::Postgres(store) => store.delete_table_schema(table),
            Self::MySql(store) => store.delete_table_schema(table),
        }
    }

    pub(crate) fn invalidate_schema_cache(&self) {
        match self {
            Self::Postgres(store) => store.invalidate_schema_cache(),
            Self::MySql(store) => store.invalidate_schema_cache(),
            Self::Redb(_) | Self::Sqlite(_) | Self::LibsqlReplica(_) => {}
        }
    }
}
