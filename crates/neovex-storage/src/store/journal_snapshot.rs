use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct MaterializedJournalSnapshot {
    pub version: u16,
    pub applied_sequence: SequenceNumber,
    pub durable_head: SequenceNumber,
    pub schema: Schema,
    pub documents: Vec<Document>,
    pub scheduled_execution_ids: Vec<String>,
}

const MATERIALIZED_JOURNAL_SNAPSHOT_VERSION: u16 = 1;

impl MaterializedJournalSnapshot {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.version != MATERIALIZED_JOURNAL_SNAPSHOT_VERSION {
            return Err(Error::InvalidInput(format!(
                "unsupported materialized journal snapshot version {}",
                self.version
            )));
        }
        if self.applied_sequence.0 > self.durable_head.0 {
            return Err(Error::InvalidInput(format!(
                "materialized journal snapshot applied sequence {} exceeds durable head {}",
                self.applied_sequence.0, self.durable_head.0
            )));
        }
        Ok(())
    }
}

impl TenantStore {
    pub fn export_materialized_journal_snapshot(&self) -> Result<MaterializedJournalSnapshot> {
        self.read_snapshot()?.export_materialized_journal_snapshot()
    }

    pub fn restore_materialized_journal_from_snapshot(
        &self,
        snapshot: &MaterializedJournalSnapshot,
    ) -> Result<()> {
        snapshot.validate()?;
        self.ensure_materialized_journal_restore_target_is_empty()?;

        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        {
            let mut schema_table = write_txn.open_table(SCHEMAS).map_err(map_redb_error)?;
            for table_schema in snapshot.schema.tables.values() {
                let payload = rmp_serde::to_vec(table_schema)
                    .map_err(|error| Error::Serialization(error.to_string()))?;
                schema_table
                    .insert(table_schema.table.as_str(), payload.as_slice())
                    .map_err(map_redb_error)?;
            }
        }
        {
            let mut documents = write_txn.open_table(DOCUMENTS).map_err(map_redb_error)?;
            for document in &snapshot.documents {
                let payload = document
                    .to_msgpack()
                    .map_err(|error| Error::Serialization(error.to_string()))?;
                let key = document_key(&document.table, &document.id);
                documents
                    .insert(key.as_slice(), payload.as_slice())
                    .map_err(map_redb_error)?;
            }
        }
        {
            let mut index_table = write_txn.open_table(INDEXES).map_err(map_redb_error)?;
            for document in &snapshot.documents {
                let Some(table_schema) = snapshot.schema.get_table(&document.table) else {
                    continue;
                };
                for key in durable_record_index_keys(document, table_schema)? {
                    index_table
                        .insert(key.as_slice(), EMPTY_TABLE_VALUE)
                        .map_err(map_redb_error)?;
                }
            }
        }
        {
            let mut executions = write_txn
                .open_table(SCHEDULED_JOB_EXECUTIONS)
                .map_err(map_redb_error)?;
            for execution_id in &snapshot.scheduled_execution_ids {
                executions
                    .insert(execution_id.as_str(), EMPTY_TABLE_VALUE)
                    .map_err(map_redb_error)?;
            }
        }
        {
            let mut metadata = write_txn.open_table(METADATA).map_err(map_redb_error)?;
            metadata
                .insert(
                    NEXT_SEQUENCE_KEY,
                    encode_u64(snapshot.applied_sequence.0.saturating_add(1)).as_slice(),
                )
                .map_err(map_redb_error)?;
            metadata
                .insert(
                    APPLIED_SEQUENCE_KEY,
                    encode_u64(snapshot.applied_sequence.0).as_slice(),
                )
                .map_err(map_redb_error)?;
        }
        self.commit_write_txn(write_txn)?;
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
        self.append_durable_records_batch(tail)?;
        self.recover_durable_journal()
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

impl TenantReadSnapshot {
    pub fn export_materialized_journal_snapshot(&self) -> Result<MaterializedJournalSnapshot> {
        let progress = self.journal_progress()?;
        Ok(MaterializedJournalSnapshot {
            version: MATERIALIZED_JOURNAL_SNAPSHOT_VERSION,
            applied_sequence: progress.applied_head,
            durable_head: progress.durable_head,
            schema: self.load_schema()?,
            documents: self.documents()?,
            scheduled_execution_ids: self.scheduled_execution_ids()?,
        })
    }

    pub fn documents(&self) -> Result<Vec<Document>> {
        let table_handle = match self.read_txn.open_table(DOCUMENTS) {
            Ok(table_handle) => table_handle,
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(error) => return Err(map_redb_error(error)),
        };

        let mut documents = Vec::new();
        for item in table_handle.iter().map_err(map_redb_error)? {
            let (_, value) = item.map_err(map_redb_error)?;
            documents.push(
                Document::from_msgpack(value.value())
                    .map_err(|error| Error::Serialization(error.to_string()))?,
            );
        }

        Ok(documents)
    }

    pub fn scheduled_execution_ids(&self) -> Result<Vec<String>> {
        let table_handle = match self.read_txn.open_table(SCHEDULED_JOB_EXECUTIONS) {
            Ok(table_handle) => table_handle,
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(error) => return Err(map_redb_error(error)),
        };

        let mut execution_ids = Vec::new();
        for item in table_handle.iter().map_err(map_redb_error)? {
            let (key, _) = item.map_err(map_redb_error)?;
            execution_ids.push(key.value().to_string());
        }
        execution_ids.sort_unstable();
        Ok(execution_ids)
    }
}
