use super::*;

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
            "SELECT table_name, id, creation_time, update_time, data_json, typed_fields_json \
             FROM {} \
             WHERE table_name = $1 \
             ORDER BY id",
            qualified_table(schema_name, "documents")
        )
    } else {
        format!(
            "SELECT table_name, id, creation_time, update_time, data_json, typed_fields_json \
             FROM {} \
             ORDER BY table_name, id",
            qualified_table(schema_name, "documents")
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
    let query = format!(
        "SELECT table_name, id, creation_time, update_time, data_json, typed_fields_json \
         FROM {} \
         WHERE table_name = $1 AND id = $2",
        qualified_table(schema_name, "documents")
    );
    session
        .query_opt(query.as_str(), &[&table.as_str(), &id.to_string()])
        .await
        .map_err(map_postgres_error)?
        .map(row_to_document)
        .transpose()
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

    let mut clauses = vec!["table_name = $1".to_string()];
    let mut params: Vec<Box<dyn ToSql + Sync + Send>> = vec![Box::new(table.as_str().to_string())];

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
        "SELECT table_name, id, creation_time, update_time, data_json, typed_fields_json \
         FROM {} \
         WHERE {} \
         ORDER BY id",
        qualified_table(schema_name, "documents"),
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
    for index in &table_schema.indexes {
        let expressions = index
            .fields
            .iter()
            .map(|field| postgres_json_extract_expr(field))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "CREATE INDEX IF NOT EXISTS {} ON {} (table_name, {}, id)",
            quote_identifier(&postgres_index_name(&table_schema.table, &index.name)),
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
            quote_identifier(&postgres_index_name(&table_schema.table, &index.name))
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
    if let Some(execution_id) = record.scheduled_execution_id.as_deref() {
        let _ =
            begin_scheduled_execution_in_session(session, schema_name, Some(execution_id)).await?;
    }

    for write in &record.writes {
        match (&write.previous, &write.current) {
            (None, Some(current)) => {
                let existing =
                    load_document_from_session(session, schema_name, &write.table, &write.doc_id)
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
                            "INSERT INTO {} (table_name, id, data_json, typed_fields_json, creation_time, update_time) VALUES ($1, $2, $3, $4, $5, $6)",
                            qualified_table(schema_name, "documents")
                        );
                        let table = write.table.as_str().to_string();
                        let id = write.doc_id.to_string();
                        let data_json = serialize_document_fields(current)?;
                        let typed_fields_json = serialize_document_typed_fields(current)?;
                        let creation_time = i64_from_timestamp(current.creation_time)?;
                        let update_time = i64_from_timestamp(current.update_time)?;
                        session
                            .execute(
                                query.as_str(),
                                &[
                                    &table,
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
                let existing =
                    load_document_from_session(session, schema_name, &write.table, &write.doc_id)
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
                    "UPDATE {} SET data_json = $3, typed_fields_json = $4, creation_time = $5, update_time = $6 WHERE table_name = $1 AND id = $2",
                    qualified_table(schema_name, "documents")
                );
                let table = write.table.as_str().to_string();
                let id = write.doc_id.to_string();
                let data_json = serialize_document_fields(current)?;
                let typed_fields_json = serialize_document_typed_fields(current)?;
                let creation_time = i64_from_timestamp(current.creation_time)?;
                let update_time = i64_from_timestamp(current.update_time)?;
                session
                    .execute(
                        query.as_str(),
                        &[
                            &table,
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
                match load_document_from_session(session, schema_name, &write.table, &write.doc_id)
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
                            "DELETE FROM {} WHERE table_name = $1 AND id = $2",
                            qualified_table(schema_name, "documents")
                        );
                        let table = write.table.as_str().to_string();
                        let id = write.doc_id.to_string();
                        session
                            .execute(query.as_str(), &[&table, &id])
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

pub(super) fn postgres_index_name(table: &TableName, index_name: &str) -> String {
    let digest = Sha256::digest(format!("{}:{index_name}", table.as_str()).as_bytes());
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

pub(super) fn matches_filters(document: &Document, filters: &[Filter]) -> Result<bool> {
    for filter in filters {
        let Some(field_value) = document.get_field(&filter.field) else {
            return Ok(false);
        };
        let matched = match filter.op {
            FilterOp::Eq => field_value == &filter.value,
            FilterOp::Neq => field_value != &filter.value,
            FilterOp::Gt => compare_values(field_value, &filter.value)? == Ordering::Greater,
            FilterOp::Gte => {
                matches!(
                    compare_values(field_value, &filter.value)?,
                    Ordering::Greater | Ordering::Equal
                )
            }
            FilterOp::Lt => compare_values(field_value, &filter.value)? == Ordering::Less,
            FilterOp::Lte => {
                matches!(
                    compare_values(field_value, &filter.value)?,
                    Ordering::Less | Ordering::Equal
                )
            }
        };
        if !matched {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn filter_documents_with_predicate<F>(
    documents: Vec<Document>,
    filters: &[Filter],
    check_cancel: &mut dyn FnMut() -> Result<()>,
    mut include_document: F,
) -> Result<Vec<Document>>
where
    F: FnMut(&Document) -> Result<bool>,
{
    let mut filtered = Vec::new();
    for document in documents {
        check_cancel()?;
        if matches_filters(&document, filters)? && include_document(&document)? {
            filtered.push(document);
        }
    }
    Ok(filtered)
}

pub(super) fn compare_values(left: &Value, right: &Value) -> Result<Ordering> {
    match (left, right) {
        (Value::String(left), Value::String(right)) => Ok(left.cmp(right)),
        (Value::Number(left), Value::Number(right)) => {
            let left = left
                .as_f64()
                .ok_or_else(|| Error::InvalidInput("unsupported numeric comparison".to_string()))?;
            let right = right
                .as_f64()
                .ok_or_else(|| Error::InvalidInput("unsupported numeric comparison".to_string()))?;
            left.partial_cmp(&right).ok_or_else(|| {
                Error::InvalidInput("invalid numeric ordering comparison".to_string())
            })
        }
        _ => Err(Error::InvalidInput(
            "comparisons only support string and number fields in phase 1".to_string(),
        )),
    }
}

pub(super) fn document_matches_exact_prefix(
    document: &Document,
    index_fields: &[String],
    exact_prefix: &[Value],
) -> bool {
    index_fields
        .iter()
        .zip(exact_prefix.iter())
        .all(|(field, value)| document.get_field(field) == Some(value))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn filter_index_documents_with_cancel(
    documents: Vec<Document>,
    table: &TableName,
    index_fields: &[String],
    exact_prefix: &[Value],
    start: Option<&Value>,
    end: Option<&Value>,
    start_inclusive: bool,
    end_inclusive: bool,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let range_field = index_fields.get(exact_prefix.len());
    let mut filtered = Vec::new();
    for document in documents {
        check_cancel()?;
        if &document.table != table {
            continue;
        }
        if !document_matches_exact_prefix(&document, index_fields, exact_prefix) {
            continue;
        }
        if let Some(range_field) = range_field
            && !document_matches_range_bounds(
                &document,
                range_field,
                start,
                end,
                start_inclusive,
                end_inclusive,
            )?
        {
            continue;
        }
        filtered.push(document);
    }
    Ok(filtered)
}

pub(super) fn index_fields_for_table_schema(
    table_schema: &TableSchema,
    index_name: &str,
) -> Result<Vec<String>> {
    table_schema
        .indexes
        .iter()
        .find(|index| index.name == index_name)
        .map(|index| index.fields.clone())
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "index '{}' not found for table '{}'",
                index_name,
                table_schema.table.as_str()
            ))
        })
}

pub(super) fn field_type_for_table_schema(
    table_schema: &TableSchema,
    field_name: &str,
) -> Result<FieldType> {
    table_schema
        .fields
        .iter()
        .find(|field| field.name == field_name)
        .map(|field| field.field_type)
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "indexed field '{}' not found for table '{}'",
                field_name,
                table_schema.table.as_str()
            ))
        })
}

pub(super) fn postgres_index_text_value(value: &Value) -> Result<String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Bool(value) => Ok(value.to_string()),
        _ => Err(Error::InvalidInput(
            "indexed values must be string, number, or boolean scalars".to_string(),
        )),
    }
}

pub(super) fn postgres_numeric_value(value: &Value) -> Result<f64> {
    value
        .as_f64()
        .ok_or_else(|| Error::InvalidInput("numeric indexed value expected".to_string()))
}

pub(super) fn postgres_numeric_extract_expr(field: &str) -> String {
    format!(
        "CAST({} AS DOUBLE PRECISION)",
        postgres_json_extract_expr(field)
    )
}

pub(super) fn append_postgres_range_clause<T>(
    clauses: &mut Vec<String>,
    params: &mut Vec<Box<dyn ToSql + Sync + Send>>,
    expr: String,
    start: Option<T>,
    end: Option<T>,
    start_inclusive: bool,
    end_inclusive: bool,
) where
    T: ToSql + Sync + Send + 'static,
{
    if let Some(start) = start {
        let operator = if start_inclusive { ">=" } else { ">" };
        clauses.push(format!("{expr} {operator} ${}", params.len() + 1));
        params.push(Box::new(start));
    }
    if let Some(end) = end {
        let operator = if end_inclusive { "<=" } else { "<" };
        clauses.push(format!("{expr} {operator} ${}", params.len() + 1));
        params.push(Box::new(end));
    }
}

pub(super) fn document_matches_range_bounds(
    document: &Document,
    field: &str,
    start: Option<&Value>,
    end: Option<&Value>,
    start_inclusive: bool,
    end_inclusive: bool,
) -> Result<bool> {
    let Some(value) = document.get_field(field) else {
        return Ok(false);
    };

    if let Some(start) = start {
        let ordering = compare_values(value, start)?;
        let passes = if start_inclusive {
            matches!(ordering, Ordering::Greater | Ordering::Equal)
        } else {
            ordering == Ordering::Greater
        };
        if !passes {
            return Ok(false);
        }
    }

    if let Some(end) = end {
        let ordering = compare_values(value, end)?;
        let passes = if end_inclusive {
            matches!(ordering, Ordering::Less | Ordering::Equal)
        } else {
            ordering == Ordering::Less
        };
        if !passes {
            return Ok(false);
        }
    }

    Ok(true)
}

pub(super) fn validate_durable_journal_stream_limit(limit: usize) -> Result<()> {
    if limit == 0 {
        return Err(Error::InvalidInput(
            "journal stream limit must be greater than zero".to_string(),
        ));
    }
    if limit > MAX_DURABLE_JOURNAL_STREAM_LIMIT {
        return Err(Error::InvalidInput(format!(
            "journal stream limit {limit} exceeds the maximum {}",
            MAX_DURABLE_JOURNAL_STREAM_LIMIT
        )));
    }
    Ok(())
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
