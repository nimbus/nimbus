use std::path::Path;
use std::sync::Arc;

use neovex_core::{
    CommitEntry, Document, DocumentId, Error, IndexDefinition, JobId, Result, ScheduledJob,
    SequenceNumber, TableName, Timestamp, WriteOp, WriteOpType,
};
use redb::backends::InMemoryBackend;
use redb::{Database, ReadTransaction, ReadableTable, TableDefinition, TableError};

use crate::commit_log::{deserialize_commit, serialize_commit};
use crate::index::{encode_index_value, index_key};
use crate::keys::{document_key, prefix_end, table_prefix};
use crate::scheduler::{cancel_scheduled_job_in_write_txn, insert_scheduled_job_in_write_txn};
use crate::simulation::{Clock, FaultInjector, FaultPoint, NoopFaultInjector, SystemClock};

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
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) fault_injector: Arc<dyn FaultInjector>,
}

#[derive(Debug, Clone)]
pub enum ResolvedWrite {
    Insert {
        document: Document,
        indexes: Vec<IndexDefinition>,
    },
    Update {
        previous: Document,
        current: Document,
        indexes: Vec<IndexDefinition>,
    },
    Delete {
        previous: Document,
        indexes: Vec<IndexDefinition>,
    },
}

#[derive(Debug, Clone)]
pub enum ResolvedScheduleOp {
    Insert { job: ScheduledJob },
    Cancel { job_id: JobId },
}

pub struct TenantReadSnapshot {
    read_txn: ReadTransaction,
}

pub struct TenantWriteCommit<T> {
    pub value: T,
    pub commit: Option<CommitEntry>,
}

pub struct TenantWriteTransaction {
    write_txn: Option<redb::WriteTransaction>,
    clock: Arc<dyn Clock>,
    fault_injector: Arc<dyn FaultInjector>,
    commit_writes: Vec<WriteOp>,
    check_cancel: Box<dyn Fn() -> Result<()> + Send>,
}

impl TenantWriteTransaction {
    fn new<Check>(
        write_txn: redb::WriteTransaction,
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
        check_cancel: Check,
    ) -> Self
    where
        Check: Fn() -> Result<()> + Send + 'static,
    {
        Self {
            write_txn: Some(write_txn),
            clock,
            fault_injector,
            commit_writes: Vec::new(),
            check_cancel: Box::new(check_cancel),
        }
    }

    pub(crate) fn write_txn(&self) -> Result<&redb::WriteTransaction> {
        self.write_txn
            .as_ref()
            .ok_or_else(|| Error::Internal("write transaction already closed".to_string()))
    }

    pub(crate) fn check_cancel(&self) -> Result<()> {
        (self.check_cancel.as_ref())()
    }

    pub fn begin_scheduled_execution(&mut self, execution_id: Option<&str>) -> Result<bool> {
        self.check_cancel()?;
        begin_scheduled_execution(self.write_txn()?, execution_id)
    }

    pub(crate) fn record_commit_write(&mut self, write: WriteOp) {
        self.commit_writes.push(write);
    }

    /// Commits all staged changes. Cancellation is checked immediately before the
    /// durable commit point and never after it returns.
    pub fn commit(mut self) -> Result<Option<CommitEntry>> {
        self.check_cancel()?;
        let Some(write_txn) = self.write_txn.take() else {
            return Err(Error::Internal(
                "write transaction already closed".to_string(),
            ));
        };
        let clock = self.clock.clone();
        let fault_injector = self.fault_injector.clone();
        let commit_writes = std::mem::take(&mut self.commit_writes);
        let check_cancel = self.check_cancel;

        let commit = if commit_writes.is_empty() {
            None
        } else {
            Some(append_commit(&write_txn, clock.now(), commit_writes)?)
        };
        commit_write_txn_cancellable(&*fault_injector, || check_cancel.as_ref()(), write_txn)?;
        Ok(commit)
    }

    pub fn rollback(mut self) {
        let _ = self.write_txn.take();
    }

    pub fn insert_document(&mut self, document: &Document) -> Result<()> {
        self.check_cancel()?;
        let payload = document
            .to_msgpack()
            .map_err(|error| Error::Serialization(error.to_string()))?;
        {
            let mut documents = self
                .write_txn()?
                .open_table(DOCUMENTS)
                .map_err(map_redb_error)?;
            let key = document_key(&document.table, &document.id);
            documents
                .insert(key.as_slice(), payload.as_slice())
                .map_err(map_redb_error)?;
        }
        self.record_commit_write(WriteOp {
            table: document.table.clone(),
            op_type: WriteOpType::Insert,
            doc_id: document.id,
            previous: None,
            current: Some(document.clone()),
        });
        Ok(())
    }

    pub fn update_document_validated<F>(
        &mut self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        validate: F,
    ) -> Result<()>
    where
        F: FnOnce(&Document, &Document) -> Result<()>,
    {
        self.check_cancel()?;
        let key = document_key(table, id);
        let (existing_document, document) = {
            let mut documents = self
                .write_txn()?
                .open_table(DOCUMENTS)
                .map_err(map_redb_error)?;
            let existing_document = {
                let existing = documents
                    .get(key.as_slice())
                    .map_err(map_redb_error)?
                    .ok_or(Error::DocumentNotFound(*id))?;
                Document::from_msgpack(existing.value())
                    .map_err(|error| Error::Serialization(error.to_string()))?
            };
            let mut document = existing_document.clone();
            for (field, value) in patch {
                document.fields.insert(field.clone(), value.clone());
            }
            validate(&existing_document, &document)?;
            let payload = document
                .to_msgpack()
                .map_err(|error| Error::Serialization(error.to_string()))?;
            documents
                .insert(key.as_slice(), payload.as_slice())
                .map_err(map_redb_error)?;
            (existing_document, document)
        };

        self.record_commit_write(WriteOp {
            table: table.clone(),
            op_type: WriteOpType::Update,
            doc_id: *id,
            previous: Some(existing_document),
            current: Some(document),
        });
        Ok(())
    }

    pub fn delete_document_validated<F>(
        &mut self,
        table: &TableName,
        id: &DocumentId,
        validate: F,
    ) -> Result<Document>
    where
        F: FnOnce(&Document) -> Result<()>,
    {
        self.check_cancel()?;
        let removed_document = {
            let mut documents = self
                .write_txn()?
                .open_table(DOCUMENTS)
                .map_err(map_redb_error)?;
            let key = document_key(table, id);
            let removed = documents.remove(key.as_slice()).map_err(map_redb_error)?;
            let removed = removed.ok_or(Error::DocumentNotFound(*id))?;
            Document::from_msgpack(removed.value())
                .map_err(|error| Error::Serialization(error.to_string()))?
        };
        validate(&removed_document)?;
        self.record_commit_write(WriteOp {
            table: table.clone(),
            op_type: WriteOpType::Delete,
            doc_id: *id,
            previous: Some(removed_document.clone()),
            current: None,
        });
        Ok(removed_document)
    }
}

fn expect_write_commit(commit: Option<CommitEntry>, expectation: &str) -> Result<CommitEntry> {
    commit.ok_or_else(|| Error::Internal(expectation.to_string()))
}

impl TenantStore {
    /// Opens or creates a tenant store on disk.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_simulation(path, Arc::new(SystemClock), Arc::new(NoopFaultInjector))
    }

    /// Opens or creates a tenant store on disk with deterministic simulation seams.
    pub fn open_with_simulation(
        path: impl AsRef<Path>,
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        let db = Database::create(path).map_err(map_redb_error)?;
        Ok(Self {
            db,
            clock,
            fault_injector,
        })
    }

    /// Creates an in-memory tenant store for tests.
    pub fn create_in_memory() -> Result<Self> {
        Self::create_in_memory_with_simulation(Arc::new(SystemClock), Arc::new(NoopFaultInjector))
    }

    /// Creates an in-memory tenant store with deterministic simulation seams.
    pub fn create_in_memory_with_simulation(
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        let db = Database::builder()
            .create_with_backend(InMemoryBackend::new())
            .map_err(map_redb_error)?;
        Ok(Self {
            db,
            clock,
            fault_injector,
        })
    }

    pub fn read_snapshot(&self) -> Result<TenantReadSnapshot> {
        Ok(TenantReadSnapshot {
            read_txn: self.db.begin_read().map_err(map_redb_error)?,
        })
    }

    pub fn begin_write_transaction(&self) -> Result<TenantWriteTransaction> {
        self.begin_write_transaction_cancellable(|| Ok(()))
    }

    pub fn begin_write_transaction_cancellable<Check>(
        &self,
        check_cancel: Check,
    ) -> Result<TenantWriteTransaction>
    where
        Check: Fn() -> Result<()> + Send + 'static,
    {
        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        Ok(TenantWriteTransaction::new(
            write_txn,
            self.clock.clone(),
            self.fault_injector.clone(),
            check_cancel,
        ))
    }

    pub fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        F: FnOnce(&mut TenantWriteTransaction) -> Result<T>,
    {
        self.execute_write_cancellable(|| Ok(()), task)
    }

    pub fn execute_write_cancellable<T, Check, F>(
        &self,
        check_cancel: Check,
        task: F,
    ) -> Result<TenantWriteCommit<T>>
    where
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(&mut TenantWriteTransaction) -> Result<T>,
    {
        let mut transaction = self.begin_write_transaction_cancellable(check_cancel)?;
        let value = task(&mut transaction)?;
        let commit = transaction.commit()?;
        Ok(TenantWriteCommit { value, commit })
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
        let committed = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(execution_id)? {
                return Ok(false);
            }
            transaction.insert_document(document)?;
            Ok(true)
        })?;
        Ok(if committed.value {
            Some(expect_write_commit(
                committed.commit,
                "deduplicated insert should record a commit entry",
            )?)
        } else {
            None
        })
    }

    /// Updates a document by applying a partial patch.
    pub fn update(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<CommitEntry> {
        self.update_validated(table, id, patch, |_, _| Ok(()))
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
        F: FnOnce(&Document, &Document) -> Result<()>,
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
        F: FnOnce(&Document, &Document) -> Result<()>,
    {
        let committed = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(execution_id)? {
                return Ok(false);
            }
            transaction.update_document_validated(table, id, patch, validate)?;
            Ok(true)
        })?;
        Ok(if committed.value {
            Some(expect_write_commit(
                committed.commit,
                "deduplicated update should record a commit entry",
            )?)
        } else {
            None
        })
    }

    /// Deletes a document if present and records a commit entry.
    pub fn delete(&self, table: &TableName, id: &DocumentId) -> Result<CommitEntry> {
        self.delete_validated_once(table, id, None, |_| Ok(()))?
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
        self.delete_validated_once(table, id, execution_id, |_| Ok(()))
    }

    /// Deletes a document if present and returns both the commit and removed snapshot.
    pub fn delete_returning_document(
        &self,
        table: &TableName,
        id: &DocumentId,
    ) -> Result<(CommitEntry, Document)> {
        self.delete_validated_returning_document(table, id, |_| Ok(()))
    }

    /// Deletes a document atomically after validating the removed snapshot.
    pub fn delete_validated_returning_document<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        validate: F,
    ) -> Result<(CommitEntry, Document)>
    where
        F: FnOnce(&Document) -> Result<()>,
    {
        self.delete_validated_once(table, id, None, validate)?
            .ok_or_else(|| Error::Internal("non-deduplicated delete should commit".to_string()))
    }

    /// Deletes a document once and returns the removed snapshot for the provided scheduled execution id.
    pub fn delete_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<(CommitEntry, Document)>>
    where
        F: FnOnce(&Document) -> Result<()>,
    {
        let committed = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(execution_id)? {
                return Ok(None);
            }
            let removed_document = transaction.delete_document_validated(table, id, validate)?;
            Ok(Some(removed_document))
        })?;
        Ok(if let Some(removed_document) = committed.value {
            Some((
                expect_write_commit(
                    committed.commit,
                    "deduplicated delete should record a commit entry",
                )?,
                removed_document,
            ))
        } else {
            None
        })
    }

    /// Fetches a document by id.
    pub fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        self.read_snapshot()?.get(table, id)
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
        self.scan_table_matching_cancellable(table, check_cancel, |_document| Ok(true))
    }

    /// Scans all documents within a logical table, only collecting rows that match the predicate.
    pub fn scan_table_matching_cancellable<F>(
        &self,
        table: &TableName,
        check_cancel: &mut dyn FnMut() -> Result<()>,
        mut include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        self.read_snapshot()?
            .scan_table_matching_cancellable(table, check_cancel, |document| {
                include_document(document)
            })
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
        self.read_snapshot()?.latest_sequence()
    }

    pub(crate) fn now(&self) -> Timestamp {
        self.clock.now()
    }

    pub(crate) fn append_commit_entry(
        &self,
        write_txn: &redb::WriteTransaction,
        writes: Vec<WriteOp>,
    ) -> Result<CommitEntry> {
        append_commit(write_txn, self.now(), writes)
    }

    pub(crate) fn commit_write_txn(&self, write_txn: redb::WriteTransaction) -> Result<()> {
        commit_write_txn_cancellable(&*self.fault_injector, || Ok(()), write_txn)
    }

    pub fn apply_resolved_write_batch(&self, writes: &[ResolvedWrite]) -> Result<CommitEntry> {
        self.apply_execution_unit_batch(writes, &[])?
            .ok_or_else(|| {
                Error::Internal("resolved write batch must contain at least one write".to_string())
            })
    }

    pub fn apply_execution_unit_batch(
        &self,
        writes: &[ResolvedWrite],
        schedule_ops: &[ResolvedScheduleOp],
    ) -> Result<Option<CommitEntry>> {
        if writes.is_empty() && schedule_ops.is_empty() {
            return Err(Error::Internal(
                "execution-unit batch must contain at least one change".to_string(),
            ));
        }

        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        let mut commit_writes = Vec::with_capacity(writes.len());

        {
            let mut documents = write_txn.open_table(DOCUMENTS).map_err(map_redb_error)?;
            let mut index_table = write_txn.open_table(INDEXES).map_err(map_redb_error)?;

            for write in writes {
                match write {
                    ResolvedWrite::Insert { document, indexes } => {
                        let key = document_key(&document.table, &document.id);
                        if documents
                            .get(key.as_slice())
                            .map_err(map_redb_error)?
                            .is_some()
                        {
                            return Err(Error::Conflict(format!(
                                "document {} changed before transaction commit",
                                document.id
                            )));
                        }

                        let payload = document
                            .to_msgpack()
                            .map_err(|error| Error::Serialization(error.to_string()))?;
                        documents
                            .insert(key.as_slice(), payload.as_slice())
                            .map_err(map_redb_error)?;
                        for index in indexes {
                            if let Some(value) = document.get_field(&index.field) {
                                let encoded = encode_index_value(value)?;
                                let index_key =
                                    index_key(&document.table, &index.name, &encoded, &document.id);
                                index_table
                                    .insert(index_key.as_slice(), EMPTY_TABLE_VALUE)
                                    .map_err(map_redb_error)?;
                            }
                        }
                        commit_writes.push(WriteOp {
                            table: document.table.clone(),
                            op_type: WriteOpType::Insert,
                            doc_id: document.id,
                            previous: None,
                            current: Some(document.clone()),
                        });
                    }
                    ResolvedWrite::Update {
                        previous,
                        current,
                        indexes,
                    } => {
                        let key = document_key(&current.table, &current.id);
                        let existing = {
                            let existing = documents
                                .get(key.as_slice())
                                .map_err(map_redb_error)?
                                .ok_or(Error::Conflict(format!(
                                "document {} changed before transaction commit",
                                current.id
                            )))?;
                            Document::from_msgpack(existing.value())
                                .map_err(|error| Error::Serialization(error.to_string()))?
                        };
                        if &existing != previous {
                            return Err(Error::Conflict(format!(
                                "document {} changed before transaction commit",
                                current.id
                            )));
                        }

                        let payload = current
                            .to_msgpack()
                            .map_err(|error| Error::Serialization(error.to_string()))?;
                        documents
                            .insert(key.as_slice(), payload.as_slice())
                            .map_err(map_redb_error)?;

                        for index in indexes {
                            let old_key = previous
                                .get_field(&index.field)
                                .map(encode_index_value)
                                .transpose()?
                                .map(|encoded| {
                                    index_key(&current.table, &index.name, &encoded, &current.id)
                                });
                            let new_key = current
                                .get_field(&index.field)
                                .map(encode_index_value)
                                .transpose()?
                                .map(|encoded| {
                                    index_key(&current.table, &index.name, &encoded, &current.id)
                                });
                            if old_key == new_key {
                                continue;
                            }
                            if let Some(old_key) = old_key {
                                index_table
                                    .remove(old_key.as_slice())
                                    .map_err(map_redb_error)?;
                            }
                            if let Some(new_key) = new_key {
                                index_table
                                    .insert(new_key.as_slice(), EMPTY_TABLE_VALUE)
                                    .map_err(map_redb_error)?;
                            }
                        }

                        commit_writes.push(WriteOp {
                            table: current.table.clone(),
                            op_type: WriteOpType::Update,
                            doc_id: current.id,
                            previous: Some(previous.clone()),
                            current: Some(current.clone()),
                        });
                    }
                    ResolvedWrite::Delete { previous, indexes } => {
                        let key = document_key(&previous.table, &previous.id);
                        let removed = documents
                            .remove(key.as_slice())
                            .map_err(map_redb_error)?
                            .ok_or(Error::Conflict(format!(
                                "document {} changed before transaction commit",
                                previous.id
                            )))?;
                        let removed = Document::from_msgpack(removed.value())
                            .map_err(|error| Error::Serialization(error.to_string()))?;
                        if &removed != previous {
                            return Err(Error::Conflict(format!(
                                "document {} changed before transaction commit",
                                previous.id
                            )));
                        }

                        for index in indexes {
                            if let Some(value) = previous.get_field(&index.field) {
                                let encoded = encode_index_value(value)?;
                                let index_key =
                                    index_key(&previous.table, &index.name, &encoded, &previous.id);
                                index_table
                                    .remove(index_key.as_slice())
                                    .map_err(map_redb_error)?;
                            }
                        }

                        commit_writes.push(WriteOp {
                            table: previous.table.clone(),
                            op_type: WriteOpType::Delete,
                            doc_id: previous.id,
                            previous: Some(previous.clone()),
                            current: None,
                        });
                    }
                }
            }
        }

        for schedule_op in schedule_ops {
            match schedule_op {
                ResolvedScheduleOp::Insert { job } => {
                    insert_scheduled_job_in_write_txn(&write_txn, job)?;
                }
                ResolvedScheduleOp::Cancel { job_id } => {
                    if !cancel_scheduled_job_in_write_txn(&write_txn, job_id)? {
                        return Err(Error::ScheduledJobNotFound(*job_id));
                    }
                }
            }
        }

        let commit = if commit_writes.is_empty() {
            None
        } else {
            Some(self.append_commit_entry(&write_txn, commit_writes)?)
        };
        self.commit_write_txn(write_txn)?;
        Ok(commit)
    }
}

impl TenantReadSnapshot {
    pub fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        let table_handle = match self.read_txn.open_table(DOCUMENTS) {
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

    pub fn scan_table_matching_cancellable<F>(
        &self,
        table: &TableName,
        check_cancel: &mut dyn FnMut() -> Result<()>,
        mut include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        let table_handle = match self.read_txn.open_table(DOCUMENTS) {
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
                    let document = Document::from_msgpack(value.value())
                        .map_err(|error| Error::Serialization(error.to_string()))?;
                    if include_document(&document)? {
                        documents.push(document);
                    }
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
                    let document = Document::from_msgpack(value.value())
                        .map_err(|error| Error::Serialization(error.to_string()))?;
                    if include_document(&document)? {
                        documents.push(document);
                    }
                }
            }
        }
        Ok(documents)
    }

    pub fn latest_sequence(&self) -> Result<SequenceNumber> {
        let table_handle = match self.read_txn.open_table(METADATA) {
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
    timestamp: Timestamp,
    writes: Vec<WriteOp>,
) -> Result<CommitEntry> {
    let sequence = next_sequence(write_txn)?;
    let entry = CommitEntry {
        sequence: SequenceNumber(sequence),
        timestamp,
        writes,
    };

    let mut log = write_txn.open_table(COMMIT_LOG).map_err(map_redb_error)?;
    let payload = serialize_commit(&entry)?;
    log.insert(sequence, payload.as_slice())
        .map_err(map_redb_error)?;

    Ok(entry)
}

pub(crate) fn commit_write_txn_cancellable<Check>(
    fault_injector: &dyn FaultInjector,
    check_cancel: Check,
    write_txn: redb::WriteTransaction,
) -> Result<()>
where
    Check: Fn() -> Result<()>,
{
    fault_injector.check(FaultPoint::StorageCommitBeforeVisibility)?;
    check_cancel()?;
    write_txn.commit().map_err(map_redb_error)?;
    fault_injector.check(FaultPoint::StorageCommitAfterVisibilityBeforeReturn)?;
    Ok(())
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
