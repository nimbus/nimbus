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

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

use nimbus_core::{
    CommitEntry, CronJob, Document, DocumentId, Error, IndexDefinition, Result, ScheduledJob,
    ScheduledJobResult, SequenceNumber, TableName, TableSchema, TenantEventRecord, Timestamp,
    TriggerWriteOrigin,
};
use serde_json::Value;
use tokio::runtime::Handle as TokioRuntimeHandle;
use tokio::sync::Semaphore;

use crate::async_storage::{
    TenantWriteOutcome, map_executor_join_error, map_executor_permit_error,
};
use crate::retention::{
    RetentionFloor, RetentionGcConfig, RetentionGcSummary, RetentionGcWatermarks,
};
use crate::sql::write_core::SqlWriteBackend;
use crate::sql::write_pipeline::SqlWritePipelineMetrics;
use crate::store::{
    JournalProgress, MaterializedJournalSnapshot, PointInTimeRestoreArchive,
    PointInTimeRestoreTarget, ResolvedScheduleOp, ResolvedWrite, TenantWriteCommit,
};
use crate::traits::{CommitterLeaseError, CommitterLeaseResult};

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
/// schema replacement, durable-journal append/apply, committer-lease fencing,
/// and the validated document mutations.
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

    // Durable journal.
    fn append_durable_records_batch(&mut self, records: &[TenantEventRecord]) -> Result<()>;
    fn apply_durable_records_batch(&mut self, records: &[TenantEventRecord]) -> Result<()>;
    /// Append and apply one fenced durable batch.
    ///
    /// `on_pipeline_progress` is invoked at the provider's own pipeline
    /// accounting boundary, which differs by dialect and is deliberately not
    /// unified: PostgreSQL pipelines the append and the apply as one ordered
    /// pair and reports progress once both have completed, while MySQL issues
    /// them as separate statements and reports progress at batch admission.
    /// The fenced wrapper uses it only to decide whether an outer-boundary
    /// cancellation has already been accounted for by the provider.
    fn append_and_apply_fenced_durable_batch(
        &mut self,
        records: &[TenantEventRecord],
        on_pipeline_progress: &mut dyn FnMut(),
    ) -> Result<()>;

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
    fn pipeline_metrics(&self) -> &SqlWritePipelineMetrics;
    fn journal_progress(&self) -> Result<JournalProgress>;
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
            if !transaction.begin_scheduled_execution(scheduled_execution_id.as_deref())? {
                return Ok(false);
            }
            transaction.apply_durable_record(&record)?;
            apply_schedule_ops_in_transaction(transaction, &schedule_ops)?;
            transaction.set_prepared_record(record);
            Ok(true)
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
            if !transaction.begin_scheduled_execution(scheduled_execution_id.as_deref())? {
                return Ok(false);
            }
            if transaction.advance_fenced_committer_lease(
                &owner_id,
                epoch,
                expected_previous,
                record.sequence,
            )? != 1
            {
                return Err(Error::PreconditionFailed(
                    FENCED_COMMITTER_LEASE_MARKER.to_string(),
                ));
            }
            transaction.apply_durable_record(&record)?;
            apply_schedule_ops_in_transaction(transaction, &schedule_ops)?;
            transaction.set_prepared_record(record);
            Ok(true)
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
        let restored_fingerprint = self
            .export_materialized_journal_snapshot()?
            .canonical_fingerprint()?;
        if restored_fingerprint != archive.target_fingerprint {
            return Err(Error::storage(
                nimbus_core::StorageErrorKind::Corruption,
                format!(
                    "point-in-time restore fingerprint mismatch: restored {} expected {}",
                    restored_fingerprint, archive.target_fingerprint
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
        let restored_fingerprint = self
            .export_materialized_journal_snapshot()?
            .canonical_fingerprint()?;
        if restored_fingerprint != archive.target_fingerprint {
            return Err(Error::storage(
                nimbus_core::StorageErrorKind::Corruption,
                format!(
                    "point-in-time restore fingerprint mismatch: restored {} expected {}",
                    restored_fingerprint, archive.target_fingerprint
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

    fn append_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        let records = records.to_vec();
        self.execute_write(move |transaction| transaction.append_durable_records_batch(&records))?;
        Ok(())
    }

    fn apply_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        let records = records.to_vec();
        self.execute_write(move |transaction| transaction.apply_durable_records_batch(&records))?;
        Ok(())
    }

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

    fn fenced_append_and_apply_durable_records_batch_cancellable<Check>(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_previous: SequenceNumber,
        records: &[TenantEventRecord],
        check_cancel: Check,
    ) -> CommitterLeaseResult<()>
    where
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
        let result = self.execute_write_cancellable(check_cancel, move |transaction| {
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
            // A cancellation can still arrive at the transaction's final
            // pre-commit check after the provider has passed its progress
            // boundary, so record only that outer-boundary case and avoid
            // double-counting an inner cancellation.
            self.pipeline_metrics().record_error(error);
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
            transaction.set_trigger_write_origin(trigger_write_origin.clone());
            transaction.set_commit_timestamp(commit_timestamp);
            for write in &writes {
                transaction.apply_resolved_write(write)?;
            }
            apply_schedule_ops_in_transaction(transaction, &schedule_ops)?;
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
// --------------------------------------------------------------------------

/// Acquire a read permit and run `task` on the blocking pool.
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
pub(crate) struct SqlBlockingWriteExecutor<S> {
    store: Arc<S>,
    permits: Arc<Semaphore>,
    runtime_handle: TokioRuntimeHandle,
    context: &'static str,
}

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
fn map_write_result<T>(result: Result<TenantWriteCommit<T>>) -> Result<TenantWriteOutcome<T>> {
    match result {
        Ok(committed) => Ok(TenantWriteOutcome::Committed(committed)),
        Err(Error::Cancelled) => Ok(TenantWriteOutcome::CancelledBeforeCommit),
        Err(error) => Err(error),
    }
}
