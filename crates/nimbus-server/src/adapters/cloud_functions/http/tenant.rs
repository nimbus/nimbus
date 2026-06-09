use nimbus_core::{Error, TenantId};

use crate::state::{AppError, AppState};

pub(super) fn resolve_cloud_functions_http_tenant(
    state: &AppState,
) -> std::result::Result<TenantId, AppError> {
    let tenants = state.engine.list_tenants().map_err(AppError::from)?;
    match tenants.as_slice() {
        [tenant_id] => Ok(tenant_id.clone()),
        [] => Err(AppError::from(Error::Conflict(
            "cloud functions http handlers require exactly one tenant, but no tenants exist"
                .to_string(),
        ))),
        _ => Err(AppError::from(Error::Conflict(
            "cloud functions http handlers require exactly one tenant; explicit multi-tenant HTTP binding is deferred to a later cloud functions phase"
                .to_string(),
        ))),
    }
}
