use nimbus_core::{
    CronJob, DocumentId, Error, Result, ScheduledJob, ScheduledJobResult, Timestamp,
};

use crate::ResolvedScheduleOp;

use super::state::MemoryState;
use super::{MemoryTenantStore, MemoryWriteTransaction};

impl MemoryState {
    pub(super) fn apply_schedule_ops(&mut self, operations: &[ResolvedScheduleOp]) -> Result<()> {
        for operation in operations {
            match operation {
                ResolvedScheduleOp::Insert { job } => {
                    self.scheduled_jobs
                        .insert((job.run_at, job.id.clone()), job.clone());
                }
                ResolvedScheduleOp::Cancel { job_id } => {
                    let key = self
                        .scheduled_jobs
                        .keys()
                        .find(|(_, id)| id == job_id)
                        .cloned()
                        .ok_or_else(|| Error::ScheduledJobNotFound(job_id.clone()))?;
                    self.scheduled_jobs.remove(&key);
                }
            }
        }
        Ok(())
    }
}

impl MemoryWriteTransaction {
    pub fn insert_scheduled_job(&mut self, job: &ScheduledJob) -> Result<()> {
        self.check_cancel()?;
        self.state
            .scheduled_jobs
            .insert((job.run_at, job.id.clone()), job.clone());
        Ok(())
    }

    pub fn claim_due_jobs(&mut self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>> {
        self.check_cancel()?;
        if max_jobs == 0 {
            return Ok(Vec::new());
        }
        let keys = self
            .state
            .scheduled_jobs
            .iter()
            .take_while(|((run_at, _), _)| *run_at <= now)
            .take(max_jobs)
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        let mut jobs = Vec::with_capacity(keys.len());
        for key in keys {
            self.check_cancel()?;
            if let Some(job) = self.state.scheduled_jobs.remove(&key) {
                self.state.running_jobs.insert(job.id.clone(), job.clone());
                jobs.push(job);
            }
        }
        Ok(jobs)
    }

    pub fn complete_scheduled_job(&mut self, job_id: &DocumentId) -> Result<()> {
        self.check_cancel()?;
        self.state.running_jobs.remove(job_id);
        Ok(())
    }

    pub fn cancel_scheduled_job(&mut self, job_id: &DocumentId) -> Result<bool> {
        self.check_cancel()?;
        let key = self
            .state
            .scheduled_jobs
            .keys()
            .find(|(_, id)| id == job_id)
            .cloned();
        Ok(key
            .and_then(|key| self.state.scheduled_jobs.remove(&key))
            .is_some())
    }

    pub fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()> {
        self.check_cancel()?;
        self.state
            .scheduled_job_results
            .insert(result.id.clone(), result.clone());
        Ok(())
    }

    pub fn save_cron_job(&mut self, cron: &CronJob) -> Result<()> {
        self.check_cancel()?;
        self.state.cron_jobs.insert(cron.name.clone(), cron.clone());
        Ok(())
    }

    pub fn delete_cron_job(&mut self, name: &str) -> Result<()> {
        self.check_cancel()?;
        self.state.cron_jobs.remove(name);
        Ok(())
    }

    pub fn recover_running_jobs(&mut self, now: Timestamp) -> Result<()> {
        self.check_cancel()?;
        let running = std::mem::take(&mut self.state.running_jobs);
        for (_, mut job) in running {
            self.check_cancel()?;
            job.run_at = job.run_at.min(now);
            self.state
                .scheduled_jobs
                .insert((job.run_at, job.id.clone()), job);
        }
        Ok(())
    }
}

impl MemoryTenantStore {
    pub fn scheduled_execution_exists(&self, execution_id: &str) -> Result<bool> {
        Ok(self
            .read_state()?
            .scheduled_execution_ids
            .contains(execution_id))
    }

    pub fn insert_scheduled_job(&self, job: &ScheduledJob) -> Result<()> {
        self.execute_write(|transaction| transaction.insert_scheduled_job(job))?;
        Ok(())
    }

    pub fn claim_due_jobs(&self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>> {
        Ok(self
            .execute_write(|transaction| transaction.claim_due_jobs(now, max_jobs))?
            .value)
    }

    pub fn complete_scheduled_job(&self, job_id: &DocumentId) -> Result<()> {
        self.execute_write(|transaction| transaction.complete_scheduled_job(job_id))?;
        Ok(())
    }

    pub fn cancel_scheduled_job(&self, job_id: &DocumentId) -> Result<bool> {
        Ok(self
            .execute_write(|transaction| transaction.cancel_scheduled_job(job_id))?
            .value)
    }

    pub fn record_scheduled_job_result(&self, result: &ScheduledJobResult) -> Result<()> {
        self.execute_write(|transaction| transaction.record_scheduled_job_result(result))?;
        Ok(())
    }

    pub fn get_scheduled_job_result(
        &self,
        job_id: &DocumentId,
    ) -> Result<Option<ScheduledJobResult>> {
        Ok(self
            .read_state()?
            .scheduled_job_results
            .get(job_id)
            .cloned())
    }

    pub fn list_scheduled_jobs(&self) -> Result<Vec<ScheduledJob>> {
        Ok(self
            .read_state()?
            .scheduled_jobs
            .values()
            .cloned()
            .collect())
    }

    pub fn save_cron_job(&self, cron: &CronJob) -> Result<()> {
        self.execute_write(|transaction| transaction.save_cron_job(cron))?;
        Ok(())
    }

    pub fn delete_cron_job(&self, name: &str) -> Result<()> {
        self.execute_write(|transaction| transaction.delete_cron_job(name))?;
        Ok(())
    }

    pub fn load_cron_jobs(&self) -> Result<Vec<CronJob>> {
        Ok(self.read_state()?.cron_jobs.values().cloned().collect())
    }

    pub fn next_scheduled_work_at(&self) -> Result<Option<Timestamp>> {
        let state = self.read_state()?;
        let pending = state.scheduled_jobs.first_key_value().map(|(key, _)| key.0);
        let cron = state
            .cron_jobs
            .values()
            .filter(|cron| cron.enabled)
            .map(|cron| cron.next_run)
            .min();
        Ok(match (pending, cron) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (Some(timestamp), None) | (None, Some(timestamp)) => Some(timestamp),
            (None, None) => None,
        })
    }

    pub fn has_scheduled_work(&self) -> Result<bool> {
        let state = self.read_state()?;
        Ok(!state.scheduled_jobs.is_empty()
            || !state.running_jobs.is_empty()
            || !state.cron_jobs.is_empty())
    }

    pub fn recover_running_jobs(&self, now: Timestamp) -> Result<()> {
        self.execute_write(move |transaction| transaction.recover_running_jobs(now))?;
        Ok(())
    }
}
