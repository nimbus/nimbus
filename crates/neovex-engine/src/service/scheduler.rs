use neovex_core::{
    CreateCronRequest, CronJob, Error, JobId, Result, ScheduleRequest, ScheduledJob,
    ScheduledJobResult, TenantId, Timestamp,
};
use neovex_storage::TenantStore;

use super::Service;

impl Service {
    /// Schedules a mutation to execute in the future.
    pub fn schedule_mutation(
        &self,
        tenant_id: &TenantId,
        request: ScheduleRequest,
    ) -> Result<JobId> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let now = Timestamp::now();
        let job = ScheduledJob {
            id: JobId::new(),
            run_at: Timestamp(now.0.saturating_add(request.run_after_ms)),
            mutation: request.mutation,
            created_at: now,
        };
        runtime.store.insert_scheduled_job(&job)?;
        Ok(job.id)
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

    /// Marks a claimed scheduled job as complete.
    pub fn complete_scheduled_job(&self, tenant_id: &TenantId, job_id: &JobId) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.complete_scheduled_job(job_id)
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

    /// Lists all pending scheduled jobs for a tenant.
    pub fn list_scheduled_jobs(&self, tenant_id: &TenantId) -> Result<Vec<ScheduledJob>> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.list_scheduled_jobs()
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

        let now = Timestamp::now();
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
        runtime.store.save_cron_job(&cron)
    }

    /// Loads cron jobs for a tenant.
    pub fn load_cron_jobs(&self, tenant_id: &TenantId) -> Result<Vec<CronJob>> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.load_cron_jobs()
    }

    /// Persists an updated cron job definition.
    pub fn update_cron_job(&self, tenant_id: &TenantId, cron: &CronJob) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.save_cron_job(cron)
    }

    /// Lists cron jobs for a tenant.
    pub fn list_cron_jobs(&self, tenant_id: &TenantId) -> Result<Vec<CronJob>> {
        self.load_cron_jobs(tenant_id)
    }

    /// Deletes a cron job definition if present.
    pub fn delete_cron_job(&self, tenant_id: &TenantId, name: &str) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.delete_cron_job(name)
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

    /// Loads tenants that have scheduled work and recovers orphaned running jobs.
    pub fn load_tenants_with_scheduled_work(&self) -> Result<()> {
        let entries = std::fs::read_dir(&self.data_dir)
            .map_err(|error| Error::Internal(error.to_string()))?;
        let now = Timestamp::now();

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
            let store = TenantStore::open(&path)?;
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
