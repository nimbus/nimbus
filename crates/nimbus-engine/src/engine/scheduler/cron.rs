use std::sync::Arc;

use nimbus_core::{CreateCronRequest, CronJob, Error, Result, TenantId, Timestamp};
use nimbus_storage::SchedulerWrite;

use super::super::Engine;
use super::access::{
    expect_scheduler_unit, read_scheduler_store, with_scheduler_runtime, write_scheduler_state,
    write_scheduler_state_blocking,
};

impl Engine {
    /// Creates a new cron job definition.
    pub fn create_cron_job(&self, tenant_id: &TenantId, request: CreateCronRequest) -> Result<()> {
        ensure_cron_name_available(
            &with_scheduler_runtime(self, tenant_id, move |runtime| {
                runtime.store.load_cron_jobs()
            })?,
            &request.name,
        )?;

        let cron = cron_job_from_request(self.now(), request);
        expect_scheduler_unit(write_scheduler_state_blocking(
            self,
            tenant_id,
            SchedulerWrite::SaveCron(cron),
        )?)?;
        self.wake_scheduler();
        Ok(())
    }

    /// Creates a new cron job definition asynchronously.
    pub async fn create_cron_job_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        request: CreateCronRequest,
    ) -> Result<()> {
        ensure_cron_name_available(
            &read_scheduler_store(self, tenant_id.clone(), move |store| store.load_cron_jobs())
                .await?,
            &request.name,
        )?;

        let cron = cron_job_from_request(self.now(), request);
        expect_scheduler_unit(
            write_scheduler_state(self, tenant_id, SchedulerWrite::SaveCron(cron)).await?,
        )?;
        self.wake_scheduler();
        Ok(())
    }

    /// Loads cron jobs for a tenant.
    pub fn load_cron_jobs(&self, tenant_id: &TenantId) -> Result<Vec<CronJob>> {
        with_scheduler_runtime(self, tenant_id, move |runtime| {
            runtime.store.load_cron_jobs()
        })
    }

    /// Loads cron jobs for a tenant asynchronously.
    pub async fn load_cron_jobs_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
    ) -> Result<Vec<CronJob>> {
        read_scheduler_store(self, tenant_id, move |store| store.load_cron_jobs()).await
    }

    /// Persists an updated cron job definition.
    pub fn update_cron_job(&self, tenant_id: &TenantId, cron: &CronJob) -> Result<()> {
        expect_scheduler_unit(write_scheduler_state_blocking(
            self,
            tenant_id,
            SchedulerWrite::SaveCron(cron.clone()),
        )?)
    }

    /// Persists an updated cron job definition asynchronously.
    pub async fn update_cron_job_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        cron: CronJob,
    ) -> Result<()> {
        expect_scheduler_unit(
            write_scheduler_state(self, tenant_id, SchedulerWrite::SaveCron(cron)).await?,
        )
    }

    /// Deletes a cron job definition if present.
    pub fn delete_cron_job(&self, tenant_id: &TenantId, name: &str) -> Result<()> {
        expect_scheduler_unit(write_scheduler_state_blocking(
            self,
            tenant_id,
            SchedulerWrite::DeleteCron(name.to_string()),
        )?)
    }

    /// Deletes a cron job definition if present asynchronously.
    pub async fn delete_cron_job_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        name: String,
    ) -> Result<()> {
        expect_scheduler_unit(
            write_scheduler_state(self, tenant_id, SchedulerWrite::DeleteCron(name)).await?,
        )
    }
}

fn ensure_cron_name_available(existing: &[CronJob], requested_name: &str) -> Result<()> {
    if existing.iter().any(|cron| cron.name == requested_name) {
        return Err(Error::AlreadyExists(format!(
            "cron job '{}' already exists",
            requested_name
        )));
    }
    Ok(())
}

fn cron_job_from_request(now: Timestamp, request: CreateCronRequest) -> CronJob {
    let next_run = request.schedule.next_after(now);
    CronJob {
        name: request.name,
        schedule: request.schedule,
        mutation: request.mutation,
        enabled: true,
        last_run: None,
        next_run,
        created_at: now,
    }
}
