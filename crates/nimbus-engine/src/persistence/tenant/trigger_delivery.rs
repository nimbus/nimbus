use super::*;

impl TenantPersistence {
    delegate_store_method!(fn trigger_delivery_cursor(&self) -> Result<TriggerDeliveryCursor>);
    delegate_store_method!(
        #[cfg(any(test, feature = "test-hooks"))]
        fn set_trigger_delivery_cursor(&self, cursor: TriggerDeliveryCursor) -> Result<()>
    );
}
