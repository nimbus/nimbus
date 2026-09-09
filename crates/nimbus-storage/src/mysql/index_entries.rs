//! Current-state index keyspace for the MySQL backend.
//!
//! One bootstrap-created `index_entries` table holds one row per maintained
//! index and document: the order-preserving encoded tuple that the shared
//! `crate::index` encoding produces. Index reads range over this keyspace and
//! never depend on per-index InnoDB keys or generated columns on `documents`,
//! so a tenant schema can carry any number of indexes and a schema apply is
//! plain DML inside the journal transaction.
//!
//! This module owns the index effects of a document write for MySQL: it
//! computes the tuple mutations once per write batch and feeds both the
//! current-state keyspace here and the `index_versions` history in
//! `super::index_versions`.

use super::index_versions::record_index_versions_for_mutations_in_session;
use super::*;
use crate::index::encoded_index_tuple_for_document;

/// The index effect of one document write on one maintained index.
///
/// `close_tuple` is the tuple the previous document version carried and
/// `open_tuple` the tuple the current version carries. Either can be absent
/// when that version has no value for the indexed fields.
pub(super) struct IndexTupleMutation {
    pub(super) table_id: String,
    pub(super) index_id: String,
    pub(super) document_id: String,
    pub(super) close_tuple: Option<Vec<u8>>,
    pub(super) open_tuple: Option<Vec<u8>>,
}

/// Records every index effect of a durable record's events in the open
/// session: the current-state keyspace and the index history.
pub(super) async fn record_index_effects_for_events_in_session<C>(
    session: &mut C,
    database_name: &str,
    sequence: SequenceNumber,
    events: &[TenantEventKind],
) -> Result<()>
where
    C: Queryable,
{
    for event in events {
        if let TenantEventKind::DocumentWrite { writes } = event {
            record_index_effects_for_writes_in_session(session, database_name, sequence, writes)
                .await?;
        }
    }
    Ok(())
}

/// Records the index effects of one write batch in the open session.
pub(super) async fn record_index_effects_for_writes_in_session<C>(
    session: &mut C,
    database_name: &str,
    sequence: SequenceNumber,
    writes: &[WriteOp],
) -> Result<()>
where
    C: Queryable,
{
    if writes.is_empty() {
        return Ok(());
    }
    let mutations = index_tuple_mutations_for_writes(session, database_name, writes).await?;
    if mutations.is_empty() {
        return Ok(());
    }
    maintain_index_entries_for_mutations_in_session(session, database_name, &mutations).await?;
    record_index_versions_for_mutations_in_session(session, database_name, sequence, &mutations)
        .await
}

async fn index_tuple_mutations_for_writes<C>(
    session: &mut C,
    database_name: &str,
    writes: &[WriteOp],
) -> Result<Vec<IndexTupleMutation>>
where
    C: Queryable,
{
    let mut schemas: HashMap<TableName, Option<TableSchema>> = HashMap::new();
    let mut mutations = Vec::new();
    for write in writes {
        let table_schema = match schemas.get(&write.table) {
            Some(table_schema) => table_schema,
            None => {
                let loaded =
                    load_table_schema_from_session(session, database_name, &write.table).await?;
                schemas.entry(write.table.clone()).or_insert(loaded)
            }
        };
        let Some(table_schema) = table_schema else {
            continue;
        };
        for index in table_schema.maintained_indexes() {
            let close_tuple = write
                .previous
                .as_ref()
                .map(|previous| encoded_index_tuple_for_document(previous, index))
                .transpose()?
                .flatten();
            let open_tuple = write
                .current
                .as_ref()
                .map(|current| encoded_index_tuple_for_document(current, index))
                .transpose()?
                .flatten();
            if close_tuple.is_some() || open_tuple.is_some() {
                mutations.push(IndexTupleMutation {
                    table_id: write.table_id.as_str().to_string(),
                    index_id: index.id.as_str().to_string(),
                    document_id: write.doc_id.to_string(),
                    close_tuple,
                    open_tuple,
                });
            }
        }
    }
    Ok(mutations)
}

/// Applies tuple mutations to the current-state keyspace. A document with an
/// open tuple owns exactly one row per index, so an open tuple is an upsert
/// by primary key and a closed tuple without a replacement is a delete.
async fn maintain_index_entries_for_mutations_in_session<C>(
    session: &mut C,
    database_name: &str,
    mutations: &[IndexTupleMutation],
) -> Result<()>
where
    C: Queryable,
{
    let upsert_query = format!(
        "INSERT INTO {} (table_id, index_id, document_id, encoded_tuple)
         VALUES (?, ?, ?, ?)
         ON DUPLICATE KEY UPDATE encoded_tuple = VALUES(encoded_tuple)",
        qualified_table(database_name, "index_entries")
    );
    let delete_query = format!(
        "DELETE FROM {} WHERE table_id = ? AND index_id = ? AND document_id = ?",
        qualified_table(database_name, "index_entries")
    );
    for mutation in mutations {
        match (&mutation.open_tuple, &mutation.close_tuple) {
            (Some(open_tuple), _) => {
                session
                    .exec_drop(
                        upsert_query.as_str(),
                        (
                            mutation.table_id.as_str(),
                            mutation.index_id.as_str(),
                            mutation.document_id.as_str(),
                            open_tuple.as_slice(),
                        ),
                    )
                    .await
                    .map_err(map_mysql_error)?;
            }
            (None, Some(_)) => {
                session
                    .exec_drop(
                        delete_query.as_str(),
                        (
                            mutation.table_id.as_str(),
                            mutation.index_id.as_str(),
                            mutation.document_id.as_str(),
                        ),
                    )
                    .await
                    .map_err(map_mysql_error)?;
            }
            (None, None) => {}
        }
    }
    Ok(())
}
