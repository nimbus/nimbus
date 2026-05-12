use nimbus_core::{CronJob, Error, Result, Timestamp};
use redb::{ReadTransaction, ReadableTable, TableError};

use crate::store::{CRON_JOBS, TenantStore, TenantWriteTransaction, map_redb_error};

impl TenantWriteTransaction {
    pub fn save_cron_job(&mut self, cron: &CronJob) -> Result<()> {
        self.check_cancel()?;
        let payload =
            rmp_serde::to_vec(cron).map_err(|error| Error::Serialization(error.to_string()))?;
        let mut table = self
            .write_txn()?
            .open_table(CRON_JOBS)
            .map_err(map_redb_error)?;
        table
            .insert(cron.name.as_str(), payload.as_slice())
            .map_err(map_redb_error)?;
        Ok(())
    }

    pub fn delete_cron_job(&mut self, name: &str) -> Result<()> {
        self.check_cancel()?;
        let mut table = match self.write_txn()?.open_table(CRON_JOBS) {
            Ok(table) => table,
            Err(TableError::TableDoesNotExist(_)) => return Ok(()),
            Err(error) => return Err(map_redb_error(error)),
        };
        table.remove(name).map_err(map_redb_error)?;
        Ok(())
    }
}

impl TenantStore {
    /// Saves or updates a cron job definition.
    pub fn save_cron_job(&self, cron: &CronJob) -> Result<()> {
        self.execute_write(move |transaction| transaction.save_cron_job(cron))?;
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
        self.execute_write(move |transaction| transaction.delete_cron_job(name))?;
        Ok(())
    }
}

pub(super) fn next_enabled_cron_run_at(read_txn: &ReadTransaction) -> Result<Option<Timestamp>> {
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
