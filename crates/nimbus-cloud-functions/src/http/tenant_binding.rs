use nimbus_core::{Error, Result, TenantId};

const RESERVED_TENANT_PREFIX: &str = "_";

/// Server-trusted tenant authority for an active Cloud Functions HTTP deployment.
///
/// HTTP request paths select a function target, never a tenant. The host creates
/// this binding from operator or deployment configuration and carries it through
/// the deployment snapshot into every HTTP invocation. Keeping the binding as a
/// distinct type prevents the served adapter from accidentally substituting a
/// caller-derived [`TenantId`] when multi-tenant storage is enabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudFunctionsHttpTenantBinding {
    tenant_id: TenantId,
}

impl CloudFunctionsHttpTenantBinding {
    /// Bind an active Cloud Functions HTTP deployment to one application tenant.
    pub fn new(tenant_id: TenantId) -> Result<Self> {
        if tenant_id.as_str().starts_with(RESERVED_TENANT_PREFIX) {
            return Err(Error::InvalidInput(format!(
                "cloud functions HTTP tenant binding cannot target reserved Nimbus tenant `{tenant_id}`"
            )));
        }
        Ok(Self { tenant_id })
    }

    /// Tenant fixed by the trusted deployment configuration.
    #[must_use]
    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_accepts_application_tenant() {
        let tenant_id = TenantId::new("tenant-a").expect("tenant id should parse");
        let binding =
            CloudFunctionsHttpTenantBinding::new(tenant_id.clone()).expect("binding should build");

        assert_eq!(binding.tenant_id(), &tenant_id);
    }

    #[test]
    fn binding_rejects_reserved_tenant() {
        let error = CloudFunctionsHttpTenantBinding::new(
            TenantId::new("_nimbus").expect("reserved tenant id should parse"),
        )
        .expect_err("reserved tenant must be refused");

        assert!(error.to_string().contains("reserved Nimbus tenant"));
    }
}
