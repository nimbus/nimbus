use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::Duration;

use nimbus_core::{
    CommitEntry, CronJob, Document, DocumentId, DurableMutationRecord, Error, Filter, JobId,
    Result, ScheduledJob, ScheduledJobResult, Schema, SequenceNumber, StorageErrorKind, TableName,
    TableSchema, Timestamp, TriggerDeliveryCursor, TriggerWriteOrigin, WriteOp, WriteOpType,
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

mod backend;
mod config;
pub mod encryption;
mod journal;
mod read;
mod resource_paths;
mod scheduler;
mod schema;
mod trigger_delivery;
mod trigger_invocations;
mod write;

use self::backend::{
    decode_u64, deserialize_json, encode_u64, expect_write_commit, load_document_from_conn,
    map_sqlite_error, row_to_document, serialize_document_fields, serialize_document_typed_fields,
    serialize_json, sql_value_from_json, table_has_entries,
};
use self::journal::{
    append_commit_entry, next_sequence_in_conn, validate_durable_journal_stream_limit,
};
use self::scheduler::{
    apply_schedule_ops_in_transaction, begin_scheduled_execution_in_conn,
    load_scheduled_jobs_from_conn,
};
pub(crate) use self::schema::rebuild_sqlite_indexes_from_loaded_schema;
use self::schema::{
    create_sqlite_indexes_for_table_schema, drop_sqlite_indexes_for_table_schema,
    index_fields_for_cached_schema, load_schema_from_conn, load_table_schema_from_conn,
};
pub use self::schema::{
    sqlite_index_scan_composite_range_query_sql, sqlite_index_scan_prefix_query_sql,
};

pub(crate) const SQLITE_INIT_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS documents (
    table_name TEXT NOT NULL,
    id TEXT NOT NULL,
    data_json TEXT NOT NULL,
    typed_fields_json TEXT NOT NULL DEFAULT '{}',
    creation_time INTEGER NOT NULL,
    update_time INTEGER NOT NULL,
    PRIMARY KEY (table_name, id)
);

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

const MIN_SQLITE_READ_CONNECTIONS: usize = 2;

pub fn sqlite_init_sql() -> &'static str {
    SQLITE_INIT_SQL
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
    dek: Option<[u8; 32]>,
    clock: Arc<dyn Clock>,
    fault_injector: Arc<dyn FaultInjector>,
    max_read_connections: usize,
    open_read_connections: Arc<AtomicUsize>,
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
    trigger_write_origin: Option<TriggerWriteOrigin>,
    check_cancel: Box<dyn Fn() -> Result<()> + Send>,
    schema_cache: Arc<RwLock<Schema>>,
    schema_cache_dirty: bool,
}

struct PooledSqliteConnection {
    conn: Option<Connection>,
    open_read_connections: Arc<AtomicUsize>,
    pool: Arc<Mutex<Vec<Connection>>>,
}
