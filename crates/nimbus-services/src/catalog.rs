use std::collections::BTreeMap;

use nimbus_core::TenantId;
use nimbus_sandbox::{SandboxHandle, SandboxSpec};

pub trait ServiceInstanceCatalog: Send + Sync + 'static {
    fn service_instances_for_tenant(&self, tenant_id: &TenantId)
    -> BTreeMap<String, SandboxHandle>;

    fn service_instance_for_name(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<SandboxHandle> {
        self.service_instances_for_tenant(tenant_id)
            .remove(service_name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceBackend {
    Sandbox(SandboxSpec),
    BuiltIn(BuiltInServiceSpec),
    External(ExternalServiceSpec),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltInServiceSpec {
    provider: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalServiceSpec {
    endpoint: String,
}

impl ServiceBackend {
    pub fn sandbox(spec: SandboxSpec) -> Self {
        Self::Sandbox(spec)
    }

    pub fn built_in(provider: impl Into<String>) -> Self {
        Self::BuiltIn(BuiltInServiceSpec::new(provider))
    }

    pub fn external(endpoint: impl Into<String>) -> Self {
        Self::External(ExternalServiceSpec::new(endpoint))
    }

    pub fn sandbox_spec(&self) -> Option<&SandboxSpec> {
        match self {
            Self::Sandbox(spec) => Some(spec),
            Self::BuiltIn(_) | Self::External(_) => None,
        }
    }

    pub fn into_sandbox_spec(self) -> Option<SandboxSpec> {
        match self {
            Self::Sandbox(spec) => Some(spec),
            Self::BuiltIn(_) | Self::External(_) => None,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::Sandbox(_) => "sandbox",
            Self::BuiltIn(_) => "built-in",
            Self::External(_) => "external",
        }
    }
}

impl BuiltInServiceSpec {
    pub fn new(provider: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
        }
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }
}

impl ExternalServiceSpec {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

pub trait ServiceDefinitionCatalog: Send + Sync + 'static {
    fn service_backend_for_tenant(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<ServiceBackend>;
}

#[derive(Debug, Default)]
pub struct EmptyServiceInstanceCatalog;

impl ServiceInstanceCatalog for EmptyServiceInstanceCatalog {
    fn service_instances_for_tenant(
        &self,
        _tenant_id: &TenantId,
    ) -> BTreeMap<String, SandboxHandle> {
        BTreeMap::new()
    }
}

#[derive(Debug, Default)]
pub struct EmptyServiceDefinitionCatalog;

impl ServiceDefinitionCatalog for EmptyServiceDefinitionCatalog {
    fn service_backend_for_tenant(
        &self,
        _tenant_id: &TenantId,
        _service_name: &str,
    ) -> Option<ServiceBackend> {
        None
    }
}

#[cfg(test)]
mod tests {
    use nimbus_core::TenantId;

    use super::{
        EmptyServiceDefinitionCatalog, EmptyServiceInstanceCatalog, ServiceDefinitionCatalog,
        ServiceInstanceCatalog,
    };

    #[test]
    fn empty_catalog_returns_none_for_unknown_service() {
        let catalog = EmptyServiceInstanceCatalog;
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");

        assert!(
            catalog
                .service_instance_for_name(&tenant_id, "db")
                .is_none(),
            "empty service instance catalog should not resolve services"
        );
    }

    #[test]
    fn empty_catalog_returns_no_tenant_sandboxes() {
        let catalog = EmptyServiceInstanceCatalog;
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");

        assert!(
            catalog.service_instances_for_tenant(&tenant_id).is_empty(),
            "empty service instance catalog should not list tenant services"
        );
    }

    #[test]
    fn empty_service_catalog_returns_none_for_unknown_service() {
        let catalog = EmptyServiceDefinitionCatalog;
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");

        assert!(
            catalog
                .service_backend_for_tenant(&tenant_id, "db")
                .is_none(),
            "empty service definition catalog should not declare services"
        );
    }
}
