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
        match self {
            Self::Redb(store) => store.insert_scheduled_job(job),
            Self::Sqlite(store) => store.insert_scheduled_job(job),
            Self::LibsqlReplica(store) => store.insert_scheduled_job(job),
            Self::Postgres(store) => store.insert_scheduled_job(job),
            Self::MySql(store) => store.insert_scheduled_job(job),
        }
    }

    pub(crate) fn claim_due_jobs(&self, now: Timestamp) -> Result<Vec<ScheduledJob>> {
        match self {
            Self::Redb(store) => store.claim_due_jobs(now),
            Self::Sqlite(store) => store.claim_due_jobs(now),
            Self::LibsqlReplica(store) => store.claim_due_jobs(now),
            Self::Postgres(store) => store.claim_due_jobs(now),
            Self::MySql(store) => store.claim_due_jobs(now),
        }
    }

    pub(crate) fn complete_scheduled_job(&self, job_id: &DocumentId) -> Result<()> {
        match self {
            Self::Redb(store) => store.complete_scheduled_job(job_id),
            Self::Sqlite(store) => store.complete_scheduled_job(job_id),
            Self::LibsqlReplica(store) => store.complete_scheduled_job(job_id),
            Self::Postgres(store) => store.complete_scheduled_job(job_id),
            Self::MySql(store) => store.complete_scheduled_job(job_id),
        }
    }

    pub(crate) fn cancel_scheduled_job(&self, job_id: &DocumentId) -> Result<bool> {
        match self {
            Self::Redb(store) => store.cancel_scheduled_job(job_id),
            Self::Sqlite(store) => store.cancel_scheduled_job(job_id),
            Self::LibsqlReplica(store) => store.cancel_scheduled_job(job_id),
            Self::Postgres(store) => store.cancel_scheduled_job(job_id),
            Self::MySql(store) => store.cancel_scheduled_job(job_id),
        }
    }

    #[cfg(test)]
    pub(crate) fn record_scheduled_job_result(&self, result: &ScheduledJobResult) -> Result<()> {
        match self {
            Self::Redb(store) => store.record_scheduled_job_result(result),
            Self::Sqlite(store) => store.record_scheduled_job_result(result),
            Self::LibsqlReplica(store) => store.record_scheduled_job_result(result),
            Self::Postgres(store) => store.record_scheduled_job_result(result),
            Self::MySql(store) => store.record_scheduled_job_result(result),
        }
    }

    pub(crate) fn save_cron_job(&self, cron: &CronJob) -> Result<()> {
        match self {
            Self::Redb(store) => store.save_cron_job(cron),
            Self::Sqlite(store) => store.save_cron_job(cron),
            Self::LibsqlReplica(store) => store.save_cron_job(cron),
            Self::Postgres(store) => store.save_cron_job(cron),
            Self::MySql(store) => store.save_cron_job(cron),
        }
    }

    pub(crate) fn delete_cron_job(&self, name: &str) -> Result<()> {
        match self {
            Self::Redb(store) => store.delete_cron_job(name),
            Self::Sqlite(store) => store.delete_cron_job(name),
            Self::LibsqlReplica(store) => store.delete_cron_job(name),
            Self::Postgres(store) => store.delete_cron_job(name),
            Self::MySql(store) => store.delete_cron_job(name),
        }
    }

    pub(crate) fn recover_running_jobs(&self, now: Timestamp) -> Result<()> {
        match self {
            Self::Redb(store) => store.recover_running_jobs(now),
            Self::Sqlite(store) => store.recover_running_jobs(now),
            Self::LibsqlReplica(store) => store.recover_running_jobs(now),
            Self::Postgres(store) => store.recover_running_jobs(now),
            Self::MySql(store) => store.recover_running_jobs(now),
        }
    }
}
