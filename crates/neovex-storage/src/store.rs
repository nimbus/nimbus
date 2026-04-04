mod journal;
mod journal_snapshot;
mod journal_stream;
mod read;
mod scan;
mod schema_rewrite;
mod write;

use std::sync::Arc;

use neovex_core::{
    CommitEntry, Document, Error, IndexDefinition, JobId, Result, ScheduledJob, SequenceNumber,
    WriteOp,
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
pub(crate) const NEXT_SEQUENCE_KEY: &str = "next_sequence";
pub(crate) const APPLIED_SEQUENCE_KEY: &str = "applied_sequence";
pub(crate) const EMPTY_TABLE_VALUE: &[u8] = &[];

/// Concrete redb-backed tenant store.
pub struct TenantStore {
    pub(crate) db: Database,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) fault_injector: Arc<dyn FaultInjector>,
    scan_metrics: Arc<ScanMetrics>,
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
    pub(crate) read_txn: ReadTransaction,
    scan_metrics: Arc<ScanMetrics>,
}

pub struct TenantWriteCommit<T> {
    pub value: T,
    pub commit: Option<CommitEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JournalProgress {
    pub durable_head: SequenceNumber,
    pub applied_head: SequenceNumber,
}

pub struct TenantWriteTransaction {
    write_txn: Option<redb::WriteTransaction>,
    clock: Arc<dyn Clock>,
    fault_injector: Arc<dyn FaultInjector>,
    commit_writes: Vec<WriteOp>,
    check_cancel: Box<dyn Fn() -> Result<()> + Send>,
}

pub(crate) fn map_redb_error(error: impl std::fmt::Display) -> Error {
    Error::Storage(error.to_string())
}
