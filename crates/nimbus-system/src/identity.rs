use nimbus_core::{Error, Result, TenantId};

pub const SYSTEM_TENANT_ID: &str = "_nimbus";
pub fn system_tenant_id() -> Result<TenantId> {
    TenantId::new(SYSTEM_TENANT_ID)
}

pub fn is_reserved_tenant_id(tenant_id: &TenantId) -> bool {
    tenant_id.as_str().starts_with('_')
}

pub fn is_system_tenant_id(tenant_id: &TenantId) -> bool {
    tenant_id.as_str() == SYSTEM_TENANT_ID
}

pub fn user_tenant_id(value: impl Into<String>) -> Result<TenantId> {
    let tenant_id = TenantId::new(value)?;
    if is_reserved_tenant_id(&tenant_id) {
        return Err(Error::InvalidInput(format!(
            "tenant ids starting with `_` are reserved for Nimbus system tenants: {tenant_id}"
        )));
    }
    Ok(tenant_id)
}
