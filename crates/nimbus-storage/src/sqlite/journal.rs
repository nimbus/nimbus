use super::*;
use crate::keys::{document_path_key, resource_locator_key};
use nimbus_core::{DocumentLocator, ResourcePathBinding};

impl SqliteTenantStore {
    pub fn metadata_blob(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.read_snapshot()?.metadata_blob(key)
    }

    pub fn journal_mode(&self) -> Result<String> {
        self.read_snapshot()?.journal_mode()
    }

    pub fn journal_progress(&self) -> Result<JournalProgress> {
        self.read_snapshot()?.journal_progress()
    }

    pub fn latest_sequence(&self) -> Result<SequenceNumber> {
        self.read_snapshot()?.latest_sequence()
    }

    pub fn applied_sequence(&self) -> Result<SequenceNumber> {
        self.read_snapshot()?.applied_sequence()
    }

    pub fn read_commit_log_from(&self, sequence: SequenceNumber) -> Result<Vec<CommitEntry>> {
        Ok(self
            .read_durable_journal_from(sequence)?
            .into_iter()
            .map(|record| record.as_commit_entry())
            .collect())
    }

    pub fn read_durable_journal_from(
        &self,
        sequence: SequenceNumber,
    ) -> Result<Vec<DurableMutationRecord>> {
        self.read_snapshot()?.read_durable_journal_from(sequence)
    }

    pub fn stream_durable_journal(
        &self,
        after: SequenceNumber,
        limit: usize,
    ) -> Result<DurableJournalPage> {
        self.read_snapshot()?.stream_durable_journal(after, limit)
    }

    pub fn export_durable_journal_bootstrap(&self) -> Result<DurableJournalBootstrap> {
        self.read_snapshot()?.export_durable_journal_bootstrap()
    }

    pub fn export_materialized_journal_snapshot(&self) -> Result<MaterializedJournalSnapshot> {
        self.read_snapshot()?.export_materialized_journal_snapshot()
    }

    pub fn restore_materialized_journal_from_snapshot(
        &self,
        snapshot: &MaterializedJournalSnapshot,
    ) -> Result<()> {
        snapshot.validate()?;
        self.ensure_materialized_journal_restore_target_is_empty()?;

        let conn = self.open_connection()?;
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(map_sqlite_error)?;
        for table_schema in snapshot.schema.tables.values() {
            conn.execute(
                "INSERT INTO schemas (table_name, schema_json) VALUES (?1, ?2)",
                params![table_schema.table.as_str(), serialize_json(table_schema)?],
            )
            .map_err(map_sqlite_error)?;
        }
        for document in &snapshot.documents {
            conn.execute(
                "INSERT INTO documents (table_name, id, data_json, typed_fields_json, creation_time, update_time)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    document.table.as_str(),
                    document.id.to_string(),
                    serialize_document_fields(document)?,
                    serialize_document_typed_fields(document)?,
                    document.creation_time.0,
                    document.update_time.0,
                ],
            )
            .map_err(map_sqlite_error)?;
        }
        for execution_id in &snapshot.scheduled_execution_ids {
            conn.execute(
                "INSERT INTO scheduled_job_executions (execution_id) VALUES (?1)",
                params![execution_id],
            )
            .map_err(map_sqlite_error)?;
        }
        for table_schema in snapshot.schema.tables.values() {
            create_sqlite_indexes_for_table_schema(&conn, table_schema)?;
        }
        put_metadata_in_conn(
            &conn,
            NEXT_SEQUENCE_KEY,
            &encode_u64(snapshot.applied_sequence.0.saturating_add(1)),
        )?;
        put_metadata_in_conn(
            &conn,
            APPLIED_SEQUENCE_KEY,
            &encode_u64(snapshot.applied_sequence.0),
        )?;
        conn.execute_batch("COMMIT").map_err(map_sqlite_error)?;
        self.replace_cached_schema(snapshot.schema.clone())?;
        Ok(())
    }

    pub fn rebuild_materialized_journal_from_snapshot(
        &self,
        snapshot: &MaterializedJournalSnapshot,
        journal_tail: &[DurableMutationRecord],
        target_sequence: Option<SequenceNumber>,
    ) -> Result<JournalProgress> {
        snapshot.validate()?;
        let available_head = journal_tail
            .last()
            .map(|record| record.sequence)
            .unwrap_or(snapshot.applied_sequence);
        if let Some(target_sequence) = target_sequence {
            if target_sequence.0 < snapshot.applied_sequence.0 {
                return Err(Error::InvalidInput(format!(
                    "rebuild target sequence {} is behind snapshot sequence {}",
                    target_sequence.0, snapshot.applied_sequence.0
                )));
            }
            if target_sequence.0 > available_head.0 {
                return Err(Error::InvalidInput(format!(
                    "rebuild target sequence {} is beyond available journal head {}",
                    target_sequence.0, available_head.0
                )));
            }
        } else if available_head.0 < snapshot.durable_head.0 {
            return Err(Error::InvalidInput(format!(
                "journal tail is incomplete for snapshot boundary: available head {} is behind snapshot durable head {}",
                available_head.0, snapshot.durable_head.0
            )));
        }

        self.restore_materialized_journal_from_snapshot(snapshot)?;
        let replay_target = target_sequence.unwrap_or_else(|| {
            journal_tail
                .last()
                .map(|record| record.sequence)
                .unwrap_or(snapshot.applied_sequence)
        });
        let tail = journal_tail
            .iter()
            .filter(|record| {
                record.sequence.0 > snapshot.applied_sequence.0
                    && record.sequence.0 <= replay_target.0
            })
            .cloned()
            .collect::<Vec<_>>();
        self.append_durable_records_batch(&tail)?;
        self.recover_durable_journal()
    }

    pub fn append_durable_records_batch(&self, records: &[DurableMutationRecord]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }

        let conn = self.open_connection()?;
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(map_sqlite_error)?;
        let mut next = latest_sequence_in_conn(&conn)?.0.saturating_add(1);
        for record in records {
            if record.sequence.0 != next {
                return Err(Error::Internal(format!(
                    "durable journal append expected sequence {}, got {}",
                    next, record.sequence.0
                )));
            }
            conn.execute(
                "INSERT INTO commit_log (sequence, record_blob) VALUES (?1, ?2)",
                params![record.sequence.0, serialize_durable_record(record)?],
            )
            .map_err(map_sqlite_error)?;
            next = next.saturating_add(1);
        }
        put_metadata_in_conn(&conn, NEXT_SEQUENCE_KEY, &encode_u64(next))?;
        self.fault_injector
            .check(FaultPoint::JournalAppendBeforeDurableFlush)?;
        conn.execute_batch("COMMIT").map_err(map_sqlite_error)?;
        self.fault_injector
            .check(FaultPoint::JournalFlushBeforeVisibility)?;
        Ok(())
    }

    pub fn apply_durable_records_batch(&self, records: &[DurableMutationRecord]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }

        let conn = self.open_connection()?;
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(map_sqlite_error)?;
        let mut applied_head = applied_sequence_in_conn(&conn)?.0;
        for record in records {
            if record.sequence.0 <= applied_head {
                continue;
            }
            if record.sequence.0 != applied_head.saturating_add(1) {
                return Err(Error::Internal(format!(
                    "durable journal apply expected sequence {}, got {}",
                    applied_head.saturating_add(1),
                    record.sequence.0
                )));
            }
            apply_durable_record_in_conn(&conn, record)?;
            applied_head = record.sequence.0;
        }

        if applied_head >= records[0].sequence.0 {
            put_metadata_in_conn(&conn, APPLIED_SEQUENCE_KEY, &encode_u64(applied_head))?;
        }
        self.fault_injector
            .check(FaultPoint::StorageCommitBeforeVisibility)?;
        conn.execute_batch("COMMIT").map_err(map_sqlite_error)?;
        self.fault_injector
            .check(FaultPoint::StorageCommitAfterVisibilityBeforeReturn)?;
        Ok(())
    }

    pub fn recover_durable_journal(&self) -> Result<JournalProgress> {
        let progress = self.journal_progress()?;
        if progress.applied_head.0 >= progress.durable_head.0 {
            return Ok(progress);
        }
        let from = SequenceNumber(progress.applied_head.0.saturating_add(1));
        let pending = self.read_durable_journal_from(from)?;
        self.apply_durable_records_batch(&pending)?;
        self.journal_progress()
    }

    fn ensure_materialized_journal_restore_target_is_empty(&self) -> Result<()> {
        let snapshot = self.read_snapshot()?;
        let progress = snapshot.journal_progress()?;
        if progress.durable_head.0 != 0
            || progress.applied_head.0 != 0
            || !snapshot.documents()?.is_empty()
            || !snapshot.load_schema()?.tables.is_empty()
            || !snapshot.scheduled_execution_ids()?.is_empty()
        {
            return Err(Error::Internal(
                "materialized journal snapshot restore requires an empty tenant store".to_string(),
            ));
        }
        Ok(())
    }
}

pub(super) fn append_commit_entry(
    conn: &Connection,
    timestamp: Timestamp,
    writes: Vec<WriteOp>,
) -> Result<CommitEntry> {
    let sequence = next_sequence_in_conn(conn)?;
    let entry = CommitEntry {
        sequence: SequenceNumber(sequence),
        timestamp,
        writes,
    };
    let payload = serialize_commit(&entry)?;
    conn.execute(
        "INSERT INTO commit_log (sequence, record_blob) VALUES (?1, ?2)",
        params![sequence, payload],
    )
    .map_err(map_sqlite_error)?;
    put_metadata_in_conn(
        conn,
        NEXT_SEQUENCE_KEY,
        &encode_u64(sequence.saturating_add(1)),
    )?;
    put_metadata_in_conn(conn, APPLIED_SEQUENCE_KEY, &encode_u64(sequence))?;
    Ok(entry)
}

pub(super) fn apply_durable_record_in_conn(
    conn: &Connection,
    record: &DurableMutationRecord,
) -> Result<()> {
    if let Some(execution_id) = record.scheduled_execution_id.as_deref() {
        let _ = begin_scheduled_execution_in_conn(conn, Some(execution_id))?;
    }

    for write in &record.writes {
        match (&write.previous, &write.current) {
            (None, Some(current)) => {
                let existing = load_document_from_conn(conn, &write.table, &write.doc_id)?;
                match existing {
                    Some(existing) if existing == *current => continue,
                    Some(_) => {
                        return Err(Error::Conflict(format!(
                            "durable journal insert replay found conflicting state for document {}",
                            write.doc_id
                        )));
                    }
                    None => {
                        conn.execute(
                            "INSERT INTO documents (table_name, id, data_json, typed_fields_json, creation_time, update_time)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                            params![
                                write.table.as_str(),
                                write.doc_id.to_string(),
                                serialize_document_fields(current)?,
                                serialize_document_typed_fields(current)?,
                                current.creation_time.0,
                                current.update_time.0,
                            ],
                        )
                        .map_err(map_sqlite_error)?;
                    }
                }
                if let Some(binding) = write.resource_path_binding.as_ref() {
                    upsert_resource_path_binding_in_conn(conn, binding)?;
                }
            }
            (Some(previous), Some(current)) => {
                let existing = load_document_from_conn(conn, &write.table, &write.doc_id)?.ok_or(
                    Error::Conflict(format!(
                        "durable journal update replay missing document {}",
                        write.doc_id
                    )),
                )?;
                if existing == *current {
                    continue;
                }
                if existing != *previous {
                    return Err(Error::Conflict(format!(
                        "durable journal update replay found conflicting state for document {}",
                        write.doc_id
                    )));
                }
                conn.execute(
                    "UPDATE documents
                     SET data_json = ?3, typed_fields_json = ?4, creation_time = ?5, update_time = ?6
                     WHERE table_name = ?1 AND id = ?2",
                    params![
                        write.table.as_str(),
                        write.doc_id.to_string(),
                        serialize_document_fields(current)?,
                        serialize_document_typed_fields(current)?,
                        current.creation_time.0,
                        current.update_time.0,
                    ],
                )
                .map_err(map_sqlite_error)?;
                if let Some(binding) = write.resource_path_binding.as_ref() {
                    upsert_resource_path_binding_in_conn(conn, binding)?;
                }
            }
            (Some(previous), None) => {
                match load_document_from_conn(conn, &write.table, &write.doc_id)? {
                    Some(existing) if existing != *previous => {
                        return Err(Error::Conflict(format!(
                            "durable journal delete replay found conflicting state for document {}",
                            write.doc_id
                        )));
                    }
                    Some(_) => {
                        conn.execute(
                            "DELETE FROM documents WHERE table_name = ?1 AND id = ?2",
                            params![write.table.as_str(), write.doc_id.to_string()],
                        )
                        .map_err(map_sqlite_error)?;
                    }
                    None => continue,
                }
                remove_resource_path_binding_in_conn(
                    conn,
                    &DocumentLocator::new(write.table.clone(), write.doc_id.clone()),
                )?;
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

fn upsert_resource_path_binding_in_conn(
    conn: &Connection,
    binding: &ResourcePathBinding,
) -> Result<()> {
    let path_key = document_path_key(&binding.document_path);
    let locator_key = resource_locator_key(&binding.locator);
    let encoded_binding =
        rmp_serde::to_vec(binding).map_err(|error| Error::Serialization(error.to_string()))?;
    let encoded_locator = rmp_serde::to_vec(&binding.locator)
        .map_err(|error| Error::Serialization(error.to_string()))?;
    conn.execute(
        "INSERT INTO resource_path_bindings (
            locator_key,
            document_path_key,
            collection_group,
            binding_blob,
            locator_blob
         ) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(locator_key) DO UPDATE SET
            document_path_key = excluded.document_path_key,
            collection_group = excluded.collection_group,
            binding_blob = excluded.binding_blob,
            locator_blob = excluded.locator_blob",
        params![
            locator_key.as_slice(),
            path_key.as_slice(),
            binding.collection_group().as_str(),
            encoded_binding.as_slice(),
            encoded_locator.as_slice(),
        ],
    )
    .map_err(map_sqlite_error)?;
    Ok(())
}

fn remove_resource_path_binding_in_conn(
    conn: &Connection,
    locator: &DocumentLocator,
) -> Result<()> {
    let locator_key = resource_locator_key(locator);
    conn.execute(
        "DELETE FROM resource_path_bindings WHERE locator_key = ?1",
        params![locator_key.as_slice()],
    )
    .map_err(map_sqlite_error)?;
    Ok(())
}

pub(super) fn applied_sequence_in_conn(conn: &Connection) -> Result<SequenceNumber> {
    Ok(SequenceNumber(
        conn.query_row(
            "SELECT value_blob FROM metadata WHERE key = ?1",
            params![APPLIED_SEQUENCE_KEY],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()
        .map_err(map_sqlite_error)?
        .map(|bytes| decode_u64(bytes.as_slice()))
        .transpose()?
        .unwrap_or(0),
    ))
}

pub(super) fn latest_sequence_in_conn(conn: &Connection) -> Result<SequenceNumber> {
    Ok(SequenceNumber(
        next_sequence_in_conn(conn)?.saturating_sub(1),
    ))
}

pub(super) fn next_sequence_in_conn(conn: &Connection) -> Result<u64> {
    let stored = conn
        .query_row(
            "SELECT value_blob FROM metadata WHERE key = ?1",
            params![NEXT_SEQUENCE_KEY],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()
        .map_err(map_sqlite_error)?;
    if let Some(bytes) = stored {
        return decode_u64(bytes.as_slice());
    }

    let latest = conn
        .query_row("SELECT MAX(sequence) FROM commit_log", [], |row| {
            row.get::<_, Option<u64>>(0)
        })
        .map_err(map_sqlite_error)?
        .unwrap_or(0);
    Ok(latest.saturating_add(1))
}

pub(super) fn put_metadata_in_conn(conn: &Connection, key: &str, value: &[u8]) -> Result<()> {
    conn.execute(
        "INSERT INTO metadata (key, value_blob) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_blob = excluded.value_blob",
        params![key, value],
    )
    .map_err(map_sqlite_error)?;
    Ok(())
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
