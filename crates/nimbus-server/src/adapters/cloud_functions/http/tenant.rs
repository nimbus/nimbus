use nimbus_core::{Error, TenantId};

use crate::state::{AppError, AppState};

pub(super) fn resolve_cloud_functions_http_tenant(
    state: &AppState,
) -> std::result::Result<TenantId, AppError> {
    let tenants = state.engine.list_tenants().map_err(AppError::from)?;
    resolve_application_tenant(tenants)
}

fn resolve_application_tenant(tenants: Vec<TenantId>) -> std::result::Result<TenantId, AppError> {
    let tenants = tenants
        .into_iter()
        .filter(|tenant_id| !nimbus_system::is_system_tenant_id(tenant_id))
        .collect::<Vec<_>>();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_tenant_does_not_make_a_single_app_tenant_ambiguous() {
        let app_tenant = TenantId::new("demo").expect("app tenant should parse");

        assert_eq!(
            resolve_application_tenant(vec![
                nimbus_system::system_tenant_id().expect("system id should parse"),
                app_tenant.clone(),
            ])
            .expect("one application tenant should resolve"),
            app_tenant
        );
    }
}
