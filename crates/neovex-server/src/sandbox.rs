use std::collections::BTreeMap;

use neovex_core::TenantId;
use neovex_sandbox::{SandboxBuildLaunchSpec, SandboxHandle, SandboxImageLaunchSpec, SandboxSpec};

pub trait SandboxCatalog: Send + Sync + 'static {
    fn sandboxes_for_tenant(&self, tenant_id: &TenantId) -> BTreeMap<String, SandboxHandle>;

    fn sandbox_for_service(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<SandboxHandle> {
        self.sandboxes_for_tenant(tenant_id).remove(service_name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxServiceLaunch {
    Image(SandboxImageLaunchSpec),
    Build(SandboxBuildLaunchSpec),
}

impl SandboxServiceLaunch {
    pub fn image(launch: SandboxImageLaunchSpec) -> Self {
        Self::Image(launch)
    }

    pub fn build(launch: SandboxBuildLaunchSpec) -> Self {
        Self::Build(launch)
    }

    pub fn spec(&self) -> &SandboxSpec {
        match self {
            Self::Image(launch) => &launch.spec,
            Self::Build(launch) => &launch.spec,
        }
    }
}

pub trait SandboxServiceCatalog: Send + Sync + 'static {
    fn sandbox_service_for_tenant(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<SandboxServiceLaunch>;
}

#[derive(Debug, Default)]
pub struct EmptySandboxCatalog;

impl SandboxCatalog for EmptySandboxCatalog {
    fn sandboxes_for_tenant(&self, _tenant_id: &TenantId) -> BTreeMap<String, SandboxHandle> {
        BTreeMap::new()
    }
}

#[derive(Debug, Default)]
pub struct EmptySandboxServiceCatalog;

impl SandboxServiceCatalog for EmptySandboxServiceCatalog {
    fn sandbox_service_for_tenant(
        &self,
        _tenant_id: &TenantId,
        _service_name: &str,
    ) -> Option<SandboxServiceLaunch> {
        None
    }
}

#[cfg(test)]
mod tests {
    use neovex_core::TenantId;

    use super::{
        EmptySandboxCatalog, EmptySandboxServiceCatalog, SandboxCatalog, SandboxServiceCatalog,
    };

    #[test]
    fn empty_catalog_returns_none_for_unknown_service() {
        let catalog = EmptySandboxCatalog;
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");

        assert!(
            catalog.sandbox_for_service(&tenant_id, "db").is_none(),
            "empty sandbox catalog should not resolve services"
        );
    }

    #[test]
    fn empty_catalog_returns_no_tenant_sandboxes() {
        let catalog = EmptySandboxCatalog;
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");

        assert!(
            catalog.sandboxes_for_tenant(&tenant_id).is_empty(),
            "empty sandbox catalog should not list tenant services"
        );
    }

    #[test]
    fn empty_service_catalog_returns_none_for_unknown_service() {
        let catalog = EmptySandboxServiceCatalog;
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");

        assert!(
            catalog
                .sandbox_service_for_tenant(&tenant_id, "db")
                .is_none(),
            "empty sandbox service catalog should not declare services"
        );
    }
}
