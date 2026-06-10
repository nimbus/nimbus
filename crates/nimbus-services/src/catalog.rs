use std::collections::BTreeMap;

use nimbus_core::TenantId;
use nimbus_sandbox::{SandboxHandle, SandboxSpec};
use nimbus_tenant::TenantVolumePolicyDecision;

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
    Sandbox(Box<SandboxSpec>),
    BuiltIn(BuiltInServiceSpec),
    External(ExternalServiceSpec),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltInServiceSpec {
    provider: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalServiceSpec {
    endpoint_url: String,
    auth: ExternalAuthPolicy,
    health: HealthCheckPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalAuthPolicy {
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthCheckPolicy {
    Http { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceDefinition {
    pub tenant_id: TenantId,
    pub name: String,
    pub backend: ServiceBackend,
    pub generation: u64,
    pub resource_version: String,
    pub created_at_millis: u64,
    pub updated_at_millis: u64,
    pub labels: BTreeMap<String, String>,
    pub source: ServiceDefinitionSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceDefinitionSource {
    StaticCatalog,
    Dynamic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxResource {
    pub tenant_id: TenantId,
    pub id: String,
    pub profile: String,
    pub spec: SandboxSpec,
    pub handle: SandboxHandle,
    pub generation: u64,
    pub resource_version: String,
    pub created_at_millis: u64,
    pub updated_at_millis: u64,
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionTarget {
    Service { name: String },
    Sandbox { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionTargetSnapshot {
    Service {
        name: String,
        generation: u64,
        backend: String,
        provider: Option<String>,
    },
    Sandbox {
        id: String,
        generation: u64,
        profile: String,
        backend: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionLifecycleState {
    Open,
    Closed,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionResource {
    pub tenant_id: TenantId,
    pub id: String,
    pub target: SessionTarget,
    pub target_snapshot: SessionTargetSnapshot,
    pub channels: Vec<String>,
    pub lifecycle_state: SessionLifecycleState,
    pub generation: u64,
    pub resource_version: String,
    pub created_at_millis: u64,
    pub updated_at_millis: u64,
    pub expires_at_millis: u64,
    pub closed_at_millis: Option<u64>,
    pub close_reason: Option<String>,
}

impl ServiceBackend {
    pub fn sandbox(spec: SandboxSpec) -> Self {
        Self::Sandbox(Box::new(spec))
    }

    pub fn built_in(provider: impl Into<String>) -> Self {
        Self::BuiltIn(BuiltInServiceSpec::new(provider))
    }

    pub fn external(
        endpoint_url: impl Into<String>,
        auth: ExternalAuthPolicy,
        health: HealthCheckPolicy,
    ) -> Self {
        Self::External(ExternalServiceSpec::new(endpoint_url, auth, health))
    }

    pub fn sandbox_spec(&self) -> Option<&SandboxSpec> {
        match self {
            Self::Sandbox(spec) => Some(spec),
            Self::BuiltIn(_) | Self::External(_) => None,
        }
    }

    pub fn into_sandbox_spec(self) -> Option<SandboxSpec> {
        match self {
            Self::Sandbox(spec) => Some(*spec),
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
    pub fn new(
        endpoint_url: impl Into<String>,
        auth: ExternalAuthPolicy,
        health: HealthCheckPolicy,
    ) -> Self {
        Self {
            endpoint_url: endpoint_url.into(),
            auth,
            health,
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint_url
    }

    pub fn auth(&self) -> ExternalAuthPolicy {
        self.auth
    }

    pub fn health(&self) -> &HealthCheckPolicy {
        &self.health
    }
}

impl ServiceDefinition {
    pub fn dynamic(
        tenant_id: TenantId,
        name: impl Into<String>,
        backend: ServiceBackend,
        generation: u64,
        resource_version: impl Into<String>,
        now_millis: u64,
        labels: BTreeMap<String, String>,
    ) -> Self {
        Self {
            tenant_id,
            name: name.into(),
            backend,
            generation,
            resource_version: resource_version.into(),
            created_at_millis: now_millis,
            updated_at_millis: now_millis,
            labels,
            source: ServiceDefinitionSource::Dynamic,
        }
    }

    pub fn static_catalog(
        tenant_id: TenantId,
        name: impl Into<String>,
        backend: ServiceBackend,
    ) -> Self {
        let name = name.into();
        Self {
            tenant_id,
            resource_version: format!("static:{name}"),
            name,
            backend,
            generation: 0,
            created_at_millis: 0,
            updated_at_millis: 0,
            labels: BTreeMap::new(),
            source: ServiceDefinitionSource::StaticCatalog,
        }
    }
}

pub trait ServiceDefinitionCatalog: Send + Sync + 'static {
    fn service_backend_for_tenant(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<ServiceBackend>;

    fn service_backends_for_tenant(
        &self,
        _tenant_id: &TenantId,
    ) -> BTreeMap<String, ServiceBackend> {
        BTreeMap::new()
    }

    fn service_volume_policy_for_tenant(
        &self,
        _tenant_id: &TenantId,
        _service_name: &str,
    ) -> TenantVolumePolicyDecision {
        TenantVolumePolicyDecision::default()
    }
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

    #[test]
    fn empty_service_catalog_returns_empty_volume_policy() {
        let catalog = EmptyServiceDefinitionCatalog;
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");

        assert!(
            catalog
                .service_volume_policy_for_tenant(&tenant_id, "db")
                .named_volumes()
                .is_empty(),
            "empty service definition catalog should not authorize service volumes"
        );
    }
}
