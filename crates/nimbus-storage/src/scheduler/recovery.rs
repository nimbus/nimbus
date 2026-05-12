use nimbus_core::{Result, Timestamp};
use redb::{ReadableTable, TableError};

use crate::store::{RUNNING_SCHEDULED_JOBS, SCHEDULED_JOBS, TenantStore, map_redb_error};

use super::codec::{deserialize_job, scheduled_job_key, serialize_job};

impl TenantStore {
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
}
