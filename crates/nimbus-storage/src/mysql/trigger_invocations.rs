use mysql_async::prelude::Queryable;

use nimbus_core::{
    Error, Result, TriggerDeliveryCursor, TriggerInvocationKey, TriggerInvocationRecord,
};

use super::*;

// Trigger-invocation row encoding, upsert syntax, and session usage are
// backend-specific and stay here. The store-level wrappers that were pure
// `execute_write` + committer-lease fencing now live once in
// `crate::sql::store_core` and reach this module through
// `SqlWriteTransactionCore`. The reads below keep their own bodies: each
// acquires a fresh read connection from its own provider type.
impl MySqlTenantStore {
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
