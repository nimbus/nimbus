use rusqlite::params;

use super::{SqliteReadSnapshot, SqliteTenantStore, SqliteWriteTransaction, map_sqlite_error};
use nimbus_core::{
    Error, Result, TriggerDeliveryCursor, TriggerInvocationKey, TriggerInvocationRecord,
};

impl SqliteTenantStore {
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
        self.read_snapshot()?.list_trigger_invocations()
    }

    pub fn trigger_invocation(
        &self,
        key: &TriggerInvocationKey,
    ) -> Result<Option<TriggerInvocationRecord>> {
        self.read_snapshot()?.trigger_invocation(key)
    }

    pub fn save_trigger_invocation(&self, record: &TriggerInvocationRecord) -> Result<()> {
        let record = record.clone();
        self.execute_write(move |transaction| transaction.save_trigger_invocation(&record))?;
        Ok(())
    }
}

impl SqliteWriteTransaction {
    pub fn materialize_trigger_invocations(
        &mut self,
        records: &[TriggerInvocationRecord],
        cursor: TriggerDeliveryCursor,
    ) -> Result<()> {
        self.check_cancel()?;
        {
            let conn = self.connection_mut()?;
            for record in records {
                let payload = rmp_serde::to_vec(record)
                    .map_err(|error| Error::Serialization(error.to_string()))?;
                conn.execute(
                    "INSERT INTO trigger_invocations (registration_id, event_id, data_blob)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(registration_id, event_id)
                     DO UPDATE SET data_blob = excluded.data_blob",
                    params![record.key.registration_id, record.key.event_id, payload,],
                )
                .map_err(map_sqlite_error)?;
            }
        }
        self.set_trigger_delivery_cursor(cursor)?;
        Ok(())
    }

    pub fn save_trigger_invocation(&mut self, record: &TriggerInvocationRecord) -> Result<()> {
        self.check_cancel()?;
        let payload =
            rmp_serde::to_vec(record).map_err(|error| Error::Serialization(error.to_string()))?;
        self.connection_mut()?
            .execute(
                "INSERT INTO trigger_invocations (registration_id, event_id, data_blob)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(registration_id, event_id)
                 DO UPDATE SET data_blob = excluded.data_blob",
                params![
                    record.key.registration_id.as_str(),
                    record.key.event_id.as_str(),
                    payload.as_slice()
                ],
            )
            .map_err(map_sqlite_error)?;
        Ok(())
    }
}

impl SqliteReadSnapshot {
    pub fn list_trigger_invocations(&self) -> Result<Vec<TriggerInvocationRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT data_blob FROM trigger_invocations
                 ORDER BY registration_id, event_id",
            )
            .map_err(map_sqlite_error)?;
        let mut rows = stmt.query([]).map_err(map_sqlite_error)?;
        let mut records = Vec::new();
        while let Some(row) = rows.next().map_err(map_sqlite_error)? {
            let payload = row.get::<_, Vec<u8>>(0).map_err(map_sqlite_error)?;
            records.push(
                rmp_serde::from_slice::<TriggerInvocationRecord>(payload.as_slice())
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
        let mut stmt = self
            .conn
            .prepare(
                "SELECT data_blob FROM trigger_invocations
                 WHERE registration_id = ?1 AND event_id = ?2",
            )
            .map_err(map_sqlite_error)?;
        let mut rows = stmt
            .query(params![key.registration_id.as_str(), key.event_id.as_str()])
            .map_err(map_sqlite_error)?;
        let Some(row) = rows.next().map_err(map_sqlite_error)? else {
            return Ok(None);
        };
        let payload = row.get::<_, Vec<u8>>(0).map_err(map_sqlite_error)?;
        Ok(Some(
            rmp_serde::from_slice::<TriggerInvocationRecord>(payload.as_slice())
                .map_err(|error| Error::Serialization(error.to_string()))?,
        ))
    }
}
