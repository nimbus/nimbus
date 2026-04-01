use std::sync::Arc;

use super::Service;
use neovex_core::{
    CreateCronRequest, CronJob, Error, JobId, Result, ScheduleRequest, ScheduledJob,
    ScheduledJobResult, TenantId, Timestamp,
};

impl Service {
    /// Schedules a mutation to execute in the future.
    pub fn schedule_mutation(
        &self,
        tenant_id: &TenantId,
        request: ScheduleRequest,
    ) -> Result<JobId> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let now = self.now();
        let job = ScheduledJob {
            id: JobId::new(),
            run_at: Timestamp(now.0.saturating_add(request.run_after_ms)),
            mutation: request.mutation,
            created_at: now,
        };
        runtime.store.insert_scheduled_job(&job)?;
        self.wake_scheduler();
        Ok(job.id)
    }

    /// Schedules a mutation to execute in the future asynchronously.
    pub async fn schedule_mutation_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        request: ScheduleRequest,
    ) -> Result<JobId> {
        self.call_blocking(move |service| service.schedule_mutation(&tenant_id, request))
            .await
    }

    /// Claims all due scheduled jobs for execution.
    pub fn claim_due_jobs(
        &self,
        tenant_id: &TenantId,
        now: Timestamp,
    ) -> Result<Vec<ScheduledJob>> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.claim_due_jobs(now)
    }

    /// Claims all due scheduled jobs for execution asynchronously.
    pub async fn claim_due_jobs_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        now: Timestamp,
    ) -> Result<Vec<ScheduledJob>> {
        self.call_blocking(move |service| service.claim_due_jobs(&tenant_id, now))
            .await
    }

    /// Marks a claimed scheduled job as complete.
    pub fn complete_scheduled_job(&self, tenant_id: &TenantId, job_id: &JobId) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.complete_scheduled_job(job_id)
    }

    /// Marks a claimed scheduled job as complete asynchronously.
    pub async fn complete_scheduled_job_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        job_id: JobId,
    ) -> Result<()> {
        self.call_blocking(move |service| service.complete_scheduled_job(&tenant_id, &job_id))
            .await
    }

    /// Cancels a pending scheduled job before it begins executing.
    pub fn cancel_scheduled_job(&self, tenant_id: &TenantId, job_id: &JobId) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        if runtime.store.cancel_scheduled_job(job_id)? {
            return Ok(());
        }
        Err(Error::ScheduledJobNotFound(*job_id))
    }

    /// Cancels a pending scheduled job before it begins executing asynchronously.
    pub async fn cancel_scheduled_job_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        job_id: JobId,
    ) -> Result<()> {
        self.call_blocking(move |service| service.cancel_scheduled_job(&tenant_id, &job_id))
            .await
    }

    /// Persists the final result for an executed scheduled job.
    pub(crate) fn record_scheduled_job_result(
        &self,
        tenant_id: &TenantId,
        result: &ScheduledJobResult,
    ) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.record_scheduled_job_result(result)
    }

    /// Persists the final result for an executed scheduled job asynchronously.
    pub(crate) async fn record_scheduled_job_result_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        result: ScheduledJobResult,
    ) -> Result<()> {
        self.call_blocking(move |service| service.record_scheduled_job_result(&tenant_id, &result))
            .await
    }

    /// Loads the final result for an executed scheduled job.
    pub fn get_scheduled_job_result(
        &self,
        tenant_id: &TenantId,
        job_id: &JobId,
    ) -> Result<ScheduledJobResult> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime
            .store
            .get_scheduled_job_result(job_id)?
            .ok_or(Error::ScheduledJobNotFound(*job_id))
    }

    /// Loads the final result for an executed scheduled job asynchronously.
    pub async fn get_scheduled_job_result_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        job_id: JobId,
    ) -> Result<ScheduledJobResult> {
        self.call_blocking(move |service| service.get_scheduled_job_result(&tenant_id, &job_id))
            .await
    }

    /// Lists all pending scheduled jobs for a tenant.
    pub fn list_scheduled_jobs(&self, tenant_id: &TenantId) -> Result<Vec<ScheduledJob>> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.list_scheduled_jobs()
    }

    /// Lists all pending scheduled jobs for a tenant asynchronously.
    pub async fn list_scheduled_jobs_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
    ) -> Result<Vec<ScheduledJob>> {
        self.call_blocking(move |service| service.list_scheduled_jobs(&tenant_id))
            .await
    }

    /// Creates a new cron job definition.
    pub fn create_cron_job(&self, tenant_id: &TenantId, request: CreateCronRequest) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let existing = runtime.store.load_cron_jobs()?;
        if existing.iter().any(|cron| cron.name == request.name) {
            return Err(Error::AlreadyExists(format!(
                "cron job '{}' already exists",
                request.name
            )));
        }

        let now = self.now();
        let next_run = request.schedule.next_after(now);
        let cron = CronJob {
            name: request.name,
            schedule: request.schedule,
            mutation: request.mutation,
            enabled: true,
            last_run: None,
            next_run,
            created_at: now,
        };
        runtime.store.save_cron_job(&cron)?;
        self.wake_scheduler();
        Ok(())
    }

    /// Creates a new cron job definition asynchronously.
    pub async fn create_cron_job_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        request: CreateCronRequest,
    ) -> Result<()> {
        self.call_blocking(move |service| service.create_cron_job(&tenant_id, request))
            .await
    }

    /// Loads cron jobs for a tenant.
    pub fn load_cron_jobs(&self, tenant_id: &TenantId) -> Result<Vec<CronJob>> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.load_cron_jobs()
    }

    /// Loads cron jobs for a tenant asynchronously.
    pub async fn load_cron_jobs_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
    ) -> Result<Vec<CronJob>> {
        self.call_blocking(move |service| service.load_cron_jobs(&tenant_id))
            .await
    }

    /// Persists an updated cron job definition.
    pub fn update_cron_job(&self, tenant_id: &TenantId, cron: &CronJob) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.save_cron_job(cron)
    }

    /// Persists an updated cron job definition asynchronously.
    pub async fn update_cron_job_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        cron: CronJob,
    ) -> Result<()> {
        self.call_blocking(move |service| service.update_cron_job(&tenant_id, &cron))
            .await
    }

    /// Lists cron jobs for a tenant.
    pub fn list_cron_jobs(&self, tenant_id: &TenantId) -> Result<Vec<CronJob>> {
        self.load_cron_jobs(tenant_id)
    }

    /// Lists cron jobs for a tenant asynchronously.
    pub async fn list_cron_jobs_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
    ) -> Result<Vec<CronJob>> {
        self.call_blocking(move |service| service.list_cron_jobs(&tenant_id))
            .await
    }

    /// Deletes a cron job definition if present.
    pub fn delete_cron_job(&self, tenant_id: &TenantId, name: &str) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.delete_cron_job(name)
    }

    /// Deletes a cron job definition if present asynchronously.
    pub async fn delete_cron_job_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        name: String,
    ) -> Result<()> {
        self.call_blocking(move |service| service.delete_cron_job(&tenant_id, &name))
            .await
    }

    /// Returns the IDs for all tenants currently loaded in memory.
    pub fn loaded_tenant_ids(&self) -> Vec<TenantId> {
        let mut tenant_ids = self
            .tenants
            .read()
            .expect("tenant registry lock should not be poisoned")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        tenant_ids.sort();
        tenant_ids
    }

    /// Returns the earliest due scheduled or cron work for a loaded tenant.
    pub(crate) fn next_scheduled_work_at(&self, tenant_id: &TenantId) -> Result<Option<Timestamp>> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.next_scheduled_work_at()
    }

    /// Returns the earliest due scheduled or cron work across all loaded tenants.
    pub(crate) fn next_loaded_scheduled_work_at(&self) -> Result<Option<Timestamp>> {
        let mut next_due: Option<Timestamp> = None;
        for tenant_id in self.loaded_tenant_ids() {
            let Some(candidate) = self.next_scheduled_work_at(&tenant_id)? else {
                continue;
            };
            next_due = Some(match next_due {
                Some(current) => current.min(candidate),
                None => candidate,
            });
        }
        Ok(next_due)
    }

    /// Returns the earliest due scheduled or cron work across all loaded tenants asynchronously.
    pub(crate) async fn next_loaded_scheduled_work_at_async(
        self: &Arc<Self>,
    ) -> Result<Option<Timestamp>> {
        self.call_blocking(move |service| service.next_loaded_scheduled_work_at())
            .await
    }

    /// Loads tenants that have scheduled work and recovers orphaned running jobs.
    pub fn load_tenants_with_scheduled_work(&self) -> Result<()> {
        let entries = std::fs::read_dir(&self.data_dir)
            .map_err(|error| Error::Internal(error.to_string()))?;
        let now = self.now();

        for entry in entries {
            let entry = entry.map_err(|error| Error::Internal(error.to_string()))?;
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "redb") {
                continue;
            }

            let stem = path.file_stem().ok_or_else(|| {
                Error::Internal(format!(
                    "tenant database path missing file stem: {}",
                    path.display()
                ))
            })?;
            let tenant_id = TenantId::new(stem.to_string_lossy().to_string())?;
            let store = self.open_tenant_store(&path)?;
            let has_scheduled_work = store.has_scheduled_work()?;
            drop(store);
            if !has_scheduled_work {
                continue;
            }

            let runtime = self.get_existing_tenant(&tenant_id)?;
            let _operation = runtime.enter_operation(&tenant_id)?;
            runtime.store.recover_running_jobs(now)?;
        }

        Ok(())
    }
}
