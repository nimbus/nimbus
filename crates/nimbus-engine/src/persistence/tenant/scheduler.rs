use super::*;

impl TenantPersistence {
    delegate_store_method!(fn scheduled_execution_exists(&self, execution_id: &str) -> Result<bool>);
    delegate_store_method!(fn get_scheduled_job_result(&self, job_id: &DocumentId) -> Result<Option<ScheduledJobResult>>);
    delegate_store_method!(fn list_scheduled_jobs(&self) -> Result<Vec<ScheduledJob>>);
    delegate_store_method!(fn load_cron_jobs(&self) -> Result<Vec<CronJob>>);
    delegate_store_method!(fn next_scheduled_work_at(&self) -> Result<Option<Timestamp>>);
    delegate_store_method!(fn has_scheduled_work(&self) -> Result<bool>);
    delegate_store_method!(fn now(&self) -> Timestamp);

    pub(crate) fn scheduler_write_cancellable<Check>(
        &self,
        operation: SchedulerWrite,
        check_cancel: Check,
    ) -> Result<SchedulerWriteResult>
    where
        Check: Fn() -> Result<()> + Send + 'static,
    {
        match_tenant_persistence!(self, |store| {
            store.scheduler_write_cancellable(operation, check_cancel)
        })
    }

    pub(crate) fn prepare_scheduler_write(
        &self,
        operation: SchedulerWrite,
    ) -> Result<nimbus_storage::PreparedSchedulerWrite> {
        match_tenant_persistence!(self, |store| { store.prepare_scheduler_write(operation) })
    }

    pub(crate) fn reconcile_scheduler_write(
        &self,
        prepared: &nimbus_storage::PreparedSchedulerWrite,
    ) -> Result<nimbus_storage::SchedulerWriteReconciliation> {
        match_tenant_persistence!(self, |store| { store.reconcile_scheduler_write(prepared) })
    }

    pub(crate) fn prepare_schedule_batch(
        &self,
        operations: &[ResolvedScheduleOp],
    ) -> Result<nimbus_storage::PreparedScheduleBatch> {
        match_tenant_persistence!(self, |store| { store.prepare_schedule_batch(operations) })
    }

    pub(crate) fn reconcile_schedule_batch(
        &self,
        prepared: &nimbus_storage::PreparedScheduleBatch,
    ) -> Result<nimbus_storage::ScheduleBatchReconciliation> {
        match_tenant_persistence!(self, |store| { store.reconcile_schedule_batch(prepared) })
    }

    pub(crate) fn schedule_batch_cancellable<Check>(
        &self,
        operations: &[ResolvedScheduleOp],
        check_cancel: Check,
    ) -> Result<()>
    where
        Check: Fn() -> Result<()> + Send + 'static,
    {
        match_tenant_persistence!(self, |store| {
            store.schedule_batch_cancellable(operations, check_cancel)
        })
    }

    pub(crate) fn fenced_scheduler_write_cancellable<Check>(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_durable_sequence: SequenceNumber,
        operation: SchedulerWrite,
        check_cancel: Check,
    ) -> nimbus_storage::CommitterLeaseResult<SchedulerWriteResult>
    where
        Check: Fn() -> Result<()> + Send + 'static,
    {
        match_tenant_persistence!(self, |store| {
            store.fenced_scheduler_write_cancellable(
                owner_id,
                epoch,
                expected_durable_sequence,
                operation,
                check_cancel,
            )
        })
    }

    pub(crate) fn fenced_schedule_batch_cancellable<Check>(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_durable_sequence: SequenceNumber,
        operations: &[ResolvedScheduleOp],
        check_cancel: Check,
    ) -> nimbus_storage::CommitterLeaseResult<()>
    where
        Check: Fn() -> Result<()> + Send + 'static,
    {
        match_tenant_persistence!(self, |store| {
            store.fenced_schedule_batch_cancellable(
                owner_id,
                epoch,
                expected_durable_sequence,
                operations,
                check_cancel,
            )
        })
    }
}
