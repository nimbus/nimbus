use nimbus_core::{
    Error, Result, TriggerDeliveryCursor, TriggerInvocationKey, TriggerInvocationRecord,
};
use redb::{ReadableTable, TableError};

use crate::keys::trigger_invocation_key;

use super::{TRIGGER_INVOCATIONS, TenantStore, TenantWriteTransaction, map_redb_error};

impl TenantWriteTransaction {
    pub fn materialize_trigger_invocations(
        &mut self,
        records: &[TriggerInvocationRecord],
        cursor: TriggerDeliveryCursor,
    ) -> Result<()> {
        self.check_cancel()?;
        {
            let mut table = self
                .write_txn()?
                .open_table(TRIGGER_INVOCATIONS)
                .map_err(map_redb_error)?;
            for record in records {
                let key = trigger_invocation_key(&record.key);
                let payload = rmp_serde::to_vec(record)
                    .map_err(|error| Error::Serialization(error.to_string()))?;
                table
                    .insert(key.as_slice(), payload.as_slice())
                    .map_err(map_redb_error)?;
            }
        }
        self.set_trigger_delivery_cursor(cursor)?;
        Ok(())
    }

    pub fn save_trigger_invocation(&mut self, record: &TriggerInvocationRecord) -> Result<()> {
        self.check_cancel()?;
        let mut table = self
            .write_txn()?
            .open_table(TRIGGER_INVOCATIONS)
            .map_err(map_redb_error)?;
        let key = trigger_invocation_key(&record.key);
        let payload =
            rmp_serde::to_vec(record).map_err(|error| Error::Serialization(error.to_string()))?;
        table
            .insert(key.as_slice(), payload.as_slice())
            .map_err(map_redb_error)?;
        Ok(())
    }
}

impl TenantStore {
    pub fn materialize_trigger_invocations(
        &self,
        records: &[TriggerInvocationRecord],
        cursor: TriggerDeliveryCursor,
    ) -> Result<()> {
        let records = records.to_vec();
        self.execute_write(move |transaction| {
            transaction.materialize_trigger_invocations(records.as_slice(), cursor)
        })?;
        Ok(())
    }

    pub fn list_trigger_invocations(&self) -> Result<Vec<TriggerInvocationRecord>> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        let table = match read_txn.open_table(TRIGGER_INVOCATIONS) {
            Ok(table) => table,
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(error) => return Err(map_redb_error(error)),
        };

        let mut records = Vec::new();
        for row_result in table.iter().map_err(map_redb_error)? {
            let (_, payload) = row_result.map_err(map_redb_error)?;
            records.push(
                rmp_serde::from_slice::<TriggerInvocationRecord>(payload.value())
                    .map_err(|error| Error::Serialization(error.to_string()))?,
            );
        }
        records.sort_by(|left, right| {
            left.commit_sequence
                .cmp(&right.commit_sequence)
                .then(left.key.cmp(&right.key))
        });
        Ok(records)
    }

    pub fn trigger_invocation(
        &self,
        key: &TriggerInvocationKey,
    ) -> Result<Option<TriggerInvocationRecord>> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        let table = match read_txn.open_table(TRIGGER_INVOCATIONS) {
            Ok(table) => table,
            Err(TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(error) => return Err(map_redb_error(error)),
        };
        let encoded_key = trigger_invocation_key(key);
        let Some(value) = table.get(encoded_key.as_slice()).map_err(map_redb_error)? else {
            return Ok(None);
        };
        Ok(Some(
            rmp_serde::from_slice::<TriggerInvocationRecord>(value.value())
                .map_err(|error| Error::Serialization(error.to_string()))?,
        ))
    }

    pub fn save_trigger_invocation(&self, record: &TriggerInvocationRecord) -> Result<()> {
        let record = record.clone();
        self.execute_write(move |transaction| transaction.save_trigger_invocation(&record))?;
        Ok(())
    }
}
