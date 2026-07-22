use nimbus_core::{JobId, Result, ScheduledJob, Timestamp};
use redb::{ReadableTable, TableError};

use crate::store::{
    RUNNING_SCHEDULED_JOBS, SCHEDULED_JOBS, TenantStore, TenantWriteTransaction, map_redb_error,
};

use super::codec::{
    deserialize_job, due_jobs_upper_bound, running_job_key, scheduled_job_key,
    scheduled_key_matches_job_id, serialize_job,
};

impl TenantWriteTransaction {
    pub fn insert_scheduled_job(&mut self, job: &ScheduledJob) -> Result<()> {
        self.check_cancel()?;
        insert_scheduled_job_in_write_txn(self.write_txn()?, job)
    }

    pub fn claim_due_jobs(&mut self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>> {
        self.check_cancel()?;
        if max_jobs == 0 {
            return Ok(Vec::new());
        }
        let due = {
            let table = match self.write_txn()?.open_table(SCHEDULED_JOBS) {
                Ok(table) => table,
                Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
                Err(error) => return Err(map_redb_error(error)),
            };
            let upper = due_jobs_upper_bound(now);
            let mut due = Vec::new();
            for entry in table
                .range::<&[u8]>(..=upper.as_slice())
                .map_err(map_redb_error)?
                .take(max_jobs)
            {
                self.check_cancel()?;
                let (key, value) = entry.map_err(map_redb_error)?;
                let job = deserialize_job(value.value())?;
                due.push((key.value().to_vec(), job));
            }
            due
        };

        if due.is_empty() {
            return Ok(Vec::new());
        }

        {
            let mut scheduled = self
                .write_txn()?
                .open_table(SCHEDULED_JOBS)
                .map_err(map_redb_error)?;
            let mut running = self
                .write_txn()?
                .open_table(RUNNING_SCHEDULED_JOBS)
                .map_err(map_redb_error)?;
            for (pending_key, job) in &due {
                self.check_cancel()?;
                scheduled
                    .remove(pending_key.as_slice())
                    .map_err(map_redb_error)?;
                let payload = serialize_job(job)?;
                let running_key = running_job_key(&job.id);
                running
                    .insert(running_key.as_slice(), payload.as_slice())
                    .map_err(map_redb_error)?;
            }
        }

        Ok(due.into_iter().map(|(_, job)| job).collect())
    }

    pub fn complete_scheduled_job(&mut self, job_id: &JobId) -> Result<()> {
        self.check_cancel()?;
        let mut table = match self.write_txn()?.open_table(RUNNING_SCHEDULED_JOBS) {
            Ok(table) => table,
            Err(TableError::TableDoesNotExist(_)) => return Ok(()),
            Err(error) => return Err(map_redb_error(error)),
        };
        let key = running_job_key(job_id);
        table.remove(key.as_slice()).map_err(map_redb_error)?;
        Ok(())
    }

    pub fn cancel_scheduled_job(&mut self, job_id: &JobId) -> Result<bool> {
        self.check_cancel()?;
        cancel_scheduled_job_in_write_txn(self.write_txn()?, job_id)
    }
}

impl TenantStore {
    /// Inserts a scheduled job into the pending queue.
    pub fn insert_scheduled_job(&self, job: &ScheduledJob) -> Result<()> {
        self.execute_write(move |transaction| transaction.insert_scheduled_job(job))?;
        Ok(())
    }

    /// Claims up to `max_jobs` scheduled jobs due at or before `now`.
    pub fn claim_due_jobs(&self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>> {
        Ok(self
            .execute_write(move |transaction| transaction.claim_due_jobs(now, max_jobs))?
            .value)
    }

    /// Marks a claimed scheduled job as finished.
    pub fn complete_scheduled_job(&self, job_id: &JobId) -> Result<()> {
        self.execute_write(move |transaction| transaction.complete_scheduled_job(job_id))?;
        Ok(())
    }

    /// Cancels a pending scheduled job if it has not started running yet.
    pub fn cancel_scheduled_job(&self, job_id: &JobId) -> Result<bool> {
        Ok(self
            .execute_write(move |transaction| transaction.cancel_scheduled_job(job_id))?
            .value)
    }

    /// Returns all pending scheduled jobs in due-order.
    pub fn list_scheduled_jobs(&self) -> Result<Vec<ScheduledJob>> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        let table = match read_txn.open_table(SCHEDULED_JOBS) {
            Ok(table) => table,
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(error) => return Err(map_redb_error(error)),
        };

        let mut jobs = Vec::new();
        for entry in table.iter().map_err(map_redb_error)? {
            let (_, value) = entry.map_err(map_redb_error)?;
            jobs.push(deserialize_job(value.value())?);
        }
        Ok(jobs)
    }

    /// Returns one pending job by identity. redb orders pending rows by
    /// deadline, so this lookup scans keys while provider-backed adapters use
    /// their primary-key indexes.
    pub fn get_pending_scheduled_job(&self, job_id: &JobId) -> Result<Option<ScheduledJob>> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        let table = match read_txn.open_table(SCHEDULED_JOBS) {
            Ok(table) => table,
            Err(TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(error) => return Err(map_redb_error(error)),
        };
        for entry in table.iter().map_err(map_redb_error)? {
            let (key, value) = entry.map_err(map_redb_error)?;
            if scheduled_key_matches_job_id(key.value(), job_id) {
                return deserialize_job(value.value()).map(Some);
            }
        }
        Ok(None)
    }

    /// Returns one claimed job by identity.
    pub fn get_running_scheduled_job(&self, job_id: &JobId) -> Result<Option<ScheduledJob>> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        let table = match read_txn.open_table(RUNNING_SCHEDULED_JOBS) {
            Ok(table) => table,
            Err(TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(error) => return Err(map_redb_error(error)),
        };
        let key = running_job_key(job_id);
        table
            .get(key.as_slice())
            .map_err(map_redb_error)?
            .map(|value| deserialize_job(value.value()))
            .transpose()
    }

    /// Peeks at the same bounded due prefix that `claim_due_jobs` would move.
    pub fn peek_due_scheduled_jobs(
        &self,
        now: Timestamp,
        max_jobs: usize,
    ) -> Result<Vec<ScheduledJob>> {
        if max_jobs == 0 {
            return Ok(Vec::new());
        }
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        let table = match read_txn.open_table(SCHEDULED_JOBS) {
            Ok(table) => table,
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(error) => return Err(map_redb_error(error)),
        };
        let upper = due_jobs_upper_bound(now);
        table
            .range::<&[u8]>(..=upper.as_slice())
            .map_err(map_redb_error)?
            .take(max_jobs)
            .map(|entry| {
                let (_, value) = entry.map_err(map_redb_error)?;
                deserialize_job(value.value())
            })
            .collect()
    }

    /// Returns all claimed jobs that have not yet been completed or recovered.
    pub fn list_running_scheduled_jobs(&self) -> Result<Vec<ScheduledJob>> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        let table = match read_txn.open_table(RUNNING_SCHEDULED_JOBS) {
            Ok(table) => table,
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(error) => return Err(map_redb_error(error)),
        };

        let mut jobs = Vec::new();
        for entry in table.iter().map_err(map_redb_error)? {
            let (_, value) = entry.map_err(map_redb_error)?;
            jobs.push(deserialize_job(value.value())?);
        }
        Ok(jobs)
    }
}

pub(crate) fn insert_scheduled_job_in_write_txn(
    write_txn: &redb::WriteTransaction,
    job: &ScheduledJob,
) -> Result<()> {
    let payload = serialize_job(job)?;
    let key = scheduled_job_key(job.run_at, &job.id);
    let mut table = write_txn
        .open_table(SCHEDULED_JOBS)
        .map_err(map_redb_error)?;
    table
        .insert(key.as_slice(), payload.as_slice())
        .map_err(map_redb_error)?;
    Ok(())
}

pub(crate) fn cancel_scheduled_job_in_write_txn(
    write_txn: &redb::WriteTransaction,
    job_id: &JobId,
) -> Result<bool> {
    let pending_key = {
        let table = match write_txn.open_table(SCHEDULED_JOBS) {
            Ok(table) => table,
            Err(TableError::TableDoesNotExist(_)) => return Ok(false),
            Err(error) => return Err(map_redb_error(error)),
        };

        let mut pending_key = None;
        for entry in table.iter().map_err(map_redb_error)? {
            let (key, _) = entry.map_err(map_redb_error)?;
            if scheduled_key_matches_job_id(key.value(), job_id) {
                pending_key = Some(key.value().to_vec());
                break;
            }
        }
        pending_key
    };

    let Some(pending_key) = pending_key else {
        return Ok(false);
    };

    let mut table = write_txn
        .open_table(SCHEDULED_JOBS)
        .map_err(map_redb_error)?;
    table
        .remove(pending_key.as_slice())
        .map_err(map_redb_error)?;
    Ok(true)
}
