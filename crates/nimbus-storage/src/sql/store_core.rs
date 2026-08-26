//! Dialect-shared SQL *store* orchestration.
//!
//! [`crate::sql::write_core`] owns the shared logic that runs *inside* a write
//! transaction. This module owns the layer immediately above it: the store-level
//! wrappers that open a write transaction, run one closure against it, and shape
//! the resulting [`TenantWriteCommit`] into each public entry point. Those
//! wrappers are expressed entirely in terms of
//! [`SqlStoreCore::execute_write`]/[`SqlStoreCore::execute_write_cancellable`]
//! plus the transaction-concept methods on [`SqlWriteTransactionCore`], so they
//! live here once instead of once per SQL backend.
//!
//! Dialect-load-bearing concerns stay in each backend's own module: SQL text and
//! parameter binding, connection and transaction types, how a transaction is
//! begun (PostgreSQL takes an advisory lock, MySQL retries a contended begin),
//! the tokio-runtime bridge each store uses to reach its async driver, and the
//! per-dialect in-flight operation ceiling in [`crate::sql::write_pipeline`].
//!
//! Storage atomicity is unchanged: nothing here performs transaction control.
//! `BEGIN`/`COMMIT`/`ROLLBACK` remain where they were, inside each backend's
//! `execute_write` bridge and [`crate::sql::write_core::sql_commit`], which also
//! keeps sole ownership of the commit-path fault points.
//!
//! Several trait methods share a name with an inherent method on the
//! implementing type. Inherent methods win method-call resolution, so the
//! forwarding impls are not recursive and existing call sites keep binding to
//! the inherent method; from the default bodies here the trait method is the
//! only one visible. This mirrors the convention already used by
//! [`crate::sql::write_core::SqlWriteBackend`].

// `Future`, `Arc`, the atomics and the tokio/executor imports below are used
// only by the durable-journal wrappers and the blocking-executor section at the
// end of this file, both of which are PostgreSQL/MySQL-only.
#[cfg(any(feature = "mysql", feature = "postgres"))]
use std::future::Future;
#[cfg(any(feature = "mysql", feature = "postgres"))]
use std::sync::Arc;
#[cfg(any(feature = "mysql", feature = "postgres"))]
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

use nimbus_core::{
    CommitEntry, CronJob, Document, DocumentId, Error, IndexDefinition, Result, ScheduledJob,
    ScheduledJobResult, SequenceNumber, TableName, TableSchema, TenantEventRecord, Timestamp,
    TriggerDeliveryCursor, TriggerInvocationRecord, TriggerWriteOrigin,
};
use serde_json::Value;
#[cfg(any(feature = "mysql", feature = "postgres"))]
use tokio::runtime::Handle as TokioRuntimeHandle;
#[cfg(any(feature = "mysql", feature = "postgres"))]
use tokio::sync::Semaphore;

use crate::FaultPoint;
#[cfg(any(feature = "mysql", feature = "postgres"))]
use crate::async_storage::{
    TenantWriteOutcome, map_executor_join_error, map_executor_permit_error,
};
use crate::retention::{
    MaterializedRetentionCheckpoint, PreparedRetentionHistory, RetentionFloor, RetentionGcConfig,
    RetentionGcSummary, RetentionGcWatermarks, RetentionHistoryState, RetentionHistorySummary,
    deserialize_retention_checkpoint, desired_journal_floor, serialize_retention_checkpoint,
};
use crate::sql::commit_effects::{
    CommitTimestampEffect, DocumentWrites, ExecutionDedup, JournalEffect, LeaseEffect, ScheduleOps,
    SqlCommitAdmission, SqlCommitEffects, TriggerOriginEffect, WatermarkEffect, sql_apply_commit,
};
#[cfg(any(feature = "mysql", feature = "postgres"))]
use crate::sql::write_core::SqlDurableJournalTransaction;
use crate::sql::write_core::SqlWriteBackend;
#[cfg(any(feature = "mysql", feature = "postgres"))]
use crate::sql::write_pipeline::SqlWritePipelineMetrics;
use crate::store::{
    JournalProgress, MaterializedJournalSnapshot, PointInTimeRestoreArchive,
    PointInTimeRestoreTarget, ResolvedScheduleOp, ResolvedWrite, TenantWriteCommit,
    describe_materialized_position,
};
use crate::traits::{CommitterLeaseError, CommitterLeaseResult};

/// Map a caller's optional scheduled-execution id onto the witness's dedup
/// effect. Both variants consult the gate, matching the behavior of passing the
/// `Option` straight through; only [`ExecutionDedup::NotDeduplicated`] skips it.
fn execution_dedup(scheduled_execution_id: Option<String>) -> ExecutionDedup {
    match scheduled_execution_id {
        Some(execution_id) => ExecutionDedup::ScheduledExecution(execution_id),
        None => ExecutionDedup::NoExecutionId,
    }
}

/// Describe a commit's scheduler effect, so the witness says what this commit
/// actually does rather than leaving an empty batch to stand for "none".
fn schedule_ops_effect(schedule_ops: Vec<ResolvedScheduleOp>) -> ScheduleOps {
    if schedule_ops.is_empty() {
        ScheduleOps::NoScheduleOps
    } else {
        ScheduleOps::Apply(schedule_ops)
    }
}

/// Sentinel carried through `Error::PreconditionFailed` so a fencing failure
/// discovered inside a write closure can be re-classified as
/// [`CommitterLeaseError::Fenced`] once the transaction has unwound.
pub(crate) const FENCED_COMMITTER_LEASE_MARKER: &str =
    "fenced committer lease during durable apply";

/// Re-classify the sentinel above; any other error passes through unchanged.
pub(crate) fn map_fenced_write_result<T>(
    result: Result<T>,
    owner_id: String,
    epoch: u64,
) -> CommitterLeaseResult<T> {
    match result {
        Ok(value) => Ok(value),
        Err(Error::PreconditionFailed(message)) if message == FENCED_COMMITTER_LEASE_MARKER => {
            Err(CommitterLeaseError::Fenced { owner_id, epoch })
        }
        Err(error) => Err(error.into()),
    }
}

fn expect_write_commit(commit: Option<CommitEntry>, expectation: &str) -> Result<CommitEntry> {
    commit.ok_or_else(|| Error::Internal(expectation.to_string()))
}

/// Apply a resolved schedule batch inside an open write transaction.
pub(crate) fn apply_schedule_ops_in_transaction<T: SqlWriteTransactionCore>(
    transaction: &mut T,
    schedule_ops: &[ResolvedScheduleOp],
) -> Result<()> {
    for schedule_op in schedule_ops {
        match schedule_op {
            ResolvedScheduleOp::Insert { job } => transaction.insert_scheduled_job(job)?,
            ResolvedScheduleOp::Cancel { job_id } => {
                if !transaction.cancel_scheduled_job(job_id)? {
                    return Err(Error::ScheduledJobNotFound(job_id.clone()));
                }
            }
        }
    }
    Ok(())
}

/// Transaction-concept seam used by the store wrappers in [`SqlStoreCore`].
///
/// This extends the in-transaction seam ([`SqlWriteBackend`]) with the
/// statements the store-level wrappers drive: scheduler and cron mutations,
/// schema replacement, committer-lease fencing, and the validated document
/// mutations. Journal replay through a write transaction is a narrower
/// concern and lives in `SqlDurableJournalTransaction` (written as a code span
/// rather than an intra-doc link because that trait is compiled only for the
/// PostgreSQL and MySQL builds).
pub(crate) trait SqlWriteTransactionCore: SqlWriteBackend {
    // Deduplication and per-transaction context.
    fn begin_scheduled_execution(&mut self, execution_id: Option<&str>) -> Result<bool>;
    fn set_prepared_record(&mut self, record: TenantEventRecord);
    fn set_trigger_write_origin(&mut self, trigger_write_origin: Option<TriggerWriteOrigin>);
    fn set_commit_timestamp(&mut self, commit_timestamp: Option<Timestamp>);

    // Committer-lease fencing. Returns the number of matched lease rows; the
    // callers treat anything other than 1 as fenced.
    fn advance_fenced_committer_lease(
        &mut self,
        owner_id: &str,
        epoch: u64,
        expected_previous: SequenceNumber,
        durable_sequence: SequenceNumber,
    ) -> Result<u64>;
    /// Confirms the lease without advancing the durable sequence. PostgreSQL
    /// reuses the advancing CAS with an unchanged sequence; MySQL cannot (a
    /// no-op `UPDATE` reports zero changed rows) and locks the lease row
    /// instead, so this stays a dialect method rather than a default.
    fn validate_fenced_committer_lease(
        &mut self,
        owner_id: &str,
        epoch: u64,
        durable_sequence: SequenceNumber,
    ) -> Result<u64>;

    // Trigger invocations. Row encoding and upsert syntax are dialect-owned
    // (`ON CONFLICT ... DO UPDATE` vs `ON DUPLICATE KEY UPDATE`); only the
    // store-level fencing wrappers below are shared.
    fn materialize_trigger_invocations(
        &mut self,
        records: &[TriggerInvocationRecord],
        cursor: TriggerDeliveryCursor,
    ) -> Result<()>;
    fn save_trigger_invocation(&mut self, record: &TriggerInvocationRecord) -> Result<()>;

    // Schema.
    fn replace_table_schema(&mut self, table_schema: &TableSchema) -> Result<()>;
    fn delete_table_schema(&mut self, table: &TableName) -> Result<()>;

    // Scheduler and cron.
    fn insert_scheduled_job(&mut self, job: &ScheduledJob) -> Result<()>;
    fn claim_due_jobs(&mut self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>>;
    fn complete_scheduled_job(&mut self, job_id: &DocumentId) -> Result<()>;
    fn cancel_scheduled_job(&mut self, job_id: &DocumentId) -> Result<bool>;
    fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()>;
    fn save_cron_job(&mut self, cron: &CronJob) -> Result<()>;
    fn delete_cron_job(&mut self, name: &str) -> Result<()>;
    fn recover_running_jobs(&mut self, now: Timestamp) -> Result<()>;

    // Documents.
    fn apply_resolved_write(&mut self, write: &ResolvedWrite) -> Result<()>;
    fn update_document_validated<F>(
        &mut self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, Value>,
        validate: F,
    ) -> Result<()>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static;
    fn delete_document_validated<F>(
        &mut self,
        table: &TableName,
        id: &DocumentId,
        validate: F,
    ) -> Result<Document>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static;

    // Retention compaction.
    fn prune_retained_versions(
        &mut self,
        document_prune_before: SequenceNumber,
        index_prune_before: SequenceNumber,
    ) -> Result<(u64, u64)>;
    /// Load the provider-owned materialized checkpoint and physical journal
    /// floor from the open transaction's snapshot.
    fn load_retention_metadata(&mut self) -> Result<(Option<Vec<u8>>, SequenceNumber)>;
    /// Load the applied head inside the transaction that will publish a
    /// checkpoint. This must not be inferred from the physical commit log.
    fn applied_sequence_for_retention(&mut self) -> Result<SequenceNumber>;
    /// Remove the durable journal prefix through `sequence`.
    fn prune_durable_journal_through(&mut self, sequence: SequenceNumber) -> Result<u64>;
    /// Publish the checkpoint and physical floor inside the current
    /// transaction. Implementations must also invalidate any local replica
    /// cache that mirrors the provider state.
    fn store_retention_metadata(
        &mut self,
        checkpoint_blob: &[u8],
        physical_floor: SequenceNumber,
    ) -> Result<()>;
}

/// Store-level seam. Each SQL backend supplies its write bridge and a handful of
/// journal reads; every wrapper below is a default method shared by all of them.
pub(crate) trait SqlStoreCore: Sized {
    type Transaction: SqlWriteTransactionCore;

    /// Run `task` in a fresh write transaction and commit it. The bridge to the
    /// backend's async driver stays in the implementing store.
    fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut Self::Transaction) -> Result<T> + Send + 'static;

    /// Cancellable counterpart of [`SqlStoreCore::execute_write`]. Its only
    /// callers are the fenced durable-journal wrapper and the blocking write
    /// executor, both PostgreSQL/MySQL-only, so the method is gated with them.
    /// The libsql replica keeps its own inherent
    /// `execute_write_cancellable`, which its async storage layer calls
    /// directly; only this trait-level forwarder disappears.
    #[cfg(any(feature = "mysql", feature = "postgres"))]
    fn execute_write_cancellable<T, Check, F>(
        &self,
        check_cancel: Check,
        task: F,
    ) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(&mut Self::Transaction) -> Result<T> + Send + 'static;

    fn retention_floor(&self) -> &RetentionFloor;
    fn journal_progress(&self) -> Result<JournalProgress>;
    fn load_retention_metadata_snapshot(&self) -> Result<(Option<Vec<u8>>, SequenceNumber)>;
    fn read_durable_journal_from(&self, sequence: SequenceNumber)
    -> Result<Vec<TenantEventRecord>>;
    fn recover_durable_journal(&self) -> Result<JournalProgress>;
    fn export_materialized_journal_snapshot(&self) -> Result<MaterializedJournalSnapshot>;

    // ---------------------------------------------------------------- prepared

    fn apply_prepared_write_batch(
        &self,
        record: &TenantEventRecord,
        schedule_ops: &[ResolvedScheduleOp],
        scheduled_execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        if record.writes.is_empty() {
            return Err(Error::Internal(
                "prepared write batch must contain at least one document write".to_string(),
            ));
        }
        let record = record.clone();
        let schedule_ops = schedule_ops.to_vec();
        let scheduled_execution_id = scheduled_execution_id.map(str::to_string);
        let committed = self.execute_write(move |transaction| {
            let effects = SqlCommitEffects {
                dedup: execution_dedup(scheduled_execution_id),
                lease: LeaseEffect::NotFenced,
                trigger_origin: TriggerOriginEffect::TransactionDefault,
                commit_timestamp: CommitTimestampEffect::ProviderAssigned,
                documents: DocumentWrites::PreparedDurableRecord(record),
                schedule_ops: schedule_ops_effect(schedule_ops),
                journal: JournalEffect::PreparedRecord,
                watermark: WatermarkEffect::AdvancedByRecordApply,
            };
            Ok(sql_apply_commit(transaction, effects)? == SqlCommitAdmission::Committed)
        })?;
        Ok(committed.value.then_some(committed.commit).flatten())
    }

    fn fenced_apply_prepared_write_batch(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_previous: SequenceNumber,
        record: &TenantEventRecord,
        schedule_ops: &[ResolvedScheduleOp],
        scheduled_execution_id: Option<&str>,
    ) -> CommitterLeaseResult<Option<CommitEntry>> {
        if record.writes.is_empty() {
            return Err(Error::Internal(
                "prepared write batch must contain at least one document write".to_string(),
            )
            .into());
        }
        let owner_id = owner_id.to_string();
        let fenced_owner_id = owner_id.clone();
        let record = record.clone();
        let schedule_ops = schedule_ops.to_vec();
        let scheduled_execution_id = scheduled_execution_id.map(str::to_string);
        let result = self.execute_write(move |transaction| {
            let durable_sequence = record.sequence;
            let effects = SqlCommitEffects {
                dedup: execution_dedup(scheduled_execution_id),
                lease: LeaseEffect::Fenced {
                    owner_id,
                    epoch,
                    expected_previous,
                    durable_sequence,
                },
                trigger_origin: TriggerOriginEffect::TransactionDefault,
                commit_timestamp: CommitTimestampEffect::ProviderAssigned,
                documents: DocumentWrites::PreparedDurableRecord(record),
                schedule_ops: schedule_ops_effect(schedule_ops),
                journal: JournalEffect::PreparedRecord,
                watermark: WatermarkEffect::AdvancedByRecordApply,
            };
            Ok(sql_apply_commit(transaction, effects)? == SqlCommitAdmission::Committed)
        });
        match result {
            Ok(committed) => Ok(committed.value.then_some(committed.commit).flatten()),
            Err(Error::PreconditionFailed(message)) if message == FENCED_COMMITTER_LEASE_MARKER => {
                Err(CommitterLeaseError::Fenced {
                    owner_id: fenced_owner_id,
                    epoch,
                })
            }
            Err(error) => Err(error.into()),
        }
    }

    // --------------------------------------------------------------- retention

    fn retention_gc_watermarks(&self, config: RetentionGcConfig) -> Result<RetentionGcWatermarks> {
        Ok(self
            .retention_floor()
            .gc_watermarks(self.journal_progress()?.applied_head, config))
    }

    fn compact_retained_versions(&self, config: RetentionGcConfig) -> Result<RetentionGcSummary> {
        let watermarks = self.retention_gc_watermarks(config)?;
        let _pin_barrier = self
            .retention_floor()
            .guard_prepared_watermarks(&watermarks)?;
        let document_prune_before = watermarks.document_versions.safe_prune_before;
        let index_prune_before = watermarks.index_versions.safe_prune_before;
        let committed = self.execute_write(move |transaction| {
            transaction.prune_retained_versions(document_prune_before, index_prune_before)
        })?;
        debug_assert!(committed.commit.is_none());
        Ok(RetentionGcSummary {
            watermarks,
            document_versions_pruned: committed.value.0,
            index_versions_pruned: committed.value.1,
        })
    }

    fn load_retention_checkpoint(
        &self,
    ) -> Result<(
        MaterializedRetentionCheckpoint,
        SequenceNumber,
        Option<Vec<u8>>,
    )> {
        let (checkpoint_blob, physical_floor) = self.load_retention_metadata_snapshot()?;
        let checkpoint = checkpoint_blob
            .as_deref()
            .map(deserialize_retention_checkpoint)
            .transpose()?
            .unwrap_or(MaterializedRetentionCheckpoint::genesis()?);
        RetentionHistoryState::new(
            checkpoint.sequence(),
            checkpoint.sequence(),
            physical_floor,
            checkpoint.clone(),
        )?;
        Ok((checkpoint, physical_floor, checkpoint_blob))
    }

    fn retention_history_state(&self, config: RetentionGcConfig) -> Result<RetentionHistoryState> {
        let watermarks = self.retention_gc_watermarks(config)?;
        let (checkpoint, physical_floor, _) = self.load_retention_checkpoint()?;
        RetentionHistoryState::new(
            watermarks.document_versions.latest_sequence,
            desired_journal_floor(&watermarks).max(checkpoint.sequence()),
            physical_floor,
            checkpoint,
        )
    }

    fn prepare_retained_history(
        &self,
        config: RetentionGcConfig,
    ) -> Result<PreparedRetentionHistory> {
        let watermarks = self.retention_gc_watermarks(config)?;
        let (checkpoint, physical_floor, expected_checkpoint_blob) =
            self.load_retention_checkpoint()?;
        let desired_floor = desired_journal_floor(&watermarks).max(checkpoint.sequence());
        let before = RetentionHistoryState::new(
            watermarks.document_versions.latest_sequence,
            desired_floor,
            physical_floor,
            checkpoint.clone(),
        )?;
        let journal_tail = self
            .read_durable_journal_from(SequenceNumber(checkpoint.sequence().0.saturating_add(1)))?;
        let candidate = checkpoint.advance(&journal_tail, desired_floor)?;
        Ok(PreparedRetentionHistory {
            watermarks,
            before,
            candidate,
            expected_checkpoint_blob,
            expected_revision: None,
        })
    }

    /// Atomically publish a prepared materialized checkpoint and prune the
    /// provider's retained history while the caller still owns the committer
    /// lease.
    fn fenced_finalize_retained_history(
        &self,
        owner_id: &str,
        epoch: u64,
        durable_sequence: SequenceNumber,
        prepared: PreparedRetentionHistory,
    ) -> CommitterLeaseResult<RetentionHistorySummary> {
        let _pin_barrier = self
            .retention_floor()
            .guard_prepared_watermarks(&prepared.watermarks)?;
        let PreparedRetentionHistory {
            watermarks,
            before,
            candidate,
            expected_checkpoint_blob,
            ..
        } = prepared;
        let candidate_blob = serialize_retention_checkpoint(&candidate)?;
        let candidate_sequence = candidate.sequence();
        let document_prune_before = watermarks.document_versions.safe_prune_before;
        let index_prune_before = watermarks.index_versions.safe_prune_before;
        let fenced_owner_id = owner_id.to_string();
        let owner_id = owner_id.to_string();
        let result = self.execute_write(move |transaction| {
            if transaction.validate_fenced_committer_lease(
                owner_id.as_str(),
                epoch,
                durable_sequence,
            )? != 1
            {
                return Err(Error::PreconditionFailed(
                    FENCED_COMMITTER_LEASE_MARKER.to_string(),
                ));
            }
            let (current_checkpoint_blob, current_physical_floor) =
                transaction.load_retention_metadata()?;
            if current_checkpoint_blob != expected_checkpoint_blob
                || current_physical_floor != before.physical_floor
            {
                return Err(Error::conflict(
                    "retention checkpoint changed while compaction was prepared".to_string(),
                ));
            }
            let applied_head = transaction.applied_sequence_for_retention()?;
            if candidate_sequence.0 > applied_head.0 {
                return Err(Error::conflict(format!(
                    "retention checkpoint target {} exceeds current applied head {}",
                    candidate_sequence.0, applied_head.0
                )));
            }
            let (document_versions_pruned, index_versions_pruned) =
                transaction.prune_retained_versions(document_prune_before, index_prune_before)?;
            let journal_records_pruned =
                transaction.prune_durable_journal_through(candidate_sequence)?;
            transaction.store_retention_metadata(&candidate_blob, candidate_sequence)?;
            transaction.check_fault(FaultPoint::RetentionCheckpointBeforeCommit)?;
            Ok((
                journal_records_pruned,
                document_versions_pruned,
                index_versions_pruned,
            ))
        });
        let committed = map_fenced_write_result(result, fenced_owner_id, epoch)?;
        debug_assert!(committed.commit.is_none());
        let after = RetentionHistoryState::new(
            before.latest_sequence,
            before.desired_floor,
            candidate_sequence,
            candidate,
        )?;
        Ok(RetentionHistorySummary {
            watermarks,
            before,
            after,
            journal_records_pruned: committed.value.0,
            document_versions_pruned: committed.value.1,
            index_versions_pruned: committed.value.2,
        })
    }

    fn fenced_compact_retained_history(
        &self,
        owner_id: &str,
        epoch: u64,
        durable_sequence: SequenceNumber,
        config: RetentionGcConfig,
    ) -> CommitterLeaseResult<RetentionHistorySummary> {
        let prepared = self.prepare_retained_history(config)?;
        self.fenced_finalize_retained_history(owner_id, epoch, durable_sequence, prepared)
    }

    // -------------------------------------------------------------------- PITR

    fn export_point_in_time_restore_archive(
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

    fn import_point_in_time_restore_archive(
        &self,
        archive: &PointInTimeRestoreArchive,
    ) -> Result<JournalProgress> {
        crate::store::validate_point_in_time_archive_for_journal_replay_import(archive)?;
        let current = self.export_materialized_journal_snapshot()?;
        crate::store::validate_materialized_journal_replay_base_is_empty(&current)?;
        self.append_durable_records_batch(&archive.journal_tail)?;
        let progress = self.recover_durable_journal()?;
        let restored_position = self
            .export_materialized_journal_snapshot()?
            .materialized_position()?;
        if restored_position != archive.target_position {
            return Err(Error::storage(
                nimbus_core::StorageErrorKind::Corruption,
                format!(
                    "point-in-time restore position mismatch: restored {} expected {}",
                    describe_materialized_position(&restored_position),
                    describe_materialized_position(&archive.target_position)
                ),
            ));
        }
        Ok(progress)
    }

    fn fenced_import_point_in_time_restore_archive(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_previous: SequenceNumber,
        archive: &PointInTimeRestoreArchive,
    ) -> CommitterLeaseResult<JournalProgress> {
        crate::store::validate_point_in_time_archive_for_journal_replay_import(archive)?;
        let current = self.export_materialized_journal_snapshot()?;
        crate::store::validate_materialized_journal_replay_base_is_empty(&current)?;
        if archive.journal_tail.is_empty() {
            return self
                .import_point_in_time_restore_archive(archive)
                .map_err(Into::into);
        }
        self.fenced_append_and_apply_durable_records_batch(
            owner_id,
            epoch,
            expected_previous,
            &archive.journal_tail,
        )?;
        let progress = self.journal_progress()?;
        let restored_position = self
            .export_materialized_journal_snapshot()?
            .materialized_position()?;
        if restored_position != archive.target_position {
            return Err(Error::storage(
                nimbus_core::StorageErrorKind::Corruption,
                format!(
                    "point-in-time restore position mismatch: restored {} expected {}",
                    describe_materialized_position(&restored_position),
                    describe_materialized_position(&archive.target_position)
                ),
            )
            .into());
        }
        Ok(progress)
    }

    // ------------------------------------------------------------------ schema

    fn replace_table_schema(&self, table_schema: &TableSchema) -> Result<()> {
        let table_schema = table_schema.clone();
        self.execute_write(move |transaction| transaction.replace_table_schema(&table_schema))?;
        Ok(())
    }

    fn fenced_replace_table_schema(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_previous: SequenceNumber,
        table_schema: &TableSchema,
    ) -> CommitterLeaseResult<()> {
        let owner_id = owner_id.to_string();
        let fenced_owner_id = owner_id.clone();
        let table_schema = table_schema.clone();
        let durable_sequence = SequenceNumber(expected_previous.0.saturating_add(1));
        let result = self.execute_write(move |transaction| {
            if transaction.advance_fenced_committer_lease(
                &owner_id,
                epoch,
                expected_previous,
                durable_sequence,
            )? != 1
            {
                return Err(Error::PreconditionFailed(
                    FENCED_COMMITTER_LEASE_MARKER.to_string(),
                ));
            }
            transaction.replace_table_schema(&table_schema)
        });
        map_fenced_write_result(result.map(|_| ()), fenced_owner_id, epoch)
    }

    fn delete_table_schema(&self, table: &TableName) -> Result<()> {
        let table = table.clone();
        self.execute_write(move |transaction| transaction.delete_table_schema(&table))?;
        Ok(())
    }

    fn fenced_delete_table_schema(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_previous: SequenceNumber,
        table: &TableName,
    ) -> CommitterLeaseResult<()> {
        let owner_id = owner_id.to_string();
        let fenced_owner_id = owner_id.clone();
        let table = table.clone();
        let durable_sequence = SequenceNumber(expected_previous.0.saturating_add(1));
        let result = self.execute_write(move |transaction| {
            if transaction.advance_fenced_committer_lease(
                &owner_id,
                epoch,
                expected_previous,
                durable_sequence,
            )? != 1
            {
                return Err(Error::PreconditionFailed(
                    FENCED_COMMITTER_LEASE_MARKER.to_string(),
                ));
            }
            transaction.delete_table_schema(&table)
        });
        map_fenced_write_result(result.map(|_| ()), fenced_owner_id, epoch)
    }

    // --------------------------------------------------------- durable journal

    /// Durable-journal append. Backends that replay through a write transaction
    /// forward to [`sql_store_append_durable_records_batch`]; the libsql replica
    /// issues its own remote batch instead.
    fn append_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()>;

    /// Durable-journal apply. See [`SqlStoreCore::append_durable_records_batch`]
    /// for why this is not a shared default.
    fn apply_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()>;

    /// Applies records read back out of the durable journal during recovery.
    ///
    /// Materially the same work as [`SqlStoreCore::apply_durable_records_batch`],
    /// but it makes nothing durable: whatever appended these records already
    /// did that, and no caller is waiting on an acknowledgement for them. It
    /// therefore names no durable records to the fault interface, so a fault
    /// armed for a client batch cannot be consumed by a replay of an older one.
    /// Recovery is the only correct caller.
    fn replay_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()>;

    fn fenced_append_and_apply_durable_records_batch(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_previous: SequenceNumber,
        records: &[TenantEventRecord],
    ) -> CommitterLeaseResult<()> {
        self.fenced_append_and_apply_durable_records_batch_cancellable(
            owner_id,
            epoch,
            expected_previous,
            records,
            || Ok(()),
        )
    }

    /// Fenced durable append-and-apply. See
    /// [`SqlStoreCore::append_durable_records_batch`] for why this is not a
    /// shared default.
    fn fenced_append_and_apply_durable_records_batch_cancellable<Check>(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_previous: SequenceNumber,
        records: &[TenantEventRecord],
        check_cancel: Check,
    ) -> CommitterLeaseResult<()>
    where
        Check: Fn() -> Result<()> + Send + 'static;

    // --------------------------------------------------------------- scheduler

    fn insert_scheduled_job(&self, job: &ScheduledJob) -> Result<()> {
        let job = job.clone();
        self.execute_write(move |transaction| transaction.insert_scheduled_job(&job))?;
        Ok(())
    }

    fn claim_due_jobs(&self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>> {
        Ok(self
            .execute_write(move |transaction| transaction.claim_due_jobs(now, max_jobs))?
            .value)
    }

    fn complete_scheduled_job(&self, job_id: &DocumentId) -> Result<()> {
        let job_id = job_id.clone();
        self.execute_write(move |transaction| transaction.complete_scheduled_job(&job_id))?;
        Ok(())
    }

    fn cancel_scheduled_job(&self, job_id: &DocumentId) -> Result<bool> {
        let job_id = job_id.clone();
        Ok(self
            .execute_write(move |transaction| transaction.cancel_scheduled_job(&job_id))?
            .value)
    }

    fn record_scheduled_job_result(&self, result: &ScheduledJobResult) -> Result<()> {
        let result = result.clone();
        self.execute_write(move |transaction| transaction.record_scheduled_job_result(&result))?;
        Ok(())
    }

    fn save_cron_job(&self, cron: &CronJob) -> Result<()> {
        let cron = cron.clone();
        self.execute_write(move |transaction| transaction.save_cron_job(&cron))?;
        Ok(())
    }

    fn delete_cron_job(&self, name: &str) -> Result<()> {
        let name = name.to_string();
        self.execute_write(move |transaction| transaction.delete_cron_job(name.as_str()))?;
        Ok(())
    }

    fn recover_running_jobs(&self, now: Timestamp) -> Result<()> {
        self.execute_write(move |transaction| transaction.recover_running_jobs(now))?;
        Ok(())
    }

    // ------------------------------------------------------ trigger invocations

    fn materialize_trigger_invocations(
        &self,
        records: &[TriggerInvocationRecord],
        cursor: TriggerDeliveryCursor,
    ) -> Result<()> {
        let records = records.to_vec();
        self.execute_write(move |transaction| {
            transaction.materialize_trigger_invocations(records.as_slice(), cursor)
        })?;
        Ok(())
    }

    fn fenced_materialize_trigger_invocations(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_previous: SequenceNumber,
        records: &[TriggerInvocationRecord],
        cursor: TriggerDeliveryCursor,
    ) -> CommitterLeaseResult<()> {
        let owner_id = owner_id.to_string();
        let fenced_owner_id = owner_id.clone();
        let records = records.to_vec();
        let durable_sequence = SequenceNumber(expected_previous.0.saturating_add(1));
        let result = self.execute_write(move |transaction| {
            if transaction.advance_fenced_committer_lease(
                &owner_id,
                epoch,
                expected_previous,
                durable_sequence,
            )? != 1
            {
                return Err(Error::PreconditionFailed(
                    FENCED_COMMITTER_LEASE_MARKER.to_string(),
                ));
            }
            transaction.materialize_trigger_invocations(records.as_slice(), cursor)
        });
        map_fenced_write_result(result.map(|_| ()), fenced_owner_id, epoch)
    }

    fn save_trigger_invocation(&self, record: &TriggerInvocationRecord) -> Result<()> {
        let record = record.clone();
        self.execute_write(move |transaction| transaction.save_trigger_invocation(&record))?;
        Ok(())
    }

    fn fenced_save_trigger_invocation(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_durable_sequence: SequenceNumber,
        record: &TriggerInvocationRecord,
    ) -> CommitterLeaseResult<()> {
        let owner_id = owner_id.to_string();
        let fenced_owner_id = owner_id.clone();
        let record = record.clone();
        let result = self.execute_write(move |transaction| {
            if transaction.validate_fenced_committer_lease(
                &owner_id,
                epoch,
                expected_durable_sequence,
            )? != 1
            {
                return Err(Error::PreconditionFailed(
                    FENCED_COMMITTER_LEASE_MARKER.to_string(),
                ));
            }
            transaction.save_trigger_invocation(&record)
        });
        map_fenced_write_result(result.map(|_| ()), fenced_owner_id, epoch)
    }

    // ---------------------------------------------------------- execution unit

    fn apply_execution_unit_batch(
        &self,
        writes: &[ResolvedWrite],
        schedule_ops: &[ResolvedScheduleOp],
    ) -> Result<Option<CommitEntry>> {
        self.apply_execution_unit_batch_with_origin(writes, schedule_ops, None, None)
    }

    fn apply_execution_unit_batch_with_origin(
        &self,
        writes: &[ResolvedWrite],
        schedule_ops: &[ResolvedScheduleOp],
        trigger_write_origin: Option<&TriggerWriteOrigin>,
        commit_timestamp: Option<Timestamp>,
    ) -> Result<Option<CommitEntry>> {
        if writes.is_empty() && schedule_ops.is_empty() {
            return Err(Error::Internal(
                "execution-unit batch must contain at least one change".to_string(),
            ));
        }

        let writes = writes.to_vec();
        let schedule_ops = schedule_ops.to_vec();
        let trigger_write_origin = trigger_write_origin.cloned();
        let committed = self.execute_write(move |transaction| {
            let effects = SqlCommitEffects {
                dedup: ExecutionDedup::NotDeduplicated,
                lease: LeaseEffect::NotFenced,
                trigger_origin: match trigger_write_origin {
                    Some(origin) => TriggerOriginEffect::Explicit(origin),
                    None => TriggerOriginEffect::TransactionDefault,
                },
                commit_timestamp: match commit_timestamp {
                    Some(timestamp) => CommitTimestampEffect::Explicit(timestamp),
                    None => CommitTimestampEffect::ProviderAssigned,
                },
                documents: DocumentWrites::ResolvedExecutionUnit(writes),
                schedule_ops: schedule_ops_effect(schedule_ops),
                journal: JournalEffect::CommitEntryFromBufferedWrites,
                watermark: WatermarkEffect::NotAdvanced,
            };
            sql_apply_commit(transaction, effects)?;
            Ok(())
        })?;
        Ok(committed.commit)
    }

    // ------------------------------------------------------------------ insert

    fn insert(&self, document: &Document) -> Result<CommitEntry> {
        self.insert_once(document, None)?
            .ok_or_else(|| Error::Internal("non-deduplicated insert should commit".to_string()))
    }

    fn insert_with_indexes(
        &self,
        document: &Document,
        _indexes: &[IndexDefinition],
    ) -> Result<CommitEntry> {
        self.insert(document)
    }

    fn insert_once(
        &self,
        document: &Document,
        execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        let document = document.clone();
        let execution_id = execution_id.map(str::to_string);
        let committed = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(execution_id.as_deref())? {
                return Ok(false);
            }
            transaction.insert_document(&document)?;
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

    fn insert_with_indexes_once(
        &self,
        document: &Document,
        _indexes: &[IndexDefinition],
        execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        self.insert_once(document, execution_id)
    }

    fn insert_with_indexes_once_at(
        &self,
        document: &Document,
        assignment: crate::DirectWriteAssignment<'_>,
    ) -> Result<Option<CommitEntry>> {
        let document = document.clone();
        let execution_id = assignment.execution_id.map(str::to_string);
        let committed = self.execute_write(move |transaction| {
            transaction.set_commit_timestamp(Some(assignment.commit_timestamp));
            if !transaction.begin_scheduled_execution(execution_id.as_deref())? {
                return Ok(false);
            }
            transaction.insert_document(&document)?;
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

    // ------------------------------------------------------------------ update

    fn update_validated<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, Value>,
        validate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        self.update_validated_once(table, id, patch, None, validate)?
            .ok_or_else(|| Error::Internal("non-deduplicated update should commit".to_string()))
    }

    fn update_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, Value>,
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<CommitEntry>>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        let table = table.clone();
        let id = id.clone();
        let patch = patch.clone();
        let execution_id = execution_id.map(str::to_string);
        let committed = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(execution_id.as_deref())? {
                return Ok(false);
            }
            transaction.update_document_validated(&table, &id, &patch, validate)?;
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

    fn update_with_indexes_validated<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, Value>,
        _indexes: &[IndexDefinition],
        validate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        self.update_validated(table, id, patch, validate)
    }

    fn update_with_indexes_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, Value>,
        _indexes: &[IndexDefinition],
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<CommitEntry>>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        self.update_validated_once(table, id, patch, execution_id, validate)
    }

    fn update_with_indexes_validated_once_at<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, Value>,
        assignment: crate::DirectWriteAssignment<'_>,
        validate: F,
    ) -> Result<Option<CommitEntry>>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        let table = table.clone();
        let id = id.clone();
        let patch = patch.clone();
        let execution_id = assignment.execution_id.map(str::to_string);
        let committed = self.execute_write(move |transaction| {
            transaction.set_commit_timestamp(Some(assignment.commit_timestamp));
            if !transaction.begin_scheduled_execution(execution_id.as_deref())? {
                return Ok(false);
            }
            transaction.update_document_validated(&table, &id, &patch, validate)?;
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

    // ------------------------------------------------------------------ delete

    fn delete_validated_returning_document<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        validate: F,
    ) -> Result<(CommitEntry, Document)>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        self.delete_validated_once(table, id, None, validate)?
            .ok_or_else(|| Error::Internal("non-deduplicated delete should commit".to_string()))
    }

    fn delete_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<(CommitEntry, Document)>>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        let table = table.clone();
        let id = id.clone();
        let execution_id = execution_id.map(str::to_string);
        let committed = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(execution_id.as_deref())? {
                return Ok(None);
            }
            let removed_document = transaction.delete_document_validated(&table, &id, validate)?;
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

    fn delete_with_indexes_validated_returning_document<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        _indexes: &[IndexDefinition],
        validate: F,
    ) -> Result<(CommitEntry, Document)>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        self.delete_validated_returning_document(table, id, validate)
    }

    fn delete_with_indexes_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        _indexes: &[IndexDefinition],
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<(CommitEntry, Document)>>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        self.delete_validated_once(table, id, execution_id, validate)
    }

    fn delete_with_indexes_validated_once_at<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        assignment: crate::DirectWriteAssignment<'_>,
        validate: F,
    ) -> Result<Option<(CommitEntry, Document)>>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        let table = table.clone();
        let id = id.clone();
        let execution_id = assignment.execution_id.map(str::to_string);
        let committed = self.execute_write(move |transaction| {
            transaction.set_commit_timestamp(Some(assignment.commit_timestamp));
            if !transaction.begin_scheduled_execution(execution_id.as_deref())? {
                return Ok(None);
            }
            let removed_document = transaction.delete_document_validated(&table, &id, validate)?;
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
}

/// Store-level counterpart of [`SqlDurableJournalTransaction`]: implemented by
/// the backends whose journal replay runs inside a write transaction, so the
/// three wrappers below can be shared between them.
#[cfg(any(feature = "mysql", feature = "postgres"))]
pub(crate) trait SqlDurableJournalStore: SqlStoreCore
where
    Self::Transaction: SqlDurableJournalTransaction,
{
    fn pipeline_metrics(&self) -> &SqlWritePipelineMetrics;
}

/// Shared body of [`SqlStoreCore::append_durable_records_batch`] for
/// transaction-replay backends.
#[cfg(any(feature = "mysql", feature = "postgres"))]
pub(crate) fn sql_store_append_durable_records_batch<S>(
    store: &S,
    records: &[TenantEventRecord],
) -> Result<()>
where
    S: SqlDurableJournalStore,
    S::Transaction: SqlDurableJournalTransaction,
{
    let records = records.to_vec();
    store.execute_write(move |transaction| {
        transaction.note_durable_records_for_fault(&records);
        transaction.append_durable_records_batch(&records)
    })?;
    Ok(())
}

/// Shared body of [`SqlStoreCore::apply_durable_records_batch`] for
/// transaction-replay backends.
#[cfg(any(feature = "mysql", feature = "postgres"))]
pub(crate) fn sql_store_apply_durable_records_batch<S>(
    store: &S,
    records: &[TenantEventRecord],
) -> Result<()>
where
    S: SqlDurableJournalStore,
    S::Transaction: SqlDurableJournalTransaction,
{
    let records = records.to_vec();
    store.execute_write(move |transaction| {
        transaction.note_durable_records_for_fault(&records);
        transaction.apply_durable_records_batch(&records)
    })?;
    Ok(())
}

/// Shared body of [`SqlStoreCore::replay_durable_records_batch`] for
/// transaction-replay backends.
///
/// Identical to [`sql_store_apply_durable_records_batch`] except that it does
/// not note the records for the fault interface — a replay makes nothing
/// durable, so it has no durable records to name.
#[cfg(any(feature = "mysql", feature = "postgres"))]
pub(crate) fn sql_store_replay_durable_records_batch<S>(
    store: &S,
    records: &[TenantEventRecord],
) -> Result<()>
where
    S: SqlDurableJournalStore,
    S::Transaction: SqlDurableJournalTransaction,
{
    let records = records.to_vec();
    store.execute_write(move |transaction| transaction.apply_durable_records_batch(&records))?;
    Ok(())
}

/// Shared body of
/// [`SqlStoreCore::fenced_append_and_apply_durable_records_batch_cancellable`]
/// for transaction-replay backends.
#[cfg(any(feature = "mysql", feature = "postgres"))]
pub(crate) fn sql_store_fenced_append_and_apply_durable_records_batch_cancellable<S, Check>(
    store: &S,
    owner_id: &str,
    epoch: u64,
    expected_previous: SequenceNumber,
    records: &[TenantEventRecord],
    check_cancel: Check,
) -> CommitterLeaseResult<()>
where
    S: SqlDurableJournalStore,
    S::Transaction: SqlDurableJournalTransaction,
    Check: Fn() -> Result<()> + Send + 'static,
{
    if records.is_empty() {
        return Err(Error::InvalidInput(
            "fenced durable apply requires at least one record".to_string(),
        )
        .into());
    }
    let owner_id = owner_id.to_string();
    let fenced_owner_id = owner_id.clone();
    let records = records.to_vec();
    let pipeline_progressed = Arc::new(AtomicBool::new(false));
    let pipeline_progressed_in_transaction = pipeline_progressed.clone();
    let result = store.execute_write_cancellable(check_cancel, move |transaction| {
        transaction.note_durable_records_for_fault(&records);
        let durable_sequence = records
            .last()
            .expect("non-empty fenced durable apply batch")
            .sequence;
        if transaction.advance_fenced_committer_lease(
            &owner_id,
            epoch,
            expected_previous,
            durable_sequence,
        )? != 1
        {
            return Err(Error::PreconditionFailed(
                FENCED_COMMITTER_LEASE_MARKER.to_string(),
            ));
        }
        crate::commit_log::ensure_applied_prefix_precedes(
            transaction.applied_sequence()?,
            records[0].sequence,
        )?;
        transaction.append_and_apply_fenced_durable_batch(&records, &mut || {
            pipeline_progressed_in_transaction.store(true, AtomicOrdering::Release);
        })
    });
    if let Err(error @ Error::Cancelled) = &result
        && pipeline_progressed.load(AtomicOrdering::Acquire)
    {
        // The provider's own pipeline accounting records errors it observes.
        // A cancellation can still arrive at the transaction's final pre-commit
        // check after the provider has passed its progress boundary, so record
        // only that outer-boundary case and avoid double-counting an inner
        // cancellation.
        store.pipeline_metrics().record_error(error);
    }
    match result {
        Ok(_) => Ok(()),
        Err(Error::PreconditionFailed(message)) if message == FENCED_COMMITTER_LEASE_MARKER => {
            Err(CommitterLeaseError::Fenced {
                owner_id: fenced_owner_id,
                epoch,
            })
        }
        Err(error) => Err(error.into()),
    }
}

/// Re-exposes every [`SqlStoreCore`] wrapper as an inherent method on a store.
///
/// The wrapper layer is public API: `nimbus-engine` calls these directly on
/// `PostgresTenantStore` and `MySqlTenantStore` through its persistence
/// dispatch. Keeping the bodies in the trait and the names inherent means
/// callers need no import and method resolution stays unambiguous where a
/// store also implements a provider trait with the same method name.
macro_rules! sql_store_core_facade {
    ($ty:ty) => {
        impl $ty {
            pub fn apply_prepared_write_batch(
                &self,
                record: &nimbus_core::TenantEventRecord,
                schedule_ops: &[crate::store::ResolvedScheduleOp],
                scheduled_execution_id: Option<&str>,
            ) -> nimbus_core::Result<Option<nimbus_core::CommitEntry>> {
                <Self as crate::sql::store_core::SqlStoreCore>::apply_prepared_write_batch(self, record, schedule_ops, scheduled_execution_id)
            }

            pub fn fenced_apply_prepared_write_batch(
                &self,
                owner_id: &str,
                epoch: u64,
                expected_previous: nimbus_core::SequenceNumber,
                record: &nimbus_core::TenantEventRecord,
                schedule_ops: &[crate::store::ResolvedScheduleOp],
                scheduled_execution_id: Option<&str>,
            ) -> crate::traits::CommitterLeaseResult<Option<nimbus_core::CommitEntry>> {
                <Self as crate::sql::store_core::SqlStoreCore>::fenced_apply_prepared_write_batch(self, owner_id, epoch, expected_previous, record, schedule_ops, scheduled_execution_id)
            }

            pub fn retention_gc_watermarks(&self, config: crate::retention::RetentionGcConfig) -> nimbus_core::Result<crate::retention::RetentionGcWatermarks> {
                <Self as crate::sql::store_core::SqlStoreCore>::retention_gc_watermarks(self, config)
            }

            pub fn compact_retained_versions(&self, config: crate::retention::RetentionGcConfig) -> nimbus_core::Result<crate::retention::RetentionGcSummary> {
                <Self as crate::sql::store_core::SqlStoreCore>::compact_retained_versions(self, config)
            }

            pub fn retention_history_state(&self, config: crate::retention::RetentionGcConfig) -> nimbus_core::Result<crate::retention::RetentionHistoryState> {
                <Self as crate::sql::store_core::SqlStoreCore>::retention_history_state(self, config)
            }

            pub fn prepare_retained_history(&self, config: crate::retention::RetentionGcConfig) -> nimbus_core::Result<crate::retention::PreparedRetentionHistory> {
                <Self as crate::sql::store_core::SqlStoreCore>::prepare_retained_history(self, config)
            }

            pub fn fenced_finalize_retained_history(
                &self,
                owner_id: &str,
                epoch: u64,
                durable_sequence: nimbus_core::SequenceNumber,
                prepared: crate::retention::PreparedRetentionHistory,
            ) -> crate::traits::CommitterLeaseResult<crate::retention::RetentionHistorySummary> {
                <Self as crate::sql::store_core::SqlStoreCore>::fenced_finalize_retained_history(self, owner_id, epoch, durable_sequence, prepared)
            }

            pub fn fenced_compact_retained_history(
                &self,
                owner_id: &str,
                epoch: u64,
                durable_sequence: nimbus_core::SequenceNumber,
                config: crate::retention::RetentionGcConfig,
            ) -> crate::traits::CommitterLeaseResult<crate::retention::RetentionHistorySummary> {
                <Self as crate::sql::store_core::SqlStoreCore>::fenced_compact_retained_history(self, owner_id, epoch, durable_sequence, config)
            }

            pub fn export_point_in_time_restore_archive(
                &self,
                target: crate::store::PointInTimeRestoreTarget,
                retention_config: crate::retention::RetentionGcConfig,
            ) -> nimbus_core::Result<crate::store::PointInTimeRestoreArchive> {
                <Self as crate::sql::store_core::SqlStoreCore>::export_point_in_time_restore_archive(self, target, retention_config)
            }

            pub fn import_point_in_time_restore_archive(
                &self,
                archive: &crate::store::PointInTimeRestoreArchive,
            ) -> nimbus_core::Result<crate::store::JournalProgress> {
                <Self as crate::sql::store_core::SqlStoreCore>::import_point_in_time_restore_archive(self, archive)
            }

            pub fn fenced_import_point_in_time_restore_archive(
                &self,
                owner_id: &str,
                epoch: u64,
                expected_previous: nimbus_core::SequenceNumber,
                archive: &crate::store::PointInTimeRestoreArchive,
            ) -> crate::traits::CommitterLeaseResult<crate::store::JournalProgress> {
                <Self as crate::sql::store_core::SqlStoreCore>::fenced_import_point_in_time_restore_archive(self, owner_id, epoch, expected_previous, archive)
            }

            pub fn replace_table_schema(&self, table_schema: &nimbus_core::TableSchema) -> nimbus_core::Result<()> {
                <Self as crate::sql::store_core::SqlStoreCore>::replace_table_schema(self, table_schema)
            }

            pub fn fenced_replace_table_schema(
                &self,
                owner_id: &str,
                epoch: u64,
                expected_previous: nimbus_core::SequenceNumber,
                table_schema: &nimbus_core::TableSchema,
            ) -> crate::traits::CommitterLeaseResult<()> {
                <Self as crate::sql::store_core::SqlStoreCore>::fenced_replace_table_schema(self, owner_id, epoch, expected_previous, table_schema)
            }

            pub fn delete_table_schema(&self, table: &nimbus_core::TableName) -> nimbus_core::Result<()> {
                <Self as crate::sql::store_core::SqlStoreCore>::delete_table_schema(self, table)
            }

            pub fn fenced_delete_table_schema(
                &self,
                owner_id: &str,
                epoch: u64,
                expected_previous: nimbus_core::SequenceNumber,
                table: &nimbus_core::TableName,
            ) -> crate::traits::CommitterLeaseResult<()> {
                <Self as crate::sql::store_core::SqlStoreCore>::fenced_delete_table_schema(self, owner_id, epoch, expected_previous, table)
            }

            pub fn append_durable_records_batch(&self, records: &[nimbus_core::TenantEventRecord]) -> nimbus_core::Result<()> {
                <Self as crate::sql::store_core::SqlStoreCore>::append_durable_records_batch(self, records)
            }

            pub fn apply_durable_records_batch(&self, records: &[nimbus_core::TenantEventRecord]) -> nimbus_core::Result<()> {
                <Self as crate::sql::store_core::SqlStoreCore>::apply_durable_records_batch(self, records)
            }

            pub fn replay_durable_records_batch(&self, records: &[nimbus_core::TenantEventRecord]) -> nimbus_core::Result<()> {
                <Self as crate::sql::store_core::SqlStoreCore>::replay_durable_records_batch(self, records)
            }

            pub fn fenced_append_and_apply_durable_records_batch(
                &self,
                owner_id: &str,
                epoch: u64,
                expected_previous: nimbus_core::SequenceNumber,
                records: &[nimbus_core::TenantEventRecord],
            ) -> crate::traits::CommitterLeaseResult<()> {
                <Self as crate::sql::store_core::SqlStoreCore>::fenced_append_and_apply_durable_records_batch(self, owner_id, epoch, expected_previous, records)
            }

            pub fn fenced_append_and_apply_durable_records_batch_cancellable<Check>(
                &self,
                owner_id: &str,
                epoch: u64,
                expected_previous: nimbus_core::SequenceNumber,
                records: &[nimbus_core::TenantEventRecord],
                check_cancel: Check,
            ) -> crate::traits::CommitterLeaseResult<()>
            where
                Check: Fn() -> nimbus_core::Result<()> + Send + 'static, {
                <Self as crate::sql::store_core::SqlStoreCore>::fenced_append_and_apply_durable_records_batch_cancellable(self, owner_id, epoch, expected_previous, records, check_cancel)
            }

            pub fn insert_scheduled_job(&self, job: &nimbus_core::ScheduledJob) -> nimbus_core::Result<()> {
                <Self as crate::sql::store_core::SqlStoreCore>::insert_scheduled_job(self, job)
            }

            pub fn claim_due_jobs(&self, now: nimbus_core::Timestamp, max_jobs: usize) -> nimbus_core::Result<Vec<nimbus_core::ScheduledJob>> {
                <Self as crate::sql::store_core::SqlStoreCore>::claim_due_jobs(self, now, max_jobs)
            }

            pub fn complete_scheduled_job(&self, job_id: &nimbus_core::DocumentId) -> nimbus_core::Result<()> {
                <Self as crate::sql::store_core::SqlStoreCore>::complete_scheduled_job(self, job_id)
            }

            pub fn cancel_scheduled_job(&self, job_id: &nimbus_core::DocumentId) -> nimbus_core::Result<bool> {
                <Self as crate::sql::store_core::SqlStoreCore>::cancel_scheduled_job(self, job_id)
            }

            pub fn record_scheduled_job_result(&self, result: &nimbus_core::ScheduledJobResult) -> nimbus_core::Result<()> {
                <Self as crate::sql::store_core::SqlStoreCore>::record_scheduled_job_result(self, result)
            }

            pub fn save_cron_job(&self, cron: &nimbus_core::CronJob) -> nimbus_core::Result<()> {
                <Self as crate::sql::store_core::SqlStoreCore>::save_cron_job(self, cron)
            }

            pub fn delete_cron_job(&self, name: &str) -> nimbus_core::Result<()> {
                <Self as crate::sql::store_core::SqlStoreCore>::delete_cron_job(self, name)
            }

            pub fn recover_running_jobs(&self, now: nimbus_core::Timestamp) -> nimbus_core::Result<()> {
                <Self as crate::sql::store_core::SqlStoreCore>::recover_running_jobs(self, now)
            }

            pub fn materialize_trigger_invocations(
                &self,
                records: &[nimbus_core::TriggerInvocationRecord],
                cursor: nimbus_core::TriggerDeliveryCursor,
            ) -> nimbus_core::Result<()> {
                <Self as crate::sql::store_core::SqlStoreCore>::materialize_trigger_invocations(self, records, cursor)
            }

            pub fn fenced_materialize_trigger_invocations(
                &self,
                owner_id: &str,
                epoch: u64,
                expected_previous: nimbus_core::SequenceNumber,
                records: &[nimbus_core::TriggerInvocationRecord],
                cursor: nimbus_core::TriggerDeliveryCursor,
            ) -> crate::traits::CommitterLeaseResult<()> {
                <Self as crate::sql::store_core::SqlStoreCore>::fenced_materialize_trigger_invocations(self, owner_id, epoch, expected_previous, records, cursor)
            }

            pub fn save_trigger_invocation(&self, record: &nimbus_core::TriggerInvocationRecord) -> nimbus_core::Result<()> {
                <Self as crate::sql::store_core::SqlStoreCore>::save_trigger_invocation(self, record)
            }

            pub fn fenced_save_trigger_invocation(
                &self,
                owner_id: &str,
                epoch: u64,
                expected_durable_sequence: nimbus_core::SequenceNumber,
                record: &nimbus_core::TriggerInvocationRecord,
            ) -> crate::traits::CommitterLeaseResult<()> {
                <Self as crate::sql::store_core::SqlStoreCore>::fenced_save_trigger_invocation(self, owner_id, epoch, expected_durable_sequence, record)
            }

            pub fn apply_execution_unit_batch(
                &self,
                writes: &[crate::store::ResolvedWrite],
                schedule_ops: &[crate::store::ResolvedScheduleOp],
            ) -> nimbus_core::Result<Option<nimbus_core::CommitEntry>> {
                <Self as crate::sql::store_core::SqlStoreCore>::apply_execution_unit_batch(self, writes, schedule_ops)
            }

            pub fn apply_execution_unit_batch_with_origin(
                &self,
                writes: &[crate::store::ResolvedWrite],
                schedule_ops: &[crate::store::ResolvedScheduleOp],
                trigger_write_origin: Option<&nimbus_core::TriggerWriteOrigin>,
                commit_timestamp: Option<nimbus_core::Timestamp>,
            ) -> nimbus_core::Result<Option<nimbus_core::CommitEntry>> {
                <Self as crate::sql::store_core::SqlStoreCore>::apply_execution_unit_batch_with_origin(self, writes, schedule_ops, trigger_write_origin, commit_timestamp)
            }

            pub fn insert(&self, document: &nimbus_core::Document) -> nimbus_core::Result<nimbus_core::CommitEntry> {
                <Self as crate::sql::store_core::SqlStoreCore>::insert(self, document)
            }

            pub fn insert_with_indexes(
                &self,
                document: &nimbus_core::Document,
                _indexes: &[nimbus_core::IndexDefinition],
            ) -> nimbus_core::Result<nimbus_core::CommitEntry> {
                <Self as crate::sql::store_core::SqlStoreCore>::insert_with_indexes(self, document, _indexes)
            }

            pub fn insert_once(
                &self,
                document: &nimbus_core::Document,
                execution_id: Option<&str>,
            ) -> nimbus_core::Result<Option<nimbus_core::CommitEntry>> {
                <Self as crate::sql::store_core::SqlStoreCore>::insert_once(self, document, execution_id)
            }

            pub fn insert_with_indexes_once(
                &self,
                document: &nimbus_core::Document,
                _indexes: &[nimbus_core::IndexDefinition],
                execution_id: Option<&str>,
            ) -> nimbus_core::Result<Option<nimbus_core::CommitEntry>> {
                <Self as crate::sql::store_core::SqlStoreCore>::insert_with_indexes_once(self, document, _indexes, execution_id)
            }

            pub fn insert_with_indexes_once_at(
                &self,
                document: &nimbus_core::Document,
                assignment: crate::DirectWriteAssignment<'_>,
            ) -> nimbus_core::Result<Option<nimbus_core::CommitEntry>> {
                <Self as crate::sql::store_core::SqlStoreCore>::insert_with_indexes_once_at(self, document, assignment)
            }

            pub fn update_validated<F>(
                &self,
                table: &nimbus_core::TableName,
                id: &nimbus_core::DocumentId,
                patch: &serde_json::Map<String, serde_json::Value>,
                validate: F,
            ) -> nimbus_core::Result<nimbus_core::CommitEntry>
            where
                F: FnOnce(&nimbus_core::Document, &nimbus_core::Document) -> nimbus_core::Result<()> + Send + 'static, {
                <Self as crate::sql::store_core::SqlStoreCore>::update_validated(self, table, id, patch, validate)
            }

            pub fn update_validated_once<F>(
                &self,
                table: &nimbus_core::TableName,
                id: &nimbus_core::DocumentId,
                patch: &serde_json::Map<String, serde_json::Value>,
                execution_id: Option<&str>,
                validate: F,
            ) -> nimbus_core::Result<Option<nimbus_core::CommitEntry>>
            where
                F: FnOnce(&nimbus_core::Document, &nimbus_core::Document) -> nimbus_core::Result<()> + Send + 'static, {
                <Self as crate::sql::store_core::SqlStoreCore>::update_validated_once(self, table, id, patch, execution_id, validate)
            }

            pub fn update_with_indexes_validated<F>(
                &self,
                table: &nimbus_core::TableName,
                id: &nimbus_core::DocumentId,
                patch: &serde_json::Map<String, serde_json::Value>,
                _indexes: &[nimbus_core::IndexDefinition],
                validate: F,
            ) -> nimbus_core::Result<nimbus_core::CommitEntry>
            where
                F: FnOnce(&nimbus_core::Document, &nimbus_core::Document) -> nimbus_core::Result<()> + Send + 'static, {
                <Self as crate::sql::store_core::SqlStoreCore>::update_with_indexes_validated(self, table, id, patch, _indexes, validate)
            }

            pub fn update_with_indexes_validated_once<F>(
                &self,
                table: &nimbus_core::TableName,
                id: &nimbus_core::DocumentId,
                patch: &serde_json::Map<String, serde_json::Value>,
                _indexes: &[nimbus_core::IndexDefinition],
                execution_id: Option<&str>,
                validate: F,
            ) -> nimbus_core::Result<Option<nimbus_core::CommitEntry>>
            where
                F: FnOnce(&nimbus_core::Document, &nimbus_core::Document) -> nimbus_core::Result<()> + Send + 'static, {
                <Self as crate::sql::store_core::SqlStoreCore>::update_with_indexes_validated_once(self, table, id, patch, _indexes, execution_id, validate)
            }

            pub fn update_with_indexes_validated_once_at<F>(
                &self,
                table: &nimbus_core::TableName,
                id: &nimbus_core::DocumentId,
                patch: &serde_json::Map<String, serde_json::Value>,
                assignment: crate::DirectWriteAssignment<'_>,
                validate: F,
            ) -> nimbus_core::Result<Option<nimbus_core::CommitEntry>>
            where
                F: FnOnce(&nimbus_core::Document, &nimbus_core::Document) -> nimbus_core::Result<()> + Send + 'static, {
                <Self as crate::sql::store_core::SqlStoreCore>::update_with_indexes_validated_once_at(self, table, id, patch, assignment, validate)
            }

            pub fn delete_validated_returning_document<F>(
                &self,
                table: &nimbus_core::TableName,
                id: &nimbus_core::DocumentId,
                validate: F,
            ) -> nimbus_core::Result<(nimbus_core::CommitEntry, nimbus_core::Document)>
            where
                F: FnOnce(&nimbus_core::Document) -> nimbus_core::Result<()> + Send + 'static, {
                <Self as crate::sql::store_core::SqlStoreCore>::delete_validated_returning_document(self, table, id, validate)
            }

            pub fn delete_validated_once<F>(
                &self,
                table: &nimbus_core::TableName,
                id: &nimbus_core::DocumentId,
                execution_id: Option<&str>,
                validate: F,
            ) -> nimbus_core::Result<Option<(nimbus_core::CommitEntry, nimbus_core::Document)>>
            where
                F: FnOnce(&nimbus_core::Document) -> nimbus_core::Result<()> + Send + 'static, {
                <Self as crate::sql::store_core::SqlStoreCore>::delete_validated_once(self, table, id, execution_id, validate)
            }

            pub fn delete_with_indexes_validated_returning_document<F>(
                &self,
                table: &nimbus_core::TableName,
                id: &nimbus_core::DocumentId,
                _indexes: &[nimbus_core::IndexDefinition],
                validate: F,
            ) -> nimbus_core::Result<(nimbus_core::CommitEntry, nimbus_core::Document)>
            where
                F: FnOnce(&nimbus_core::Document) -> nimbus_core::Result<()> + Send + 'static, {
                <Self as crate::sql::store_core::SqlStoreCore>::delete_with_indexes_validated_returning_document(self, table, id, _indexes, validate)
            }

            pub fn delete_with_indexes_validated_once<F>(
                &self,
                table: &nimbus_core::TableName,
                id: &nimbus_core::DocumentId,
                _indexes: &[nimbus_core::IndexDefinition],
                execution_id: Option<&str>,
                validate: F,
            ) -> nimbus_core::Result<Option<(nimbus_core::CommitEntry, nimbus_core::Document)>>
            where
                F: FnOnce(&nimbus_core::Document) -> nimbus_core::Result<()> + Send + 'static, {
                <Self as crate::sql::store_core::SqlStoreCore>::delete_with_indexes_validated_once(self, table, id, _indexes, execution_id, validate)
            }

            pub fn delete_with_indexes_validated_once_at<F>(
                &self,
                table: &nimbus_core::TableName,
                id: &nimbus_core::DocumentId,
                assignment: crate::DirectWriteAssignment<'_>,
                validate: F,
            ) -> nimbus_core::Result<Option<(nimbus_core::CommitEntry, nimbus_core::Document)>>
            where
                F: FnOnce(&nimbus_core::Document) -> nimbus_core::Result<()> + Send + 'static, {
                <Self as crate::sql::store_core::SqlStoreCore>::delete_with_indexes_validated_once_at(self, table, id, assignment, validate)
            }

        }
    };
}

pub(crate) use sql_store_core_facade;

// --------------------------------------------------------------------------
// Semaphore-bounded blocking executors shared by the network-backed SQL stores.
//
// PostgreSQL and MySQL keep provider-owned blocking write executors because
// transaction and session lifecycles are coupled to their async clients. The
// generic `async_storage::write` executor is only shared across the embedded
// blocking-store seam.
//
// libsql does not use this section: its reads are served from the local SQLite
// replica cache and its writes go to the remote primary. Every item below
// therefore carries the `postgres`-or-`mysql` gate so a libsql-only build does
// not compile it as dead code.
// --------------------------------------------------------------------------

/// Acquire a read permit and run `task` on the blocking pool.
#[cfg(any(feature = "mysql", feature = "postgres"))]
pub(crate) async fn sql_execute_read<S, T, F>(
    permits: &Arc<Semaphore>,
    runtime_handle: &TokioRuntimeHandle,
    store: &Arc<S>,
    context: &'static str,
    task: F,
) -> Result<T>
where
    S: Send + Sync + 'static,
    T: Send + 'static,
    F: FnOnce(Arc<S>) -> Result<T> + Send + 'static,
{
    let permit = permits
        .clone()
        .acquire_owned()
        .await
        .map_err(|error| map_executor_permit_error(context, error))?;
    let store = store.clone();
    runtime_handle
        .spawn_blocking(move || {
            let _permit = permit;
            task(store)
        })
        .await
        .map_err(|error| map_executor_join_error(context, error))?
}

/// Cancellable variant of [`sql_execute_read`]. Cancellation short-circuits
/// before the permit is granted; afterwards it is cooperative, surfacing through
/// the `check_cancel` handed to the blocking task.
#[cfg(any(feature = "mysql", feature = "postgres"))]
pub(crate) async fn sql_execute_read_cancellable<S, T, Fut, Check, F>(
    permits: &Arc<Semaphore>,
    runtime_handle: &TokioRuntimeHandle,
    store: &Arc<S>,
    context: &'static str,
    cancel_wait: Fut,
    check_cancel: Check,
    task: F,
) -> Result<T>
where
    S: Send + Sync + 'static,
    T: Send + 'static,
    Fut: Future<Output = ()> + Send,
    Check: Fn() -> Result<()> + Send + 'static,
    F: FnOnce(Arc<S>, &mut dyn FnMut() -> Result<()>) -> Result<T> + Send + 'static,
{
    tokio::pin!(cancel_wait);

    let permit = tokio::select! {
        _ = &mut cancel_wait => return Err(Error::Cancelled),
        permit = permits.clone().acquire_owned() => permit
            .map_err(|error| map_executor_permit_error(context, error))?,
    };

    let cancelled = Arc::new(AtomicBool::new(false));
    let store = store.clone();
    let cancelled_for_task = cancelled.clone();
    let mut handle = runtime_handle.spawn_blocking(move || {
        let _permit = permit;
        let mut combined_cancel = || {
            if cancelled_for_task.load(AtomicOrdering::SeqCst) {
                return Err(Error::Cancelled);
            }
            check_cancel()
        };
        task(store, &mut combined_cancel)
    });

    tokio::select! {
        _ = &mut cancel_wait => {
            cancelled.store(true, AtomicOrdering::SeqCst);
            Err(Error::Cancelled)
        }
        result = &mut handle => result
            .map_err(|error| map_executor_join_error(context, error))?,
    }
}

/// Semaphore-bounded blocking write executor over a [`SqlStoreCore`] store.
#[cfg(any(feature = "mysql", feature = "postgres"))]
pub(crate) struct SqlBlockingWriteExecutor<S> {
    store: Arc<S>,
    permits: Arc<Semaphore>,
    runtime_handle: TokioRuntimeHandle,
    context: &'static str,
}

#[cfg(any(feature = "mysql", feature = "postgres"))]
impl<S> Clone for SqlBlockingWriteExecutor<S> {
    fn clone(&self) -> Self {
        Self {
            store: self.store.clone(),
            permits: self.permits.clone(),
            runtime_handle: self.runtime_handle.clone(),
            context: self.context,
        }
    }
}

#[cfg(any(feature = "mysql", feature = "postgres"))]
impl<S> SqlBlockingWriteExecutor<S>
where
    S: SqlStoreCore + Send + Sync + 'static,
{
    pub(crate) fn new(
        store: Arc<S>,
        runtime_handle: TokioRuntimeHandle,
        write_parallelism: usize,
        context: &'static str,
    ) -> Self {
        Self {
            store,
            permits: Arc::new(Semaphore::new(write_parallelism)),
            runtime_handle,
            context,
        }
    }

    pub(crate) async fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut S::Transaction) -> Result<T> + Send + 'static,
    {
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|error| map_executor_permit_error(self.context, error))?;
        let store = self.store.clone();
        let context = self.context;
        self.runtime_handle
            .spawn_blocking(move || {
                let _permit = permit;
                store.execute_write(task)
            })
            .await
            .map_err(|error| map_executor_join_error(context, error))?
    }

    pub(crate) async fn execute_write_cancellable<T, Fut, Check, F>(
        &self,
        cancel_wait: Fut,
        check_cancel: Check,
        task: F,
    ) -> Result<TenantWriteOutcome<T>>
    where
        T: Send + 'static,
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(&mut S::Transaction) -> Result<T> + Send + 'static,
    {
        tokio::pin!(cancel_wait);

        let permit = tokio::select! {
            _ = &mut cancel_wait => return Ok(TenantWriteOutcome::CancelledBeforeCommit),
            permit = self.permits.clone().acquire_owned() => permit
                .map_err(|error| map_executor_permit_error(self.context, error))?,
        };

        let cancelled = Arc::new(AtomicBool::new(false));
        let store = self.store.clone();
        let cancelled_for_task = cancelled.clone();
        let context = self.context;
        let mut handle = self.runtime_handle.spawn_blocking(move || {
            let _permit = permit;
            store.execute_write_cancellable(
                move || {
                    if cancelled_for_task.load(AtomicOrdering::SeqCst) {
                        return Err(Error::Cancelled);
                    }
                    check_cancel()
                },
                task,
            )
        });

        tokio::select! {
            result = &mut handle => map_write_result(result
                .map_err(|error| map_executor_join_error(context, error))?),
            _ = &mut cancel_wait => {
                cancelled.store(true, AtomicOrdering::SeqCst);
                map_write_result(handle.await
                    .map_err(|error| map_executor_join_error(context, error))?)
            }
        }
    }
}

/// A write that reached its commit point must be reported as committed even if
/// the caller's cancellation raced the response path.
#[cfg(any(feature = "mysql", feature = "postgres"))]
fn map_write_result<T>(result: Result<TenantWriteCommit<T>>) -> Result<TenantWriteOutcome<T>> {
    match result {
        Ok(committed) => Ok(TenantWriteOutcome::Committed(committed)),
        Err(Error::Cancelled) => Ok(TenantWriteOutcome::CancelledBeforeCommit),
        Err(error) => Err(error),
    }
}
