use std::sync::Arc;

use neovex_core::TenantId;
use neovex_engine::Service;

use super::super::error::MongoError;

pub const DEFAULT_TENANT: &str = "default";

pub fn resolve_tenant(db_name: &str) -> Result<TenantId, MongoError> {
    match db_name {
        "admin" | "local" | "config" => TenantId::new(DEFAULT_TENANT).map_err(MongoError::from),
        other => TenantId::new(other).map_err(MongoError::from),
    }
}

pub fn ensure_tenant(service: &Arc<Service>, tenant_id: &TenantId) -> Result<(), MongoError> {
    match service.create_tenant(tenant_id.clone()) {
        Ok(()) => Ok(()),
        Err(neovex_core::Error::AlreadyExists(_)) => Ok(()),
        Err(e) => Err(MongoError::from(e)),
    }
}
