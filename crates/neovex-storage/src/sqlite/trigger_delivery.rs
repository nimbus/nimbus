use rusqlite::{OptionalExtension, params};

use super::{
    SqliteReadSnapshot, SqliteTenantStore, SqliteWriteTransaction, TriggerDeliveryCursor,
    map_sqlite_error,
};
use crate::store::TRIGGER_DELIVERY_CURSOR_KEY;

impl SqliteTenantStore {
    pub fn trigger_delivery_cursor(&self) -> neovex_core::Result<TriggerDeliveryCursor> {
        self.read_snapshot()?.trigger_delivery_cursor()
    }

    pub fn set_trigger_delivery_cursor(
        &self,
        cursor: TriggerDeliveryCursor,
    ) -> neovex_core::Result<()> {
        self.execute_write(|transaction| transaction.set_trigger_delivery_cursor(cursor))?;
        Ok(())
    }
}

impl SqliteWriteTransaction {
    pub fn set_trigger_delivery_cursor(
        &mut self,
        cursor: TriggerDeliveryCursor,
    ) -> neovex_core::Result<()> {
        self.check_cancel()?;
        self.connection_mut()?
            .execute(
                "INSERT INTO metadata (key, value_blob) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value_blob = excluded.value_blob",
                params![
                    TRIGGER_DELIVERY_CURSOR_KEY,
                    super::encode_u64(cursor.materialized_through.0).to_vec()
                ],
            )
            .map_err(map_sqlite_error)?;
        Ok(())
    }
}

impl SqliteReadSnapshot {
    pub fn trigger_delivery_cursor(&self) -> neovex_core::Result<TriggerDeliveryCursor> {
        let materialized_through = self
            .conn
            .query_row(
                "SELECT value_blob FROM metadata WHERE key = ?1",
                params![TRIGGER_DELIVERY_CURSOR_KEY],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(map_sqlite_error)?
            .map(|bytes| super::decode_u64(bytes.as_slice()))
            .transpose()?
            .unwrap_or(0);
        Ok(TriggerDeliveryCursor::new(neovex_core::SequenceNumber(
            materialized_through,
        )))
    }
}
