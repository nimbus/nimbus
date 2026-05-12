use nimbus_core::{
    Error, Result, TriggerDeliveryCursor, TriggerInvocationKey, TriggerInvocationRecord,
};

use super::{LibsqlReplicaTenantStore, LibsqlReplicaWriteTransaction, map_libsql_error};

impl LibsqlReplicaTenantStore {
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
        self.block_on(async move {
            let conn = self.remote_connection()?;
            let mut rows = conn
                .query(
                    "SELECT data_blob FROM trigger_invocations
                     ORDER BY registration_id, event_id",
                    (),
                )
                .await
                .map_err(map_libsql_error)?;
            let mut records = Vec::new();
            while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
                let payload = row.get::<Vec<u8>>(0).map_err(map_libsql_error)?;
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
        })
    }

    pub fn trigger_invocation(
        &self,
        key: &TriggerInvocationKey,
    ) -> Result<Option<TriggerInvocationRecord>> {
        let key = key.clone();
        self.block_on(async move {
            let conn = self.remote_connection()?;
            let mut rows = conn
                .query(
                    "SELECT data_blob FROM trigger_invocations
                     WHERE registration_id = ?1 AND event_id = ?2",
                    libsql::params![key.registration_id, key.event_id],
                )
                .await
                .map_err(map_libsql_error)?;
            let Some(row) = rows.next().await.map_err(map_libsql_error)? else {
                return Ok(None);
            };
            let payload = row.get::<Vec<u8>>(0).map_err(map_libsql_error)?;
            Ok(Some(
                rmp_serde::from_slice::<TriggerInvocationRecord>(payload.as_slice())
                    .map_err(|error| Error::Serialization(error.to_string()))?,
            ))
        })
    }

    pub fn save_trigger_invocation(&self, record: &TriggerInvocationRecord) -> Result<()> {
        let record = record.clone();
        self.execute_write(move |transaction| transaction.save_trigger_invocation(&record))?;
        Ok(())
    }
}

impl LibsqlReplicaWriteTransaction {
    pub fn materialize_trigger_invocations(
        &mut self,
        records: &[TriggerInvocationRecord],
        cursor: TriggerDeliveryCursor,
    ) -> Result<()> {
        self.check_cancel()?;
        for record in records {
            let payload = rmp_serde::to_vec(record)
                .map_err(|error| Error::Serialization(error.to_string()))?;
            self.store.block_on(async {
                self.session()?
                    .execute(
                        "INSERT INTO trigger_invocations (registration_id, event_id, data_blob)
                         VALUES (?1, ?2, ?3)
                         ON CONFLICT(registration_id, event_id)
                         DO UPDATE SET data_blob = excluded.data_blob",
                        libsql::params![
                            record.key.registration_id.clone(),
                            record.key.event_id.clone(),
                            payload
                        ],
                    )
                    .await
                    .map_err(map_libsql_error)?;
                Ok(())
            })?;
        }
        self.set_trigger_delivery_cursor(cursor)?;
        Ok(())
    }

    pub fn save_trigger_invocation(&mut self, record: &TriggerInvocationRecord) -> Result<()> {
        self.check_cancel()?;
        let payload =
            rmp_serde::to_vec(record).map_err(|error| Error::Serialization(error.to_string()))?;
        self.store.block_on(async {
            self.session()?
                .execute(
                    "INSERT INTO trigger_invocations (registration_id, event_id, data_blob)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(registration_id, event_id)
                     DO UPDATE SET data_blob = excluded.data_blob",
                    libsql::params![
                        record.key.registration_id.clone(),
                        record.key.event_id.clone(),
                        payload
                    ],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        })
    }
}
