use std::path::Path;
use std::sync::Arc;

use neovex_core::{
    CommitEntry, Document, DocumentId, Error, Result, TableName, WriteOp, WriteOpType,
};
use redb::ReadableTable;
use redb::backends::InMemoryBackend;

use crate::index::index_key_for_document;
use crate::keys::document_key;
use crate::scheduler::{cancel_scheduled_job_in_write_txn, insert_scheduled_job_in_write_txn};
use crate::simulation::{Clock, FaultInjector, FaultPoint, NoopFaultInjector, SystemClock};

use super::journal::{append_commit, begin_scheduled_execution, commit_write_txn_cancellable};
use super::scan::ScanMetrics;
use super::{
    DOCUMENTS, EMPTY_TABLE_VALUE, INDEXES, ResolvedScheduleOp, ResolvedWrite, TenantStore,
    TenantWriteCommit, TenantWriteTransaction, map_redb_error,
};

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
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_simulation(path, Arc::new(SystemClock), Arc::new(NoopFaultInjector))
    }

    pub fn open_with_simulation(
        path: impl AsRef<Path>,
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        let db = redb::Database::create(path).map_err(map_redb_error)?;
        Ok(Self {
            db,
            clock,
            fault_injector,
            scan_metrics: Arc::new(ScanMetrics::new()),
        })
    }

    pub fn create_in_memory() -> Result<Self> {
        Self::create_in_memory_with_simulation(Arc::new(SystemClock), Arc::new(NoopFaultInjector))
    }

    pub fn create_in_memory_with_simulation(
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        let db = redb::Database::builder()
            .create_with_backend(InMemoryBackend::new())
            .map_err(map_redb_error)?;
        Ok(Self {
            db,
            clock,
            fault_injector,
            scan_metrics: Arc::new(ScanMetrics::new()),
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

    pub fn insert(&self, document: &Document) -> Result<CommitEntry> {
        self.insert_once(document, None)?
            .ok_or_else(|| Error::Internal("non-deduplicated insert should commit".to_string()))
    }

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

    pub fn update(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<CommitEntry> {
        self.update_validated(table, id, patch, |_, _| Ok(()))
    }

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

    pub fn delete(&self, table: &TableName, id: &DocumentId) -> Result<CommitEntry> {
        self.delete_validated_once(table, id, None, |_| Ok(()))?
            .map(|commit| commit.0)
            .ok_or_else(|| Error::Internal("non-deduplicated delete should commit".to_string()))
    }

    pub fn delete_once(
        &self,
        table: &TableName,
        id: &DocumentId,
        execution_id: Option<&str>,
    ) -> Result<Option<(CommitEntry, Document)>> {
        self.delete_validated_once(table, id, execution_id, |_| Ok(()))
    }

    pub fn delete_returning_document(
        &self,
        table: &TableName,
        id: &DocumentId,
    ) -> Result<(CommitEntry, Document)> {
        self.delete_validated_returning_document(table, id, |_| Ok(()))
    }

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

    pub(crate) fn commit_write_txn(&self, write_txn: redb::WriteTransaction) -> Result<()> {
        commit_write_txn_cancellable(&*self.fault_injector, || Ok(()), write_txn)
    }

    pub fn check_fault(&self, point: FaultPoint) -> Result<()> {
        self.fault_injector.check(point)
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
                            if let Some(index_key) = index_key_for_document(document, index)? {
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
                            let old_key = index_key_for_document(previous, index)?;
                            let new_key = index_key_for_document(current, index)?;
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
                            if let Some(index_key) = index_key_for_document(previous, index)? {
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
