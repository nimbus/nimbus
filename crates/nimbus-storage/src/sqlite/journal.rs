#[cfg(any(test, feature = "test-hooks"))]
use super::config::observe_sqlite_foreground_commit;
#[cfg(test)]
use super::config::{
    SqliteWriteStatementConcept, observe_sqlite_cached_statement,
    observe_sqlite_current_document_encode,
};
use super::*;
use crate::keys::{document_path_key, resource_locator_key};
use crate::simulation::DurableApplyKind;
use crate::sqlite::document_versions::{
    record_document_versions_for_events_in_conn, record_document_versions_for_writes_in_conn,
};
use crate::sqlite::index_versions::{
    record_index_versions_for_events_in_conn, record_index_versions_for_writes_in_conn,
};
use crate::store::TRIGGER_DELIVERY_CURSOR_KEY;
use crate::table_identity::{
    DEFAULT_TABLE_NAMESPACE, deleting_table_namespace, hidden_table_namespace,
};
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
    ) -> Result<Vec<TenantEventRecord>> {
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

        let conn = self.acquire_writer_connection()?;
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(map_sqlite_error)?;
        for identity in &snapshot.table_identities {
            ensure_table_identity_in_conn(&conn, identity)?;
        }
        for table_schema in snapshot.schema.tables.values() {
            conn.execute(
                "INSERT INTO schemas (table_name, schema_json) VALUES (?1, ?2)",
                params![table_schema.table.as_str(), serialize_json(table_schema)?],
            )
            .map_err(map_sqlite_error)?;
        }
        for document in &snapshot.documents {
            let table_id = snapshot.default_table_id(&document.table)?;
            cached_execute(
                &conn,
                "INSERT INTO documents (table_id, id, data_json, typed_fields_json, creation_time, update_time)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    table_id.as_str(),
                    document.id.to_string(),
                    serialize_document_fields(document)?,
                    serialize_document_typed_fields(document)?,
                    document.creation_time.0,
                    document.update_time.0,
                ],
            )?;
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
        #[cfg(any(test, feature = "test-hooks"))]
        let commit_started = std::time::Instant::now();
        conn.execute_batch("COMMIT").map_err(map_sqlite_error)?;
        #[cfg(any(test, feature = "test-hooks"))]
        observe_sqlite_foreground_commit(&self.path, &conn, commit_started.elapsed());
        self.release_writer_connection(conn);
        self.replace_cached_schema(snapshot.schema.clone())?;
        Ok(())
    }

    pub fn rebuild_materialized_journal_from_snapshot(
        &self,
        snapshot: &MaterializedJournalSnapshot,
        journal_tail: &[TenantEventRecord],
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

    pub fn export_point_in_time_restore_archive(
        &self,
        target: PointInTimeRestoreTarget,
        retention_config: RetentionGcConfig,
    ) -> Result<PointInTimeRestoreArchive> {
        let records = self.read_durable_journal_from(SequenceNumber(1))?;
        let progress = self.journal_progress()?;
        let watermarks = self.retention_gc_watermarks(retention_config)?;
        crate::store::build_point_in_time_restore_archive(
            target,
            records,
            progress.durable_head,
            watermarks.document_versions.safe_prune_before,
        )
    }

    pub fn import_point_in_time_restore_archive(
        &self,
        archive: &PointInTimeRestoreArchive,
    ) -> Result<JournalProgress> {
        archive.validate()?;
        let progress = self.rebuild_materialized_journal_from_snapshot(
            &archive.base_snapshot,
            &archive.journal_tail,
            Some(archive.target_sequence),
        )?;
        let restored_fingerprint = self
            .export_materialized_journal_snapshot()?
            .canonical_fingerprint()?;
        if restored_fingerprint != archive.target_fingerprint {
            return Err(Error::storage(
                nimbus_core::StorageErrorKind::Corruption,
                format!(
                    "point-in-time restore fingerprint mismatch: restored {} expected {}",
                    restored_fingerprint, archive.target_fingerprint
                ),
            ));
        }
        Ok(progress)
    }

    pub fn append_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }

        let conn = self.acquire_writer_connection()?;
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(map_sqlite_error)?;
        #[cfg(test)]
        observe_sqlite_cached_statement(
            &self.path,
            SqliteWriteStatementConcept::JournalNextSequenceRead,
        );
        let mut next = latest_sequence_in_conn(&conn)?.0.saturating_add(1);
        for record in records {
            if record.sequence.0 != next {
                return Err(Error::Internal(format!(
                    "durable journal append expected sequence {}, got {}",
                    next, record.sequence.0
                )));
            }
            #[cfg(test)]
            observe_sqlite_cached_statement(&self.path, SqliteWriteStatementConcept::JournalInsert);
            cached_execute(
                &conn,
                "INSERT INTO commit_log (sequence, record_blob) VALUES (?1, ?2)",
                params![record.sequence.0, serialize_tenant_event_record(record)?],
            )?;
            next = next.saturating_add(1);
        }
        #[cfg(test)]
        observe_sqlite_cached_statement(&self.path, SqliteWriteStatementConcept::NextSequenceWrite);
        put_metadata_in_conn(&conn, NEXT_SEQUENCE_KEY, &encode_u64(next))?;
        self.fault_injector
            .check_durable_records(FaultPoint::JournalAppendBeforeDurableFlush, records)?;
        #[cfg(any(test, feature = "test-hooks"))]
        let commit_started = std::time::Instant::now();
        conn.execute_batch("COMMIT").map_err(map_sqlite_error)?;
        #[cfg(any(test, feature = "test-hooks"))]
        observe_sqlite_foreground_commit(&self.path, &conn, commit_started.elapsed());
        self.release_writer_connection(conn);
        self.fault_injector
            .check_durable_records(FaultPoint::JournalFlushBeforeVisibility, records)?;
        Ok(())
    }

    pub fn apply_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        self.apply_durable_records_batch_as(records, DurableApplyKind::ClientBatch)
    }

    /// See [`DurableApplyKind::JournalReplay`]: recovery re-applies records that
    /// are already durable, so this boundary names none.
    pub fn replay_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        self.apply_durable_records_batch_as(records, DurableApplyKind::JournalReplay)
    }

    fn apply_durable_records_batch_as(
        &self,
        records: &[TenantEventRecord],
        kind: DurableApplyKind,
    ) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let fault_records = kind.newly_durable_records(records);

        let schema_cache_dirty = records.iter().any(durable_record_changes_schema_cache);
        let conn = self.acquire_writer_connection()?;
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(map_sqlite_error)?;
        #[cfg(test)]
        observe_sqlite_cached_statement(
            &self.path,
            SqliteWriteStatementConcept::AppliedSequenceRead,
        );
        let mut applied_head = applied_sequence_in_conn(&conn)?.0;
        let mut apply_context = SqliteBatchApplyContext::new();
        for record in records {
            if record.sequence.0 <= applied_head {
                #[cfg(test)]
                observe_sqlite_cached_statement(
                    &self.path,
                    SqliteWriteStatementConcept::DurableRecordRead,
                );
                let payload = conn
                    .prepare_cached("SELECT record_blob FROM commit_log WHERE sequence = ?1")
                    .map_err(map_sqlite_error)?
                    .query_row(params![record.sequence.0], |row| row.get::<_, Vec<u8>>(0))
                    .optional()
                    .map_err(map_sqlite_error)?;
                let durable = payload
                    .as_deref()
                    .map(deserialize_tenant_event_record)
                    .transpose()?;
                crate::commit_log::ensure_applied_record_matches(record, durable.as_ref())?;
                continue;
            }
            if record.sequence.0 != applied_head.saturating_add(1) {
                return Err(Error::Internal(format!(
                    "durable journal apply expected sequence {}, got {}",
                    applied_head.saturating_add(1),
                    record.sequence.0
                )));
            }
            apply_durable_record_in_conn(
                &conn,
                record,
                &mut apply_context,
                #[cfg(test)]
                &self.path,
            )?;
            applied_head = record.sequence.0;
        }

        if applied_head >= records[0].sequence.0 {
            #[cfg(test)]
            observe_sqlite_cached_statement(
                &self.path,
                SqliteWriteStatementConcept::AppliedSequenceWrite,
            );
            put_metadata_in_conn(&conn, APPLIED_SEQUENCE_KEY, &encode_u64(applied_head))?;
        }
        self.fault_injector
            .check_durable_records(FaultPoint::StorageCommitBeforeVisibility, fault_records)?;
        #[cfg(any(test, feature = "test-hooks"))]
        let commit_started = std::time::Instant::now();
        conn.execute_batch("COMMIT").map_err(map_sqlite_error)?;
        #[cfg(any(test, feature = "test-hooks"))]
        observe_sqlite_foreground_commit(&self.path, &conn, commit_started.elapsed());
        if schema_cache_dirty {
            self.replace_cached_schema(load_schema_from_conn(&conn)?)?;
        }
        self.release_writer_connection(conn);
        self.fault_injector.check_durable_records(
            FaultPoint::StorageCommitAfterVisibilityBeforeReturn,
            fault_records,
        )?;
        Ok(())
    }

    pub fn recover_durable_journal(&self) -> Result<JournalProgress> {
        let progress = self.journal_progress()?;
        if progress.applied_head.0 >= progress.durable_head.0 {
            return Ok(progress);
        }
        let from = SequenceNumber(progress.applied_head.0.saturating_add(1));
        let pending = self.read_durable_journal_from(from)?;
        self.replay_durable_records_batch(&pending)?;
        self.journal_progress()
    }

    fn ensure_materialized_journal_restore_target_is_empty(&self) -> Result<()> {
        let snapshot = self.read_snapshot()?;
        let progress = snapshot.journal_progress()?;
        if progress.durable_head.0 != 0
            || progress.applied_head.0 != 0
            || !snapshot.documents()?.is_empty()
            || !snapshot.load_schema()?.tables.is_empty()
            || !snapshot.table_identities()?.is_empty()
            || !snapshot.scheduled_execution_ids()?.is_empty()
        {
            return Err(Error::Internal(
                "materialized journal snapshot restore requires an empty tenant store".to_string(),
            ));
        }
        Ok(())
    }
}

fn durable_record_changes_schema_cache(record: &TenantEventRecord) -> bool {
    record.events.iter().any(|event| {
        matches!(
            event,
            TenantEventKind::SchemaChange { .. }
                | TenantEventKind::TableLifecycle {
                    lifecycle: TableLifecycleEvent::HardDelete { .. }
                }
        )
    })
}

pub(super) fn append_commit_entry(
    conn: &Connection,
    timestamp: Timestamp,
    writes: Vec<WriteOp>,
    events: Vec<TenantEventKind>,
    #[cfg(test)] observation_path: &Path,
) -> Result<CommitEntry> {
    #[cfg(test)]
    observe_sqlite_cached_statement(
        observation_path,
        SqliteWriteStatementConcept::JournalNextSequenceRead,
    );
    let sequence = next_sequence_in_conn(conn)?;
    let record = append_tenant_event_record(
        conn,
        SequenceNumber(sequence),
        timestamp,
        events,
        #[cfg(test)]
        observation_path,
    )?;
    Ok(CommitEntry {
        sequence: record.sequence,
        timestamp: record.timestamp,
        writes,
    })
}

pub(super) fn append_prepared_commit_entry(
    conn: &Connection,
    record: &TenantEventRecord,
    #[cfg(test)] observation_path: &Path,
) -> Result<CommitEntry> {
    record.validate_integrity()?;
    #[cfg(test)]
    observe_sqlite_cached_statement(
        observation_path,
        SqliteWriteStatementConcept::JournalNextSequenceRead,
    );
    let expected = next_sequence_in_conn(conn)?;
    if record.sequence.0 != expected {
        return Err(Error::conflict(format!(
            "prepared commit expected storage sequence {expected}, got {}",
            record.sequence.0
        )));
    }
    #[cfg(test)]
    observe_sqlite_cached_statement(
        observation_path,
        SqliteWriteStatementConcept::AppliedSequenceRead,
    );
    crate::commit_log::ensure_applied_prefix_precedes(
        applied_sequence_in_conn(conn)?,
        record.sequence,
    )?;
    let payload = serialize_tenant_event_record(record)?;
    #[cfg(test)]
    observe_sqlite_cached_statement(observation_path, SqliteWriteStatementConcept::JournalInsert);
    cached_execute(
        conn,
        "INSERT INTO commit_log (sequence, record_blob) VALUES (?1, ?2)",
        params![record.sequence.0, payload],
    )?;
    #[cfg(test)]
    observe_sqlite_cached_statement(
        observation_path,
        SqliteWriteStatementConcept::NextSequenceWrite,
    );
    put_metadata_in_conn(
        conn,
        NEXT_SEQUENCE_KEY,
        &encode_u64(record.sequence.0.saturating_add(1)),
    )?;
    #[cfg(test)]
    observe_sqlite_cached_statement(
        observation_path,
        SqliteWriteStatementConcept::AppliedSequenceWrite,
    );
    put_metadata_in_conn(conn, APPLIED_SEQUENCE_KEY, &encode_u64(record.sequence.0))?;
    Ok(record.as_commit_entry())
}

pub(super) fn append_tenant_event_record(
    conn: &Connection,
    sequence: SequenceNumber,
    timestamp: Timestamp,
    events: Vec<TenantEventKind>,
    #[cfg(test)] observation_path: &Path,
) -> Result<TenantEventRecord> {
    #[cfg(test)]
    observe_sqlite_cached_statement(
        observation_path,
        SqliteWriteStatementConcept::AppliedSequenceRead,
    );
    crate::commit_log::ensure_applied_prefix_precedes(applied_sequence_in_conn(conn)?, sequence)?;
    let record = TenantEventRecord::from_events(sequence, timestamp, events)?;
    let mut apply_context = SqliteBatchApplyContext::new();
    record_document_versions_for_events_in_conn(
        conn,
        record.sequence,
        record.timestamp,
        &record.events,
        &mut apply_context,
        #[cfg(test)]
        observation_path,
    )?;
    record_index_versions_for_events_in_conn(
        conn,
        record.sequence,
        &record.events,
        &mut apply_context,
        #[cfg(test)]
        observation_path,
    )?;
    let payload = serialize_tenant_event_record(&record)?;
    #[cfg(test)]
    observe_sqlite_cached_statement(observation_path, SqliteWriteStatementConcept::JournalInsert);
    cached_execute(
        conn,
        "INSERT INTO commit_log (sequence, record_blob) VALUES (?1, ?2)",
        params![sequence.0, payload],
    )?;
    #[cfg(test)]
    observe_sqlite_cached_statement(
        observation_path,
        SqliteWriteStatementConcept::NextSequenceWrite,
    );
    put_metadata_in_conn(
        conn,
        NEXT_SEQUENCE_KEY,
        &encode_u64(sequence.0.saturating_add(1)),
    )?;
    #[cfg(test)]
    observe_sqlite_cached_statement(
        observation_path,
        SqliteWriteStatementConcept::AppliedSequenceWrite,
    );
    put_metadata_in_conn(conn, APPLIED_SEQUENCE_KEY, &encode_u64(sequence.0))?;
    Ok(record)
}

pub(super) fn apply_durable_record_in_conn(
    conn: &Connection,
    record: &TenantEventRecord,
    apply_context: &mut SqliteBatchApplyContext,
    #[cfg(test)] observation_path: &Path,
) -> Result<()> {
    if record.events.is_empty() {
        if let Some(execution_id) = record.scheduled_execution_id.as_deref() {
            let _ = begin_scheduled_execution_in_conn(conn, Some(execution_id))?;
        }
        record_document_versions_for_writes_in_conn(
            conn,
            record.sequence,
            record.timestamp,
            &record.writes,
            apply_context,
            #[cfg(test)]
            observation_path,
        )?;
        record_index_versions_for_writes_in_conn(
            conn,
            record.sequence,
            &record.writes,
            apply_context,
            #[cfg(test)]
            observation_path,
        )?;
        return apply_document_writes_in_conn(
            conn,
            record.sequence,
            &record.writes,
            apply_context,
            #[cfg(test)]
            observation_path,
        );
    }

    record_document_versions_for_events_in_conn(
        conn,
        record.sequence,
        record.timestamp,
        &record.events,
        apply_context,
        #[cfg(test)]
        observation_path,
    )?;
    record_index_versions_for_events_in_conn(
        conn,
        record.sequence,
        &record.events,
        apply_context,
        #[cfg(test)]
        observation_path,
    )?;
    for event in &record.events {
        apply_tenant_event_in_conn(
            conn,
            record.sequence,
            event,
            apply_context,
            #[cfg(test)]
            observation_path,
        )?;
    }
    Ok(())
}

fn apply_tenant_event_in_conn(
    conn: &Connection,
    sequence: SequenceNumber,
    event: &TenantEventKind,
    apply_context: &mut SqliteBatchApplyContext,
    #[cfg(test)] observation_path: &Path,
) -> Result<()> {
    match event {
        TenantEventKind::DocumentWrite { writes } => apply_document_writes_in_conn(
            conn,
            sequence,
            writes,
            apply_context,
            #[cfg(test)]
            observation_path,
        ),
        TenantEventKind::SchemaChange { change } => {
            apply_schema_change_in_conn(conn, change)?;
            // Later records in this batch must observe the post-change
            // schema, index plans, and catalog identities.
            apply_context.invalidate_table_invariants();
            Ok(())
        }
        TenantEventKind::TableLifecycle { lifecycle } => {
            apply_table_lifecycle_in_conn(conn, lifecycle)?;
            apply_context.invalidate_table_invariants();
            Ok(())
        }
        TenantEventKind::IndexLifecycle { .. } => {
            // Index lifecycle intervals are derived at read time; drop cached
            // plans anyway so no stale maintained-index view survives.
            apply_context.invalidate_table_invariants();
            Ok(())
        }
        TenantEventKind::Barrier { .. } => Ok(()),
        TenantEventKind::ScheduledExecution { execution_id } => {
            let _ = begin_scheduled_execution_in_conn(conn, Some(execution_id))?;
            Ok(())
        }
        TenantEventKind::TriggerDelivery { cursor } => put_metadata_in_conn(
            conn,
            TRIGGER_DELIVERY_CURSOR_KEY,
            &encode_u64(cursor.materialized_through.0),
        ),
    }
}

fn apply_document_writes_in_conn(
    conn: &Connection,
    sequence: SequenceNumber,
    writes: &[WriteOp],
    apply_context: &mut SqliteBatchApplyContext,
    #[cfg(test)] observation_path: &Path,
) -> Result<()> {
    for write in writes {
        apply_document_write_in_conn(
            conn,
            sequence,
            write,
            apply_context,
            #[cfg(test)]
            observation_path,
        )?;
    }
    Ok(())
}

fn apply_document_write_in_conn(
    conn: &Connection,
    sequence: SequenceNumber,
    write: &WriteOp,
    apply_context: &mut SqliteBatchApplyContext,
    #[cfg(test)] observation_path: &Path,
) -> Result<()> {
    apply_context.ensure_table_identity(
        conn,
        &write.table,
        &write.table_id,
        #[cfg(test)]
        observation_path,
    )?;
    match (&write.previous, &write.current) {
        (None, Some(current)) => {
            #[cfg(test)]
            observe_sqlite_cached_statement(
                observation_path,
                SqliteWriteStatementConcept::DocumentPreimageRead,
            );
            let existing = load_document_by_table_id_from_conn(
                conn,
                &write.table,
                &write.table_id,
                &write.doc_id,
            )?;
            match existing {
                Some(existing) if existing == *current => return Ok(()),
                Some(_) => {
                    return Err(crate::commit_log::durable_replay_preimage_corruption(
                        sequence,
                        "insert",
                        write.doc_id.as_str(),
                        "found unexpected state",
                    ));
                }
                None => {
                    #[cfg(test)]
                    {
                        observe_sqlite_current_document_encode(observation_path);
                        observe_sqlite_cached_statement(
                            observation_path,
                            SqliteWriteStatementConcept::LiveDocumentInsert,
                        );
                    }
                    cached_execute(
                        conn,
                        "INSERT INTO documents (table_id, id, data_json, typed_fields_json, creation_time, update_time)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        params![
                            write.table_id.as_str(),
                            write.doc_id.to_string(),
                            serialize_document_fields(current)?,
                            serialize_document_typed_fields(current)?,
                            current.creation_time.0,
                            current.update_time.0,
                        ],
                    )?;
                }
            }
            if let Some(binding) = write.resource_path_binding.as_ref() {
                upsert_resource_path_binding_in_conn(
                    conn,
                    binding,
                    #[cfg(test)]
                    observation_path,
                )?;
            }
        }
        (Some(previous), Some(current)) => {
            #[cfg(test)]
            observe_sqlite_cached_statement(
                observation_path,
                SqliteWriteStatementConcept::DocumentPreimageRead,
            );
            let existing = load_document_by_table_id_from_conn(
                conn,
                &write.table,
                &write.table_id,
                &write.doc_id,
            )?
            .ok_or_else(|| {
                crate::commit_log::durable_replay_preimage_corruption(
                    sequence,
                    "update",
                    write.doc_id.as_str(),
                    "is missing the expected pre-image",
                )
            })?;
            if existing == *current {
                return Ok(());
            }
            if existing != *previous {
                return Err(crate::commit_log::durable_replay_preimage_corruption(
                    sequence,
                    "update",
                    write.doc_id.as_str(),
                    "found a pre-image mismatch",
                ));
            }
            #[cfg(test)]
            {
                observe_sqlite_current_document_encode(observation_path);
                observe_sqlite_cached_statement(
                    observation_path,
                    SqliteWriteStatementConcept::LiveDocumentUpdate,
                );
            }
            cached_execute(
                conn,
                "UPDATE documents
                 SET data_json = ?3, typed_fields_json = ?4, creation_time = ?5, update_time = ?6
                 WHERE table_id = ?1 AND id = ?2",
                params![
                    write.table_id.as_str(),
                    write.doc_id.to_string(),
                    serialize_document_fields(current)?,
                    serialize_document_typed_fields(current)?,
                    current.creation_time.0,
                    current.update_time.0,
                ],
            )?;
            if let Some(binding) = write.resource_path_binding.as_ref() {
                upsert_resource_path_binding_in_conn(
                    conn,
                    binding,
                    #[cfg(test)]
                    observation_path,
                )?;
            }
        }
        (Some(previous), None) => {
            #[cfg(test)]
            observe_sqlite_cached_statement(
                observation_path,
                SqliteWriteStatementConcept::DocumentPreimageRead,
            );
            match load_document_by_table_id_from_conn(
                conn,
                &write.table,
                &write.table_id,
                &write.doc_id,
            )? {
                Some(existing) if existing != *previous => {
                    return Err(crate::commit_log::durable_replay_preimage_corruption(
                        sequence,
                        "delete",
                        write.doc_id.as_str(),
                        "found a pre-image mismatch",
                    ));
                }
                Some(_) => {
                    #[cfg(test)]
                    observe_sqlite_cached_statement(
                        observation_path,
                        SqliteWriteStatementConcept::LiveDocumentDelete,
                    );
                    cached_execute(
                        conn,
                        "DELETE FROM documents WHERE table_id = ?1 AND id = ?2",
                        params![write.table_id.as_str(), write.doc_id.to_string()],
                    )?;
                }
                None => return Ok(()),
            }
            remove_resource_path_binding_in_conn(
                conn,
                &DocumentLocator::new(write.table.clone(), write.doc_id.clone()),
                #[cfg(test)]
                observation_path,
            )?;
        }
        (None, None) => {
            return Err(Error::Internal(
                "durable journal write must include a previous or current document".to_string(),
            ));
        }
    }
    Ok(())
}

fn apply_schema_change_in_conn(conn: &Connection, change: &SchemaChangeEvent) -> Result<()> {
    match change {
        SchemaChangeEvent::SetTable {
            table,
            table_id,
            previous,
            current,
        } => {
            ensure_table_id_in_conn(conn, table, table_id)?;
            if let Some(previous) = previous {
                drop_sqlite_indexes_for_table_schema(conn, previous)?;
            }
            conn.execute(
                "INSERT INTO schemas (table_name, schema_json) VALUES (?1, ?2)
                 ON CONFLICT(table_name) DO UPDATE SET schema_json = excluded.schema_json",
                params![table.as_str(), serialize_json(current)?],
            )
            .map_err(map_sqlite_error)?;
            create_sqlite_indexes_for_table_schema(conn, current)
        }
        SchemaChangeEvent::DeleteTable {
            table, previous, ..
        } => {
            if let Some(previous) = previous {
                drop_sqlite_indexes_for_table_schema(conn, previous)?;
            }
            conn.execute(
                "DELETE FROM schemas WHERE table_name = ?1",
                params![table.as_str()],
            )
            .map_err(map_sqlite_error)?;
            Ok(())
        }
    }
}

fn apply_table_lifecycle_in_conn(conn: &Connection, lifecycle: &TableLifecycleEvent) -> Result<()> {
    match lifecycle {
        TableLifecycleEvent::StageHidden { table, table_id } => {
            conn.execute(
                "INSERT INTO table_catalog (namespace, table_name, table_id, state)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    hidden_table_namespace(table_id),
                    table.as_str(),
                    table_id.as_str(),
                    TableState::Hidden.as_str()
                ],
            )
            .map_err(map_sqlite_error)?;
            Ok(())
        }
        TableLifecycleEvent::ActivateHidden {
            table, table_id, ..
        } => {
            if let Some(active_table_id) = resolve_table_id_in_conn(conn, table)? {
                conn.execute(
                    "UPDATE table_catalog
                     SET namespace = ?1, state = ?2
                     WHERE namespace = ?3 AND table_name = ?4",
                    params![
                        deleting_table_namespace(&active_table_id),
                        TableState::Deleting.as_str(),
                        DEFAULT_TABLE_NAMESPACE,
                        table.as_str()
                    ],
                )
                .map_err(map_sqlite_error)?;
            }
            conn.execute(
                "UPDATE table_catalog
                 SET namespace = ?1, state = ?2
                 WHERE namespace = ?3 AND table_name = ?4 AND table_id = ?5",
                params![
                    DEFAULT_TABLE_NAMESPACE,
                    TableState::Active.as_str(),
                    hidden_table_namespace(table_id),
                    table.as_str(),
                    table_id.as_str()
                ],
            )
            .map_err(map_sqlite_error)?;
            Ok(())
        }
        TableLifecycleEvent::MarkDeleting { table, table_id } => {
            conn.execute(
                "UPDATE table_catalog
                 SET namespace = ?1, state = ?2
                 WHERE namespace = ?3 AND table_name = ?4 AND table_id = ?5",
                params![
                    deleting_table_namespace(table_id),
                    TableState::Deleting.as_str(),
                    DEFAULT_TABLE_NAMESPACE,
                    table.as_str(),
                    table_id.as_str()
                ],
            )
            .map_err(map_sqlite_error)?;
            Ok(())
        }
        TableLifecycleEvent::HardDelete { table, table_id } => {
            cached_execute(
                conn,
                "DELETE FROM documents WHERE table_id = ?1",
                params![table_id.as_str()],
            )?;
            conn.execute(
                "DELETE FROM table_catalog WHERE table_id = ?1",
                params![table_id.as_str()],
            )
            .map_err(map_sqlite_error)?;
            if resolve_table_id_in_conn(conn, table)?.is_none() {
                if let Some(schema) = load_table_schema_from_conn(conn, table)? {
                    drop_sqlite_indexes_for_table_schema(conn, &schema)?;
                }
                conn.execute(
                    "DELETE FROM schemas WHERE table_name = ?1",
                    params![table.as_str()],
                )
                .map_err(map_sqlite_error)?;
            }
            Ok(())
        }
    }
}

fn upsert_resource_path_binding_in_conn(
    conn: &Connection,
    binding: &ResourcePathBinding,
    #[cfg(test)] observation_path: &Path,
) -> Result<()> {
    let path_key = document_path_key(&binding.document_path);
    let locator_key = resource_locator_key(&binding.locator);
    let encoded_binding =
        rmp_serde::to_vec(binding).map_err(|error| Error::Serialization(error.to_string()))?;
    let encoded_locator = rmp_serde::to_vec(&binding.locator)
        .map_err(|error| Error::Serialization(error.to_string()))?;
    #[cfg(test)]
    observe_sqlite_cached_statement(
        observation_path,
        SqliteWriteStatementConcept::ResourceBindingUpsert,
    );
    cached_execute(
        conn,
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
    )?;
    Ok(())
}

fn remove_resource_path_binding_in_conn(
    conn: &Connection,
    locator: &DocumentLocator,
    #[cfg(test)] observation_path: &Path,
) -> Result<()> {
    let locator_key = resource_locator_key(locator);
    #[cfg(test)]
    observe_sqlite_cached_statement(
        observation_path,
        SqliteWriteStatementConcept::ResourceBindingDelete,
    );
    cached_execute(
        conn,
        "DELETE FROM resource_path_bindings WHERE locator_key = ?1",
        params![locator_key.as_slice()],
    )?;
    Ok(())
}

pub(super) fn applied_sequence_in_conn(conn: &Connection) -> Result<SequenceNumber> {
    Ok(SequenceNumber(
        conn.prepare_cached("SELECT value_blob FROM metadata WHERE key = ?1")
            .map_err(map_sqlite_error)?
            .query_row(params![APPLIED_SEQUENCE_KEY], |row| {
                row.get::<_, Vec<u8>>(0)
            })
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
        .prepare_cached("SELECT value_blob FROM metadata WHERE key = ?1")
        .map_err(map_sqlite_error)?
        .query_row(params![NEXT_SEQUENCE_KEY], |row| row.get::<_, Vec<u8>>(0))
        .optional()
        .map_err(map_sqlite_error)?;
    if let Some(bytes) = stored {
        return decode_u64(bytes.as_slice());
    }

    let latest = conn
        .prepare_cached("SELECT MAX(sequence) FROM commit_log")
        .map_err(map_sqlite_error)?
        .query_row([], |row| row.get::<_, Option<u64>>(0))
        .map_err(map_sqlite_error)?
        .unwrap_or(0);
    Ok(latest.saturating_add(1))
}

pub(super) fn put_metadata_in_conn(conn: &Connection, key: &str, value: &[u8]) -> Result<()> {
    cached_execute(
        conn,
        "INSERT INTO metadata (key, value_blob) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_blob = excluded.value_blob",
        params![key, value],
    )?;
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
