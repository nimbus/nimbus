use neovex_core::{CronJob, Error, JobId, Result, ScheduledJob, ScheduledJobResult, Timestamp};
use redb::{ReadableTable, TableError};

use crate::store::{
    CRON_JOBS, RUNNING_SCHEDULED_JOBS, SCHEDULED_JOB_RESULTS, SCHEDULED_JOBS, TenantStore,
    map_redb_error,
};

impl TenantStore {
    /// Returns the earliest due timestamp across pending scheduled jobs and enabled cron jobs.
    pub fn next_scheduled_work_at(&self) -> Result<Option<Timestamp>> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        let next_job_at = next_pending_scheduled_job_at(&read_txn)?;
        let next_cron_at = next_enabled_cron_run_at(&read_txn)?;

        Ok(match (next_job_at, next_cron_at) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        })
    }

    /// Inserts a scheduled job into the pending queue.
    pub fn insert_scheduled_job(&self, job: &ScheduledJob) -> Result<()> {
        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        insert_scheduled_job_in_write_txn(&write_txn, job)?;
        self.commit_write_txn(write_txn)?;
        Ok(())
    }

    /// Claims all scheduled jobs due at or before `now`.
    pub fn claim_due_jobs(&self, now: Timestamp) -> Result<Vec<ScheduledJob>> {
        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        let due = {
            let table = match write_txn.open_table(SCHEDULED_JOBS) {
                Ok(table) => table,
                Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
                Err(error) => return Err(map_redb_error(error)),
            };
            let upper = due_jobs_upper_bound(now);
            let mut due = Vec::new();
            for entry in table
                .range::<&[u8]>(..=upper.as_slice())
                .map_err(map_redb_error)?
            {
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
            let mut scheduled = write_txn
                .open_table(SCHEDULED_JOBS)
                .map_err(map_redb_error)?;
            let mut running = write_txn
                .open_table(RUNNING_SCHEDULED_JOBS)
                .map_err(map_redb_error)?;
            for (pending_key, job) in &due {
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

        self.commit_write_txn(write_txn)?;
        Ok(due.into_iter().map(|(_, job)| job).collect())
    }

    /// Marks a claimed scheduled job as finished.
    pub fn complete_scheduled_job(&self, job_id: &JobId) -> Result<()> {
        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        {
            let mut table = match write_txn.open_table(RUNNING_SCHEDULED_JOBS) {
                Ok(table) => table,
                Err(TableError::TableDoesNotExist(_)) => return Ok(()),
                Err(error) => return Err(map_redb_error(error)),
            };
            let key = running_job_key(job_id);
            table.remove(key.as_slice()).map_err(map_redb_error)?;
        }
        self.commit_write_txn(write_txn)?;
        Ok(())
    }

    /// Cancels a pending scheduled job if it has not started running yet.
    pub fn cancel_scheduled_job(&self, job_id: &JobId) -> Result<bool> {
        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        let removed = cancel_scheduled_job_in_write_txn(&write_txn, job_id)?;
        self.commit_write_txn(write_txn)?;
        Ok(removed)
    }

    /// Persists the final result for an executed scheduled job.
    pub fn record_scheduled_job_result(&self, result: &ScheduledJobResult) -> Result<()> {
        let payload =
            rmp_serde::to_vec(result).map_err(|error| Error::Serialization(error.to_string()))?;
        let key = scheduled_job_result_key(&result.id);
        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        {
            let mut table = write_txn
                .open_table(SCHEDULED_JOB_RESULTS)
                .map_err(map_redb_error)?;
            table
                .insert(key.as_slice(), payload.as_slice())
                .map_err(map_redb_error)?;
        }
        self.commit_write_txn(write_txn)?;
        Ok(())
    }

    /// Loads the final result for an executed scheduled job if present.
    pub fn get_scheduled_job_result(&self, job_id: &JobId) -> Result<Option<ScheduledJobResult>> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        let table = match read_txn.open_table(SCHEDULED_JOB_RESULTS) {
            Ok(table) => table,
            Err(TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(error) => return Err(map_redb_error(error)),
        };

        let key = scheduled_job_result_key(job_id);
        let value = table.get(key.as_slice()).map_err(map_redb_error)?;
        value
            .map(|value| {
                rmp_serde::from_slice(value.value())
                    .map_err(|error| Error::Serialization(error.to_string()))
            })
            .transpose()
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

    /// Moves orphaned running jobs back into the pending queue.
    pub fn recover_running_jobs(&self, now: Timestamp) -> Result<()> {
        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        let running_jobs = {
            let table = match write_txn.open_table(RUNNING_SCHEDULED_JOBS) {
                Ok(table) => table,
                Err(TableError::TableDoesNotExist(_)) => return Ok(()),
                Err(error) => return Err(map_redb_error(error)),
            };
            let mut jobs = Vec::new();
            for entry in table.iter().map_err(map_redb_error)? {
                let (key, value) = entry.map_err(map_redb_error)?;
                jobs.push((key.value().to_vec(), deserialize_job(value.value())?));
            }
            jobs
        };

        if running_jobs.is_empty() {
            return Ok(());
        }

        {
            let mut running = write_txn
                .open_table(RUNNING_SCHEDULED_JOBS)
                .map_err(map_redb_error)?;
            let mut scheduled = write_txn
                .open_table(SCHEDULED_JOBS)
                .map_err(map_redb_error)?;
            for (running_key, mut job) in running_jobs {
                job.run_at = now;
                let payload = serialize_job(&job)?;
                let pending_key = scheduled_job_key(job.run_at, &job.id);
                scheduled
                    .insert(pending_key.as_slice(), payload.as_slice())
                    .map_err(map_redb_error)?;
                running
                    .remove(running_key.as_slice())
                    .map_err(map_redb_error)?;
            }
        }

        self.commit_write_txn(write_txn)?;
        Ok(())
    }

    /// Returns true when a tenant has scheduled or cron work.
    pub fn has_scheduled_work(&self) -> Result<bool> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        if table_has_entries_bytes(&read_txn, SCHEDULED_JOBS)?
            || table_has_entries_bytes(&read_txn, RUNNING_SCHEDULED_JOBS)?
            || table_has_entries_str(&read_txn, CRON_JOBS)?
        {
            return Ok(true);
        }
        Ok(false)
    }

    /// Saves or updates a cron job definition.
    pub fn save_cron_job(&self, cron: &CronJob) -> Result<()> {
        let payload =
            rmp_serde::to_vec(cron).map_err(|error| Error::Serialization(error.to_string()))?;
        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        {
            let mut table = write_txn.open_table(CRON_JOBS).map_err(map_redb_error)?;
            table
                .insert(cron.name.as_str(), payload.as_slice())
                .map_err(map_redb_error)?;
        }
        self.commit_write_txn(write_txn)?;
        Ok(())
    }

    /// Loads all cron jobs sorted by name.
    pub fn load_cron_jobs(&self) -> Result<Vec<CronJob>> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        let table = match read_txn.open_table(CRON_JOBS) {
            Ok(table) => table,
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(error) => return Err(map_redb_error(error)),
        };

        let mut crons = Vec::new();
        for entry in table.iter().map_err(map_redb_error)? {
            let (_, value) = entry.map_err(map_redb_error)?;
            let cron: CronJob = rmp_serde::from_slice(value.value())
                .map_err(|error| Error::Serialization(error.to_string()))?;
            crons.push(cron);
        }
        crons.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(crons)
    }

    /// Deletes a cron job definition if present.
    pub fn delete_cron_job(&self, name: &str) -> Result<()> {
        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        {
            let mut table = match write_txn.open_table(CRON_JOBS) {
                Ok(table) => table,
                Err(TableError::TableDoesNotExist(_)) => return Ok(()),
                Err(error) => return Err(map_redb_error(error)),
            };
            table.remove(name).map_err(map_redb_error)?;
        }
        self.commit_write_txn(write_txn)?;
        Ok(())
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

fn scheduled_job_key(run_at: Timestamp, id: &JobId) -> Vec<u8> {
    let mut key = Vec::with_capacity(24);
    key.extend_from_slice(&run_at.0.to_be_bytes());
    key.extend_from_slice(&id.to_bytes());
    key
}

fn running_job_key(id: &JobId) -> [u8; 16] {
    id.to_bytes()
}

fn due_jobs_upper_bound(now: Timestamp) -> [u8; 24] {
    let mut key = [0xff; 24];
    key[..8].copy_from_slice(&now.0.to_be_bytes());
    key
}

fn scheduled_job_result_key(id: &JobId) -> [u8; 16] {
    id.to_bytes()
}

fn scheduled_key_matches_job_id(key: &[u8], job_id: &JobId) -> bool {
    key.len() >= 24 && key[8..24] == job_id.to_bytes()
}

fn serialize_job(job: &ScheduledJob) -> Result<Vec<u8>> {
    rmp_serde::to_vec(job).map_err(|error| Error::Serialization(error.to_string()))
}

fn deserialize_job(bytes: &[u8]) -> Result<ScheduledJob> {
    rmp_serde::from_slice(bytes).map_err(|error| Error::Serialization(error.to_string()))
}

fn table_has_entries_bytes(
    read_txn: &redb::ReadTransaction,
    definition: redb::TableDefinition<&[u8], &[u8]>,
) -> Result<bool> {
    let table = match read_txn.open_table(definition) {
        Ok(table) => table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(false),
        Err(error) => return Err(map_redb_error(error)),
    };
    Ok(table.iter().map_err(map_redb_error)?.next().is_some())
}

fn table_has_entries_str(
    read_txn: &redb::ReadTransaction,
    definition: redb::TableDefinition<&str, &[u8]>,
) -> Result<bool> {
    let table = match read_txn.open_table(definition) {
        Ok(table) => table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(false),
        Err(error) => return Err(map_redb_error(error)),
    };
    Ok(table.iter().map_err(map_redb_error)?.next().is_some())
}

fn next_pending_scheduled_job_at(read_txn: &redb::ReadTransaction) -> Result<Option<Timestamp>> {
    let table = match read_txn.open_table(SCHEDULED_JOBS) {
        Ok(table) => table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(None),
        Err(error) => return Err(map_redb_error(error)),
    };
    let Some(entry) = table.iter().map_err(map_redb_error)?.next() else {
        return Ok(None);
    };
    let (key, _) = entry.map_err(map_redb_error)?;
    let bytes = key.value();
    let run_at = u64::from_be_bytes(
        bytes[..8]
            .try_into()
            .expect("scheduled job keys always start with an 8-byte timestamp"),
    );
    Ok(Some(Timestamp(run_at)))
}

fn next_enabled_cron_run_at(read_txn: &redb::ReadTransaction) -> Result<Option<Timestamp>> {
    let table = match read_txn.open_table(CRON_JOBS) {
        Ok(table) => table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(None),
        Err(error) => return Err(map_redb_error(error)),
    };

    let mut next_run: Option<Timestamp> = None;
    for entry in table.iter().map_err(map_redb_error)? {
        let (_, value) = entry.map_err(map_redb_error)?;
        let cron: CronJob = rmp_serde::from_slice(value.value())
            .map_err(|error| Error::Serialization(error.to_string()))?;
        if !cron.enabled {
            continue;
        }
        next_run = Some(match next_run {
            Some(current) => current.min(cron.next_run),
            None => cron.next_run,
        });
    }
    Ok(next_run)
}
