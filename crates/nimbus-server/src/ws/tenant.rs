use axum::http::{HeaderMap, HeaderValue};
use nimbus_core::{Error, TenantId};

use crate::state::AppError;

const TENANT_HEADER: &str = "X-Tenant-Id";

pub(crate) fn extract_tenant_id(
    headers: &HeaderMap,
    query_tenant_id: Option<String>,
) -> Result<TenantId, AppError> {
    if let Some(value) = headers.get(TENANT_HEADER) {
        let tenant_id = header_value_to_string(value)?;
        return TenantId::new(tenant_id).map_err(AppError::from);
    }

    if let Some(tenant_id) = query_tenant_id {
        return TenantId::new(tenant_id).map_err(AppError::from);
    }

    Err(AppError::from(Error::InvalidInput(
        "missing X-Tenant-Id header or tenant_id query parameter".to_string(),
    )))
}

fn header_value_to_string(value: &HeaderValue) -> Result<String, AppError> {
    value
        .to_str()
        .map(|value| value.to_string())
        .map_err(|error| {
            AppError::from(Error::InvalidInput(format!(
                "invalid X-Tenant-Id header: {error}"
            )))
        })
}
