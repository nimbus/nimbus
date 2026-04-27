use super::*;

impl LibsqlReplicaTenantStore {
    pub fn trigger_delivery_cursor(&self) -> Result<TriggerDeliveryCursor> {
        self.block_on(async move {
            let conn = self.remote_connection()?;
            Ok(load_remote_metadata_u64(&conn, TRIGGER_DELIVERY_CURSOR_KEY)
                .await?
                .map(SequenceNumber)
                .map(TriggerDeliveryCursor::new)
                .unwrap_or_default())
        })
    }

    pub fn set_trigger_delivery_cursor(&self, cursor: TriggerDeliveryCursor) -> Result<()> {
        self.block_on(async move {
            let conn = self.remote_connection()?;
            put_remote_metadata_u64(
                &conn,
                TRIGGER_DELIVERY_CURSOR_KEY,
                cursor.materialized_through.0,
            )
            .await
        })
    }
}

impl LibsqlReplicaWriteTransaction {
    pub fn set_trigger_delivery_cursor(&mut self, cursor: TriggerDeliveryCursor) -> Result<()> {
        self.check_cancel()?;
        self.store.block_on(async {
            put_remote_metadata_u64(
                self.session()?,
                TRIGGER_DELIVERY_CURSOR_KEY,
                cursor.materialized_through.0,
            )
            .await
        })
    }
}
