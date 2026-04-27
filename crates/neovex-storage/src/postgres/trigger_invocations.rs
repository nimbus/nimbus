use neovex_core::{
    Error, Result, TriggerDeliveryCursor, TriggerInvocationKey, TriggerInvocationRecord,
};

use super::*;

// Trigger-invocation persistence stays provider-owned because row encoding,
// upsert semantics, and session usage are backend-specific. The shared seam is
// the engine-level trigger contract, not a synthetic cross-database SQL helper.
impl PostgresTenantStore {
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
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            load_trigger_invocations_from_session(&client, &schema_name).await
        })
    }

    pub fn trigger_invocation(
        &self,
        key: &TriggerInvocationKey,
    ) -> Result<Option<TriggerInvocationRecord>> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let key = key.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            load_trigger_invocation_from_session(&client, &schema_name, &key).await
        })
    }

    pub fn save_trigger_invocation(&self, record: &TriggerInvocationRecord) -> Result<()> {
        let record = record.clone();
        self.execute_write(move |transaction| transaction.save_trigger_invocation(&record))?;
        Ok(())
    }
}

impl PostgresWriteTransaction {
    pub fn materialize_trigger_invocations(
        &mut self,
        records: &[TriggerInvocationRecord],
        cursor: TriggerDeliveryCursor,
    ) -> Result<()> {
        self.check_cancel()?;
        let query = format!(
            "INSERT INTO {} (registration_id, event_id, data_blob) VALUES ($1, $2, $3)
             ON CONFLICT(registration_id, event_id)
             DO UPDATE SET data_blob = EXCLUDED.data_blob",
            qualified_table(&self.schema_name, "trigger_invocations")
        );
        let client = self.session()?;
        for record in records {
            let registration_id = record.key.registration_id.clone();
            let event_id = record.key.event_id.clone();
            let payload = rmp_serde::to_vec(record)
                .map_err(|error| Error::Serialization(error.to_string()))?;
            let query = query.clone();
            self.block_on(async move {
                client
                    .execute(query.as_str(), &[&registration_id, &event_id, &payload])
                    .await
                    .map_err(map_postgres_error)?;
                Ok(())
            })?;
        }
        self.set_trigger_delivery_cursor(cursor)?;
        Ok(())
    }

    pub fn save_trigger_invocation(&mut self, record: &TriggerInvocationRecord) -> Result<()> {
        self.check_cancel()?;
        let query = format!(
            "INSERT INTO {} (registration_id, event_id, data_blob) VALUES ($1, $2, $3)
             ON CONFLICT(registration_id, event_id)
             DO UPDATE SET data_blob = EXCLUDED.data_blob",
            qualified_table(&self.schema_name, "trigger_invocations")
        );
        let registration_id = record.key.registration_id.clone();
        let event_id = record.key.event_id.clone();
        let payload =
            rmp_serde::to_vec(record).map_err(|error| Error::Serialization(error.to_string()))?;
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute(query.as_str(), &[&registration_id, &event_id, &payload])
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        })
    }
}

async fn load_trigger_invocations_from_session<C>(
    session: &C,
    schema_name: &str,
) -> Result<Vec<TriggerInvocationRecord>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT data_blob FROM {} ORDER BY registration_id, event_id",
        qualified_table(schema_name, "trigger_invocations")
    );
    let rows = session
        .query(query.as_str(), &[])
        .await
        .map_err(map_postgres_error)?;
    let mut records = rows
        .into_iter()
        .map(|row| {
            rmp_serde::from_slice::<TriggerInvocationRecord>(row.get::<_, Vec<u8>>(0).as_slice())
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

async fn load_trigger_invocation_from_session<C>(
    session: &C,
    schema_name: &str,
    key: &TriggerInvocationKey,
) -> Result<Option<TriggerInvocationRecord>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT data_blob FROM {} WHERE registration_id = $1 AND event_id = $2",
        qualified_table(schema_name, "trigger_invocations")
    );
    let row = session
        .query_opt(query.as_str(), &[&key.registration_id, &key.event_id])
        .await
        .map_err(map_postgres_error)?;
    row.map(|row| {
        rmp_serde::from_slice::<TriggerInvocationRecord>(row.get::<_, Vec<u8>>(0).as_slice())
            .map_err(|error| Error::Serialization(error.to_string()))
    })
    .transpose()
}
