use super::*;

impl TenantPersistence {
    delegate_store_method!(fn scheduled_execution_exists(&self, execution_id: &str) -> Result<bool>);
    delegate_store_method!(fn get_scheduled_job_result(&self, job_id: &DocumentId) -> Result<Option<ScheduledJobResult>>);
    delegate_store_method!(fn list_scheduled_jobs(&self) -> Result<Vec<ScheduledJob>>);
    delegate_store_method!(fn load_cron_jobs(&self) -> Result<Vec<CronJob>>);
    delegate_store_method!(fn next_scheduled_work_at(&self) -> Result<Option<Timestamp>>);
    delegate_store_method!(fn has_scheduled_work(&self) -> Result<bool>);
    delegate_store_method!(fn now(&self) -> Timestamp);

    pub(crate) fn insert_scheduled_job(&self, job: &ScheduledJob) -> Result<()> {
        match_tenant_persistence!(self, |store| store.insert_scheduled_job(job))
    }

    pub(crate) fn claim_due_jobs(&self, now: Timestamp) -> Result<Vec<ScheduledJob>> {
        match_tenant_persistence!(self, |store| store.claim_due_jobs(now))
    }

    pub(crate) fn complete_scheduled_job(&self, job_id: &DocumentId) -> Result<()> {
        match_tenant_persistence!(self, |store| store.complete_scheduled_job(job_id))
    }

    pub(crate) fn cancel_scheduled_job(&self, job_id: &DocumentId) -> Result<bool> {
        match_tenant_persistence!(self, |store| store.cancel_scheduled_job(job_id))
    }

    #[cfg(test)]
    pub(crate) fn record_scheduled_job_result(&self, result: &ScheduledJobResult) -> Result<()> {
        match_tenant_persistence!(self, |store| store.record_scheduled_job_result(result))
    }

    pub(crate) fn save_cron_job(&self, cron: &CronJob) -> Result<()> {
        match_tenant_persistence!(self, |store| store.save_cron_job(cron))
    }

    pub(crate) fn delete_cron_job(&self, name: &str) -> Result<()> {
        match_tenant_persistence!(self, |store| store.delete_cron_job(name))
    }

    pub(crate) fn recover_running_jobs(&self, now: Timestamp) -> Result<()> {
        match_tenant_persistence!(self, |store| store.recover_running_jobs(now))
    }
}
