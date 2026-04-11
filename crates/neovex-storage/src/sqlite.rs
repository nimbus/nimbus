use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::Duration;

use neovex_core::{
    CommitEntry, CronJob, Document, DocumentId, DurableMutationRecord, Error, Filter, JobId,
    Result, ScheduledJob, ScheduledJobResult, Schema, SequenceNumber, TableName, TableSchema,
    Timestamp, WriteOp, WriteOpType,
};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::commit_log::{deserialize_durable_record, serialize_commit, serialize_durable_record};
use crate::simulation::{Clock, FaultInjector, FaultPoint, NoopFaultInjector, SystemClock};
use crate::store::{
    APPLIED_SEQUENCE_KEY, DurableJournalBootstrap, DurableJournalPage, JournalProgress,
    MAX_DURABLE_JOURNAL_STREAM_LIMIT, MaterializedJournalSnapshot, NEXT_SEQUENCE_KEY,
    ResolvedScheduleOp, ResolvedWrite, TenantWriteCommit,
};

pub(crate) const SQLITE_INIT_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS documents (
    table_name TEXT NOT NULL,
    id TEXT NOT NULL,
    data_json TEXT NOT NULL,
    creation_time INTEGER NOT NULL,
    PRIMARY KEY (table_name, id)
);

CREATE TABLE IF NOT EXISTS schemas (
    table_name TEXT NOT NULL PRIMARY KEY,
    schema_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS scheduled_jobs (
    id TEXT NOT NULL PRIMARY KEY,
    data_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS running_scheduled_jobs (
    id TEXT NOT NULL PRIMARY KEY,
    data_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS scheduled_job_results (
    job_id TEXT NOT NULL PRIMARY KEY,
    data_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS scheduled_job_executions (
    execution_id TEXT NOT NULL PRIMARY KEY
);

CREATE TABLE IF NOT EXISTS cron_jobs (
    name TEXT NOT NULL PRIMARY KEY,
    data_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS commit_log (
    sequence INTEGER NOT NULL PRIMARY KEY,
    record_blob BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS metadata (
    key TEXT NOT NULL PRIMARY KEY,
    value_blob BLOB NOT NULL
);
"#;

pub fn sqlite_init_sql() -> &'static str {
    SQLITE_INIT_SQL
}

/// SQLite-backed tenant store foundation for the migration workstream.
///
/// This type intentionally starts small: it owns connection opening,
/// WAL-oriented initialization, metadata access, and the cancellable
/// read/write transaction boundary. Later SQLite items will fill in the
/// document, planner, scheduler, and journal semantics on top of this
/// foundation.
pub struct SqliteTenantStore {
    path: PathBuf,
    clock: Arc<dyn Clock>,
    fault_injector: Arc<dyn FaultInjector>,
    read_connections: Arc<Mutex<Vec<Connection>>>,
    schema_cache: Arc<RwLock<Schema>>,
}

pub struct SqliteReadSnapshot {
    conn: PooledSqliteConnection,
    schema_cache: Arc<RwLock<Schema>>,
}

pub struct SqliteWriteTransaction {
    conn: Option<Connection>,
    clock: Arc<dyn Clock>,
    fault_injector: Arc<dyn FaultInjector>,
    commit_writes: Vec<WriteOp>,
    check_cancel: Box<dyn Fn() -> Result<()> + Send>,
    schema_cache: Arc<RwLock<Schema>>,
    schema_cache_dirty: bool,
}

struct PooledSqliteConnection {
    conn: Option<Connection>,
    pool: Arc<Mutex<Vec<Connection>>>,
}

impl SqliteTenantStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_simulation(path, Arc::new(SystemClock), Arc::new(NoopFaultInjector))
    }

    pub fn open_with_simulation(
        path: impl AsRef<Path>,
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| Error::Internal(error.to_string()))?;
        }
        let store = Self {
            path,
            clock,
            fault_injector,
            read_connections: Arc::new(Mutex::new(Vec::new())),
            schema_cache: Arc::new(RwLock::new(Schema::default())),
        };
        let conn = store.open_connection()?;
        let schema = load_schema_from_conn(&conn)?;
        store.replace_cached_schema(schema)?;
        store.lock_read_connections()?.push(conn);
        Ok(store)
    }

    pub fn read_snapshot(&self) -> Result<SqliteReadSnapshot> {
        Ok(SqliteReadSnapshot {
            conn: self.acquire_read_connection()?,
            schema_cache: self.schema_cache.clone(),
        })
    }

    pub fn begin_write_transaction(&self) -> Result<SqliteWriteTransaction> {
        self.begin_write_transaction_cancellable(|| Ok(()))
    }

    pub fn begin_write_transaction_cancellable<Check>(
        &self,
        check_cancel: Check,
    ) -> Result<SqliteWriteTransaction>
    where
        Check: Fn() -> Result<()> + Send + 'static,
    {
        let conn = self.open_connection()?;
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(map_sqlite_error)?;
        Ok(SqliteWriteTransaction {
            conn: Some(conn),
            clock: self.clock.clone(),
            fault_injector: self.fault_injector.clone(),
            commit_writes: Vec::new(),
            check_cancel: Box::new(check_cancel),
            schema_cache: self.schema_cache.clone(),
            schema_cache_dirty: false,
        })
    }

    pub fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        F: FnOnce(&mut SqliteWriteTransaction) -> Result<T>,
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
        F: FnOnce(&mut SqliteWriteTransaction) -> Result<T>,
    {
        let mut transaction = self.begin_write_transaction_cancellable(check_cancel)?;
        let value = match task(&mut transaction) {
            Ok(value) => value,
            Err(error) => {
                transaction.rollback();
                return Err(error);
            }
        };
        let commit = transaction.commit()?;
        Ok(TenantWriteCommit { value, commit })
    }

    pub fn metadata_blob(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.read_snapshot()?.metadata_blob(key)
    }

    pub fn load_schema(&self) -> Result<Schema> {
        Ok(self.read_schema_cache()?.clone())
    }

    pub fn save_table_schema(&self, table_schema: &TableSchema) -> Result<()> {
        self.execute_write(move |transaction| transaction.save_table_schema(table_schema))?;
        Ok(())
    }

    pub fn replace_table_schema(&self, table_schema: &TableSchema) -> Result<()> {
        self.execute_write(move |transaction| transaction.replace_table_schema(table_schema))?;
        Ok(())
    }

    pub fn replace_schema(&self, schema: &Schema) -> Result<()> {
        let current = self.load_schema()?;
        if current == *schema {
            return Ok(());
        }

        let mut tables_to_remove = current
            .tables
            .keys()
            .filter(|table| !schema.tables.contains_key(*table))
            .cloned()
            .collect::<Vec<_>>();
        tables_to_remove.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));

        let mut tables_to_replace = schema
            .tables
            .iter()
            .filter_map(|(table, table_schema)| {
                (current.tables.get(table) != Some(table_schema)).then_some(table_schema.clone())
            })
            .collect::<Vec<_>>();
        tables_to_replace
            .sort_unstable_by(|left, right| left.table.as_str().cmp(right.table.as_str()));

        self.execute_write(move |transaction| {
            for table in &tables_to_remove {
                transaction.delete_table_schema(table)?;
            }
            for table_schema in &tables_to_replace {
                transaction.replace_table_schema(table_schema)?;
            }
            Ok(())
        })?;
        Ok(())
    }

    pub fn delete_table_schema_entry(&self, table: &TableName) -> Result<()> {
        self.execute_write(move |transaction| transaction.delete_table_schema_entry(table))?;
        Ok(())
    }

    pub fn delete_table_schema(&self, table: &TableName) -> Result<()> {
        self.execute_write(move |transaction| transaction.delete_table_schema(table))?;
        Ok(())
    }

    pub fn insert_document_for_testing(&self, document: &Document) -> Result<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO documents (table_name, id, data_json, creation_time)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                document.table.as_str(),
                document.id.to_string(),
                serde_json::to_string(&document.fields)
                    .map_err(|error| Error::Serialization(error.to_string()))?,
                document.creation_time.0,
            ],
        )
        .map_err(map_sqlite_error)?;
        Ok(())
    }

    pub fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        self.read_snapshot()?.get(table, id)
    }

    pub fn scan_table_matching_with_filters_cancellable<F>(
        &self,
        table: &TableName,
        filters: &[Filter],
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        self.read_snapshot()?
            .scan_table_matching_with_filters_cancellable(
                table,
                filters,
                check_cancel,
                include_document,
            )
    }

    pub fn scan_table_matching_cancellable<F>(
        &self,
        table: &TableName,
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        self.scan_table_matching_with_filters_cancellable(
            table,
            &[],
            check_cancel,
            include_document,
        )
    }

    pub fn index_scan_eq(
        &self,
        table: &TableName,
        index_name: &str,
        value: &serde_json::Value,
    ) -> Result<Vec<Document>> {
        self.index_scan_eq_cancellable(table, index_name, value, &mut || Ok(()))
    }

    pub fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &serde_json::Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.read_snapshot()?
            .index_scan_eq_cancellable(table, index_name, value, check_cancel)
    }

    pub fn index_scan_prefix(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[serde_json::Value],
    ) -> Result<Vec<Document>> {
        self.index_scan_prefix_cancellable(table, index_name, prefix_values, &mut || Ok(()))
    }

    pub fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[serde_json::Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.read_snapshot()?.index_scan_prefix_cancellable(
            table,
            index_name,
            prefix_values,
            check_cancel,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: Option<&serde_json::Value>,
        end: Option<&serde_json::Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.read_snapshot()?.index_scan_range_cancellable(
            table,
            index_name,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[serde_json::Value],
        start: Option<&serde_json::Value>,
        end: Option<&serde_json::Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.read_snapshot()?
            .index_scan_composite_range_cancellable(
                table,
                index_name,
                exact_prefix,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            )
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

    pub fn now(&self) -> Timestamp {
        self.clock.now()
    }

    pub fn check_fault(&self, point: FaultPoint) -> Result<()> {
        self.fault_injector.check(point)
    }

    pub fn applied_sequence(&self) -> Result<SequenceNumber> {
        self.read_snapshot()?.applied_sequence()
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
                "INSERT INTO documents (table_name, id, data_json, creation_time)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    document.table.as_str(),
                    document.id.to_string(),
                    serialize_document_fields(document)?,
                    document.creation_time.0,
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

    pub fn scheduled_execution_exists(&self, execution_id: &str) -> Result<bool> {
        self.read_snapshot()?
            .scheduled_execution_exists(execution_id)
    }

    pub fn scan_table(&self, table: &TableName) -> Result<Vec<Document>> {
        self.scan_table_cancellable(table, &mut || Ok(()))
    }

    pub fn scan_table_cancellable(
        &self,
        table: &TableName,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.scan_table_matching_with_filters_cancellable(table, &[], check_cancel, |_| Ok(true))
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

    pub fn insert_with_indexes(
        &self,
        document: &Document,
        _indexes: &[neovex_core::IndexDefinition],
    ) -> Result<CommitEntry> {
        self.insert(document)
    }

    pub fn insert_with_indexes_once(
        &self,
        document: &Document,
        _indexes: &[neovex_core::IndexDefinition],
        execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        self.insert_once(document, execution_id)
    }

    pub fn update_with_indexes(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        _indexes: &[neovex_core::IndexDefinition],
    ) -> Result<CommitEntry> {
        self.update(table, id, patch)
    }

    pub fn update_with_indexes_validated<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        _indexes: &[neovex_core::IndexDefinition],
        validate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&Document, &Document) -> Result<()>,
    {
        self.update_validated(table, id, patch, validate)
    }

    pub fn update_with_indexes_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        _indexes: &[neovex_core::IndexDefinition],
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<CommitEntry>>
    where
        F: FnOnce(&Document, &Document) -> Result<()>,
    {
        self.update_validated_once(table, id, patch, execution_id, validate)
    }

    pub fn delete_with_indexes(
        &self,
        table: &TableName,
        id: &DocumentId,
        _indexes: &[neovex_core::IndexDefinition],
    ) -> Result<CommitEntry> {
        self.delete(table, id)
    }

    pub fn delete_with_indexes_once(
        &self,
        table: &TableName,
        id: &DocumentId,
        _indexes: &[neovex_core::IndexDefinition],
        execution_id: Option<&str>,
    ) -> Result<Option<(CommitEntry, Document)>> {
        self.delete_once(table, id, execution_id)
    }

    pub fn delete_with_indexes_returning_document(
        &self,
        table: &TableName,
        id: &DocumentId,
        _indexes: &[neovex_core::IndexDefinition],
    ) -> Result<(CommitEntry, Document)> {
        self.delete_returning_document(table, id)
    }

    pub fn delete_with_indexes_validated_returning_document<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        _indexes: &[neovex_core::IndexDefinition],
        validate: F,
    ) -> Result<(CommitEntry, Document)>
    where
        F: FnOnce(&Document) -> Result<()>,
    {
        self.delete_validated_returning_document(table, id, validate)
    }

    pub fn delete_with_indexes_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        _indexes: &[neovex_core::IndexDefinition],
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<(CommitEntry, Document)>>
    where
        F: FnOnce(&Document) -> Result<()>,
    {
        self.delete_validated_once(table, id, execution_id, validate)
    }

    pub fn insert_scheduled_job(&self, job: &ScheduledJob) -> Result<()> {
        self.execute_write(move |transaction| transaction.insert_scheduled_job(job))?;
        Ok(())
    }

    pub fn claim_due_jobs(&self, now: Timestamp) -> Result<Vec<ScheduledJob>> {
        Ok(self
            .execute_write(move |transaction| transaction.claim_due_jobs(now))?
            .value)
    }

    pub fn complete_scheduled_job(&self, job_id: &JobId) -> Result<()> {
        self.execute_write(move |transaction| transaction.complete_scheduled_job(job_id))?;
        Ok(())
    }

    pub fn cancel_scheduled_job(&self, job_id: &JobId) -> Result<bool> {
        Ok(self
            .execute_write(move |transaction| transaction.cancel_scheduled_job(job_id))?
            .value)
    }

    pub fn list_scheduled_jobs(&self) -> Result<Vec<ScheduledJob>> {
        self.read_snapshot()?.list_scheduled_jobs()
    }

    pub fn record_scheduled_job_result(&self, result: &ScheduledJobResult) -> Result<()> {
        self.execute_write(move |transaction| transaction.record_scheduled_job_result(result))?;
        Ok(())
    }

    pub fn get_scheduled_job_result(&self, job_id: &JobId) -> Result<Option<ScheduledJobResult>> {
        self.read_snapshot()?.get_scheduled_job_result(job_id)
    }

    pub fn save_cron_job(&self, cron: &CronJob) -> Result<()> {
        self.execute_write(move |transaction| transaction.save_cron_job(cron))?;
        Ok(())
    }

    pub fn load_cron_jobs(&self) -> Result<Vec<CronJob>> {
        self.read_snapshot()?.load_cron_jobs()
    }

    pub fn delete_cron_job(&self, name: &str) -> Result<()> {
        self.execute_write(move |transaction| transaction.delete_cron_job(name))?;
        Ok(())
    }

    pub fn next_scheduled_work_at(&self) -> Result<Option<Timestamp>> {
        self.read_snapshot()?.next_scheduled_work_at()
    }

    pub fn has_scheduled_work(&self) -> Result<bool> {
        self.read_snapshot()?.has_scheduled_work()
    }

    pub fn recover_running_jobs(&self, now: Timestamp) -> Result<()> {
        self.execute_write(move |transaction| transaction.recover_running_jobs(now))?;
        Ok(())
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

        let committed = self.execute_write(move |transaction| {
            for write in writes {
                transaction.apply_resolved_write(write)?;
            }
            apply_schedule_ops_in_transaction(transaction, schedule_ops)?;
            Ok(())
        })?;
        Ok(committed.commit)
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

    fn open_connection(&self) -> Result<Connection> {
        let conn = Connection::open(&self.path).map_err(map_sqlite_error)?;
        initialize_connection(&conn)?;
        Ok(conn)
    }

    fn acquire_read_connection(&self) -> Result<PooledSqliteConnection> {
        let conn = self
            .lock_read_connections()?
            .pop()
            .map(Ok)
            .unwrap_or_else(|| self.open_connection())?;
        Ok(PooledSqliteConnection {
            conn: Some(conn),
            pool: self.read_connections.clone(),
        })
    }

    fn lock_read_connections(&self) -> Result<MutexGuard<'_, Vec<Connection>>> {
        self.read_connections
            .lock()
            .map_err(|_| Error::Internal("sqlite read connection pool lock poisoned".to_string()))
    }

    fn read_schema_cache(&self) -> Result<RwLockReadGuard<'_, Schema>> {
        self.schema_cache
            .read()
            .map_err(|_| Error::Internal("sqlite schema cache lock poisoned".to_string()))
    }

    fn write_schema_cache(&self) -> Result<RwLockWriteGuard<'_, Schema>> {
        self.schema_cache
            .write()
            .map_err(|_| Error::Internal("sqlite schema cache lock poisoned".to_string()))
    }

    fn replace_cached_schema(&self, schema: Schema) -> Result<()> {
        *self.write_schema_cache()? = schema;
        Ok(())
    }
}

impl SqliteReadSnapshot {
    pub fn load_schema(&self) -> Result<Schema> {
        Ok(self
            .schema_cache
            .read()
            .map_err(|_| Error::Internal("sqlite schema cache lock poisoned".to_string()))?
            .clone())
    }

    pub fn read_durable_journal_from(
        &self,
        sequence: SequenceNumber,
    ) -> Result<Vec<DurableMutationRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT record_blob
                 FROM commit_log
                 WHERE sequence >= ?1
                 ORDER BY sequence",
            )
            .map_err(map_sqlite_error)?;
        let mut rows = stmt.query(params![sequence.0]).map_err(map_sqlite_error)?;
        let mut records = Vec::new();
        while let Some(row) = rows.next().map_err(map_sqlite_error)? {
            let payload: Vec<u8> = row.get(0).map_err(map_sqlite_error)?;
            records.push(deserialize_durable_record(payload.as_slice())?);
        }
        Ok(records)
    }

    pub fn stream_durable_journal(
        &self,
        after: SequenceNumber,
        limit: usize,
    ) -> Result<DurableJournalPage> {
        validate_durable_journal_stream_limit(limit)?;

        let latest_sequence = self.latest_sequence()?;
        let cursor_floor = self.durable_journal_cursor_floor()?;
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

        let mut stmt = self
            .conn
            .prepare(
                "SELECT record_blob
                 FROM commit_log
                 WHERE sequence > ?1
                 ORDER BY sequence
                 LIMIT ?2",
            )
            .map_err(map_sqlite_error)?;
        let mut rows = stmt
            .query(params![
                after.0,
                u64::try_from(limit.saturating_add(1)).unwrap_or(u64::MAX)
            ])
            .map_err(map_sqlite_error)?;
        let mut records = Vec::with_capacity(limit);
        let mut has_more = false;
        while let Some(row) = rows.next().map_err(map_sqlite_error)? {
            let payload: Vec<u8> = row.get(0).map_err(map_sqlite_error)?;
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

    pub fn export_durable_journal_bootstrap(&self) -> Result<DurableJournalBootstrap> {
        let snapshot = self.export_materialized_journal_snapshot()?;
        let cursor_floor = self.durable_journal_cursor_floor()?;
        Ok(DurableJournalBootstrap {
            resume_after: snapshot.applied_sequence,
            bootstrap_cut: snapshot.durable_head,
            snapshot,
            cursor_floor,
        })
    }

    pub fn metadata_blob(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.conn
            .query_row(
                "SELECT value_blob FROM metadata WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_sqlite_error)
    }

    pub fn journal_mode(&self) -> Result<String> {
        let mode = self
            .conn
            .pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))
            .map_err(map_sqlite_error)?;
        Ok(mode.to_ascii_lowercase())
    }

    pub fn latest_sequence(&self) -> Result<SequenceNumber> {
        Ok(SequenceNumber(
            next_sequence_in_conn(&self.conn)?.saturating_sub(1),
        ))
    }

    pub fn applied_sequence(&self) -> Result<SequenceNumber> {
        Ok(SequenceNumber(
            self.metadata_blob(APPLIED_SEQUENCE_KEY)?
                .map(|bytes| decode_u64(bytes.as_slice()))
                .transpose()?
                .unwrap_or(0),
        ))
    }

    pub fn journal_progress(&self) -> Result<JournalProgress> {
        Ok(JournalProgress {
            durable_head: self.latest_sequence()?,
            applied_head: self.applied_sequence()?,
        })
    }

    pub fn export_materialized_journal_snapshot(&self) -> Result<MaterializedJournalSnapshot> {
        let progress = self.journal_progress()?;
        Ok(MaterializedJournalSnapshot {
            version: 1,
            applied_sequence: progress.applied_head,
            durable_head: progress.durable_head,
            schema: self.load_schema()?,
            documents: self.documents()?,
            scheduled_execution_ids: self.scheduled_execution_ids()?,
        })
    }

    pub fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        self.conn
            .query_row(
                "SELECT creation_time, data_json
                 FROM documents
                 WHERE table_name = ?1 AND id = ?2",
                params![table.as_str(), id.to_string()],
                |row| {
                    Ok(row_to_document(
                        table,
                        id,
                        row.get(0)?,
                        row.get::<_, String>(1)?,
                    ))
                },
            )
            .optional()
            .map_err(map_sqlite_error)?
            .transpose()
    }

    pub fn scheduled_execution_exists(&self, execution_id: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM scheduled_job_executions WHERE execution_id = ?1",
                params![execution_id],
                |_| Ok(()),
            )
            .optional()
            .map_err(map_sqlite_error)?
            .is_some())
    }

    pub fn documents(&self) -> Result<Vec<Document>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT table_name, id, creation_time, data_json
                 FROM documents
                 ORDER BY table_name, id",
            )
            .map_err(map_sqlite_error)?;
        let mut rows = stmt.query([]).map_err(map_sqlite_error)?;
        let mut documents = Vec::new();
        while let Some(row) = rows.next().map_err(map_sqlite_error)? {
            let table_name = row.get::<_, String>(0).map_err(map_sqlite_error)?;
            let table = TableName::new(table_name)
                .map_err(|error| Error::Serialization(error.to_string()))?;
            let id =
                DocumentId::from_str(row.get::<_, String>(1).map_err(map_sqlite_error)?.as_str())
                    .map_err(|error| Error::Serialization(error.to_string()))?;
            documents.push(row_to_document(
                &table,
                &id,
                row.get(2).map_err(map_sqlite_error)?,
                row.get::<_, String>(3).map_err(map_sqlite_error)?,
            )?);
        }
        Ok(documents)
    }

    pub fn scheduled_execution_ids(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT execution_id
                 FROM scheduled_job_executions
                 ORDER BY execution_id",
            )
            .map_err(map_sqlite_error)?;
        let mut rows = stmt.query([]).map_err(map_sqlite_error)?;
        let mut execution_ids = Vec::new();
        while let Some(row) = rows.next().map_err(map_sqlite_error)? {
            execution_ids.push(row.get::<_, String>(0).map_err(map_sqlite_error)?);
        }
        Ok(execution_ids)
    }

    pub fn scan_table_matching_with_filters_cancellable<F>(
        &self,
        table: &TableName,
        _filters: &[Filter],
        check_cancel: &mut dyn FnMut() -> Result<()>,
        mut include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT id, creation_time, data_json
                 FROM documents
                 WHERE table_name = ?1
                 ORDER BY id",
            )
            .map_err(map_sqlite_error)?;
        let mut rows = stmt
            .query(params![table.as_str()])
            .map_err(map_sqlite_error)?;
        let mut documents = Vec::new();
        while let Some(row) = rows.next().map_err(map_sqlite_error)? {
            check_cancel()?;
            let id = row.get::<_, String>(0).map_err(map_sqlite_error)?;
            let document = row_to_document(
                table,
                &DocumentId::from_str(id.as_str())
                    .map_err(|error| Error::Serialization(error.to_string()))?,
                row.get(1).map_err(map_sqlite_error)?,
                row.get::<_, String>(2).map_err(map_sqlite_error)?,
            )?;
            if include_document(&document)? {
                documents.push(document);
            }
        }
        Ok(documents)
    }

    pub fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &serde_json::Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.index_scan_prefix_cancellable(
            table,
            index_name,
            std::slice::from_ref(value),
            check_cancel,
        )
    }

    pub fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[serde_json::Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let fields = index_fields_for_cached_schema(&self.schema_cache, table, index_name)?;
        if prefix_values.len() > fields.len() {
            return Err(Error::InvalidInput(format!(
                "index prefix length {} exceeds field count {} for index {}",
                prefix_values.len(),
                fields.len(),
                index_name
            )));
        }
        let sql = sqlite_index_scan_prefix_query_sql(&fields, prefix_values.len())?;
        let mut params = vec![SqlValue::Text(table.as_str().to_string())];
        params.extend(
            prefix_values
                .iter()
                .map(sql_value_from_json)
                .collect::<Result<Vec<_>>>()?,
        );
        self.query_documents(&sql, params, table, check_cancel)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: Option<&serde_json::Value>,
        end: Option<&serde_json::Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.index_scan_composite_range_cancellable(
            table,
            index_name,
            &[],
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[serde_json::Value],
        start: Option<&serde_json::Value>,
        end: Option<&serde_json::Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let fields = index_fields_for_cached_schema(&self.schema_cache, table, index_name)?;
        if exact_prefix.len() >= fields.len() {
            return Err(Error::InvalidInput(format!(
                "composite range prefix length {} must be smaller than field count {} for index {}",
                exact_prefix.len(),
                fields.len(),
                index_name
            )));
        }
        let mut params = vec![SqlValue::Text(table.as_str().to_string())];
        params.extend(
            exact_prefix
                .iter()
                .map(sql_value_from_json)
                .collect::<Result<Vec<_>>>()?,
        );
        if let Some(start) = start {
            params.push(sql_value_from_json(start)?);
        }
        if let Some(end) = end {
            params.push(sql_value_from_json(end)?);
        }
        let sql = sqlite_index_scan_composite_range_query_sql(
            &fields,
            exact_prefix.len(),
            start.is_some(),
            end.is_some(),
            start_inclusive,
            end_inclusive,
        )?;
        self.query_documents(&sql, params, table, check_cancel)
    }

    fn query_documents(
        &self,
        sql: &str,
        params: Vec<SqlValue>,
        table: &TableName,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let mut stmt = self.conn.prepare_cached(sql).map_err(map_sqlite_error)?;
        let mut rows = stmt
            .query(rusqlite::params_from_iter(params))
            .map_err(map_sqlite_error)?;
        let mut documents = Vec::new();
        while let Some(row) = rows.next().map_err(map_sqlite_error)? {
            check_cancel()?;
            let id =
                DocumentId::from_str(row.get::<_, String>(0).map_err(map_sqlite_error)?.as_str())
                    .map_err(|error| Error::Serialization(error.to_string()))?;
            documents.push(row_to_document(
                table,
                &id,
                row.get(1).map_err(map_sqlite_error)?,
                row.get::<_, String>(2).map_err(map_sqlite_error)?,
            )?);
        }
        Ok(documents)
    }

    pub fn list_scheduled_jobs(&self) -> Result<Vec<ScheduledJob>> {
        load_scheduled_jobs_from_conn(&self.conn, "scheduled_jobs")
    }

    pub fn get_scheduled_job_result(&self, job_id: &JobId) -> Result<Option<ScheduledJobResult>> {
        self.conn
            .query_row(
                "SELECT data_json FROM scheduled_job_results WHERE job_id = ?1",
                params![job_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(map_sqlite_error)?
            .map(|json| deserialize_json::<ScheduledJobResult>(json.as_str()))
            .transpose()
    }

    pub fn load_cron_jobs(&self) -> Result<Vec<CronJob>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT data_json
                 FROM cron_jobs
                 ORDER BY name",
            )
            .map_err(map_sqlite_error)?;
        let mut rows = stmt.query([]).map_err(map_sqlite_error)?;
        let mut crons = Vec::new();
        while let Some(row) = rows.next().map_err(map_sqlite_error)? {
            crons.push(deserialize_json::<CronJob>(
                row.get::<_, String>(0).map_err(map_sqlite_error)?.as_str(),
            )?);
        }
        Ok(crons)
    }

    pub fn next_scheduled_work_at(&self) -> Result<Option<Timestamp>> {
        let next_job_at = self.list_scheduled_jobs()?.first().map(|job| job.run_at);
        let next_cron_at = self
            .load_cron_jobs()?
            .into_iter()
            .filter(|cron| cron.enabled)
            .map(|cron| cron.next_run)
            .min();
        Ok(match (next_job_at, next_cron_at) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        })
    }

    pub fn has_scheduled_work(&self) -> Result<bool> {
        Ok(table_has_entries(&self.conn, "scheduled_jobs")?
            || table_has_entries(&self.conn, "running_scheduled_jobs")?
            || table_has_entries(&self.conn, "cron_jobs")?)
    }

    pub fn durable_journal_cursor_floor(&self) -> Result<SequenceNumber> {
        let sequence = self
            .conn
            .query_row(
                "SELECT sequence FROM commit_log ORDER BY sequence ASC LIMIT 1",
                [],
                |row| row.get::<_, u64>(0),
            )
            .optional()
            .map_err(map_sqlite_error)?;
        Ok(SequenceNumber(
            sequence.map(|value| value.saturating_sub(1)).unwrap_or(0),
        ))
    }
}

impl SqliteWriteTransaction {
    pub fn save_table_schema(&mut self, table_schema: &TableSchema) -> Result<()> {
        self.check_cancel()?;
        self.connection_mut()?
            .execute(
                "INSERT INTO schemas (table_name, schema_json) VALUES (?1, ?2)
                 ON CONFLICT(table_name) DO UPDATE SET schema_json = excluded.schema_json",
                params![table_schema.table.as_str(), serialize_json(table_schema)?],
            )
            .map_err(map_sqlite_error)?;
        self.schema_cache_dirty = true;
        Ok(())
    }

    pub fn replace_table_schema(&mut self, table_schema: &TableSchema) -> Result<()> {
        self.check_cancel()?;
        if let Some(previous) =
            load_table_schema_from_conn(self.connection_mut()?, &table_schema.table)?
        {
            drop_sqlite_indexes_for_table_schema(self.connection_mut()?, &previous)?;
        }
        self.save_table_schema(table_schema)?;
        create_sqlite_indexes_for_table_schema(self.connection_mut()?, table_schema)?;
        Ok(())
    }

    pub fn delete_table_schema_entry(&mut self, table: &TableName) -> Result<()> {
        self.check_cancel()?;
        self.connection_mut()?
            .execute(
                "DELETE FROM schemas WHERE table_name = ?1",
                params![table.as_str()],
            )
            .map_err(map_sqlite_error)?;
        self.schema_cache_dirty = true;
        Ok(())
    }

    pub fn delete_table_schema(&mut self, table: &TableName) -> Result<()> {
        self.check_cancel()?;
        if let Some(previous) = load_table_schema_from_conn(self.connection_mut()?, table)? {
            drop_sqlite_indexes_for_table_schema(self.connection_mut()?, &previous)?;
        }
        self.delete_table_schema_entry(table)
    }

    pub fn put_metadata(&mut self, key: &str, value: &[u8]) -> Result<()> {
        self.check_cancel()?;
        self.connection_mut()?
            .execute(
                "INSERT INTO metadata (key, value_blob) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value_blob = excluded.value_blob",
                params![key, value],
            )
            .map_err(map_sqlite_error)?;
        Ok(())
    }

    pub fn begin_scheduled_execution(&mut self, execution_id: Option<&str>) -> Result<bool> {
        self.check_cancel()?;
        let Some(execution_id) = execution_id else {
            return Ok(true);
        };

        let inserted = self
            .connection_mut()?
            .execute(
                "INSERT OR IGNORE INTO scheduled_job_executions (execution_id) VALUES (?1)",
                params![execution_id],
            )
            .map_err(map_sqlite_error)?;
        Ok(inserted == 1)
    }

    pub fn insert_document(&mut self, document: &Document) -> Result<()> {
        self.check_cancel()?;
        self.connection_mut()?
            .execute(
                "INSERT INTO documents (table_name, id, data_json, creation_time)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    document.table.as_str(),
                    document.id.to_string(),
                    serialize_document_fields(document)?,
                    document.creation_time.0,
                ],
            )
            .map_err(map_sqlite_error)?;
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
        let existing_document = self
            .load_document(table, id)?
            .ok_or(Error::DocumentNotFound(*id))?;
        let mut document = existing_document.clone();
        for (field, value) in patch {
            document.fields.insert(field.clone(), value.clone());
        }
        validate(&existing_document, &document)?;
        self.connection_mut()?
            .execute(
                "UPDATE documents
                 SET data_json = ?3, creation_time = ?4
                 WHERE table_name = ?1 AND id = ?2",
                params![
                    table.as_str(),
                    id.to_string(),
                    serialize_document_fields(&document)?,
                    document.creation_time.0,
                ],
            )
            .map_err(map_sqlite_error)?;
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
        let removed_document = self
            .load_document(table, id)?
            .ok_or(Error::DocumentNotFound(*id))?;
        validate(&removed_document)?;
        self.connection_mut()?
            .execute(
                "DELETE FROM documents WHERE table_name = ?1 AND id = ?2",
                params![table.as_str(), id.to_string()],
            )
            .map_err(map_sqlite_error)?;
        self.record_commit_write(WriteOp {
            table: table.clone(),
            op_type: WriteOpType::Delete,
            doc_id: *id,
            previous: Some(removed_document.clone()),
            current: None,
        });
        Ok(removed_document)
    }

    pub fn insert_scheduled_job(&mut self, job: &ScheduledJob) -> Result<()> {
        self.check_cancel()?;
        self.connection_mut()?
            .execute(
                "INSERT INTO scheduled_jobs (id, data_json) VALUES (?1, ?2)",
                params![job.id.to_string(), serialize_json(job)?],
            )
            .map_err(map_sqlite_error)?;
        Ok(())
    }

    pub fn claim_due_jobs(&mut self, now: Timestamp) -> Result<Vec<ScheduledJob>> {
        self.check_cancel()?;
        let due = load_scheduled_jobs_from_conn(self.connection_mut()?, "scheduled_jobs")?
            .into_iter()
            .filter(|job| job.run_at.0 <= now.0)
            .collect::<Vec<_>>();
        for job in &due {
            self.check_cancel()?;
            self.connection_mut()?
                .execute(
                    "DELETE FROM scheduled_jobs WHERE id = ?1",
                    params![job.id.to_string()],
                )
                .map_err(map_sqlite_error)?;
            self.connection_mut()?
                .execute(
                    "INSERT INTO running_scheduled_jobs (id, data_json) VALUES (?1, ?2)",
                    params![job.id.to_string(), serialize_json(job)?],
                )
                .map_err(map_sqlite_error)?;
        }
        Ok(due)
    }

    pub fn complete_scheduled_job(&mut self, job_id: &JobId) -> Result<()> {
        self.check_cancel()?;
        self.connection_mut()?
            .execute(
                "DELETE FROM running_scheduled_jobs WHERE id = ?1",
                params![job_id.to_string()],
            )
            .map_err(map_sqlite_error)?;
        Ok(())
    }

    pub fn cancel_scheduled_job(&mut self, job_id: &JobId) -> Result<bool> {
        self.check_cancel()?;
        let affected = self
            .connection_mut()?
            .execute(
                "DELETE FROM scheduled_jobs WHERE id = ?1",
                params![job_id.to_string()],
            )
            .map_err(map_sqlite_error)?;
        Ok(affected == 1)
    }

    pub fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()> {
        self.check_cancel()?;
        self.connection_mut()?
            .execute(
                "INSERT INTO scheduled_job_results (job_id, data_json) VALUES (?1, ?2)
                 ON CONFLICT(job_id) DO UPDATE SET data_json = excluded.data_json",
                params![result.id.to_string(), serialize_json(result)?],
            )
            .map_err(map_sqlite_error)?;
        Ok(())
    }

    pub fn save_cron_job(&mut self, cron: &CronJob) -> Result<()> {
        self.check_cancel()?;
        self.connection_mut()?
            .execute(
                "INSERT INTO cron_jobs (name, data_json) VALUES (?1, ?2)
                 ON CONFLICT(name) DO UPDATE SET data_json = excluded.data_json",
                params![cron.name, serialize_json(cron)?],
            )
            .map_err(map_sqlite_error)?;
        Ok(())
    }

    pub fn delete_cron_job(&mut self, name: &str) -> Result<()> {
        self.check_cancel()?;
        self.connection_mut()?
            .execute("DELETE FROM cron_jobs WHERE name = ?1", params![name])
            .map_err(map_sqlite_error)?;
        Ok(())
    }

    pub fn recover_running_jobs(&mut self, now: Timestamp) -> Result<()> {
        self.check_cancel()?;
        let running_jobs =
            load_scheduled_jobs_from_conn(self.connection_mut()?, "running_scheduled_jobs")?;
        for mut job in running_jobs {
            self.check_cancel()?;
            job.run_at = now;
            self.connection_mut()?
                .execute(
                    "INSERT INTO scheduled_jobs (id, data_json) VALUES (?1, ?2)",
                    params![job.id.to_string(), serialize_json(&job)?],
                )
                .map_err(map_sqlite_error)?;
            self.connection_mut()?
                .execute(
                    "DELETE FROM running_scheduled_jobs WHERE id = ?1",
                    params![job.id.to_string()],
                )
                .map_err(map_sqlite_error)?;
        }
        Ok(())
    }

    pub fn apply_resolved_write(&mut self, write: &ResolvedWrite) -> Result<()> {
        match write {
            ResolvedWrite::Insert { document, .. } => {
                self.check_cancel()?;
                if self.load_document(&document.table, &document.id)?.is_some() {
                    return Err(Error::Conflict(format!(
                        "document {} changed before transaction commit",
                        document.id
                    )));
                }
                self.insert_document(document)
            }
            ResolvedWrite::Update {
                previous, current, ..
            } => {
                self.check_cancel()?;
                let existing =
                    self.load_document(&current.table, &current.id)?
                        .ok_or(Error::Conflict(format!(
                            "document {} changed before transaction commit",
                            current.id
                        )))?;
                if existing != *previous {
                    return Err(Error::Conflict(format!(
                        "document {} changed before transaction commit",
                        current.id
                    )));
                }
                self.connection_mut()?
                    .execute(
                        "UPDATE documents
                         SET data_json = ?3, creation_time = ?4
                         WHERE table_name = ?1 AND id = ?2",
                        params![
                            current.table.as_str(),
                            current.id.to_string(),
                            serialize_document_fields(current)?,
                            current.creation_time.0,
                        ],
                    )
                    .map_err(map_sqlite_error)?;
                self.record_commit_write(WriteOp {
                    table: current.table.clone(),
                    op_type: WriteOpType::Update,
                    doc_id: current.id,
                    previous: Some(previous.clone()),
                    current: Some(current.clone()),
                });
                Ok(())
            }
            ResolvedWrite::Delete { previous, .. } => {
                self.check_cancel()?;
                let existing =
                    self.load_document(&previous.table, &previous.id)?
                        .ok_or(Error::Conflict(format!(
                            "document {} changed before transaction commit",
                            previous.id
                        )))?;
                if existing != *previous {
                    return Err(Error::Conflict(format!(
                        "document {} changed before transaction commit",
                        previous.id
                    )));
                }
                self.connection_mut()?
                    .execute(
                        "DELETE FROM documents WHERE table_name = ?1 AND id = ?2",
                        params![previous.table.as_str(), previous.id.to_string()],
                    )
                    .map_err(map_sqlite_error)?;
                self.record_commit_write(WriteOp {
                    table: previous.table.clone(),
                    op_type: WriteOpType::Delete,
                    doc_id: previous.id,
                    previous: Some(previous.clone()),
                    current: None,
                });
                Ok(())
            }
        }
    }

    pub(crate) fn check_cancel(&self) -> Result<()> {
        (self.check_cancel.as_ref())()
    }

    pub fn commit(mut self) -> Result<Option<CommitEntry>> {
        self.check_cancel()?;
        let Some(conn) = self.conn.take() else {
            return Err(Error::Internal(
                "sqlite write transaction already closed".to_string(),
            ));
        };
        let commit = if self.commit_writes.is_empty() {
            None
        } else {
            Some(append_commit_entry(
                &conn,
                self.clock.now(),
                std::mem::take(&mut self.commit_writes),
            )?)
        };
        self.fault_injector
            .check(FaultPoint::StorageCommitBeforeVisibility)?;
        conn.execute_batch("COMMIT").map_err(map_sqlite_error)?;
        if self.schema_cache_dirty {
            let schema = load_schema_from_conn(&conn)?;
            *self
                .schema_cache
                .write()
                .map_err(|_| Error::Internal("sqlite schema cache lock poisoned".to_string()))? =
                schema;
        }
        self.fault_injector
            .check(FaultPoint::StorageCommitAfterVisibilityBeforeReturn)?;
        Ok(commit)
    }

    pub fn rollback(mut self) {
        if let Some(conn) = self.conn.take() {
            let _ = conn.execute_batch("ROLLBACK");
        }
    }

    fn connection_mut(&mut self) -> Result<&mut Connection> {
        self.conn
            .as_mut()
            .ok_or_else(|| Error::Internal("sqlite write transaction already closed".to_string()))
    }

    fn record_commit_write(&mut self, write: WriteOp) {
        self.commit_writes.push(write);
    }

    fn load_document(&mut self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        load_document_from_conn(self.connection_mut()?, table, id)
    }
}

impl Deref for PooledSqliteConnection {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        self.conn
            .as_ref()
            .expect("pooled sqlite connection should not be empty while borrowed")
    }
}

impl Drop for PooledSqliteConnection {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take()
            && let Ok(mut pool) = self.pool.lock()
        {
            pool.push(conn);
        }
    }
}

fn initialize_connection(conn: &Connection) -> Result<()> {
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(map_sqlite_error)?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(map_sqlite_error)?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(map_sqlite_error)?;
    conn.pragma_update(None, "synchronous", "FULL")
        .map_err(map_sqlite_error)?;
    conn.execute_batch(SQLITE_INIT_SQL)
        .map_err(map_sqlite_error)?;
    Ok(())
}

pub(crate) fn rebuild_sqlite_indexes_from_loaded_schema(conn: &Connection) -> Result<()> {
    let schema = load_schema_from_conn(conn)?;
    for table_schema in schema.tables.values() {
        create_sqlite_indexes_for_table_schema(conn, table_schema)?;
    }
    Ok(())
}

fn load_schema_from_conn(conn: &Connection) -> Result<Schema> {
    let mut stmt = conn
        .prepare_cached("SELECT schema_json FROM schemas ORDER BY table_name")
        .map_err(map_sqlite_error)?;
    let mut rows = stmt.query([]).map_err(map_sqlite_error)?;
    let mut schema = Schema::default();
    while let Some(row) = rows.next().map_err(map_sqlite_error)? {
        let table_schema: TableSchema =
            serde_json::from_str(row.get::<_, String>(0).map_err(map_sqlite_error)?.as_str())
                .map_err(|error| Error::Serialization(error.to_string()))?;
        schema
            .tables
            .insert(table_schema.table.clone(), table_schema);
    }
    Ok(schema)
}

fn expect_write_commit(commit: Option<CommitEntry>, expectation: &str) -> Result<CommitEntry> {
    commit.ok_or_else(|| Error::Internal(expectation.to_string()))
}

fn append_commit_entry(
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

fn apply_durable_record_in_conn(conn: &Connection, record: &DurableMutationRecord) -> Result<()> {
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
                            "INSERT INTO documents (table_name, id, data_json, creation_time)
                             VALUES (?1, ?2, ?3, ?4)",
                            params![
                                write.table.as_str(),
                                write.doc_id.to_string(),
                                serialize_document_fields(current)?,
                                current.creation_time.0,
                            ],
                        )
                        .map_err(map_sqlite_error)?;
                    }
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
                     SET data_json = ?3, creation_time = ?4
                     WHERE table_name = ?1 AND id = ?2",
                    params![
                        write.table.as_str(),
                        write.doc_id.to_string(),
                        serialize_document_fields(current)?,
                        current.creation_time.0,
                    ],
                )
                .map_err(map_sqlite_error)?;
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

fn applied_sequence_in_conn(conn: &Connection) -> Result<SequenceNumber> {
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

fn latest_sequence_in_conn(conn: &Connection) -> Result<SequenceNumber> {
    Ok(SequenceNumber(
        next_sequence_in_conn(conn)?.saturating_sub(1),
    ))
}

fn begin_scheduled_execution_in_conn(
    conn: &Connection,
    execution_id: Option<&str>,
) -> Result<bool> {
    let Some(execution_id) = execution_id else {
        return Ok(true);
    };
    let inserted = conn
        .execute(
            "INSERT OR IGNORE INTO scheduled_job_executions (execution_id) VALUES (?1)",
            params![execution_id],
        )
        .map_err(map_sqlite_error)?;
    Ok(inserted == 1)
}

fn next_sequence_in_conn(conn: &Connection) -> Result<u64> {
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

fn load_document_from_conn(
    conn: &Connection,
    table: &TableName,
    id: &DocumentId,
) -> Result<Option<Document>> {
    conn.query_row(
        "SELECT creation_time, data_json
         FROM documents
         WHERE table_name = ?1 AND id = ?2",
        params![table.as_str(), id.to_string()],
        |row| {
            Ok(row_to_document(
                table,
                id,
                row.get(0)?,
                row.get::<_, String>(1)?,
            ))
        },
    )
    .optional()
    .map_err(map_sqlite_error)?
    .transpose()
}

fn put_metadata_in_conn(conn: &Connection, key: &str, value: &[u8]) -> Result<()> {
    conn.execute(
        "INSERT INTO metadata (key, value_blob) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_blob = excluded.value_blob",
        params![key, value],
    )
    .map_err(map_sqlite_error)?;
    Ok(())
}

fn load_scheduled_jobs_from_conn(conn: &Connection, table_name: &str) -> Result<Vec<ScheduledJob>> {
    let sql = format!("SELECT data_json FROM {table_name}");
    let mut stmt = conn.prepare(sql.as_str()).map_err(map_sqlite_error)?;
    let mut rows = stmt.query([]).map_err(map_sqlite_error)?;
    let mut jobs = Vec::new();
    while let Some(row) = rows.next().map_err(map_sqlite_error)? {
        jobs.push(deserialize_json::<ScheduledJob>(
            row.get::<_, String>(0).map_err(map_sqlite_error)?.as_str(),
        )?);
    }
    jobs.sort_by(|left, right| left.run_at.cmp(&right.run_at).then(left.id.cmp(&right.id)));
    Ok(jobs)
}

fn load_table_schema_from_conn(
    conn: &Connection,
    table: &TableName,
) -> Result<Option<TableSchema>> {
    conn.query_row(
        "SELECT schema_json FROM schemas WHERE table_name = ?1",
        params![table.as_str()],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(map_sqlite_error)?
    .map(|json| deserialize_json::<TableSchema>(json.as_str()))
    .transpose()
}

fn create_sqlite_indexes_for_table_schema(
    conn: &Connection,
    table_schema: &TableSchema,
) -> Result<()> {
    for index in &table_schema.indexes {
        let sql = format!(
            "CREATE INDEX IF NOT EXISTS \"{}\" ON documents ({})",
            sqlite_index_name(&table_schema.table, &index.name),
            sqlite_index_columns(&index.fields)
        );
        conn.execute_batch(&sql).map_err(map_sqlite_error)?;
    }
    Ok(())
}

fn drop_sqlite_indexes_for_table_schema(
    conn: &Connection,
    table_schema: &TableSchema,
) -> Result<()> {
    for index in &table_schema.indexes {
        let sql = format!(
            "DROP INDEX IF EXISTS \"{}\"",
            sqlite_index_name(&table_schema.table, &index.name)
        );
        conn.execute_batch(&sql).map_err(map_sqlite_error)?;
    }
    Ok(())
}

/// Builds the exact SQLite statement shape used for single-field and
/// exact-prefix indexed scans.
///
/// Benchmark/report tooling uses this instead of reimplementing the SQL so
/// `EXPLAIN QUERY PLAN` output stays aligned with the production read path.
pub fn sqlite_index_scan_prefix_query_sql<S>(fields: &[S], prefix_len: usize) -> Result<String>
where
    S: AsRef<str>,
{
    if prefix_len > fields.len() {
        return Err(Error::InvalidInput(format!(
            "index prefix length {} exceeds field count {}",
            prefix_len,
            fields.len()
        )));
    }

    let where_clauses = exact_prefix_clauses(&fields[..prefix_len]);
    let order_by = sqlite_order_by_fields_after_exact_prefix(fields, prefix_len);
    Ok(format!(
        "SELECT id, creation_time, data_json
         FROM documents
         WHERE table_name = ?1 AND {}
         ORDER BY {}",
        where_clauses.join(" AND "),
        order_by
    ))
}

/// Builds the exact SQLite statement shape used for composite exact-prefix plus
/// range indexed scans.
///
/// Benchmark/report tooling uses this instead of reimplementing the SQL so
/// `EXPLAIN QUERY PLAN` output stays aligned with the production read path.
#[allow(clippy::too_many_arguments)]
pub fn sqlite_index_scan_composite_range_query_sql<S>(
    fields: &[S],
    exact_prefix_len: usize,
    has_start: bool,
    has_end: bool,
    start_inclusive: bool,
    end_inclusive: bool,
) -> Result<String>
where
    S: AsRef<str>,
{
    if exact_prefix_len >= fields.len() {
        return Err(Error::InvalidInput(format!(
            "composite range prefix length {} must be smaller than field count {}",
            exact_prefix_len,
            fields.len()
        )));
    }

    let range_field = fields[exact_prefix_len].as_ref();
    let mut clauses = exact_prefix_clauses(&fields[..exact_prefix_len]);
    let mut next_param = exact_prefix_len + 2;
    if has_start {
        clauses.push(format!(
            "{} {} ?{}",
            json_extract_expr(range_field),
            if start_inclusive { ">=" } else { ">" },
            next_param
        ));
        next_param += 1;
    }
    if has_end {
        clauses.push(format!(
            "{} {} ?{}",
            json_extract_expr(range_field),
            if end_inclusive { "<=" } else { "<" },
            next_param
        ));
    }

    Ok(format!(
        "SELECT id, creation_time, data_json
         FROM documents
         WHERE table_name = ?1 AND {}
         ORDER BY {}",
        clauses.join(" AND "),
        sqlite_order_by_fields_after_exact_prefix(fields, exact_prefix_len)
    ))
}

fn apply_schedule_ops_in_transaction(
    transaction: &mut SqliteWriteTransaction,
    schedule_ops: &[ResolvedScheduleOp],
) -> Result<()> {
    for schedule_op in schedule_ops {
        match schedule_op {
            ResolvedScheduleOp::Insert { job } => transaction.insert_scheduled_job(job)?,
            ResolvedScheduleOp::Cancel { job_id } => {
                if !transaction.cancel_scheduled_job(job_id)? {
                    return Err(Error::ScheduledJobNotFound(*job_id));
                }
            }
        }
    }
    Ok(())
}

fn table_has_entries(conn: &Connection, table_name: &str) -> Result<bool> {
    let sql = format!("SELECT 1 FROM {table_name} LIMIT 1");
    Ok(conn
        .query_row(sql.as_str(), [], |_| Ok(()))
        .optional()
        .map_err(map_sqlite_error)?
        .is_some())
}

fn serialize_json<T>(value: &T) -> Result<String>
where
    T: Serialize,
{
    serde_json::to_string(value).map_err(|error| Error::Serialization(error.to_string()))
}

fn deserialize_json<T>(json: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_str(json).map_err(|error| Error::Serialization(error.to_string()))
}

fn serialize_document_fields(document: &Document) -> Result<String> {
    serde_json::to_string(&document.fields).map_err(|error| Error::Serialization(error.to_string()))
}

fn encode_u64(value: u64) -> [u8; 8] {
    value.to_be_bytes()
}

fn decode_u64(bytes: &[u8]) -> Result<u64> {
    let array: [u8; 8] = bytes.try_into().map_err(|_| {
        Error::Internal("expected 8 bytes when decoding sqlite metadata".to_string())
    })?;
    Ok(u64::from_be_bytes(array))
}

fn row_to_document(
    table: &TableName,
    id: &DocumentId,
    creation_time: u64,
    data_json: String,
) -> Result<Document> {
    Ok(Document {
        id: *id,
        table: table.clone(),
        creation_time: Timestamp(creation_time),
        fields: serde_json::from_str(&data_json)
            .map_err(|error| Error::Serialization(error.to_string()))?,
    })
}

fn index_fields_for_schema(
    schema: &Schema,
    table: &TableName,
    index_name: &str,
) -> Result<Vec<String>> {
    let Some(table_schema) = schema.get_table(table) else {
        return Err(Error::SchemaNotFound(table.clone()));
    };
    table_schema
        .indexes
        .iter()
        .find(|definition| definition.name == index_name)
        .map(|definition| definition.fields.clone())
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "index not found for table {}: {}",
                table, index_name
            ))
        })
}

fn index_fields_for_cached_schema(
    schema_cache: &Arc<RwLock<Schema>>,
    table: &TableName,
    index_name: &str,
) -> Result<Vec<String>> {
    let schema = schema_cache
        .read()
        .map_err(|_| Error::Internal("sqlite schema cache lock poisoned".to_string()))?;
    index_fields_for_schema(&schema, table, index_name)
}

fn sqlite_index_name(table: &TableName, index_name: &str) -> String {
    format!(
        "idx_{}_{}",
        sanitize_identifier_component(table.as_str()),
        sanitize_identifier_component(index_name)
    )
}

fn sqlite_index_columns<S>(fields: &[S]) -> String
where
    S: AsRef<str>,
{
    let mut columns = Vec::with_capacity(fields.len() + 2);
    columns.push("table_name".to_string());
    columns.extend(fields.iter().map(|field| json_extract_expr(field.as_ref())));
    columns.push("id".to_string());
    columns.join(", ")
}

fn sqlite_order_by_fields_after_exact_prefix<S>(fields: &[S], exact_prefix_len: usize) -> String
where
    S: AsRef<str>,
{
    let mut columns = fields
        .iter()
        .skip(exact_prefix_len)
        .map(|field| json_extract_expr(field.as_ref()))
        .collect::<Vec<_>>();
    columns.push("id".to_string());
    columns.join(", ")
}

fn exact_prefix_clauses<S>(fields: &[S]) -> Vec<String>
where
    S: AsRef<str>,
{
    fields
        .iter()
        .enumerate()
        .map(|(index, field)| format!("{} = ?{}", json_extract_expr(field.as_ref()), index + 2))
        .collect()
}

fn json_extract_expr(field: &str) -> String {
    format!(
        "json_extract(data_json, '$.\"{}\"')",
        field.replace('"', "\\\"")
    )
}

fn sanitize_identifier_component(input: &str) -> String {
    input
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

fn sql_value_from_json(value: &serde_json::Value) -> Result<SqlValue> {
    match value {
        serde_json::Value::Null => Ok(SqlValue::Null),
        serde_json::Value::Bool(value) => Ok(SqlValue::Integer(i64::from(*value))),
        serde_json::Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                Ok(SqlValue::Integer(value))
            } else if let Some(value) = number.as_u64() {
                i64::try_from(value)
                    .map(SqlValue::Integer)
                    .map_err(|_| Error::InvalidInput(format!("numeric value exceeds i64: {value}")))
            } else if let Some(value) = number.as_f64() {
                Ok(SqlValue::Real(value))
            } else {
                Err(Error::InvalidInput(format!(
                    "unsupported numeric value: {number}"
                )))
            }
        }
        serde_json::Value::String(value) => Ok(SqlValue::Text(value.clone())),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => Err(Error::InvalidInput(
            "SQLite index scans do not support array or object comparison values".to_string(),
        )),
    }
}

fn map_sqlite_error(error: rusqlite::Error) -> Error {
    Error::Storage(error.to_string())
}

fn validate_durable_journal_stream_limit(limit: usize) -> Result<()> {
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
