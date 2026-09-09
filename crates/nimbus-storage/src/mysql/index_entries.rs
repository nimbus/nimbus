//! Current-state index keyspace for the MySQL backend.
//!
//! One bootstrap-created `index_entries` table holds one row per maintained
//! index and document: the order-preserving encoded tuple that the shared
//! `crate::index` encoding produces. Index reads range over this keyspace and
//! never depend on per-index InnoDB keys or generated columns on `documents`,
//! so a tenant schema can carry any number of indexes and a schema apply is
//! plain DML inside the journal transaction.
//!
//! This module owns every MySQL statement that touches the keyspace: the
//! index effects of a document write (computed once per write batch and
//! shared with the `index_versions` history in `super::index_versions`), the
//! purge and backfill that a schema change or table delete needs, and the
//! candidate select that an index read runs. The byte range for that select
//! comes from the dialect-neutral planner in `crate::sql::index_keyspace`.

use super::index_versions::record_index_versions_for_mutations_in_session;
use super::*;
use crate::index::encoded_index_tuple_for_document;
use crate::range_bound::borrow_index_range_bound;
use crate::sql::index_keyspace::IndexTupleScanBounds;
use nimbus_core::IndexId;

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

/// Documents read per page while a schema apply backfills an index.
const INDEX_BACKFILL_PAGE_SIZE: usize = 500;

/// Brings the keyspace in step with a schema change for one table identity.
///
/// The diff runs over maintained indexes by `IndexId` and field list: an
/// index that the current schema no longer maintains, or whose fields
/// changed under a reused id, is purged; an index that the previous schema
/// did not maintain with the same fields is backfilled from `documents`.
/// Every statement is DML, so the change commits or rolls back with the
/// schema row and the journal entry in the caller's transaction.
pub(super) async fn reconcile_index_entries_for_table_schema_in_session<C>(
    session: &mut C,
    database_name: &str,
    table_id: &TableId,
    previous: Option<&TableSchema>,
    current: &TableSchema,
) -> Result<()>
where
    C: Queryable,
{
    let previous_indexes: Vec<&IndexDefinition> = previous
        .map(|schema| schema.maintained_indexes().collect())
        .unwrap_or_default();
    let same_definition = |left: &IndexDefinition, right: &IndexDefinition| {
        left.id == right.id && left.fields == right.fields
    };

    for index in &previous_indexes {
        if !current
            .maintained_indexes()
            .any(|candidate| same_definition(candidate, index))
        {
            purge_index_entries_for_index_in_session(session, database_name, table_id, &index.id)
                .await?;
        }
    }

    let added: Vec<&IndexDefinition> = current
        .maintained_indexes()
        .filter(|index| {
            !previous_indexes
                .iter()
                .any(|candidate| same_definition(candidate, index))
        })
        .collect();
    if added.is_empty() {
        return Ok(());
    }
    backfill_index_entries_in_session(session, database_name, table_id, &current.table, &added)
        .await
}

/// Removes every keyspace row for one table identity. Table hard delete and
/// schema delete call this beside their own row deletes.
pub(super) async fn purge_index_entries_for_table_in_session<C>(
    session: &mut C,
    database_name: &str,
    table_id: &TableId,
) -> Result<()>
where
    C: Queryable,
{
    let query = format!(
        "DELETE FROM {} WHERE table_id = ?",
        qualified_table(database_name, "index_entries")
    );
    session
        .exec_drop(query, (table_id.as_str(),))
        .await
        .map_err(map_mysql_error)
}

async fn purge_index_entries_for_index_in_session<C>(
    session: &mut C,
    database_name: &str,
    table_id: &TableId,
    index_id: &IndexId,
) -> Result<()>
where
    C: Queryable,
{
    let query = format!(
        "DELETE FROM {} WHERE table_id = ? AND index_id = ?",
        qualified_table(database_name, "index_entries")
    );
    session
        .exec_drop(query, (table_id.as_str(), index_id.as_str()))
        .await
        .map_err(map_mysql_error)
}

/// Streams the table's documents in id order, one bounded page at a time,
/// and upserts the tuple of every added index for each document that
/// carries one. The page loop keys on the last id read, so it never holds
/// the whole table in memory and never re-reads a row.
async fn backfill_index_entries_in_session<C>(
    session: &mut C,
    database_name: &str,
    table_id: &TableId,
    table: &TableName,
    indexes: &[&IndexDefinition],
) -> Result<()>
where
    C: Queryable,
{
    let page_query = format!(
        "SELECT id, creation_time, update_time, data_json, typed_fields_json \
         FROM {} WHERE table_id = ? AND id > ? ORDER BY id LIMIT {}",
        qualified_table(database_name, "documents"),
        INDEX_BACKFILL_PAGE_SIZE
    );
    let upsert_query = format!(
        "INSERT INTO {} (table_id, index_id, document_id, encoded_tuple)
         VALUES (?, ?, ?, ?)
         ON DUPLICATE KEY UPDATE encoded_tuple = VALUES(encoded_tuple)",
        qualified_table(database_name, "index_entries")
    );
    let mut after_id = String::new();
    loop {
        let rows: Vec<(String, u64, u64, String, String)> = session
            .exec(page_query.as_str(), (table_id.as_str(), after_id.as_str()))
            .await
            .map_err(map_mysql_error)?;
        let Some((last_id, _, _, _, _)) = rows.last() else {
            return Ok(());
        };
        let next_after_id = last_id.clone();
        let mut entries: Vec<(String, String, String, Vec<u8>)> = Vec::new();
        for (id, creation_time, update_time, data_json, typed_fields_json) in rows {
            let document_id = DocumentId::from_str(&id)
                .map_err(|error| Error::Serialization(error.to_string()))?;
            let document = row_to_document(
                table,
                &document_id,
                creation_time,
                update_time,
                data_json,
                typed_fields_json,
            )?;
            for index in indexes {
                if let Some(tuple) = encoded_index_tuple_for_document(&document, index)? {
                    entries.push((
                        table_id.as_str().to_string(),
                        index.id.as_str().to_string(),
                        id.clone(),
                        tuple,
                    ));
                }
            }
        }
        if !entries.is_empty() {
            session
                .exec_batch(upsert_query.as_str(), entries)
                .await
                .map_err(map_mysql_error)?;
        }
        after_id = next_after_id;
    }
}

/// Selects the candidate documents for one index read from the keyspace:
/// the rows of the index whose encoded tuple falls in the planned byte
/// range, joined to their documents, in tuple order. The Rust predicate in
/// `crate::sql::predicate` decides membership after this returns.
pub(super) async fn load_index_candidate_documents_from_session<C>(
    session: &mut C,
    database_name: &str,
    table: &TableName,
    table_schema: &TableSchema,
    index_name: &str,
    exact_prefix: &[Value],
    bounds: crate::range_bound::OwnedIndexRangeBounds,
) -> Result<Vec<Document>>
where
    C: Queryable,
{
    let index = table_schema
        .queryable_indexes()
        .find(|index| index.name == index_name)
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "index '{}' not found for table '{}'",
                index_name,
                table_schema.table.as_str()
            ))
        })?;
    let Some(table_id) = load_table_id_from_session(session, database_name, table).await? else {
        return Ok(Vec::new());
    };
    let crate::range_bound::OwnedIndexRangeBounds { start, end } = bounds;
    let (start_key, end_key) = match IndexTupleScanBounds::for_scan(
        exact_prefix,
        borrow_index_range_bound(&start),
        borrow_index_range_bound(&end),
    )? {
        IndexTupleScanBounds::Empty => return Ok(Vec::new()),
        IndexTupleScanBounds::Bounds { start_key, end_key } => (start_key, end_key),
    };

    let mut clauses = vec![
        "e.table_id = ?".to_string(),
        "e.index_id = ?".to_string(),
        "e.encoded_tuple >= ?".to_string(),
    ];
    let mut params = vec![
        MySqlValue::Bytes(table_id.as_str().as_bytes().to_vec()),
        MySqlValue::Bytes(index.id.as_str().as_bytes().to_vec()),
        MySqlValue::Bytes(start_key),
    ];
    if let Some(end_key) = end_key {
        clauses.push("e.encoded_tuple < ?".to_string());
        params.push(MySqlValue::Bytes(end_key));
    }
    let sql = format!(
        "SELECT c.table_name, d.id, d.creation_time, d.update_time, d.data_json, d.typed_fields_json \
         FROM {} AS e \
         JOIN {} AS d ON d.table_id = e.table_id AND d.id = e.document_id \
         JOIN {} AS c ON c.table_id = d.table_id \
         WHERE {} \
         ORDER BY e.encoded_tuple, e.document_id",
        qualified_table(database_name, "index_entries"),
        qualified_table(database_name, "documents"),
        qualified_table(database_name, "table_catalog"),
        clauses.join(" AND ")
    );
    let rows: Vec<Row> = session
        .exec(sql, Params::Positional(params))
        .await
        .map_err(map_mysql_error)?;
    rows.into_iter()
        .map(|row| {
            let (table_name, id, creation_time, update_time, data_json, typed_fields_json): (
                String,
                String,
                u64,
                u64,
                String,
                String,
            ) = mysql_async::from_row(row);
            let table = TableName::new(table_name)?;
            let id = DocumentId::from_str(&id)
                .map_err(|error| Error::Serialization(error.to_string()))?;
            row_to_document(
                &table,
                &id,
                creation_time,
                update_time,
                data_json,
                typed_fields_json,
            )
        })
        .collect()
}
