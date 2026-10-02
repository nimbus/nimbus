use crate::sql::table_lifecycle::{
    SqlTableLifecycleSession, SqlTableLifecycleStore, SqlTableLifecycleTransaction,
    sql_table_lifecycle_facade,
};

use super::dialect::{PostgresHandle, PostgresSession};
use super::*;

impl<H: PostgresHandle> SqlTableLifecycleSession for PostgresSession<H> {
    // PostgreSQL index entries live in native indexes on `documents`, so the
    // document delete already removed them.
    async fn purge_table_index_entries(&mut self, _: &str, _: &TableId) -> Result<()> {
        Ok(())
    }
}

impl SqlTableLifecycleStore for PostgresTenantStore {
    fn latest_sequence(&self) -> Result<SequenceNumber> {
        self.latest_sequence()
    }
}

impl SqlTableLifecycleTransaction for PostgresWriteTransaction {
    fn after_table_identity_removed(&mut self, table: &TableName) -> Result<()> {
        if let Some(previous) = self.load_table_schema(table)? {
            self.drop_table_indexes(&previous)?;
        }
        self.delete_table_schema_entry(table)?;
        self.notification.schema_changed = true;
        self.schema_cache_changed = true;
        Ok(())
    }
}

sql_table_lifecycle_facade!(PostgresTenantStore, PostgresWriteTransaction);
