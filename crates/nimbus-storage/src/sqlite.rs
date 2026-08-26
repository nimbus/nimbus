use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::Duration;

use nimbus_core::{
    CommitEntry, CronJob, Document, DocumentId, Error, Filter, IdSource, IndexLifecycleEvent,
    JobId, Result, ScheduledJob, ScheduledJobResult, Schema, SchemaChangeEvent, SequenceNumber,
    StorageErrorKind, SystemIdSource, SystemWallClock, TableId, TableLifecycleEvent, TableName,
    TableSchema, TableState, TenantEventKind, TenantEventRecord, Timestamp, TriggerDeliveryCursor,
    TriggerWriteOrigin, WallClock, WriteOp, WriteOpType,
};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::commit_log::{deserialize_tenant_event_record, serialize_tenant_event_record};
use crate::simulation::{FaultInjector, FaultPoint, NoopFaultInjector};
use crate::store::{
    APPLIED_SEQUENCE_KEY, DurableJournalBootstrap, DurableJournalPage, JournalProgress,
    MAX_DURABLE_JOURNAL_STREAM_LIMIT, MaterializedJournalSnapshot, NEXT_SEQUENCE_KEY,
    PointInTimeRestoreArchive, PointInTimeRestoreTarget, ResolvedScheduleOp, ResolvedWrite,
    TenantWriteCommit,
};
use crate::{
    MaterializedRetentionCheckpoint, MaterializedVerificationGeneration,
    MaterializedVerificationInvalidator, RetentionFloor, RetentionGcConfig, RetentionGcSummary,
    RetentionHistoryState, RetentionHistorySummary,
};
use nimbus_crypto::DataEncryptionKey;

mod apply_context;
mod backend;
mod config;
mod document_versions;
pub mod encryption;
mod index_versions;
mod journal;
mod read;
// The libsql replica cache is the only reason these SQLite operations exist;
// `test` keeps the reconciliation path compiled for the SQLite-foundation
// coverage that exercises it directly.
#[cfg(any(test, feature = "libsql"))]
pub(crate) mod replica_cache;
mod resource_paths;
mod scheduler;
mod schema;
mod table_lifecycle;
mod trigger_delivery;
mod trigger_invocations;
mod write;

use self::apply_context::SqliteBatchApplyContext;
use self::backend::{
    cached_execute, decode_u64, deserialize_json, encode_u64, ensure_table_id_in_conn,
    ensure_table_identity_in_conn, expect_write_commit, load_document_by_table_id_from_conn,
    load_document_from_conn, map_sqlite_error, resolve_or_create_table_id_in_conn,
    resolve_table_id_in_conn, row_to_document, serialize_document_fields,
    serialize_document_typed_fields, serialize_json, sql_value_from_json, table_has_entries,
};
#[cfg(test)]
pub(crate) use self::config::SqliteWriteStatementConcept;
#[cfg(any(test, feature = "test-hooks"))]
pub use self::config::{
    SqlitePassiveCheckpointProbe, SqliteWalCheckpointObservationSnapshot,
    disable_sqlite_wal_checkpoint_observation, probe_sqlite_passive_checkpoint,
    reset_sqlite_wal_checkpoint_observation, sqlite_wal_checkpoint_observation_snapshot,
};
use self::journal::{
    append_commit_entry, next_sequence_in_conn, validate_durable_journal_stream_limit,
};
pub(crate) use self::scheduler::scheduled_run_at_key;
use self::scheduler::{
    apply_schedule_ops_in_transaction, begin_scheduled_execution_in_conn,
    load_due_scheduled_jobs_from_conn, load_scheduled_job_by_id_from_conn,
    load_scheduled_jobs_from_conn,
};
use self::schema::{
    create_sqlite_indexes_for_table_schema, drop_sqlite_indexes_for_table_schema,
    index_fields_for_cached_schema, load_schema_from_conn, load_table_schema_from_conn,
};
pub use self::schema::{
    sqlite_index_scan_composite_range_query_sql, sqlite_index_scan_prefix_query_sql,
};

pub(crate) const SQLITE_INIT_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS table_catalog (
    namespace TEXT NOT NULL DEFAULT 'default',
    table_name TEXT NOT NULL,
    table_id TEXT NOT NULL UNIQUE,
    state TEXT NOT NULL DEFAULT 'active',
    PRIMARY KEY (namespace, table_name)
);

CREATE TABLE IF NOT EXISTS documents (
    table_id TEXT NOT NULL,
    id TEXT NOT NULL,
    data_json TEXT NOT NULL,
    typed_fields_json TEXT NOT NULL DEFAULT '{}',
    creation_time INTEGER NOT NULL,
    update_time INTEGER NOT NULL,
    PRIMARY KEY (table_id, id),
    FOREIGN KEY (table_id) REFERENCES table_catalog(table_id)
);

CREATE TABLE IF NOT EXISTS document_versions (
    table_id TEXT NOT NULL,
    id TEXT NOT NULL,
    commit_sequence INTEGER NOT NULL,
    commit_time INTEGER NOT NULL,
    tombstone INTEGER NOT NULL CHECK (tombstone IN (0, 1)),
    data_json TEXT,
    typed_fields_json TEXT,
    creation_time INTEGER,
    update_time INTEGER,
    PRIMARY KEY (table_id, id, commit_sequence),
    CHECK (
        (
            tombstone = 1
            AND data_json IS NULL
            AND typed_fields_json IS NULL
            AND creation_time IS NULL
            AND update_time IS NULL
        )
        OR (
            tombstone = 0
            AND data_json IS NOT NULL
            AND typed_fields_json IS NOT NULL
            AND creation_time IS NOT NULL
            AND update_time IS NOT NULL
        )
    )
);

CREATE TABLE IF NOT EXISTS index_versions (
    table_id TEXT NOT NULL,
    index_id TEXT NOT NULL,
    encoded_tuple BLOB NOT NULL,
    document_id TEXT NOT NULL,
    visible_from INTEGER NOT NULL,
    visible_until INTEGER,
    PRIMARY KEY (table_id, index_id, encoded_tuple, document_id, visible_from)
);

CREATE INDEX IF NOT EXISTS idx_index_versions_visibility
    ON index_versions (table_id, index_id, encoded_tuple, document_id, visible_from);

CREATE TABLE IF NOT EXISTS schemas (
    table_name TEXT NOT NULL PRIMARY KEY,
    schema_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS resource_path_bindings (
    locator_key BLOB NOT NULL PRIMARY KEY,
    document_path_key BLOB NOT NULL UNIQUE,
    collection_group TEXT NOT NULL,
    binding_blob BLOB NOT NULL,
    locator_blob BLOB NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_resource_path_bindings_collection_group_path
    ON resource_path_bindings (collection_group, document_path_key);

CREATE TABLE IF NOT EXISTS scheduled_jobs (
    id TEXT NOT NULL PRIMARY KEY,
    run_at TEXT NOT NULL,
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

CREATE TABLE IF NOT EXISTS trigger_invocations (
    registration_id TEXT NOT NULL,
    event_id TEXT NOT NULL,
    data_blob BLOB NOT NULL,
    PRIMARY KEY (registration_id, event_id)
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

// Floor for the read pool. The engine legitimately holds several read
// snapshots concurrently during one mutation (measured envelope: 5 on the
// atomic-write precondition path), so the floor must exceed that even when
// `available_parallelism` is small (4-core CI runners). Override with
// NIMBUS_SQLITE_MAX_READ_CONNECTIONS for diagnostics.
pub(crate) const MIN_SQLITE_READ_CONNECTIONS: usize = 8;

pub fn sqlite_init_sql() -> &'static str {
    SQLITE_INIT_SQL
}

impl SqliteTenantStore {
    pub fn retention_gc_watermarks(
        &self,
        config: RetentionGcConfig,
    ) -> Result<crate::RetentionGcWatermarks> {
        Ok(self
            .retention_floor
            .gc_watermarks(self.journal_progress()?.applied_head, config))
    }

    pub fn compact_retained_versions(
        &self,
        config: RetentionGcConfig,
    ) -> Result<RetentionGcSummary> {
        let watermarks = self.retention_gc_watermarks(config)?;
        let document_prune_before = watermarks.document_versions.safe_prune_before;
        let index_prune_before = watermarks.index_versions.safe_prune_before;
        if document_prune_before.0 == 0 && index_prune_before.0 == 0 {
            return Ok(RetentionGcSummary {
                watermarks,
                document_versions_pruned: 0,
                index_versions_pruned: 0,
            });
        }

        let mut transaction = self.begin_write_transaction()?;
        let document_versions_pruned = document_versions::prune_document_versions_before_in_conn(
            transaction.connection_mut()?,
            document_prune_before,
        )?;
        let index_versions_pruned = index_versions::prune_index_versions_before_in_conn(
            transaction.connection_mut()?,
            index_prune_before,
        )?;
        let commit = transaction.commit()?;
        debug_assert!(commit.is_none());
        Ok(RetentionGcSummary {
            watermarks,
            document_versions_pruned,
            index_versions_pruned,
        })
    }

    pub fn retention_history_state(
        &self,
        config: RetentionGcConfig,
    ) -> Result<RetentionHistoryState> {
        let watermarks = self.retention_gc_watermarks(config)?;
        let (checkpoint, physical_floor, _) = self.load_retention_checkpoint()?;
        RetentionHistoryState::new(
            crate::retention::desired_journal_floor(&watermarks).max(checkpoint.sequence()),
            physical_floor,
            checkpoint,
        )
    }

    pub fn compact_retained_history(
        &self,
        config: RetentionGcConfig,
    ) -> Result<RetentionHistorySummary> {
        let watermarks = self.retention_gc_watermarks(config)?;
        let (checkpoint, physical_floor, expected_checkpoint_blob) =
            self.load_retention_checkpoint()?;
        let desired_floor =
            crate::retention::desired_journal_floor(&watermarks).max(checkpoint.sequence());
        let before = RetentionHistoryState::new(desired_floor, physical_floor, checkpoint.clone())?;
        let journal_tail = self
            .read_durable_journal_from(SequenceNumber(checkpoint.sequence().0.saturating_add(1)))?;
        let candidate = checkpoint.advance(&journal_tail, desired_floor)?;
        let candidate_blob = crate::retention::serialize_retention_checkpoint(&candidate)?;

        let mut transaction = self.begin_write_transaction()?;
        let current_checkpoint_blob = transaction
            .connection_mut()?
            .query_row(
                "SELECT value_blob FROM metadata WHERE key = ?1",
                [crate::retention::RETENTION_CHECKPOINT_METADATA_KEY],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(map_sqlite_error)?;
        if current_checkpoint_blob != expected_checkpoint_blob {
            return Err(Error::conflict(
                "retention checkpoint changed while compaction was prepared".to_string(),
            ));
        }
        let applied_head = transaction
            .connection_mut()?
            .query_row(
                "SELECT value_blob FROM metadata WHERE key = ?1",
                [APPLIED_SEQUENCE_KEY],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(map_sqlite_error)?
            .as_deref()
            .map(decode_u64)
            .transpose()?
            .unwrap_or(0);
        if candidate.sequence().0 > applied_head {
            return Err(Error::conflict(format!(
                "retention checkpoint target {} exceeds current applied head {}",
                candidate.sequence().0,
                applied_head
            )));
        }
        let document_versions_pruned = document_versions::prune_document_versions_before_in_conn(
            transaction.connection_mut()?,
            watermarks.document_versions.safe_prune_before,
        )?;
        let index_versions_pruned = index_versions::prune_index_versions_before_in_conn(
            transaction.connection_mut()?,
            watermarks.index_versions.safe_prune_before,
        )?;
        let journal_records_pruned = transaction
            .connection_mut()?
            .execute(
                "DELETE FROM commit_log WHERE sequence <= ?1",
                [candidate.sequence().0],
            )
            .map_err(map_sqlite_error)? as u64;
        transaction.put_metadata(
            crate::retention::RETENTION_CHECKPOINT_METADATA_KEY,
            candidate_blob.as_slice(),
        )?;
        transaction.put_metadata(
            crate::retention::RETENTION_PHYSICAL_FLOOR_METADATA_KEY,
            candidate.sequence().0.to_be_bytes().as_slice(),
        )?;
        self.fault_injector
            .check(FaultPoint::RetentionCheckpointBeforeCommit)?;
        let commit = transaction.commit()?;
        debug_assert!(commit.is_none());
        self.fault_injector
            .check(FaultPoint::RetentionCheckpointAfterCommit)?;

        let after = RetentionHistoryState::new(desired_floor, candidate.sequence(), candidate)?;
        Ok(RetentionHistorySummary {
            watermarks,
            before,
            after,
            journal_records_pruned,
            document_versions_pruned,
            index_versions_pruned,
        })
    }

    pub(crate) fn load_retention_checkpoint(
        &self,
    ) -> Result<(
        MaterializedRetentionCheckpoint,
        SequenceNumber,
        Option<Vec<u8>>,
    )> {
        let checkpoint_blob =
            self.metadata_blob(crate::retention::RETENTION_CHECKPOINT_METADATA_KEY)?;
        let checkpoint = checkpoint_blob
            .as_deref()
            .map(crate::retention::deserialize_retention_checkpoint)
            .transpose()?
            .unwrap_or(MaterializedRetentionCheckpoint::genesis()?);
        let physical_floor = self
            .metadata_blob(crate::retention::RETENTION_PHYSICAL_FLOOR_METADATA_KEY)?
            .as_deref()
            .map(crate::retention::decode_retention_floor)
            .transpose()?
            .unwrap_or(SequenceNumber(0));
        RetentionHistoryState::new(checkpoint.sequence(), physical_floor, checkpoint.clone())?;
        Ok((checkpoint, physical_floor, checkpoint_blob))
    }

    pub(crate) fn install_imported_retention_checkpoint(
        &self,
        checkpoint: &MaterializedRetentionCheckpoint,
    ) -> Result<()> {
        checkpoint.validate()?;
        let applied_head = self.journal_progress()?.applied_head;
        if checkpoint.sequence().0 > applied_head.0 {
            return Err(Error::InvalidInput(format!(
                "imported retention checkpoint {} exceeds restored applied head {}",
                checkpoint.sequence().0,
                applied_head.0
            )));
        }
        let checkpoint_blob = crate::retention::serialize_retention_checkpoint(checkpoint)?;
        let mut transaction = self.begin_write_transaction()?;
        transaction.put_metadata(
            crate::retention::RETENTION_CHECKPOINT_METADATA_KEY,
            checkpoint_blob.as_slice(),
        )?;
        transaction.put_metadata(
            crate::retention::RETENTION_PHYSICAL_FLOOR_METADATA_KEY,
            checkpoint.sequence().0.to_be_bytes().as_slice(),
        )?;
        let commit = transaction.commit()?;
        debug_assert!(commit.is_none());
        Ok(())
    }
}

/// SQLite-backed tenant store split into concept-owned provider modules.
///
/// `config.rs` owns connection opening and pooling, `read.rs` and `write.rs`
/// own snapshot and transaction behavior, `scheduler.rs` and `journal.rs`
/// own lifecycle-specific orchestration, and `schema.rs` or `backend.rs`
/// own low-level schema, index, and SQLite utility helpers.
///
/// When `dek` is `Some`, all connections use SQLCipher encryption with the
/// provided 32-byte data encryption key. The DEK is applied via `PRAGMA key`
/// before any other operations, and temporary storage is hardened to prevent
/// plaintext temp file spills.
pub struct SqliteTenantStore {
    path: PathBuf,
    dek: Option<DataEncryptionKey>,
    clock: Arc<dyn WallClock>,
    fault_injector: Arc<dyn FaultInjector>,
    id_source: Arc<dyn IdSource>,
    max_read_connections: usize,
    open_read_connections: Arc<AtomicUsize>,
    read_connections: Arc<Mutex<Vec<Connection>>>,
    /// Resident writer connection. Write paths take it for one transaction
    /// and return it only after a clean COMMIT or ROLLBACK; every error path
    /// drops the connection instead so the next writer reopens from scratch.
    /// The mutex guards only the take/put itself and is never held across a
    /// transaction, so non-committer writers (replica reconciliation) coexist
    /// exactly as before: a concurrent writer finds the slot empty and opens
    /// its own connection.
    writer_slot: Arc<Mutex<Option<Connection>>>,
    schema_cache: Arc<RwLock<Schema>>,
    materialized_verification: MaterializedVerificationInvalidator,
    pub(crate) retention_floor: Arc<RetentionFloor>,
}

impl SqliteTenantStore {
    pub fn materialized_verification_generation(&self) -> MaterializedVerificationGeneration {
        self.materialized_verification.generation()
    }

    pub fn materialized_verification_generation_is_current(
        &self,
        generation: MaterializedVerificationGeneration,
    ) -> bool {
        self.materialized_verification.is_current(generation)
    }
}

pub struct SqliteReadSnapshot {
    conn: PooledSqliteConnection,
    schema_cache: Arc<RwLock<Schema>>,
}

pub struct SqliteWriteTransaction {
    conn: Option<Connection>,
    writer_slot: Arc<Mutex<Option<Connection>>>,
    #[cfg(any(test, feature = "test-hooks"))]
    observation_path: PathBuf,
    clock: Arc<dyn WallClock>,
    fault_injector: Arc<dyn FaultInjector>,
    id_source: Arc<dyn IdSource>,
    commit_writes: Vec<WriteOp>,
    tenant_events: Vec<TenantEventKind>,
    prepared_record: Option<TenantEventRecord>,
    /// Durable journal records this transaction is about to make visible,
    /// retained past `prepared_record.take()` so the commit-sequence fault
    /// checks stay records-scoped.
    durable_records_for_fault: Vec<TenantEventRecord>,
    trigger_write_origin: Option<TriggerWriteOrigin>,
    commit_timestamp: Option<Timestamp>,
    check_cancel: Box<dyn Fn() -> Result<()> + Send>,
    schema_cache: Arc<RwLock<Schema>>,
    schema_cache_dirty: bool,
}

struct PooledSqliteConnection {
    conn: Option<Connection>,
    open_read_connections: Arc<AtomicUsize>,
    pool: Arc<Mutex<Vec<Connection>>>,
}
