use mysql_async::prelude::Queryable;

use nimbus_core::{
    Error, Result, TriggerDeliveryCursor, TriggerInvocationKey, TriggerInvocationRecord,
};

use super::*;
use crate::CommitterLeaseResult;

// Trigger-invocation persistence stays provider-owned because row encoding,
// upsert semantics, and session usage are backend-specific. The shared seam is
// the engine-level trigger contract, not a synthetic cross-database SQL helper.
impl MySqlTenantStore {
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
        expected_previous: SequenceNumber,
        records: &[TriggerInvocationRecord],
        cursor: TriggerDeliveryCursor,
    ) -> CommitterLeaseResult<()> {
        let owner_id = owner_id.to_string();
        let fenced_owner_id = owner_id.clone();
        let records = records.to_vec();
        let durable_sequence = SequenceNumber(expected_previous.0.saturating_add(1));
        let result = self.execute_write(move |transaction| {
            if transaction.advance_fenced_committer_lease(
                &owner_id,
                epoch,
                expected_previous,
                durable_sequence,
            )? != 1
            {
                return Err(Error::PreconditionFailed(
                    super::write::FENCED_COMMITTER_LEASE_MARKER.to_string(),
                ));
            }
            transaction.materialize_trigger_invocations(records.as_slice(), cursor)
        });
        super::write::map_fenced_write_result(result.map(|_| ()), fenced_owner_id, epoch)
    }

    pub fn list_trigger_invocations(&self) -> Result<Vec<TriggerInvocationRecord>> {
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            load_trigger_invocations_from_session(&mut conn, &database_name).await
        })
    }

    pub fn trigger_invocation(
        &self,
        key: &TriggerInvocationKey,
    ) -> Result<Option<TriggerInvocationRecord>> {
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        let key = key.clone();
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            load_trigger_invocation_from_session(&mut conn, &database_name, &key).await
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
        expected_durable_sequence: SequenceNumber,
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
                    super::write::FENCED_COMMITTER_LEASE_MARKER.to_string(),
                ));
            }
            transaction.save_trigger_invocation(&record)
        });
        super::write::map_fenced_write_result(result.map(|_| ()), fenced_owner_id, epoch)
    }
}

impl MySqlWriteTransaction {
    pub fn materialize_trigger_invocations(
        &mut self,
        records: &[TriggerInvocationRecord],
        cursor: TriggerDeliveryCursor,
    ) -> Result<()> {
        self.check_cancel()?;
        let query = format!(
            "INSERT INTO {} (registration_id, event_id, data_blob) VALUES (?, ?, ?)
             ON DUPLICATE KEY UPDATE data_blob = VALUES(data_blob)",
            qualified_table(&self.database_name, "trigger_invocations")
        );
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        for record in records {
            let payload = rmp_serde::to_vec(record)
                .map_err(|error| Error::Serialization(error.to_string()))?;
            Self::block_on(&runtime_handle, async {
                conn.exec_drop(
                    query.as_str(),
                    (
                        record.key.registration_id.as_str(),
                        record.key.event_id.as_str(),
                        payload,
                    ),
                )
                .await
                .map_err(map_mysql_error)
            })?;
        }
        self.set_trigger_delivery_cursor(cursor)?;
        Ok(())
    }

    pub fn save_trigger_invocation(&mut self, record: &TriggerInvocationRecord) -> Result<()> {
        self.check_cancel()?;
        let query = format!(
            "INSERT INTO {} (registration_id, event_id, data_blob) VALUES (?, ?, ?)
             ON DUPLICATE KEY UPDATE data_blob = VALUES(data_blob)",
            qualified_table(&self.database_name, "trigger_invocations")
        );
        let payload =
            rmp_serde::to_vec(record).map_err(|error| Error::Serialization(error.to_string()))?;
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async {
            conn.exec_drop(
                query.as_str(),
                (
                    record.key.registration_id.as_str(),
                    record.key.event_id.as_str(),
                    payload,
                ),
            )
            .await
            .map_err(map_mysql_error)
        })?;
        Ok(())
    }
}

async fn load_trigger_invocations_from_session(
    conn: &mut Conn,
    database_name: &str,
) -> Result<Vec<TriggerInvocationRecord>> {
    let query = format!(
        "SELECT data_blob FROM {} ORDER BY registration_id, event_id",
        qualified_table(database_name, "trigger_invocations")
    );
    let rows = conn
        .exec::<Row, _, _>(query, ())
        .await
        .map_err(map_mysql_error)?;
    let mut records = rows
        .into_iter()
        .map(|row| {
            let payload = row.get::<Vec<u8>, _>(0).ok_or_else(|| {
                Error::Internal("missing trigger invocation payload column".to_string())
            })?;
            rmp_serde::from_slice::<TriggerInvocationRecord>(payload.as_slice())
                .map_err(|error| Error::Serialization(error.to_string()))
        })
        .collect::<Result<Vec<_>>>()?;
    records.sort_by(|left, right| {
        left.commit_sequence
            .cmp(&right.commit_sequence)
            .then(left.key.cmp(&right.key))
    });
    Ok(records)
}

async fn load_trigger_invocation_from_session(
    conn: &mut Conn,
    database_name: &str,
    key: &TriggerInvocationKey,
) -> Result<Option<TriggerInvocationRecord>> {
    let query = format!(
        "SELECT data_blob FROM {} WHERE registration_id = ? AND event_id = ?",
        qualified_table(database_name, "trigger_invocations")
    );
    let row = conn
        .exec_first::<Row, _, _>(query, (key.registration_id.as_str(), key.event_id.as_str()))
        .await
        .map_err(map_mysql_error)?;
    row.map(|row| {
        let payload = row.get::<Vec<u8>, _>(0).ok_or_else(|| {
            Error::Internal("missing trigger invocation payload column".to_string())
        })?;
        rmp_serde::from_slice::<TriggerInvocationRecord>(payload.as_slice())
            .map_err(|error| Error::Serialization(error.to_string()))
    })
    .transpose()
}
