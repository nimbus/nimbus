use nimbus_core::{Error, Result, TenantId};

pub(crate) const SYSTEM_TENANT_ID: &str = "_nimbus";
pub(crate) fn system_tenant_id() -> Result<TenantId> {
    TenantId::new(SYSTEM_TENANT_ID)
}

pub(crate) fn is_reserved_tenant_id(tenant_id: &TenantId) -> bool {
    tenant_id.as_str().starts_with('_')
}

pub(crate) fn is_system_tenant_id(tenant_id: &TenantId) -> bool {
    tenant_id.as_str() == SYSTEM_TENANT_ID
}

pub(crate) fn user_tenant_id(value: impl Into<String>) -> Result<TenantId> {
    let tenant_id = TenantId::new(value)?;
    if is_reserved_tenant_id(&tenant_id) {
        return Err(Error::InvalidInput(format!(
            "tenant ids starting with `_` are reserved for Nimbus system tenants: {tenant_id}"
        )));
    }
    Ok(tenant_id)
}
