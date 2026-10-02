use super::dialect::{MySqlHandle, MySqlSession};
use super::document_versions::get_document_version_at_from_session;
use super::index_entries::IndexTupleMutation;
use super::*;
use crate::sql::index_history::sql_historical_index_facade;
use crate::sql::index_versions::{
    SqlIndexVersionSession, SqlIndexVersionStore, ensure_index_version_storage_format_in_session,
    sql_index_version_facade,
};

impl<H: MySqlHandle> SqlIndexVersionSession for MySqlSession<H> {
    async fn load_metadata_u64(&mut self, database_name: &str, key: &str) -> Result<Option<u64>> {
        load_metadata_u64_from_session(self.driver(), database_name, key).await
    }

    async fn upsert_metadata_u64(
        &mut self,
        database_name: &str,
        key: &str,
        value: u64,
    ) -> Result<()> {
        let query = format!(
            "INSERT INTO {} (key_name, value_u64) VALUES (?, ?)
             ON DUPLICATE KEY UPDATE value_u64 = VALUES(value_u64)",
            qualified_table(database_name, "metadata")
        );
        self.driver()
            .exec_drop(query, (key, value))
            .await
            .map_err(map_mysql_error)
    }

    async fn load_retention_read_floors(
        &mut self,
        database_name: &str,
    ) -> Result<crate::RetentionReadFloors> {
        load_retention_read_floors_from_session(self.driver(), database_name).await
    }

    async fn document_version_at(
        &mut self,
        database_name: &str,
        table: &TableName,
        table_id: &TableId,
        document_id: &DocumentId,
        sequence: SequenceNumber,
    ) -> Result<Option<Document>> {
        get_document_version_at_from_session(
            self.driver(),
            database_name,
            table,
            table_id,
            document_id,
            sequence,
        )
        .await
    }
}

impl SqlIndexVersionStore for MySqlTenantStore {}

sql_index_version_facade!(MySqlTenantStore);
sql_historical_index_facade!(MySqlTenantStore);

/// Closes and opens visibility intervals for the tuple mutations of one write
/// batch. The caller computes the mutations once in
/// `super::index_entries` and applies them to the current-state keyspace in
/// the same session.
pub(super) async fn record_index_versions_for_mutations_in_session<C>(
    session: &mut C,
    database_name: &str,
    sequence: SequenceNumber,
    mutations: &[IndexTupleMutation],
) -> Result<()>
where
    C: Queryable,
{
    if mutations.is_empty() {
        return Ok(());
    }

    ensure_index_version_storage_format_in_session(&mut MySqlSession(&mut *session), database_name)
        .await?;
    let close_query = format!(
        "UPDATE {}
         SET visible_until = ?
         WHERE table_id = ?
           AND index_id = ?
           AND encoded_tuple_hash = ?
           AND encoded_tuple = ?
           AND document_id = ?
           AND visible_until IS NULL",
        qualified_table(database_name, "index_versions")
    );
    let open_query = format!(
        "INSERT INTO {} (
            table_id,
            index_id,
            encoded_tuple_hash,
            encoded_tuple,
            document_id,
            visible_from,
            visible_until
         ) VALUES (?, ?, ?, ?, ?, ?, NULL)",
        qualified_table(database_name, "index_versions")
    );

    for mutation in mutations {
        if let Some(close_tuple) = mutation.close_tuple.as_deref() {
            let tuple_hash = encoded_tuple_hash(close_tuple);
            session
                .exec_drop(
                    close_query.as_str(),
                    (
                        sequence.0,
                        mutation.table_id.as_str(),
                        mutation.index_id.as_str(),
                        tuple_hash.as_slice(),
                        close_tuple,
                        mutation.document_id.as_str(),
                    ),
                )
                .await
                .map_err(map_mysql_error)?;
        }
        if let Some(open_tuple) = mutation.open_tuple.as_deref() {
            let tuple_hash = encoded_tuple_hash(open_tuple);
            session
                .exec_drop(
                    open_query.as_str(),
                    (
                        mutation.table_id.as_str(),
                        mutation.index_id.as_str(),
                        tuple_hash.as_slice(),
                        open_tuple,
                        mutation.document_id.as_str(),
                        sequence.0,
                    ),
                )
                .await
                .map_err(map_mysql_error)?;
        }
    }
    Ok(())
}

fn encoded_tuple_hash(encoded_tuple: &[u8]) -> Vec<u8> {
    Sha256::digest(encoded_tuple).to_vec()
}
