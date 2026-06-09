use std::collections::BTreeMap;

use nimbus_core::TenantId;
use nimbus_sandbox::{SandboxBuildLaunchSpec, SandboxHandle, SandboxImageLaunchSpec, SandboxSpec};

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
pub enum ServiceImplementation {
    SandboxBacked(SandboxBackedServiceImplementation),
    BuiltIn(BuiltInServiceImplementation),
    External(ExternalServiceImplementation),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxBackedServiceImplementation {
    Image(SandboxImageLaunchSpec),
    Build(SandboxBuildLaunchSpec),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltInServiceImplementation {
    capability: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalServiceImplementation {
    endpoint: String,
}

impl ServiceImplementation {
    pub fn sandbox_image(launch: SandboxImageLaunchSpec) -> Self {
        Self::SandboxBacked(SandboxBackedServiceImplementation::Image(launch))
    }

    pub fn sandbox_build(launch: SandboxBuildLaunchSpec) -> Self {
        Self::SandboxBacked(SandboxBackedServiceImplementation::Build(launch))
    }

    pub fn built_in(capability: impl Into<String>) -> Self {
        Self::BuiltIn(BuiltInServiceImplementation::new(capability))
    }

    pub fn external(endpoint: impl Into<String>) -> Self {
        Self::External(ExternalServiceImplementation::new(endpoint))
    }

    pub fn sandbox_backed(&self) -> Option<&SandboxBackedServiceImplementation> {
        match self {
            Self::SandboxBacked(implementation) => Some(implementation),
            Self::BuiltIn(_) | Self::External(_) => None,
        }
    }

    pub fn into_sandbox_backed(self) -> Option<SandboxBackedServiceImplementation> {
        match self {
            Self::SandboxBacked(implementation) => Some(implementation),
            Self::BuiltIn(_) | Self::External(_) => None,
        }
    }

    pub fn implementation_kind(&self) -> &'static str {
        match self {
            Self::SandboxBacked(_) => "sandbox-backed",
            Self::BuiltIn(_) => "built-in",
            Self::External(_) => "external",
        }
    }
}

impl SandboxBackedServiceImplementation {
    pub fn spec(&self) -> &SandboxSpec {
        match self {
            Self::Image(launch) => &launch.spec,
            Self::Build(launch) => &launch.spec,
        }
    }
}

impl BuiltInServiceImplementation {
    pub fn new(capability: impl Into<String>) -> Self {
        Self {
            capability: capability.into(),
        }
    }

    pub fn capability(&self) -> &str {
        &self.capability
    }
}

impl ExternalServiceImplementation {
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
    fn service_implementation_for_tenant(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<ServiceImplementation>;
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
    fn service_implementation_for_tenant(
        &self,
        _tenant_id: &TenantId,
        _service_name: &str,
    ) -> Option<ServiceImplementation> {
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
                .service_implementation_for_tenant(&tenant_id, "db")
                .is_none(),
            "empty service definition catalog should not declare services"
        );
    }
}
