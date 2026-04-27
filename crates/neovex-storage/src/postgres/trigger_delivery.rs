use super::*;

impl PostgresTenantStore {
    pub fn trigger_delivery_cursor(&self) -> Result<TriggerDeliveryCursor> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            Ok(
                load_metadata_u64_from_session(&client, &schema_name, TRIGGER_DELIVERY_CURSOR_KEY)
                    .await?
                    .map(SequenceNumber)
                    .map(TriggerDeliveryCursor::new)
                    .unwrap_or_default(),
            )
        })
    }

    pub fn set_trigger_delivery_cursor(&self, cursor: TriggerDeliveryCursor) -> Result<()> {
        self.execute_write(move |transaction| transaction.set_trigger_delivery_cursor(cursor))?;
        Ok(())
    }
}

impl PostgresWriteTransaction {
    pub fn set_trigger_delivery_cursor(&mut self, cursor: TriggerDeliveryCursor) -> Result<()> {
        let query = format!(
            "INSERT INTO {} (key, value_blob) VALUES ($1, $2)
             ON CONFLICT(key) DO UPDATE SET value_blob = EXCLUDED.value_blob",
            qualified_table(&self.schema_name, "metadata")
        );
        let key = TRIGGER_DELIVERY_CURSOR_KEY.to_string();
        let value = encode_u64(cursor.materialized_through.0).to_vec();
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute(query.as_str(), &[&key, &value])
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        })
    }
}
