use nimbus_core::{CronJob, DocumentId, Result, ScheduledJob, ScheduledJobResult, Timestamp};
#[cfg(any(test, feature = "test-hooks"))]
use nimbus_storage::MemoryWriteTransaction;
use nimbus_storage::{
    LibsqlReplicaWriteTransaction, MySqlWriteTransaction, PostgresWriteTransaction,
    TenantWriteTransaction as RedbWriteTransaction,
};

#[cfg(any(test, feature = "test-hooks"))]
impl TenantPersistenceWriteOps for MemoryWriteTransaction {
    fn insert_scheduled_job(&mut self, job: &ScheduledJob) -> Result<()> {
        self.insert_scheduled_job(job)
    }

    fn claim_due_jobs(&mut self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>> {
        self.claim_due_jobs(now, max_jobs)
    }

    fn complete_scheduled_job(&mut self, job_id: &DocumentId) -> Result<()> {
        self.complete_scheduled_job(job_id)
    }

    fn cancel_scheduled_job(&mut self, job_id: &DocumentId) -> Result<bool> {
        self.cancel_scheduled_job(job_id)
    }

    fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()> {
        self.record_scheduled_job_result(result)
    }

    fn save_cron_job(&mut self, cron: &CronJob) -> Result<()> {
        self.save_cron_job(cron)
    }

    fn delete_cron_job(&mut self, name: &str) -> Result<()> {
        self.delete_cron_job(name)
    }
}

pub(crate) trait TenantPersistenceWriteOps {
    fn insert_scheduled_job(&mut self, job: &ScheduledJob) -> Result<()>;
    fn claim_due_jobs(&mut self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>>;
    fn complete_scheduled_job(&mut self, job_id: &DocumentId) -> Result<()>;
    fn cancel_scheduled_job(&mut self, job_id: &DocumentId) -> Result<bool>;
    fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()>;
    fn save_cron_job(&mut self, cron: &CronJob) -> Result<()>;
    fn delete_cron_job(&mut self, name: &str) -> Result<()>;
}

impl TenantPersistenceWriteOps for RedbWriteTransaction {
    fn insert_scheduled_job(&mut self, job: &ScheduledJob) -> Result<()> {
        self.insert_scheduled_job(job)
    }

    fn claim_due_jobs(&mut self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>> {
        self.claim_due_jobs(now, max_jobs)
    }

    fn complete_scheduled_job(&mut self, job_id: &DocumentId) -> Result<()> {
        self.complete_scheduled_job(job_id)
    }

    fn cancel_scheduled_job(&mut self, job_id: &DocumentId) -> Result<bool> {
        self.cancel_scheduled_job(job_id)
    }

    fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()> {
        self.record_scheduled_job_result(result)
    }

    fn save_cron_job(&mut self, cron: &CronJob) -> Result<()> {
        self.save_cron_job(cron)
    }

    fn delete_cron_job(&mut self, name: &str) -> Result<()> {
        self.delete_cron_job(name)
    }
}

impl TenantPersistenceWriteOps for nimbus_storage::SqliteWriteTransaction {
    fn insert_scheduled_job(&mut self, job: &ScheduledJob) -> Result<()> {
        self.insert_scheduled_job(job)
    }

    fn claim_due_jobs(&mut self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>> {
        self.claim_due_jobs(now, max_jobs)
    }

    fn complete_scheduled_job(&mut self, job_id: &DocumentId) -> Result<()> {
        self.complete_scheduled_job(job_id)
    }

    fn cancel_scheduled_job(&mut self, job_id: &DocumentId) -> Result<bool> {
        self.cancel_scheduled_job(job_id)
    }

    fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()> {
        self.record_scheduled_job_result(result)
    }

    fn save_cron_job(&mut self, cron: &CronJob) -> Result<()> {
        self.save_cron_job(cron)
    }

    fn delete_cron_job(&mut self, name: &str) -> Result<()> {
        self.delete_cron_job(name)
    }
}

impl TenantPersistenceWriteOps for PostgresWriteTransaction {
    fn insert_scheduled_job(&mut self, job: &ScheduledJob) -> Result<()> {
        self.insert_scheduled_job(job)
    }

    fn claim_due_jobs(&mut self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>> {
        self.claim_due_jobs(now, max_jobs)
    }

    fn complete_scheduled_job(&mut self, job_id: &DocumentId) -> Result<()> {
        self.complete_scheduled_job(job_id)
    }

    fn cancel_scheduled_job(&mut self, job_id: &DocumentId) -> Result<bool> {
        self.cancel_scheduled_job(job_id)
    }

    fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()> {
        self.record_scheduled_job_result(result)
    }

    fn save_cron_job(&mut self, cron: &CronJob) -> Result<()> {
        self.save_cron_job(cron)
    }

    fn delete_cron_job(&mut self, name: &str) -> Result<()> {
        self.delete_cron_job(name)
    }
}

impl TenantPersistenceWriteOps for LibsqlReplicaWriteTransaction {
    fn insert_scheduled_job(&mut self, job: &ScheduledJob) -> Result<()> {
        self.insert_scheduled_job(job)
    }

    fn claim_due_jobs(&mut self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>> {
        self.claim_due_jobs(now, max_jobs)
    }

    fn complete_scheduled_job(&mut self, job_id: &DocumentId) -> Result<()> {
        self.complete_scheduled_job(job_id)
    }

    fn cancel_scheduled_job(&mut self, job_id: &DocumentId) -> Result<bool> {
        self.cancel_scheduled_job(job_id)
    }

    fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()> {
        self.record_scheduled_job_result(result)
    }

    fn save_cron_job(&mut self, cron: &CronJob) -> Result<()> {
        self.save_cron_job(cron)
    }

    fn delete_cron_job(&mut self, name: &str) -> Result<()> {
        self.delete_cron_job(name)
    }
}

impl TenantPersistenceWriteOps for MySqlWriteTransaction {
    fn insert_scheduled_job(&mut self, job: &ScheduledJob) -> Result<()> {
        self.insert_scheduled_job(job)
    }

    fn claim_due_jobs(&mut self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>> {
        self.claim_due_jobs(now, max_jobs)
    }

    fn complete_scheduled_job(&mut self, job_id: &DocumentId) -> Result<()> {
        self.complete_scheduled_job(job_id)
    }

    fn cancel_scheduled_job(&mut self, job_id: &DocumentId) -> Result<bool> {
        self.cancel_scheduled_job(job_id)
    }

    fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()> {
        self.record_scheduled_job_result(result)
    }

    fn save_cron_job(&mut self, cron: &CronJob) -> Result<()> {
        self.save_cron_job(cron)
    }

    fn delete_cron_job(&mut self, name: &str) -> Result<()> {
        self.delete_cron_job(name)
    }
}
