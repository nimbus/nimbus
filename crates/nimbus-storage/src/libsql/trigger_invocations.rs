use nimbus_core::{
    Error, Result, TriggerDeliveryCursor, TriggerInvocationKey, TriggerInvocationRecord,
};

use super::{
    FENCED_COMMITTER_LEASE_MARKER, LibsqlReplicaTenantStore, LibsqlReplicaWriteTransaction,
    map_libsql_error, take_single_remote_row,
};
use crate::CommitterLeaseResult;

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

    pub fn fenced_materialize_trigger_invocations(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_previous: nimbus_core::SequenceNumber,
        records: &[TriggerInvocationRecord],
        cursor: TriggerDeliveryCursor,
    ) -> CommitterLeaseResult<()> {
        let owner_id = owner_id.to_string();
        let fenced_owner_id = owner_id.clone();
        let records = records.to_vec();
        let durable_sequence = nimbus_core::SequenceNumber(expected_previous.0.saturating_add(1));
        let result = self.execute_write(move |transaction| {
            if transaction.advance_fenced_committer_lease(
                &owner_id,
                epoch,
                expected_previous,
                durable_sequence,
            )? != 1
            {
                return Err(Error::PreconditionFailed(
                    FENCED_COMMITTER_LEASE_MARKER.to_string(),
                ));
            }
            transaction.materialize_trigger_invocations(records.as_slice(), cursor)
        });
        super::write::map_fenced_write_result(result.map(|_| ()), fenced_owner_id, epoch)
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
            let rows = conn
                .query(
                    "SELECT data_blob FROM trigger_invocations
                     WHERE registration_id = ?1 AND event_id = ?2",
                    libsql::params![key.registration_id, key.event_id],
                )
                .await
                .map_err(map_libsql_error)?;
            let Some(row) = take_single_remote_row(rows).await? else {
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

    pub fn fenced_save_trigger_invocation(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_durable_sequence: nimbus_core::SequenceNumber,
        record: &TriggerInvocationRecord,
    ) -> CommitterLeaseResult<()> {
        let owner_id = owner_id.to_string();
        let fenced_owner_id = owner_id.clone();
        let record = record.clone();
        let result = self.execute_write(move |transaction| {
            if transaction.validate_fenced_committer_lease(
                &owner_id,
                epoch,
                expected_durable_sequence,
            )? != 1
            {
                return Err(Error::PreconditionFailed(
                    FENCED_COMMITTER_LEASE_MARKER.to_string(),
                ));
            }
            transaction.save_trigger_invocation(&record)
        });
        super::write::map_fenced_write_result(result.map(|_| ()), fenced_owner_id, epoch)
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
