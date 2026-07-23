use super::*;

impl TenantPersistence {
    delegate_store_method!(fn trigger_delivery_cursor(&self) -> Result<TriggerDeliveryCursor>);
}
