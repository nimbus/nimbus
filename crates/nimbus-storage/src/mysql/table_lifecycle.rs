use crate::sql::table_lifecycle::{
    SqlTableLifecycleSession, SqlTableLifecycleStore, SqlTableLifecycleTransaction,
    sql_table_lifecycle_facade,
};

use super::dialect::{MySqlHandle, MySqlSession};
use super::*;

impl<H: MySqlHandle> SqlTableLifecycleSession for MySqlSession<H> {
    async fn purge_table_index_entries(
        &mut self,
        database_name: &str,
        table_id: &TableId,
    ) -> Result<()> {
        super::index_entries::purge_index_entries_for_table_in_session(
            self.driver(),
            database_name,
            table_id,
        )
        .await
    }
}

impl SqlTableLifecycleStore for MySqlTenantStore {
    fn latest_sequence(&self) -> Result<SequenceNumber> {
        self.latest_sequence()
    }
}

impl SqlTableLifecycleTransaction for MySqlWriteTransaction {
    fn after_table_identity_removed(&mut self, table: &TableName) -> Result<()> {
        self.delete_table_schema_entry(table)?;
        self.schema_cache_changed = true;
        Ok(())
    }
}

sql_table_lifecycle_facade!(MySqlTenantStore, MySqlWriteTransaction);
