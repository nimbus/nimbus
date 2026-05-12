mod journal;
mod journal_snapshot;
mod journal_stream;
mod read;
mod resource_paths;
mod scan;
mod schema_rewrite;
mod trigger_delivery;
mod trigger_invocations;
mod write;

use std::sync::Arc;

use nimbus_core::{
    CommitEntry, Document, Error, IndexDefinition, JobId, ResourcePathBinding, Result,
    ScheduledJob, SequenceNumber, WriteOp,
};
use redb::{Database, ReadTransaction, TableDefinition};

use self::scan::ScanMetrics;
#[cfg(test)]
pub(crate) use self::scan::ScanStats;
use crate::simulation::{Clock, FaultInjector};

pub use journal_snapshot::MaterializedJournalSnapshot;
pub use journal_stream::{
    DEFAULT_DURABLE_JOURNAL_STREAM_LIMIT, DurableJournalBootstrap, DurableJournalPage,
    MAX_DURABLE_JOURNAL_STREAM_LIMIT,
};

pub(crate) const DOCUMENTS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("documents");
pub(crate) const INDEXES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("indexes");
pub(crate) const RESOURCE_PATH_BINDINGS: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("resource_path_bindings");
pub(crate) const RESOURCE_PATH_LOOKUP: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("resource_path_lookup");
pub(crate) const COLLECTION_GROUP_BINDINGS: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("collection_group_bindings");
pub(crate) const SCHEMAS: TableDefinition<&str, &[u8]> = TableDefinition::new("schemas");
pub(crate) const SCHEDULED_JOBS: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("scheduled_jobs");
pub(crate) const RUNNING_SCHEDULED_JOBS: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("running_scheduled_jobs");
pub(crate) const SCHEDULED_JOB_RESULTS: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("scheduled_job_results");
pub(crate) const TRIGGER_INVOCATIONS: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("trigger_invocations");
pub(crate) const SCHEDULED_JOB_EXECUTIONS: TableDefinition<&str, &[u8]> =
    TableDefinition::new("scheduled_job_executions");
pub(crate) const CRON_JOBS: TableDefinition<&str, &[u8]> = TableDefinition::new("cron_jobs");
pub(crate) const COMMIT_LOG: TableDefinition<u64, &[u8]> = TableDefinition::new("commit_log");
pub(crate) const METADATA: TableDefinition<&str, &[u8]> = TableDefinition::new("metadata");
pub(crate) const NEXT_SEQUENCE_KEY: &str = "next_sequence";
pub(crate) const APPLIED_SEQUENCE_KEY: &str = "applied_sequence";
pub(crate) const TRIGGER_DELIVERY_CURSOR_KEY: &str = "trigger_delivery_cursor";
pub(crate) const EMPTY_TABLE_VALUE: &[u8] = &[];

/// Authoritative tenant persistence surface during the migration window.
///
/// The engine currently depends on this type for more than CRUD:
///
/// - direct point reads, scans, and planner-backed index reads
/// - validated direct writes
/// - execution-unit batch application over `ResolvedWrite` and
///   `ResolvedScheduleOp`
/// - durable journal append/read/stream/bootstrap APIs
/// - materialized journal snapshot export, restore, rebuild, and recovery
///
/// SQLite migration work should preserve those product semantics while swapping
/// the storage mechanics underneath them.
pub struct TenantStore {
    pub(crate) db: Database,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) fault_injector: Arc<dyn FaultInjector>,
    scan_metrics: Arc<ScanMetrics>,
}

/// Execution-unit document diff that has already been validated by the engine.
///
/// Storage must apply these resolved changes atomically with any sibling
/// scheduler operations and emit the resulting `CommitEntry` semantics expected
/// by the engine.
#[derive(Debug, Clone)]
pub enum ResolvedWrite {
    Insert {
        document: Document,
        indexes: Vec<IndexDefinition>,
        resource_path_binding: Option<ResourcePathBinding>,
    },
    Update {
        previous: Document,
        current: Document,
        indexes: Vec<IndexDefinition>,
        resource_path_binding: Option<ResourcePathBinding>,
    },
    Delete {
        previous: Document,
        indexes: Vec<IndexDefinition>,
    },
}

/// Scheduler-side changes that co-commit with execution-unit document writes.
#[derive(Debug, Clone)]
pub enum ResolvedScheduleOp {
    Insert { job: ScheduledJob },
    Cancel { job_id: JobId },
}

/// Stable read snapshot used by the planner, journal bootstrap, and recovery
/// flows.
pub struct TenantReadSnapshot {
    pub(crate) read_txn: ReadTransaction,
    scan_metrics: Arc<ScanMetrics>,
}

/// Result of a write task plus the optional durable mutation commit it produced.
///
/// `commit` stays optional because some transaction helpers mutate schema or
/// scheduler state without emitting a logical document commit.
pub struct TenantWriteCommit<T> {
    pub value: T,
    pub commit: Option<CommitEntry>,
}

/// Tracks the durable journal head separately from the engine-visible applied
/// head.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JournalProgress {
    pub durable_head: SequenceNumber,
    pub applied_head: SequenceNumber,
}

/// Mutable write transaction that preserves the existing direct-write and
/// journal-commit lifecycle.
///
/// Call sites use this transaction for validated document writes, schema
/// persistence, scheduler transitions, and any other write that must decide
/// between rollback and durable commit before returning to the engine.
pub struct TenantWriteTransaction {
    write_txn: Option<redb::WriteTransaction>,
    clock: Arc<dyn Clock>,
    fault_injector: Arc<dyn FaultInjector>,
    commit_writes: Vec<WriteOp>,
    check_cancel: Box<dyn Fn() -> Result<()> + Send>,
}

pub(crate) fn map_redb_error(error: impl std::fmt::Display) -> Error {
    Error::storage(nimbus_core::StorageErrorKind::Other, error.to_string())
}
