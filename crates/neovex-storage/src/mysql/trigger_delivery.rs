use super::*;

// Trigger-delivery cursor persistence stays provider-owned because metadata
// storage, upsert syntax, and session management are backend-specific. Share
// the engine-facing cursor contract instead of forcing a fake generic SQL
// layer across MySQL, Postgres, and SQLite.
impl MySqlTenantStore {
    pub fn trigger_delivery_cursor(&self) -> Result<TriggerDeliveryCursor> {
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            Ok(load_metadata_u64_from_session(
                &mut conn,
                &database_name,
                TRIGGER_DELIVERY_CURSOR_KEY,
            )
            .await?
            .map(SequenceNumber)
            .map(TriggerDeliveryCursor::new)
            .unwrap_or_default())
        })
    }

    pub fn set_trigger_delivery_cursor(&self, cursor: TriggerDeliveryCursor) -> Result<()> {
        self.execute_write(move |transaction| transaction.set_trigger_delivery_cursor(cursor))?;
        Ok(())
    }
}

impl MySqlWriteTransaction {
    pub fn set_trigger_delivery_cursor(&mut self, cursor: TriggerDeliveryCursor) -> Result<()> {
        let query = format!(
            "INSERT INTO {} (key_name, value_u64) VALUES (?, ?)
             ON DUPLICATE KEY UPDATE value_u64 = VALUES(value_u64)",
            qualified_table(&self.database_name, "metadata")
        );
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(
                query,
                (TRIGGER_DELIVERY_CURSOR_KEY, cursor.materialized_through.0),
            )
            .await
            .map_err(map_mysql_error)
        })
    }
}
