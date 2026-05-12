use nimbus_core::{Result, Timestamp};
use redb::{ReadTransaction, ReadableTable, TableError};

use crate::store::{
    CRON_JOBS, RUNNING_SCHEDULED_JOBS, SCHEDULED_JOBS, TenantStore, map_redb_error,
};

use super::cron::next_enabled_cron_run_at;

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
}

fn table_has_entries_bytes(
    read_txn: &ReadTransaction,
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
    read_txn: &ReadTransaction,
    definition: redb::TableDefinition<&str, &[u8]>,
) -> Result<bool> {
    let table = match read_txn.open_table(definition) {
        Ok(table) => table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(false),
        Err(error) => return Err(map_redb_error(error)),
    };
    Ok(table.iter().map_err(map_redb_error)?.next().is_some())
}

fn next_pending_scheduled_job_at(read_txn: &ReadTransaction) -> Result<Option<Timestamp>> {
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
