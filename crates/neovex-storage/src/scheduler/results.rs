use neovex_core::{Error, JobId, Result, ScheduledJobResult};
use redb::TableError;

use crate::store::{SCHEDULED_JOB_RESULTS, TenantStore, TenantWriteTransaction, map_redb_error};

use super::codec::scheduled_job_result_key;

impl TenantWriteTransaction {
    pub fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()> {
        self.check_cancel()?;
        let payload =
            rmp_serde::to_vec(result).map_err(|error| Error::Serialization(error.to_string()))?;
        let key = scheduled_job_result_key(&result.id);
        let mut table = self
            .write_txn()?
            .open_table(SCHEDULED_JOB_RESULTS)
            .map_err(map_redb_error)?;
        table
            .insert(key.as_slice(), payload.as_slice())
            .map_err(map_redb_error)?;
        Ok(())
    }
}

impl TenantStore {
    /// Persists the final result for an executed scheduled job.
    pub fn record_scheduled_job_result(&self, result: &ScheduledJobResult) -> Result<()> {
        self.execute_write(move |transaction| transaction.record_scheduled_job_result(result))?;
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
}
