#![allow(dead_code)]

use super::*;

impl TenantPersistence {
    delegate_store_method!(fn trigger_delivery_cursor(&self) -> Result<TriggerDeliveryCursor>);
    delegate_store_method!(fn set_trigger_delivery_cursor(&self, cursor: TriggerDeliveryCursor) -> Result<()>);
}
