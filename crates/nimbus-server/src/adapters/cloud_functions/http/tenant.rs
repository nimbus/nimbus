use nimbus_cloud_functions::CloudFunctionsHttpTenantBinding;
use nimbus_core::Error;

use crate::state::{AppError, AppState, DeploymentState};

pub(super) fn resolve_cloud_functions_http_tenant(
    state: &AppState,
    deployment: &DeploymentState,
) -> std::result::Result<CloudFunctionsHttpTenantBinding, AppError> {
    let binding = deployment.cloud_functions_http_tenant().ok_or_else(|| {
        AppError::from(Error::conflict(
            "cloud functions HTTP handlers require a trusted deployment tenant binding".to_owned(),
        ))
    })?;
    state
        .engine
        .ensure_tenant_exists(binding.tenant_id())
        .map_err(AppError::from)?;
    Ok(binding)
}
