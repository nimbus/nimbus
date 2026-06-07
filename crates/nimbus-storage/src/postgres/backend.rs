use super::table_lifecycle::{
    activate_hidden_table_identity_in_session, hard_delete_table_identity_in_session,
    mark_table_deleting_in_session, stage_hidden_table_identity_in_session,
};
use super::*;
use crate::postgres::document_versions::{
    record_document_versions_for_events_in_session, record_document_versions_for_writes_in_session,
};
use crate::postgres::index_versions::{
    record_index_versions_for_events_in_session, record_index_versions_for_writes_in_session,
};
use crate::table_identity::{
    DEFAULT_TABLE_NAMESPACE, deleting_table_namespace, hidden_table_namespace,
};

pub(super) fn cached_schema(schema_cache: &RwLock<Option<Schema>>) -> Option<Schema> {
    schema_cache.read().ok().and_then(|guard| guard.clone())
}

pub(super) fn publish_schema_cache(schema_cache: &RwLock<Option<Schema>>, schema: &Schema) {
    if let Ok(mut guard) = schema_cache.write() {
        *guard = Some(schema.clone());
    }
}

pub(super) fn invalidate_schema_cache_handle(schema_cache: &RwLock<Option<Schema>>) {
    if let Ok(mut guard) = schema_cache.write() {
        *guard = None;
    }
}

pub(super) async fn load_schema_from_session<C>(session: &C, schema_name: &str) -> Result<Schema>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT schema_json FROM {} ORDER BY table_name",
        qualified_table(schema_name, "schemas")
    );
    let rows = session
        .query(query.as_str(), &[])
        .await
        .map_err(map_postgres_error)?;
    let mut schema = Schema::default();
    for row in rows {
        let table_schema: TableSchema = serde_json::from_str(row.get::<_, String>(0).as_str())
            .map_err(|error| Error::Serialization(error.to_string()))?;
        schema
            .tables
            .insert(table_schema.table.clone(), table_schema);
    }
    Ok(schema)
}

pub(super) async fn load_journal_progress_from_session<C>(
    session: &C,
    schema_name: &str,
) -> Result<JournalProgress>
where
    C: GenericClient + Sync,
{
    let durable_head = load_latest_sequence_from_session(session, schema_name).await?;
    let applied_head = load_metadata_u64_from_session(session, schema_name, APPLIED_SEQUENCE_KEY)
        .await?
        .map(SequenceNumber)
        .unwrap_or(SequenceNumber(0));
    Ok(JournalProgress {
        durable_head,
        applied_head,
    })
}

pub(super) async fn load_latest_sequence_from_session<C>(
    session: &C,
    schema_name: &str,
) -> Result<SequenceNumber>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT COALESCE(MAX(sequence), 0) FROM {}",
        qualified_table(schema_name, "commit_log")
    );
    let row = session
        .query_one(query.as_str(), &[])
        .await
        .map_err(map_postgres_error)?;
    sequence_number_from_i64(row.get::<_, i64>(0))
}

pub(super) async fn load_documents_from_session<C>(
    session: &C,
    schema_name: &str,
    table: Option<&TableName>,
) -> Result<Vec<Document>>
where
    C: GenericClient + Sync,
{
    let query = if table.is_some() {
        format!(
            "SELECT c.table_name, d.id, d.creation_time, d.update_time, d.data_json, d.typed_fields_json \
             FROM {} AS d \
             JOIN {} AS c ON c.table_id = d.table_id \
             WHERE c.namespace = 'default' AND c.table_name = $1 \
             ORDER BY d.id",
            qualified_table(schema_name, "documents"),
            qualified_table(schema_name, "table_catalog")
        )
    } else {
        format!(
            "SELECT c.table_name, d.id, d.creation_time, d.update_time, d.data_json, d.typed_fields_json \
             FROM {} AS d \
             JOIN {} AS c ON c.table_id = d.table_id \
             WHERE c.namespace = 'default' \
             ORDER BY c.table_name, d.id",
            qualified_table(schema_name, "documents"),
            qualified_table(schema_name, "table_catalog")
        )
    };

    let rows = match table {
        Some(table) => session
            .query(query.as_str(), &[&table.as_str()])
            .await
            .map_err(map_postgres_error)?,
        None => session
            .query(query.as_str(), &[])
            .await
            .map_err(map_postgres_error)?,
    };

    rows.into_iter().map(row_to_document).collect()
}

pub(super) async fn load_document_from_session<C>(
    session: &C,
    schema_name: &str,
    table: &TableName,
    id: &DocumentId,
) -> Result<Option<Document>>
where
    C: GenericClient + Sync,
{
    let Some(table_id) = load_table_id_from_session(session, schema_name, table).await? else {
        return Ok(None);
    };
    let query = format!(
        "SELECT $1::text AS table_name, id, creation_time, update_time, data_json, typed_fields_json \
         FROM {} \
         WHERE table_id = $2 AND id = $3",
        qualified_table(schema_name, "documents")
    );
    session
        .query_opt(
            query.as_str(),
            &[&table.as_str(), &table_id.as_str(), &id.to_string()],
        )
        .await
        .map_err(map_postgres_error)?
        .map(row_to_document)
        .transpose()
}

pub(super) async fn load_document_by_table_id_from_session<C>(
    session: &C,
    schema_name: &str,
    table: &TableName,
    table_id: &TableId,
    id: &DocumentId,
) -> Result<Option<Document>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT $1::text AS table_name, id, creation_time, update_time, data_json, typed_fields_json \
         FROM {} \
         WHERE table_id = $2 AND id = $3",
        qualified_table(schema_name, "documents")
    );
    session
        .query_opt(
            query.as_str(),
            &[&table.as_str(), &table_id.as_str(), &id.to_string()],
        )
        .await
        .map_err(map_postgres_error)?
        .map(row_to_document)
        .transpose()
}

pub(super) async fn load_table_id_from_session<C>(
    session: &C,
    schema_name: &str,
    table: &TableName,
) -> Result<Option<TableId>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT table_id, state FROM {} WHERE namespace = $1 AND table_name = $2",
        qualified_table(schema_name, "table_catalog")
    );
    let Some(row) = session
        .query_opt(query.as_str(), &[&"default", &table.as_str()])
        .await
        .map_err(map_postgres_error)?
    else {
        return Ok(None);
    };
    let state = TableState::from_str(row.get::<_, String>(1).as_str())?;
    if state != TableState::Active {
        return Err(Error::Conflict(format!(
            "logical table {} is in {} lifecycle state",
            table, state
        )));
    }
    Ok(Some(TableId::from_str(row.get::<_, String>(0).as_str())?))
}

pub(super) async fn load_table_identities_from_session<C>(
    session: &C,
    schema_name: &str,
) -> Result<Vec<crate::TableIdentitySnapshotEntry>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT namespace, table_name, table_id, state
         FROM {}
         ORDER BY namespace, table_name, table_id, state",
        qualified_table(schema_name, "table_catalog")
    );
    session
        .query(query.as_str(), &[])
        .await
        .map_err(map_postgres_error)?
        .into_iter()
        .map(|row| {
            Ok(crate::TableIdentitySnapshotEntry {
                namespace: row.get::<_, String>(0),
                table: TableName::new(row.get::<_, String>(1))?,
                table_id: TableId::from_str(row.get::<_, String>(2).as_str())?,
                state: TableState::from_str(row.get::<_, String>(3).as_str())?,
            })
        })
        .collect()
}

pub(super) async fn resolve_or_create_table_id_in_session<C>(
    session: &C,
    schema_name: &str,
    table: &TableName,
) -> Result<TableId>
where
    C: GenericClient + Sync,
{
    if let Some(table_id) = load_table_id_from_session(session, schema_name, table).await? {
        return Ok(table_id);
    }
    let table_id = TableId::new();
    let query = format!(
        "INSERT INTO {} (namespace, table_name, table_id, state)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT(namespace, table_name) DO NOTHING",
        qualified_table(schema_name, "table_catalog")
    );
    session
        .execute(
            query.as_str(),
            &[
                &"default",
                &table.as_str(),
                &table_id.as_str(),
                &TableState::Active.as_str(),
            ],
        )
        .await
        .map_err(map_postgres_error)?;
    load_table_id_from_session(session, schema_name, table)
        .await?
        .ok_or_else(|| {
            Error::Internal(format!(
                "failed to resolve table id for logical table {} after catalog insert",
                table
            ))
        })
}

pub(super) async fn ensure_table_id_in_session<C>(
    session: &C,
    schema_name: &str,
    table: &TableName,
    table_id: &TableId,
) -> Result<()>
where
    C: GenericClient + Sync,
{
    let hidden_namespace = hidden_table_namespace(table_id);
    let staged_hidden = match catalog_identity_row_from_session(
        session,
        schema_name,
        hidden_namespace.as_str(),
        table,
    )
    .await?
    {
        Some((hidden_id, TableState::Hidden)) if hidden_id == *table_id => true,
        Some((hidden_id, state)) => {
            return Err(Error::Conflict(format!(
                "hidden identity slot for logical table {} and table id {} contains {} in {} state",
                table, table_id, hidden_id, state
            )));
        }
        None => false,
    };

    match catalog_identity_row_from_session(session, schema_name, DEFAULT_TABLE_NAMESPACE, table)
        .await?
    {
        Some((existing, TableState::Active)) if existing == *table_id => {
            if staged_hidden {
                return Err(Error::Conflict(format!(
                    "logical table {} already has active table id {} and a duplicate hidden slot",
                    table, table_id
                )));
            }
            return Ok(());
        }
        Some((existing, state)) if existing == *table_id => {
            return Err(Error::Conflict(format!(
                "logical table {} is assigned table id {} in {} lifecycle state",
                table, table_id, state
            )));
        }
        Some((existing, TableState::Active)) => {
            ensure_table_id_available_in_session(
                session,
                schema_name,
                table_id,
                Some((hidden_namespace.as_str(), table)),
            )
            .await?;
            let query = format!(
                "UPDATE {}
                 SET namespace = $1, state = $2
                 WHERE namespace = $3 AND table_name = $4",
                qualified_table(schema_name, "table_catalog")
            );
            let deleting_namespace = deleting_table_namespace(&existing);
            session
                .execute(
                    query.as_str(),
                    &[
                        &deleting_namespace,
                        &TableState::Deleting.as_str(),
                        &DEFAULT_TABLE_NAMESPACE,
                        &table.as_str(),
                    ],
                )
                .await
                .map_err(map_postgres_error)?;
            if staged_hidden {
                let query = format!(
                    "DELETE FROM {} WHERE namespace = $1 AND table_name = $2",
                    qualified_table(schema_name, "table_catalog")
                );
                session
                    .execute(query.as_str(), &[&hidden_namespace, &table.as_str()])
                    .await
                    .map_err(map_postgres_error)?;
            }
            let query = format!(
                "INSERT INTO {} (namespace, table_name, table_id, state) VALUES ($1, $2, $3, $4)",
                qualified_table(schema_name, "table_catalog")
            );
            session
                .execute(
                    query.as_str(),
                    &[
                        &DEFAULT_TABLE_NAMESPACE,
                        &table.as_str(),
                        &table_id.as_str(),
                        &TableState::Active.as_str(),
                    ],
                )
                .await
                .map_err(map_postgres_error)?;
            return Ok(());
        }
        Some((existing, state)) => {
            return Err(Error::Conflict(format!(
                "logical table {} is already assigned table id {} in {} lifecycle state, journal references {}",
                table, existing, state, table_id
            )));
        }
        None => {}
    }
    ensure_table_id_available_in_session(
        session,
        schema_name,
        table_id,
        Some((hidden_namespace.as_str(), table)),
    )
    .await?;
    if staged_hidden {
        let query = format!(
            "DELETE FROM {} WHERE namespace = $1 AND table_name = $2",
            qualified_table(schema_name, "table_catalog")
        );
        session
            .execute(query.as_str(), &[&hidden_namespace, &table.as_str()])
            .await
            .map_err(map_postgres_error)?;
    }
    let query = format!(
        "INSERT INTO {} (namespace, table_name, table_id, state) VALUES ($1, $2, $3, $4)",
        qualified_table(schema_name, "table_catalog")
    );
    session
        .execute(
            query.as_str(),
            &[
                &"default",
                &table.as_str(),
                &table_id.as_str(),
                &TableState::Active.as_str(),
            ],
        )
        .await
        .map_err(map_postgres_error)?;
    Ok(())
}

async fn catalog_identity_row_from_session<C>(
    session: &C,
    schema_name: &str,
    namespace: &str,
    table: &TableName,
) -> Result<Option<(TableId, TableState)>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT table_id, state FROM {} WHERE namespace = $1 AND table_name = $2",
        qualified_table(schema_name, "table_catalog")
    );
    session
        .query_opt(query.as_str(), &[&namespace, &table.as_str()])
        .await
        .map_err(map_postgres_error)?
        .map(|row| {
            Ok((
                TableId::from_str(row.get::<_, String>(0).as_str())?,
                TableState::from_str(row.get::<_, String>(1).as_str())?,
            ))
        })
        .transpose()
}

async fn ensure_table_id_available_in_session<C>(
    session: &C,
    schema_name: &str,
    table_id: &TableId,
    allowed_key: Option<(&str, &TableName)>,
) -> Result<()>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT namespace, table_name, state FROM {} WHERE table_id = $1",
        qualified_table(schema_name, "table_catalog")
    );
    let Some(row) = session
        .query_opt(query.as_str(), &[&table_id.as_str()])
        .await
        .map_err(map_postgres_error)?
    else {
        return Ok(());
    };
    let namespace = row.get::<_, String>(0);
    let table = TableName::new(row.get::<_, String>(1))?;
    let state = TableState::from_str(row.get::<_, String>(2).as_str())?;
    if allowed_key
        .map(|(allowed_namespace, allowed_table)| {
            allowed_namespace == namespace && allowed_table == &table
        })
        .unwrap_or(false)
    {
        return Ok(());
    }
    Err(Error::Conflict(format!(
        "table id {} is already assigned to logical table {} in namespace {} with {} state",
        table_id, table, namespace, state
    )))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn load_index_candidate_documents_from_session<C>(
    session: &C,
    schema_name: &str,
    table: &TableName,
    table_schema: &TableSchema,
    index_name: &str,
    exact_prefix: &[Value],
    start: Option<&Value>,
    end: Option<&Value>,
    start_inclusive: bool,
    end_inclusive: bool,
) -> Result<Vec<Document>>
where
    C: GenericClient + Sync,
{
    let index_fields = index_fields_for_table_schema(table_schema, index_name)?;
    let range_field = index_fields.get(exact_prefix.len());

    let Some(table_id) = load_table_id_from_session(session, schema_name, table).await? else {
        return Ok(Vec::new());
    };
    let mut clauses = vec!["d.table_id = $1".to_string()];
    let mut params: Vec<Box<dyn ToSql + Sync + Send>> = vec![Box::new(table_id.to_string())];

    for (field, value) in index_fields.iter().zip(exact_prefix.iter()) {
        clauses.push(format!(
            "{} = ${}",
            postgres_json_extract_expr(field),
            params.len() + 1
        ));
        params.push(Box::new(postgres_index_text_value(value)?));
    }

    if let Some(range_field) = range_field {
        let field_type = field_type_for_table_schema(table_schema, range_field)?;
        match field_type {
            FieldType::String => {
                append_postgres_range_clause(
                    &mut clauses,
                    &mut params,
                    postgres_json_extract_expr(range_field),
                    start.map(postgres_index_text_value).transpose()?,
                    end.map(postgres_index_text_value).transpose()?,
                    start_inclusive,
                    end_inclusive,
                );
            }
            FieldType::Number => {
                append_postgres_range_clause(
                    &mut clauses,
                    &mut params,
                    postgres_numeric_extract_expr(range_field),
                    start.map(postgres_numeric_value).transpose()?,
                    end.map(postgres_numeric_value).transpose()?,
                    start_inclusive,
                    end_inclusive,
                );
            }
            _ if start.is_some() || end.is_some() => {
                return Err(Error::InvalidInput(
                    "range scans only support string and number indexed fields".to_string(),
                ));
            }
            _ => {}
        }
    }

    let sql = format!(
        "SELECT c.table_name, d.id, d.creation_time, d.update_time, d.data_json, d.typed_fields_json \
         FROM {} AS d \
         JOIN {} AS c ON c.table_id = d.table_id \
         WHERE {} \
         ORDER BY d.id",
        qualified_table(schema_name, "documents"),
        qualified_table(schema_name, "table_catalog"),
        clauses.join(" AND ")
    );
    let param_refs = params
        .iter()
        .map(|param| param.as_ref() as &(dyn ToSql + Sync))
        .collect::<Vec<_>>();
    let rows = session
        .query(sql.as_str(), &param_refs)
        .await
        .map_err(map_postgres_error)?;
    rows.into_iter().map(row_to_document).collect()
}

pub(super) async fn load_table_schema_from_session<C>(
    session: &C,
    schema_name: &str,
    table: &TableName,
) -> Result<Option<TableSchema>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT schema_json FROM {} WHERE table_name = $1",
        qualified_table(schema_name, "schemas")
    );
    session
        .query_opt(query.as_str(), &[&table.as_str()])
        .await
        .map_err(map_postgres_error)?
        .map(|row| deserialize_json::<TableSchema>(row.get::<_, String>(0).as_str()))
        .transpose()
}

pub(super) async fn load_scheduled_execution_ids_from_session<C>(
    session: &C,
    schema_name: &str,
) -> Result<Vec<String>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT execution_id FROM {} ORDER BY execution_id",
        qualified_table(schema_name, "scheduled_job_executions")
    );
    let rows = session
        .query(query.as_str(), &[])
        .await
        .map_err(map_postgres_error)?;
    Ok(rows
        .into_iter()
        .map(|row| row.get::<_, String>(0))
        .collect())
}

pub(super) async fn load_scheduled_jobs_from_session<C>(
    session: &C,
    schema_name: &str,
    table_name: &str,
) -> Result<Vec<ScheduledJob>>
where
    C: GenericClient + Sync,
{
    let order_by = if table_name == "scheduled_jobs" {
        "run_at, id"
    } else {
        "id"
    };
    let query = format!(
        "SELECT data_json FROM {} ORDER BY {order_by}",
        qualified_table(schema_name, table_name)
    );
    let rows = session
        .query(query.as_str(), &[])
        .await
        .map_err(map_postgres_error)?;
    rows.into_iter()
        .map(|row| deserialize_json::<ScheduledJob>(row.get::<_, String>(0).as_str()))
        .collect()
}

pub(super) async fn load_scheduled_job_result_from_session<C>(
    session: &C,
    schema_name: &str,
    job_id: &DocumentId,
) -> Result<Option<ScheduledJobResult>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT data_json FROM {} WHERE job_id = $1",
        qualified_table(schema_name, "scheduled_job_results")
    );
    session
        .query_opt(query.as_str(), &[&job_id.to_string()])
        .await
        .map_err(map_postgres_error)?
        .map(|row| deserialize_json::<ScheduledJobResult>(row.get::<_, String>(0).as_str()))
        .transpose()
}

pub(super) async fn load_cron_jobs_from_session<C>(
    session: &C,
    schema_name: &str,
) -> Result<Vec<CronJob>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT data_json FROM {} ORDER BY name",
        qualified_table(schema_name, "cron_jobs")
    );
    let rows = session
        .query(query.as_str(), &[])
        .await
        .map_err(map_postgres_error)?;
    rows.into_iter()
        .map(|row| deserialize_json::<CronJob>(row.get::<_, String>(0).as_str()))
        .collect()
}

pub(super) async fn table_has_rows_in_session<C>(
    session: &C,
    schema_name: &str,
    table_name: &str,
) -> Result<bool>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT 1 FROM {} LIMIT 1",
        qualified_table(schema_name, table_name)
    );
    session
        .query_opt(query.as_str(), &[])
        .await
        .map(|row| row.is_some())
        .map_err(map_postgres_error)
}

pub(super) async fn load_durable_records_from_session<C>(
    session: &C,
    schema_name: &str,
    sequence: SequenceNumber,
) -> Result<Vec<DurableMutationRecord>>
where
    C: GenericClient + Sync,
{
    let from = i64_from_sequence(sequence)?;
    let query = format!(
        "SELECT record_blob FROM {} WHERE sequence >= $1 ORDER BY sequence",
        qualified_table(schema_name, "commit_log")
    );
    let rows = session
        .query(query.as_str(), &[&from])
        .await
        .map_err(map_postgres_error)?;
    rows.into_iter()
        .map(|row| {
            let payload: Vec<u8> = row.get(0);
            deserialize_durable_record(payload.as_slice())
        })
        .collect()
}

pub(super) async fn load_durable_journal_cursor_floor_from_session<C>(
    session: &C,
    schema_name: &str,
) -> Result<SequenceNumber>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT MIN(sequence) FROM {}",
        qualified_table(schema_name, "commit_log")
    );
    let row = session
        .query_one(query.as_str(), &[])
        .await
        .map_err(map_postgres_error)?;
    let min_sequence = row.get::<_, Option<i64>>(0);
    match min_sequence {
        Some(sequence) => Ok(SequenceNumber(
            sequence_number_from_i64(sequence)?.0.saturating_sub(1),
        )),
        None => Ok(SequenceNumber(0)),
    }
}

pub(super) async fn stream_durable_journal_from_session<C>(
    session: &C,
    schema_name: &str,
    after: SequenceNumber,
    limit: usize,
) -> Result<DurableJournalPage>
where
    C: GenericClient + Sync,
{
    let latest_sequence = load_latest_sequence_from_session(session, schema_name).await?;
    let cursor_floor = load_durable_journal_cursor_floor_from_session(session, schema_name).await?;
    if after.0 < cursor_floor.0 {
        return Err(Error::InvalidInput(format!(
            "journal cursor {} is behind the retention floor {}",
            after.0, cursor_floor.0
        )));
    }
    if after.0 > latest_sequence.0 {
        return Err(Error::InvalidInput(format!(
            "journal cursor {} is ahead of the latest durable sequence {}",
            after.0, latest_sequence.0
        )));
    }

    let after_i64 = i64_from_sequence(after)?;
    let limit_i64 = i64::try_from(limit.saturating_add(1))
        .map_err(|_| Error::InvalidInput("journal stream limit overflow".to_string()))?;
    let query = format!(
        "SELECT record_blob FROM {} WHERE sequence > $1 ORDER BY sequence LIMIT $2",
        qualified_table(schema_name, "commit_log")
    );
    let rows = session
        .query(query.as_str(), &[&after_i64, &limit_i64])
        .await
        .map_err(map_postgres_error)?;
    let mut records = Vec::with_capacity(limit);
    let mut has_more = false;
    for row in rows {
        let payload: Vec<u8> = row.get(0);
        if records.len() == limit {
            has_more = true;
            break;
        }
        records.push(deserialize_durable_record(payload.as_slice())?);
    }

    let next_cursor = records
        .last()
        .map(|record| record.sequence)
        .unwrap_or(after);
    Ok(DurableJournalPage {
        records,
        next_cursor,
        latest_sequence,
        cursor_floor,
        has_more,
    })
}

pub(super) async fn load_metadata_u64_from_session<C>(
    session: &C,
    schema_name: &str,
    key: &str,
) -> Result<Option<u64>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT value_blob FROM {} WHERE key = $1",
        qualified_table(schema_name, "metadata")
    );
    let row = session
        .query_opt(query.as_str(), &[&key])
        .await
        .map_err(map_postgres_error)?;
    row.map(|row| {
        let bytes: Vec<u8> = row.get(0);
        decode_u64(bytes.as_slice())
    })
    .transpose()
}

pub(super) fn row_to_document(row: tokio_postgres::Row) -> Result<Document> {
    let table = TableName::new(row.get::<_, String>(0))?;
    let id = DocumentId::from_str(row.get::<_, String>(1).as_str())
        .map_err(|error| Error::InvalidInput(error.to_string()))?;
    let creation_time = timestamp_from_i64(row.get::<_, i64>(2))?;
    let update_time = timestamp_from_i64(row.get::<_, i64>(3))?;
    let fields =
        serde_json::from_str::<serde_json::Map<String, Value>>(row.get::<_, String>(4).as_str())
            .map_err(|error| Error::Serialization(error.to_string()))?;
    let typed_fields = serde_json::from_str(row.get::<_, String>(5).as_str())
        .map_err(|error| Error::Serialization(error.to_string()))?;
    Ok(Document {
        id,
        table,
        creation_time,
        update_time,
        fields,
        typed_fields,
    })
}

pub(super) async fn begin_scheduled_execution_in_session<C>(
    session: &C,
    schema_name: &str,
    execution_id: Option<&str>,
) -> Result<bool>
where
    C: GenericClient + Sync,
{
    let Some(execution_id) = execution_id else {
        return Ok(true);
    };

    let query = format!(
        "INSERT INTO {} (execution_id) VALUES ($1) ON CONFLICT DO NOTHING",
        qualified_table(schema_name, "scheduled_job_executions")
    );
    let inserted = session
        .execute(query.as_str(), &[&execution_id])
        .await
        .map_err(map_postgres_error)?;
    Ok(inserted == 1)
}

pub(super) async fn create_postgres_indexes_for_table_schema<C>(
    session: &C,
    schema_name: &str,
    table_schema: &TableSchema,
) -> Result<()>
where
    C: GenericClient + Sync,
{
    for index in table_schema.maintained_indexes() {
        let expressions = index
            .fields
            .iter()
            .map(|field| postgres_json_extract_expr(field))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "CREATE INDEX IF NOT EXISTS {} ON {} (table_id, {}, id)",
            quote_identifier(&postgres_index_name(&index.id)),
            qualified_table(schema_name, "documents"),
            expressions
        );
        session
            .batch_execute(sql.as_str())
            .await
            .map_err(map_postgres_error)?;
    }
    Ok(())
}

pub(super) async fn drop_postgres_indexes_for_table_schema<C>(
    session: &C,
    schema_name: &str,
    table_schema: &TableSchema,
) -> Result<()>
where
    C: GenericClient + Sync,
{
    for index in &table_schema.indexes {
        let sql = format!(
            "DROP INDEX IF EXISTS {}.{}",
            quote_identifier(schema_name),
            quote_identifier(&postgres_index_name(&index.id))
        );
        session
            .batch_execute(sql.as_str())
            .await
            .map_err(map_postgres_error)?;
    }
    Ok(())
}

pub(super) async fn apply_durable_record_in_session<C>(
    session: &C,
    schema_name: &str,
    record: &DurableMutationRecord,
) -> Result<()>
where
    C: GenericClient + Sync,
{
    if record.events.is_empty() {
        if let Some(execution_id) = record.scheduled_execution_id.as_deref() {
            let _ = begin_scheduled_execution_in_session(session, schema_name, Some(execution_id))
                .await?;
        }
        record_document_versions_for_writes_in_session(
            session,
            schema_name,
            record.sequence,
            record.timestamp,
            &record.writes,
        )
        .await?;
        record_index_versions_for_writes_in_session(
            session,
            schema_name,
            record.sequence,
            &record.writes,
        )
        .await?;
        return apply_document_writes_in_session(session, schema_name, &record.writes).await;
    }

    record_document_versions_for_events_in_session(
        session,
        schema_name,
        record.sequence,
        record.timestamp,
        &record.events,
    )
    .await?;
    record_index_versions_for_events_in_session(
        session,
        schema_name,
        record.sequence,
        &record.events,
    )
    .await?;
    for event in &record.events {
        apply_tenant_event_in_session(session, schema_name, event).await?;
    }

    Ok(())
}

async fn apply_tenant_event_in_session<C>(
    session: &C,
    schema_name: &str,
    event: &TenantEventKind,
) -> Result<()>
where
    C: GenericClient + Sync,
{
    match event {
        TenantEventKind::DocumentWrite { writes } => {
            apply_document_writes_in_session(session, schema_name, writes).await
        }
        TenantEventKind::SchemaChange { change } => {
            apply_schema_change_in_session(session, schema_name, change).await
        }
        TenantEventKind::TableLifecycle { lifecycle } => {
            apply_table_lifecycle_in_session(session, schema_name, lifecycle).await
        }
        TenantEventKind::IndexLifecycle { .. } | TenantEventKind::Barrier { .. } => Ok(()),
        TenantEventKind::ScheduledExecution { execution_id } => {
            let _ = begin_scheduled_execution_in_session(session, schema_name, Some(execution_id))
                .await?;
            Ok(())
        }
        TenantEventKind::TriggerDelivery { cursor } => {
            let query = format!(
                "INSERT INTO {} (key, value_blob) VALUES ($1, $2)
                 ON CONFLICT(key) DO UPDATE SET value_blob = EXCLUDED.value_blob",
                qualified_table(schema_name, "metadata")
            );
            let value = encode_u64(cursor.materialized_through.0);
            session
                .execute(query.as_str(), &[&TRIGGER_DELIVERY_CURSOR_KEY, &&value[..]])
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        }
    }
}

async fn apply_document_writes_in_session<C>(
    session: &C,
    schema_name: &str,
    writes: &[WriteOp],
) -> Result<()>
where
    C: GenericClient + Sync,
{
    for write in writes {
        match (&write.previous, &write.current) {
            (None, Some(current)) => {
                ensure_table_id_in_session(session, schema_name, &write.table, &write.table_id)
                    .await?;
                let existing = load_document_by_table_id_from_session(
                    session,
                    schema_name,
                    &write.table,
                    &write.table_id,
                    &write.doc_id,
                )
                .await?;
                match existing {
                    Some(existing) if existing == *current => continue,
                    Some(_) => {
                        return Err(Error::Conflict(format!(
                            "durable journal insert replay found conflicting state for document {}",
                            write.doc_id
                        )));
                    }
                    None => {
                        let query = format!(
                            "INSERT INTO {} (table_id, id, data_json, typed_fields_json, creation_time, update_time) VALUES ($1, $2, $3, $4, $5, $6)",
                            qualified_table(schema_name, "documents")
                        );
                        let id = write.doc_id.to_string();
                        let data_json = serialize_document_fields(current)?;
                        let typed_fields_json = serialize_document_typed_fields(current)?;
                        let creation_time = i64_from_timestamp(current.creation_time)?;
                        let update_time = i64_from_timestamp(current.update_time)?;
                        session
                            .execute(
                                query.as_str(),
                                &[
                                    &write.table_id.as_str(),
                                    &id,
                                    &data_json,
                                    &typed_fields_json,
                                    &creation_time,
                                    &update_time,
                                ],
                            )
                            .await
                            .map_err(map_postgres_error)?;
                    }
                }
            }
            (Some(previous), Some(current)) => {
                ensure_table_id_in_session(session, schema_name, &write.table, &write.table_id)
                    .await?;
                let existing = load_document_by_table_id_from_session(
                    session,
                    schema_name,
                    &write.table,
                    &write.table_id,
                    &write.doc_id,
                )
                .await?
                .ok_or(Error::Conflict(format!(
                    "durable journal update replay missing document {}",
                    write.doc_id
                )))?;
                if existing == *current {
                    continue;
                }
                if existing != *previous {
                    return Err(Error::Conflict(format!(
                        "durable journal update replay found conflicting state for document {}",
                        write.doc_id
                    )));
                }
                let query = format!(
                    "UPDATE {} SET data_json = $3, typed_fields_json = $4, creation_time = $5, update_time = $6 WHERE table_id = $1 AND id = $2",
                    qualified_table(schema_name, "documents")
                );
                let id = write.doc_id.to_string();
                let data_json = serialize_document_fields(current)?;
                let typed_fields_json = serialize_document_typed_fields(current)?;
                let creation_time = i64_from_timestamp(current.creation_time)?;
                let update_time = i64_from_timestamp(current.update_time)?;
                session
                    .execute(
                        query.as_str(),
                        &[
                            &write.table_id.as_str(),
                            &id,
                            &data_json,
                            &typed_fields_json,
                            &creation_time,
                            &update_time,
                        ],
                    )
                    .await
                    .map_err(map_postgres_error)?;
            }
            (Some(previous), None) => {
                ensure_table_id_in_session(session, schema_name, &write.table, &write.table_id)
                    .await?;
                match load_document_by_table_id_from_session(
                    session,
                    schema_name,
                    &write.table,
                    &write.table_id,
                    &write.doc_id,
                )
                .await?
                {
                    Some(existing) if existing != *previous => {
                        return Err(Error::Conflict(format!(
                            "durable journal delete replay found conflicting state for document {}",
                            write.doc_id
                        )));
                    }
                    Some(_) => {
                        let query = format!(
                            "DELETE FROM {} WHERE table_id = $1 AND id = $2",
                            qualified_table(schema_name, "documents")
                        );
                        let id = write.doc_id.to_string();
                        session
                            .execute(query.as_str(), &[&write.table_id.as_str(), &id])
                            .await
                            .map_err(map_postgres_error)?;
                    }
                    None => continue,
                }
            }
            (None, None) => {
                return Err(Error::Internal(
                    "durable journal write must include a previous or current document".to_string(),
                ));
            }
        }
    }

    Ok(())
}

async fn apply_schema_change_in_session<C>(
    session: &C,
    schema_name: &str,
    change: &SchemaChangeEvent,
) -> Result<()>
where
    C: GenericClient + Sync,
{
    match change {
        SchemaChangeEvent::SetTable {
            table,
            table_id,
            previous,
            current,
        } => {
            ensure_table_id_in_session(session, schema_name, table, table_id).await?;
            if let Some(previous) = previous {
                drop_postgres_indexes_for_table_schema(session, schema_name, previous).await?;
            }
            let query = format!(
                "INSERT INTO {} (table_name, schema_json) VALUES ($1, $2)
                 ON CONFLICT(table_name) DO UPDATE SET schema_json = EXCLUDED.schema_json",
                qualified_table(schema_name, "schemas")
            );
            let schema_json = serialize_json(current)?;
            session
                .execute(query.as_str(), &[&table.as_str(), &schema_json])
                .await
                .map_err(map_postgres_error)?;
            create_postgres_indexes_for_table_schema(session, schema_name, current).await
        }
        SchemaChangeEvent::DeleteTable {
            table, previous, ..
        } => {
            if let Some(previous) = previous {
                drop_postgres_indexes_for_table_schema(session, schema_name, previous).await?;
            }
            let query = format!(
                "DELETE FROM {} WHERE table_name = $1",
                qualified_table(schema_name, "schemas")
            );
            session
                .execute(query.as_str(), &[&table.as_str()])
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        }
    }
}

async fn apply_table_lifecycle_in_session<C>(
    session: &C,
    schema_name: &str,
    lifecycle: &TableLifecycleEvent,
) -> Result<()>
where
    C: GenericClient + Sync,
{
    match lifecycle {
        TableLifecycleEvent::StageHidden { table, table_id } => {
            stage_hidden_table_identity_in_session(session, schema_name, table, table_id).await
        }
        TableLifecycleEvent::ActivateHidden {
            table, table_id, ..
        } => {
            let _ =
                activate_hidden_table_identity_in_session(session, schema_name, table, table_id)
                    .await?;
            Ok(())
        }
        TableLifecycleEvent::MarkDeleting { table, .. } => {
            let _ = mark_table_deleting_in_session(session, schema_name, table).await?;
            Ok(())
        }
        TableLifecycleEvent::HardDelete { table, table_id } => {
            if hard_delete_table_identity_in_session(session, schema_name, table_id)
                .await?
                .is_some()
                && load_table_id_from_session(session, schema_name, table)
                    .await?
                    .is_none()
            {
                if let Some(schema) =
                    load_table_schema_from_session(session, schema_name, table).await?
                {
                    drop_postgres_indexes_for_table_schema(session, schema_name, &schema).await?;
                }
                let query = format!(
                    "DELETE FROM {} WHERE table_name = $1",
                    qualified_table(schema_name, "schemas")
                );
                session
                    .execute(query.as_str(), &[&table.as_str()])
                    .await
                    .map_err(map_postgres_error)?;
            }
            Ok(())
        }
    }
}

pub(super) fn sequence_number_from_i64(value: i64) -> Result<SequenceNumber> {
    u64::try_from(value)
        .map(SequenceNumber)
        .map_err(|_| Error::Internal(format!("negative PostgreSQL sequence value: {value}")))
}

pub(super) fn timestamp_from_i64(value: i64) -> Result<Timestamp> {
    u64::try_from(value)
        .map(Timestamp)
        .map_err(|_| Error::Internal(format!("negative PostgreSQL timestamp value: {value}")))
}

pub(super) fn i64_from_sequence(sequence: SequenceNumber) -> Result<i64> {
    i64::try_from(sequence.0).map_err(|_| {
        Error::InvalidInput(format!("sequence {} exceeds PostgreSQL BIGINT", sequence.0))
    })
}

pub(super) fn i64_from_timestamp(timestamp: Timestamp) -> Result<i64> {
    i64::try_from(timestamp.0).map_err(|_| {
        Error::InvalidInput(format!(
            "timestamp {} exceeds PostgreSQL BIGINT",
            timestamp.0
        ))
    })
}

pub(super) fn claim_due_jobs_upper_bound(timestamp: Timestamp) -> i64 {
    i64::try_from(timestamp.0).unwrap_or(i64::MAX)
}

pub(super) fn tenant_advisory_lock_key(tenant_id: &TenantId) -> i64 {
    let digest = Sha256::digest(tenant_id.as_str().as_bytes());
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    i64::from_be_bytes(bytes)
}

pub(super) fn postgres_index_name(index_id: &nimbus_core::IndexId) -> String {
    let digest = Sha256::digest(index_id.as_str().as_bytes());
    let mut suffix = String::with_capacity(24);
    for byte in digest.iter().take(12) {
        let _ = write!(&mut suffix, "{byte:02x}");
    }
    format!("idx_{suffix}")
}

pub(super) fn postgres_json_extract_expr(field: &str) -> String {
    format!(
        "jsonb_extract_path_text(data_json::jsonb, {})",
        postgres_string_literal(field)
    )
}

pub(super) fn postgres_string_literal(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for character in value.chars() {
        if character == '\'' {
            quoted.push('\'');
        }
        quoted.push(character);
    }
    quoted.push('\'');
    quoted
}

pub(super) fn expect_write_commit(
    commit: Option<CommitEntry>,
    expectation: &str,
) -> Result<CommitEntry> {
    commit.ok_or_else(|| Error::Internal(expectation.to_string()))
}

pub(super) fn serialize_json<T>(value: &T) -> Result<String>
where
    T: serde::Serialize,
{
    serde_json::to_string(value).map_err(|error| Error::Serialization(error.to_string()))
}

pub(super) fn deserialize_json<T>(json: &str) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_str(json).map_err(|error| Error::Serialization(error.to_string()))
}

pub(super) fn serialize_document_fields(document: &Document) -> Result<String> {
    serde_json::to_string(&document.fields).map_err(|error| Error::Serialization(error.to_string()))
}

pub(super) fn serialize_document_typed_fields(document: &Document) -> Result<String> {
    serde_json::to_string(&document.typed_fields)
        .map_err(|error| Error::Serialization(error.to_string()))
}

pub(super) fn decode_u64(bytes: &[u8]) -> Result<u64> {
    let bytes: [u8; 8] = bytes
        .try_into()
        .map_err(|_| Error::Serialization("invalid u64 metadata blob".to_string()))?;
    Ok(u64::from_le_bytes(bytes))
}

pub(super) fn encode_u64(value: u64) -> [u8; 8] {
    value.to_le_bytes()
}

pub(super) fn default_postgres_read_parallelism() -> usize {
    std::thread::available_parallelism()
        .map(|parallelism| parallelism.get().max(MIN_POSTGRES_READ_PARALLELISM))
        .unwrap_or(MIN_POSTGRES_READ_PARALLELISM)
}

pub(super) fn apply_schedule_ops_in_transaction(
    transaction: &mut PostgresWriteTransaction,
    schedule_ops: &[ResolvedScheduleOp],
) -> Result<()> {
    for schedule_op in schedule_ops {
        match schedule_op {
            ResolvedScheduleOp::Insert { job } => transaction.insert_scheduled_job(job)?,
            ResolvedScheduleOp::Cancel { job_id } => {
                if !transaction.cancel_scheduled_job(job_id)? {
                    return Err(Error::ScheduledJobNotFound(job_id.clone()));
                }
            }
        }
    }
    Ok(())
}

pub(super) fn map_pool_error(error: PoolError) -> Error {
    Error::storage(
        StorageErrorKind::Unavailable,
        format!("postgres pool error: {error}"),
    )
}

pub(super) fn map_build_error(error: BuildError) -> Error {
    Error::storage(
        StorageErrorKind::Unavailable,
        format!("postgres pool build error: {error}"),
    )
}

pub(super) fn map_join_error(error: tokio::task::JoinError) -> Error {
    Error::Internal(format!("postgres executor join error: {error}"))
}

pub(super) fn map_permit_error(error: tokio::sync::AcquireError) -> Error {
    Error::Internal(format!("postgres executor permit error: {error}"))
}

pub(super) fn map_postgres_error(error: tokio_postgres::Error) -> Error {
    if let Some(db_error) = error.as_db_error() {
        let code = db_error.code().code();
        let mut message = format!(
            "postgres error [{:?}]: {}",
            db_error.code(),
            db_error.message()
        );
        if let Some(detail) = db_error.detail() {
            let _ = write!(&mut message, " (detail: {detail})");
        }
        if let Some(hint) = db_error.hint() {
            let _ = write!(&mut message, " (hint: {hint})");
        }
        return match code {
            "40001" | "40P01" | "55P03" => Error::storage(StorageErrorKind::Transient, message),
            "08000" | "08001" | "08003" | "08004" | "08006" | "08007" | "08P01" => {
                Error::storage(StorageErrorKind::Unavailable, message)
            }
            "42501" => Error::PermissionDenied(message),
            "53100" | "53200" | "53300" | "53400" => Error::ResourceExhausted(message),
            "57P03" => Error::storage(StorageErrorKind::Unavailable, message),
            "58P01" | "58P02" => Error::storage(StorageErrorKind::Io, message),
            "XX001" | "XX002" => Error::storage(StorageErrorKind::Corruption, message),
            _ if code.starts_with("08") => Error::storage(StorageErrorKind::Unavailable, message),
            _ if code.starts_with("53") => Error::ResourceExhausted(message),
            _ => Error::storage(StorageErrorKind::Other, message),
        };
    }

    if error.is_closed() {
        Error::storage(
            StorageErrorKind::Unavailable,
            format!("postgres error: {error}"),
        )
    } else {
        Error::storage(StorageErrorKind::Other, format!("postgres error: {error}"))
    }
}
