use std::path::Path;

use neovex_core::{
    CommitEntry, Document, DocumentId, Error, Result, SequenceNumber, TableName, Timestamp,
    WriteOp, WriteOpType,
};
use redb::backends::InMemoryBackend;
use redb::{Database, ReadableTable, TableDefinition, TableError};

use crate::commit_log::{deserialize_commit, serialize_commit};
use crate::keys::{document_key, prefix_end, table_prefix};

pub(crate) const DOCUMENTS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("documents");
pub(crate) const INDEXES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("indexes");
pub(crate) const SCHEMAS: TableDefinition<&str, &[u8]> = TableDefinition::new("schemas");
pub(crate) const SCHEDULED_JOBS: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("scheduled_jobs");
pub(crate) const RUNNING_SCHEDULED_JOBS: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("running_scheduled_jobs");
pub(crate) const SCHEDULED_JOB_RESULTS: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("scheduled_job_results");
pub(crate) const SCHEDULED_JOB_EXECUTIONS: TableDefinition<&str, &[u8]> =
    TableDefinition::new("scheduled_job_executions");
pub(crate) const CRON_JOBS: TableDefinition<&str, &[u8]> = TableDefinition::new("cron_jobs");
pub(crate) const COMMIT_LOG: TableDefinition<u64, &[u8]> = TableDefinition::new("commit_log");
pub(crate) const METADATA: TableDefinition<&str, &[u8]> = TableDefinition::new("metadata");
const NEXT_SEQUENCE_KEY: &str = "next_sequence";
const EMPTY_TABLE_VALUE: &[u8] = &[];

/// Concrete redb-backed tenant store.
pub struct TenantStore {
    pub(crate) db: Database,
}

impl TenantStore {
    /// Opens or creates a tenant store on disk.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let db = Database::create(path).map_err(map_redb_error)?;
        Ok(Self { db })
    }

    /// Creates an in-memory tenant store for tests.
    pub fn create_in_memory() -> Result<Self> {
        let db = Database::builder()
            .create_with_backend(InMemoryBackend::new())
            .map_err(map_redb_error)?;
        Ok(Self { db })
    }

    /// Inserts a document and appends a commit entry.
    pub fn insert(&self, document: &Document) -> Result<CommitEntry> {
        self.insert_once(document, None)?
            .ok_or_else(|| Error::Internal("non-deduplicated insert should commit".to_string()))
    }

    /// Inserts a document once for the provided scheduled execution id.
    pub fn insert_once(
        &self,
        document: &Document,
        execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        let write = WriteOp {
            table: document.table.clone(),
            op_type: WriteOpType::Insert,
            doc_id: document.id,
        };
        let payload = document
            .to_msgpack()
            .map_err(|error| Error::Serialization(error.to_string()))?;

        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        if !begin_scheduled_execution(&write_txn, execution_id)? {
            return Ok(None);
        }
        {
            let mut documents = write_txn.open_table(DOCUMENTS).map_err(map_redb_error)?;
            let key = document_key(&document.table, &document.id);
            documents
                .insert(key.as_slice(), payload.as_slice())
                .map_err(map_redb_error)?;
        }
        let commit = append_commit(&write_txn, vec![write])?;
        write_txn.commit().map_err(map_redb_error)?;
        Ok(Some(commit))
    }

    /// Updates a document by applying a partial patch.
    pub fn update(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<CommitEntry> {
        self.update_validated(table, id, patch, |_| Ok(()))
    }

    /// Updates a document by applying a partial patch after validating the merged result.
    pub fn update_validated<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        validate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&Document) -> Result<()>,
    {
        self.update_validated_once(table, id, patch, None, validate)?
            .ok_or_else(|| Error::Internal("non-deduplicated update should commit".to_string()))
    }

    /// Updates a document once for the provided scheduled execution id.
    pub fn update_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<CommitEntry>>
    where
        F: FnOnce(&Document) -> Result<()>,
    {
        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        if !begin_scheduled_execution(&write_txn, execution_id)? {
            return Ok(None);
        }
        {
            let mut documents = write_txn.open_table(DOCUMENTS).map_err(map_redb_error)?;
            let key = document_key(table, id);
            let mut document = {
                let existing = documents
                    .get(key.as_slice())
                    .map_err(map_redb_error)?
                    .ok_or(Error::DocumentNotFound(*id))?;
                Document::from_msgpack(existing.value())
                    .map_err(|error| Error::Serialization(error.to_string()))?
            };
            for (field, value) in patch {
                document.fields.insert(field.clone(), value.clone());
            }
            validate(&document)?;
            let payload = document
                .to_msgpack()
                .map_err(|error| Error::Serialization(error.to_string()))?;
            documents
                .insert(key.as_slice(), payload.as_slice())
                .map_err(map_redb_error)?;
        }

        let commit = append_commit(
            &write_txn,
            vec![WriteOp {
                table: table.clone(),
                op_type: WriteOpType::Update,
                doc_id: *id,
            }],
        )?;
        write_txn.commit().map_err(map_redb_error)?;
        Ok(Some(commit))
    }

    /// Deletes a document if present and records a commit entry.
    pub fn delete(&self, table: &TableName, id: &DocumentId) -> Result<CommitEntry> {
        self.delete_once(table, id, None)?
            .map(|commit| commit.0)
            .ok_or_else(|| Error::Internal("non-deduplicated delete should commit".to_string()))
    }

    /// Deletes a document once for the provided scheduled execution id.
    pub fn delete_once(
        &self,
        table: &TableName,
        id: &DocumentId,
        execution_id: Option<&str>,
    ) -> Result<Option<(CommitEntry, Document)>> {
        self.delete_once_returning_document(table, id, execution_id)
    }

    /// Deletes a document if present and returns both the commit and removed snapshot.
    pub fn delete_returning_document(
        &self,
        table: &TableName,
        id: &DocumentId,
    ) -> Result<(CommitEntry, Document)> {
        self.delete_once_returning_document(table, id, None)?
            .ok_or_else(|| Error::Internal("non-deduplicated delete should commit".to_string()))
    }

    /// Deletes a document once and returns the removed snapshot for the provided scheduled execution id.
    pub fn delete_once_returning_document(
        &self,
        table: &TableName,
        id: &DocumentId,
        execution_id: Option<&str>,
    ) -> Result<Option<(CommitEntry, Document)>> {
        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        if !begin_scheduled_execution(&write_txn, execution_id)? {
            return Ok(None);
        }
        let removed_document = {
            let mut documents = write_txn.open_table(DOCUMENTS).map_err(map_redb_error)?;
            let key = document_key(table, id);
            let removed = documents.remove(key.as_slice()).map_err(map_redb_error)?;
            let removed = removed.ok_or(Error::DocumentNotFound(*id))?;
            Document::from_msgpack(removed.value())
                .map_err(|error| Error::Serialization(error.to_string()))?
        };

        let commit = append_commit(
            &write_txn,
            vec![WriteOp {
                table: table.clone(),
                op_type: WriteOpType::Delete,
                doc_id: *id,
            }],
        )?;
        write_txn.commit().map_err(map_redb_error)?;
        Ok(Some((commit, removed_document)))
    }

    /// Fetches a document by id.
    pub fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        let table_handle = match read_txn.open_table(DOCUMENTS) {
            Ok(table_handle) => table_handle,
            Err(TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(error) => return Err(map_redb_error(error)),
        };

        let key = document_key(table, id);
        match table_handle.get(key.as_slice()).map_err(map_redb_error)? {
            Some(value) => Ok(Some(
                Document::from_msgpack(value.value())
                    .map_err(|error| Error::Serialization(error.to_string()))?,
            )),
            None => Ok(None),
        }
    }

    /// Scans all documents within a logical table.
    pub fn scan_table(&self, table: &TableName) -> Result<Vec<Document>> {
        self.scan_table_cancellable(table, &mut || Ok(()))
    }

    /// Scans all documents within a logical table, checking for cancellation between rows.
    pub fn scan_table_cancellable(
        &self,
        table: &TableName,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        let table_handle = match read_txn.open_table(DOCUMENTS) {
            Ok(table_handle) => table_handle,
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(error) => return Err(map_redb_error(error)),
        };

        let start = table_prefix(table);
        let mut documents = Vec::new();
        match prefix_end(&start) {
            Some(end) => {
                let iter = table_handle
                    .range(start.as_slice()..end.as_slice())
                    .map_err(map_redb_error)?;
                for item in iter {
                    check_cancel()?;
                    let (_, value) = item.map_err(map_redb_error)?;
                    documents.push(
                        Document::from_msgpack(value.value())
                            .map_err(|error| Error::Serialization(error.to_string()))?,
                    );
                }
            }
            None => {
                let iter = table_handle
                    .range(start.as_slice()..)
                    .map_err(map_redb_error)?;
                for item in iter {
                    check_cancel()?;
                    let (key, value) = item.map_err(map_redb_error)?;
                    if !key.value().starts_with(&start) {
                        break;
                    }
                    documents.push(
                        Document::from_msgpack(value.value())
                            .map_err(|error| Error::Serialization(error.to_string()))?,
                    );
                }
            }
        }
        Ok(documents)
    }

    /// Reads commit log entries from the provided starting sequence.
    pub fn read_commit_log_from(&self, sequence: SequenceNumber) -> Result<Vec<CommitEntry>> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        let table_handle = match read_txn.open_table(COMMIT_LOG) {
            Ok(table_handle) => table_handle,
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(error) => return Err(map_redb_error(error)),
        };

        let mut entries = Vec::new();
        for item in table_handle.range(sequence.0..).map_err(map_redb_error)? {
            let (_, value) = item.map_err(map_redb_error)?;
            entries.push(deserialize_commit(value.value())?);
        }
        Ok(entries)
    }

    /// Returns the latest committed sequence number.
    pub fn latest_sequence(&self) -> Result<SequenceNumber> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        let table_handle = match read_txn.open_table(METADATA) {
            Ok(table_handle) => table_handle,
            Err(TableError::TableDoesNotExist(_)) => return Ok(SequenceNumber(0)),
            Err(error) => return Err(map_redb_error(error)),
        };

        let next = match table_handle
            .get(NEXT_SEQUENCE_KEY)
            .map_err(map_redb_error)?
        {
            Some(value) => decode_u64(value.value())?,
            None => 1,
        };
        Ok(SequenceNumber(next.saturating_sub(1)))
    }
}

pub(crate) fn append_commit(
    write_txn: &redb::WriteTransaction,
    writes: Vec<WriteOp>,
) -> Result<CommitEntry> {
    let sequence = next_sequence(write_txn)?;
    let entry = CommitEntry {
        sequence: SequenceNumber(sequence),
        timestamp: Timestamp::now(),
        writes,
    };

    let mut log = write_txn.open_table(COMMIT_LOG).map_err(map_redb_error)?;
    let payload = serialize_commit(&entry)?;
    log.insert(sequence, payload.as_slice())
        .map_err(map_redb_error)?;

    Ok(entry)
}

pub(crate) fn begin_scheduled_execution(
    write_txn: &redb::WriteTransaction,
    execution_id: Option<&str>,
) -> Result<bool> {
    let Some(execution_id) = execution_id else {
        return Ok(true);
    };

    let mut executions = write_txn
        .open_table(SCHEDULED_JOB_EXECUTIONS)
        .map_err(map_redb_error)?;
    if executions
        .get(execution_id)
        .map_err(map_redb_error)?
        .is_some()
    {
        return Ok(false);
    }
    executions
        .insert(execution_id, EMPTY_TABLE_VALUE)
        .map_err(map_redb_error)?;
    Ok(true)
}

fn next_sequence(write_txn: &redb::WriteTransaction) -> Result<u64> {
    let mut metadata = write_txn.open_table(METADATA).map_err(map_redb_error)?;
    let current = match metadata.get(NEXT_SEQUENCE_KEY).map_err(map_redb_error)? {
        Some(value) => decode_u64(value.value())?,
        None => 1,
    };
    let next = current + 1;
    metadata
        .insert(NEXT_SEQUENCE_KEY, encode_u64(next).as_slice())
        .map_err(map_redb_error)?;
    Ok(current)
}

fn encode_u64(value: u64) -> [u8; 8] {
    value.to_be_bytes()
}

fn decode_u64(bytes: &[u8]) -> Result<u64> {
    let array: [u8; 8] = bytes
        .try_into()
        .map_err(|_| Error::Internal("expected 8 bytes when decoding u64 metadata".to_string()))?;
    Ok(u64::from_be_bytes(array))
}

pub(crate) fn map_redb_error(error: impl std::fmt::Display) -> Error {
    Error::Storage(error.to_string())
}
