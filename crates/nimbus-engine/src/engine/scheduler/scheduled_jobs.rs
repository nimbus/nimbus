use std::{future, sync::Arc, time::Duration};

use nimbus_core::{
    Error, JobId, Mutation, Result, ScheduleRequest, ScheduledJob, ScheduledJobResult, TenantId,
    Timestamp,
};
use nimbus_storage::{FaultPoint, TenantWriteOutcome};

use super::super::Engine;
use super::access::{
    read_scheduler_store, with_scheduler_runtime, write_scheduler_transaction,
    write_scheduler_transaction_cancellable,
};

pub(crate) const SCHEDULED_JOB_CLAIM_BATCH_LIMIT: usize = 128;

impl Engine {
    /// Schedules a mutation to execute in the future.
    pub fn schedule_mutation(
        &self,
        tenant_id: &TenantId,
        request: ScheduleRequest,
    ) -> Result<JobId> {
        let job = scheduled_job_from_request(self.now(), request);
        self.persist_scheduled_job(tenant_id, job)
    }

    /// Schedules a mutation at an exact wall-clock timestamp.
    pub fn schedule_mutation_at(
        &self,
        tenant_id: &TenantId,
        run_at: Timestamp,
        mutation: Mutation,
    ) -> Result<JobId> {
        let job = scheduled_job_at(self.now(), run_at, mutation);
        self.persist_scheduled_job(tenant_id, job)
    }

    fn persist_scheduled_job(&self, tenant_id: &TenantId, job: ScheduledJob) -> Result<JobId> {
        let job_id = job.id.clone();
        let job_id_for_insert = job_id.clone();
        with_scheduler_runtime(self, tenant_id, move |runtime| {
            runtime.store.insert_scheduled_job(&job)?;
            Ok(job_id_for_insert.clone())
        })?;
        self.wake_scheduler();
        Ok(job_id)
    }

    /// Schedules a mutation to execute in the future asynchronously.
    pub async fn schedule_mutation_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        request: ScheduleRequest,
    ) -> Result<JobId> {
        self.schedule_mutation_async_cancellable(tenant_id, request, future::pending(), || Ok(()))
            .await
    }

    /// Schedules a mutation at an exact wall-clock timestamp asynchronously.
    pub async fn schedule_mutation_at_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        run_at: Timestamp,
        mutation: Mutation,
    ) -> Result<JobId> {
        self.schedule_mutation_at_async_cancellable(
            tenant_id,
            run_at,
            mutation,
            future::pending(),
            || Ok(()),
        )
        .await
    }

    /// Schedules a mutation to execute in the future asynchronously with cooperative cancellation.
    pub async fn schedule_mutation_async_cancellable<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        request: ScheduleRequest,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<JobId>
    where
        Fut: future::Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        let job = scheduled_job_from_request(self.now(), request);
        self.persist_scheduled_job_async_cancellable(tenant_id, job, cancel_wait, check_cancel)
            .await
    }

    /// Schedules a mutation at an exact wall-clock timestamp asynchronously
    /// with cooperative cancellation.
    pub async fn schedule_mutation_at_async_cancellable<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        run_at: Timestamp,
        mutation: Mutation,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<JobId>
    where
        Fut: future::Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        let job = scheduled_job_at(self.now(), run_at, mutation);
        self.persist_scheduled_job_async_cancellable(tenant_id, job, cancel_wait, check_cancel)
            .await
    }

    async fn persist_scheduled_job_async_cancellable<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        job: ScheduledJob,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<JobId>
    where
        Fut: future::Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        let job_id = job.id.clone();
        let outcome = write_scheduler_transaction_cancellable(
            self,
            tenant_id,
            cancel_wait,
            check_cancel,
            move |transaction| {
                transaction.insert_scheduled_job(&job)?;
                Ok(job_id.clone())
            },
        )
        .await?;
        let job_id = committed_or_cancelled(outcome)?;
        self.wake_scheduler();
        Ok(job_id)
    }

    /// Claims a bounded batch of due scheduled jobs for execution.
    pub fn claim_due_jobs(
        &self,
        tenant_id: &TenantId,
        now: Timestamp,
    ) -> Result<Vec<ScheduledJob>> {
        with_scheduler_runtime(self, tenant_id, move |runtime| {
            runtime
                .store
                .claim_due_jobs(now, SCHEDULED_JOB_CLAIM_BATCH_LIMIT)
        })
    }

    /// Claims a bounded batch of due scheduled jobs for execution asynchronously.
    pub async fn claim_due_jobs_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        now: Timestamp,
    ) -> Result<Vec<ScheduledJob>> {
        write_scheduler_transaction(self, tenant_id, move |transaction| {
            transaction.claim_due_jobs(now, SCHEDULED_JOB_CLAIM_BATCH_LIMIT)
        })
        .await
    }

    /// Marks a claimed scheduled job as complete.
    pub fn complete_scheduled_job(&self, tenant_id: &TenantId, job_id: &JobId) -> Result<()> {
        self.storage_fault_injector
            .check(FaultPoint::ScheduledJobCompleteBeforeWrite)?;
        with_scheduler_runtime(self, tenant_id, move |runtime| {
            runtime.store.complete_scheduled_job(job_id)
        })
    }

    /// Marks a claimed scheduled job as complete asynchronously.
    pub async fn complete_scheduled_job_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        job_id: JobId,
    ) -> Result<()> {
        self.storage_fault_injector
            .check(FaultPoint::ScheduledJobCompleteBeforeWrite)?;
        write_scheduler_transaction(self, tenant_id, move |transaction| {
            transaction.complete_scheduled_job(&job_id)
        })
        .await
    }

    /// Cancels a pending scheduled job before it begins executing.
    pub fn cancel_scheduled_job(&self, tenant_id: &TenantId, job_id: &JobId) -> Result<()> {
        let removed = with_scheduler_runtime(self, tenant_id, move |runtime| {
            runtime.store.cancel_scheduled_job(job_id)
        })?;
        scheduled_job_removed_or_not_found(removed, job_id.clone())
    }

    /// Cancels a pending scheduled job before it begins executing asynchronously.
    pub async fn cancel_scheduled_job_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        job_id: JobId,
    ) -> Result<()> {
        self.cancel_scheduled_job_async_cancellable(tenant_id, job_id, future::pending(), || Ok(()))
            .await
    }

    /// Cancels a pending scheduled job before it begins executing asynchronously with cooperative cancellation.
    pub async fn cancel_scheduled_job_async_cancellable<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        job_id: JobId,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<()>
    where
        Fut: future::Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        let job_id_for_cancel = job_id.clone();
        let outcome = write_scheduler_transaction_cancellable(
            self,
            tenant_id,
            cancel_wait,
            check_cancel,
            move |transaction| transaction.cancel_scheduled_job(&job_id_for_cancel),
        )
        .await?;
        let removed = committed_or_cancelled(outcome)?;
        scheduled_job_removed_or_not_found(removed, job_id)
    }

    /// Persists the final result for an executed scheduled job.
    #[cfg(test)]
    pub(crate) fn record_scheduled_job_result(
        &self,
        tenant_id: &TenantId,
        result: &ScheduledJobResult,
    ) -> Result<()> {
        self.storage_fault_injector
            .check(FaultPoint::ScheduledJobRecordResultBeforeWrite)?;
        with_scheduler_runtime(self, tenant_id, move |runtime| {
            runtime.store.record_scheduled_job_result(result)
        })
    }

    /// Persists the final result for an executed scheduled job asynchronously.
    pub(crate) async fn record_scheduled_job_result_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        result: ScheduledJobResult,
    ) -> Result<()> {
        self.storage_fault_injector
            .check(FaultPoint::ScheduledJobRecordResultBeforeWrite)?;
        write_scheduler_transaction(self, tenant_id, move |transaction| {
            transaction.record_scheduled_job_result(&result)
        })
        .await
    }

    /// Loads the final result for an executed scheduled job.
    pub fn get_scheduled_job_result(
        &self,
        tenant_id: &TenantId,
        job_id: &JobId,
    ) -> Result<ScheduledJobResult> {
        with_scheduler_runtime(self, tenant_id, move |runtime| {
            runtime
                .store
                .get_scheduled_job_result(job_id)?
                .ok_or(Error::ScheduledJobNotFound(job_id.clone()))
        })
    }

    /// Loads the final result for an executed scheduled job asynchronously.
    pub async fn get_scheduled_job_result_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        job_id: JobId,
    ) -> Result<ScheduledJobResult> {
        read_scheduler_store(self, tenant_id, move |store| {
            store
                .get_scheduled_job_result(&job_id)?
                .ok_or(Error::ScheduledJobNotFound(job_id))
        })
        .await
    }

    /// Lists all pending scheduled jobs for a tenant.
    pub fn list_scheduled_jobs(&self, tenant_id: &TenantId) -> Result<Vec<ScheduledJob>> {
        with_scheduler_runtime(self, tenant_id, move |runtime| {
            runtime.store.list_scheduled_jobs()
        })
    }

    /// Lists all pending scheduled jobs for a tenant asynchronously.
    pub async fn list_scheduled_jobs_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
    ) -> Result<Vec<ScheduledJob>> {
        read_scheduler_store(self, tenant_id, move |store| store.list_scheduled_jobs()).await
    }
}

fn scheduled_job_from_request(now: Timestamp, request: ScheduleRequest) -> ScheduledJob {
    scheduled_job_at(
        now,
        now.saturating_add_duration(Duration::from_millis(request.run_after_ms)),
        request.mutation,
    )
}

fn scheduled_job_at(created_at: Timestamp, run_at: Timestamp, mutation: Mutation) -> ScheduledJob {
    ScheduledJob {
        id: JobId::new(),
        run_at,
        mutation,
        created_at,
    }
}

fn committed_or_cancelled<T>(outcome: TenantWriteOutcome<T>) -> Result<T> {
    match outcome {
        TenantWriteOutcome::CancelledBeforeCommit => Err(Error::Cancelled),
        TenantWriteOutcome::Committed(committed) => Ok(committed.value),
    }
}

fn scheduled_job_removed_or_not_found(removed: bool, job_id: JobId) -> Result<()> {
    if removed {
        return Ok(());
    }
    Err(Error::ScheduledJobNotFound(job_id))
}
