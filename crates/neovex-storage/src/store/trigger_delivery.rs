use neovex_core::{Result, TriggerDeliveryCursor};
use redb::TableError;

use super::journal::{decode_u64, encode_u64};
use super::{
    METADATA, TRIGGER_DELIVERY_CURSOR_KEY, TenantStore, TenantWriteTransaction, map_redb_error,
};

impl TenantStore {
    pub fn trigger_delivery_cursor(&self) -> Result<TriggerDeliveryCursor> {
        let snapshot = self.read_snapshot()?;
        let metadata = match snapshot.read_txn.open_table(METADATA) {
            Ok(metadata) => metadata,
            Err(TableError::TableDoesNotExist(_)) => {
                return Ok(TriggerDeliveryCursor::default());
            }
            Err(error) => return Err(map_redb_error(error)),
        };
        let materialized_through = metadata
            .get(TRIGGER_DELIVERY_CURSOR_KEY)
            .map_err(map_redb_error)?
            .map(|value| decode_u64(value.value()))
            .transpose()?
            .unwrap_or(0);
        Ok(TriggerDeliveryCursor::new(neovex_core::SequenceNumber(
            materialized_through,
        )))
    }

    pub fn set_trigger_delivery_cursor(&self, cursor: TriggerDeliveryCursor) -> Result<()> {
        self.execute_write(|transaction| transaction.set_trigger_delivery_cursor(cursor))?;
        Ok(())
    }
}

impl TenantWriteTransaction {
    pub fn set_trigger_delivery_cursor(&mut self, cursor: TriggerDeliveryCursor) -> Result<()> {
        self.check_cancel()?;
        let mut metadata = self
            .write_txn()?
            .open_table(METADATA)
            .map_err(map_redb_error)?;
        metadata
            .insert(
                TRIGGER_DELIVERY_CURSOR_KEY,
                encode_u64(cursor.materialized_through.0).as_slice(),
            )
            .map_err(map_redb_error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn trigger_delivery_cursor_round_trips_in_redb_metadata() {
        let dir = tempdir().expect("temporary directory should create");
        let path = dir.path().join("tenant.redb");
        let store = TenantStore::open(&path).expect("store should open");

        assert_eq!(
            store.trigger_delivery_cursor().expect("cursor should load"),
            TriggerDeliveryCursor::default()
        );

        store
            .set_trigger_delivery_cursor(TriggerDeliveryCursor::new(neovex_core::SequenceNumber(
                42,
            )))
            .expect("cursor should persist");

        assert_eq!(
            store
                .trigger_delivery_cursor()
                .expect("cursor should round trip"),
            TriggerDeliveryCursor::new(neovex_core::SequenceNumber(42))
        );
    }
}
