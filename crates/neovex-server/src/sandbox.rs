use neovex_core::TenantId;
use neovex_sandbox::SandboxHandle;

pub trait SandboxCatalog: Send + Sync + 'static {
    fn sandbox_for_service(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<SandboxHandle>;
}

#[derive(Debug, Default)]
pub struct EmptySandboxCatalog;

impl SandboxCatalog for EmptySandboxCatalog {
    fn sandbox_for_service(
        &self,
        _tenant_id: &TenantId,
        _service_name: &str,
    ) -> Option<SandboxHandle> {
        None
    }
}

#[cfg(test)]
mod tests {
    use neovex_core::TenantId;

    use super::{EmptySandboxCatalog, SandboxCatalog};

    #[test]
    fn empty_catalog_returns_none_for_unknown_service() {
        let catalog = EmptySandboxCatalog;
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");

        assert!(
            catalog.sandbox_for_service(&tenant_id, "db").is_none(),
            "empty sandbox catalog should not resolve services"
        );
    }
}
