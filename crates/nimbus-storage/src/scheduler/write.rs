use nimbus_core::{
    CronJob, DocumentId, Error, Result, ScheduledJob, ScheduledJobResult, SequenceNumber, Timestamp,
};

use crate::{
    CommitterLeaseError, CommitterLeaseResult, LibsqlReplicaTenantStore,
    LibsqlReplicaWriteTransaction, MemoryTenantStore, MemoryWriteTransaction, MySqlTenantStore,
    MySqlWriteTransaction, PostgresTenantStore, PostgresWriteTransaction, SqliteTenantStore,
    SqliteWriteTransaction, TenantStore, TenantWriteTransaction,
};

const FENCED_SCHEDULER_WRITE_MARKER: &str = "fenced committer lease during scheduler write";

/// One durable scheduler-state transition owned by the tenant committer.
///
/// The operation is data so every backend executes the same state machine in
/// one transaction. Provider adapters additionally validate the held lease in
/// that transaction without advancing the journal sequence.
#[derive(Debug, Clone)]
pub enum SchedulerWrite {
    Insert(ScheduledJob),
    ClaimDue { now: Timestamp, max_jobs: usize },
    Complete(DocumentId),
    Cancel(DocumentId),
    RecordResult(ScheduledJobResult),
    SaveCron(CronJob),
    DeleteCron(String),
    RecoverRunning { now: Timestamp },
}

#[derive(Debug, Clone, PartialEq)]
pub enum SchedulerWriteResult {
    Unit,
    Claimed(Vec<ScheduledJob>),
    Removed(bool),
}

trait SchedulerWriteTransaction {
    fn insert_scheduled_job(&mut self, job: &ScheduledJob) -> Result<()>;
    fn claim_due_jobs(&mut self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>>;
    fn complete_scheduled_job(&mut self, job_id: &DocumentId) -> Result<()>;
    fn cancel_scheduled_job(&mut self, job_id: &DocumentId) -> Result<bool>;
    fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()>;
    fn save_cron_job(&mut self, cron: &CronJob) -> Result<()>;
    fn delete_cron_job(&mut self, name: &str) -> Result<()>;
    fn recover_running_jobs(&mut self, now: Timestamp) -> Result<()>;
}

macro_rules! impl_scheduler_write_transaction {
    ($transaction:ty) => {
        impl SchedulerWriteTransaction for $transaction {
            fn insert_scheduled_job(&mut self, job: &ScheduledJob) -> Result<()> {
                <$transaction>::insert_scheduled_job(self, job)
            }

            fn claim_due_jobs(
                &mut self,
                now: Timestamp,
                max_jobs: usize,
            ) -> Result<Vec<ScheduledJob>> {
                <$transaction>::claim_due_jobs(self, now, max_jobs)
            }

            fn complete_scheduled_job(&mut self, job_id: &DocumentId) -> Result<()> {
                <$transaction>::complete_scheduled_job(self, job_id)
            }

            fn cancel_scheduled_job(&mut self, job_id: &DocumentId) -> Result<bool> {
                <$transaction>::cancel_scheduled_job(self, job_id)
            }

            fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()> {
                <$transaction>::record_scheduled_job_result(self, result)
            }

            fn save_cron_job(&mut self, cron: &CronJob) -> Result<()> {
                <$transaction>::save_cron_job(self, cron)
            }

            fn delete_cron_job(&mut self, name: &str) -> Result<()> {
                <$transaction>::delete_cron_job(self, name)
            }

            fn recover_running_jobs(&mut self, now: Timestamp) -> Result<()> {
                <$transaction>::recover_running_jobs(self, now)
            }
        }
    };
}

impl_scheduler_write_transaction!(TenantWriteTransaction);
impl_scheduler_write_transaction!(SqliteWriteTransaction);
impl_scheduler_write_transaction!(LibsqlReplicaWriteTransaction);
impl_scheduler_write_transaction!(PostgresWriteTransaction);
impl_scheduler_write_transaction!(MySqlWriteTransaction);
impl_scheduler_write_transaction!(MemoryWriteTransaction);

fn apply_scheduler_write(
    transaction: &mut impl SchedulerWriteTransaction,
    operation: SchedulerWrite,
) -> Result<SchedulerWriteResult> {
    match operation {
        SchedulerWrite::Insert(job) => {
            transaction.insert_scheduled_job(&job)?;
            Ok(SchedulerWriteResult::Unit)
        }
        SchedulerWrite::ClaimDue { now, max_jobs } => transaction
            .claim_due_jobs(now, max_jobs)
            .map(SchedulerWriteResult::Claimed),
        SchedulerWrite::Complete(job_id) => {
            transaction.complete_scheduled_job(&job_id)?;
            Ok(SchedulerWriteResult::Unit)
        }
        SchedulerWrite::Cancel(job_id) => transaction
            .cancel_scheduled_job(&job_id)
            .map(SchedulerWriteResult::Removed),
        SchedulerWrite::RecordResult(result) => {
            transaction.record_scheduled_job_result(&result)?;
            Ok(SchedulerWriteResult::Unit)
        }
        SchedulerWrite::SaveCron(cron) => {
            transaction.save_cron_job(&cron)?;
            Ok(SchedulerWriteResult::Unit)
        }
        SchedulerWrite::DeleteCron(name) => {
            transaction.delete_cron_job(&name)?;
            Ok(SchedulerWriteResult::Unit)
        }
        SchedulerWrite::RecoverRunning { now } => {
            transaction.recover_running_jobs(now)?;
            Ok(SchedulerWriteResult::Unit)
        }
    }
}

/// Transactional scheduler-write seam shared by every production backend.
pub trait SchedulerWriteStore {
    fn scheduler_write_cancellable<Check>(
        &self,
        operation: SchedulerWrite,
        check_cancel: Check,
    ) -> Result<SchedulerWriteResult>
    where
        Check: Fn() -> Result<()> + Send + 'static;

    fn fenced_scheduler_write_cancellable<Check>(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_durable_sequence: SequenceNumber,
        operation: SchedulerWrite,
        check_cancel: Check,
    ) -> CommitterLeaseResult<SchedulerWriteResult>
    where
        Check: Fn() -> Result<()> + Send + 'static,
    {
        let _ = (
            owner_id,
            epoch,
            expected_durable_sequence,
            operation,
            check_cancel,
        );
        Err(CommitterLeaseError::Unsupported)
    }
}

macro_rules! impl_scheduler_write_store {
    ($store:ty) => {
        impl SchedulerWriteStore for $store {
            fn scheduler_write_cancellable<Check>(
                &self,
                operation: SchedulerWrite,
                check_cancel: Check,
            ) -> Result<SchedulerWriteResult>
            where
                Check: Fn() -> Result<()> + Send + 'static,
            {
                self.execute_write_cancellable(check_cancel, move |transaction| {
                    apply_scheduler_write(transaction, operation)
                })
                .map(|committed| committed.value)
            }
        }
    };
}

impl_scheduler_write_store!(TenantStore);
impl_scheduler_write_store!(SqliteTenantStore);
impl_scheduler_write_store!(MemoryTenantStore);

macro_rules! impl_provider_scheduler_write_store {
    ($store:ty) => {
        impl SchedulerWriteStore for $store {
            fn scheduler_write_cancellable<Check>(
                &self,
                operation: SchedulerWrite,
                check_cancel: Check,
            ) -> Result<SchedulerWriteResult>
            where
                Check: Fn() -> Result<()> + Send + 'static,
            {
                self.execute_write_cancellable(check_cancel, move |transaction| {
                    apply_scheduler_write(transaction, operation)
                })
                .map(|committed| committed.value)
            }

            fn fenced_scheduler_write_cancellable<Check>(
                &self,
                owner_id: &str,
                epoch: u64,
                expected_durable_sequence: SequenceNumber,
                operation: SchedulerWrite,
                check_cancel: Check,
            ) -> CommitterLeaseResult<SchedulerWriteResult>
            where
                Check: Fn() -> Result<()> + Send + 'static,
            {
                let owner_id = owner_id.to_string();
                let fenced_owner_id = owner_id.clone();
                let result = self.execute_write_cancellable(check_cancel, move |transaction| {
                    if transaction.validate_fenced_committer_lease(
                        &owner_id,
                        epoch,
                        expected_durable_sequence,
                    )? != 1
                    {
                        return Err(Error::PreconditionFailed(
                            FENCED_SCHEDULER_WRITE_MARKER.to_string(),
                        ));
                    }
                    apply_scheduler_write(transaction, operation)
                });
                match result {
                    Ok(committed) => Ok(committed.value),
                    Err(Error::PreconditionFailed(message))
                        if message == FENCED_SCHEDULER_WRITE_MARKER =>
                    {
                        Err(CommitterLeaseError::Fenced {
                            owner_id: fenced_owner_id,
                            epoch,
                        })
                    }
                    Err(error) => Err(error.into()),
                }
            }
        }
    };
}

impl_provider_scheduler_write_store!(PostgresTenantStore);
impl_provider_scheduler_write_store!(MySqlTenantStore);
impl_provider_scheduler_write_store!(LibsqlReplicaTenantStore);
