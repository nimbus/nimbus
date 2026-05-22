use nimbus_core::{Error, PrincipalContext, Result, TenantId};
use nimbus_runtime::RuntimeBundle;
use nimbus_sandbox::{SandboxBackendKind, SandboxSpec};

use crate::sandbox::SandboxServiceLaunch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TenantIsolationAuthority {
    Operator,
    Application { principal: PrincipalContext },
    System,
}

impl TenantIsolationAuthority {
    fn describe(&self) -> String {
        match self {
            Self::Operator => "operator".to_string(),
            Self::System => "system".to_string(),
            Self::Application { principal } if principal.authenticated => {
                "application(authenticated)".to_string()
            }
            Self::Application { .. } => "application(anonymous)".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TenantIsolationContext {
    tenant_id: TenantId,
    authority: TenantIsolationAuthority,
    surface: &'static str,
    deployment_generation: Option<u64>,
}

impl TenantIsolationContext {
    pub(crate) fn operator(tenant_id: TenantId, surface: &'static str) -> Self {
        Self {
            tenant_id,
            authority: TenantIsolationAuthority::Operator,
            surface,
            deployment_generation: None,
        }
    }

    pub(crate) fn application(
        tenant_id: TenantId,
        principal: PrincipalContext,
        surface: &'static str,
    ) -> Self {
        Self {
            tenant_id,
            authority: TenantIsolationAuthority::Application { principal },
            surface,
            deployment_generation: None,
        }
    }

    pub(crate) fn system(tenant_id: TenantId, surface: &'static str) -> Self {
        Self {
            tenant_id,
            authority: TenantIsolationAuthority::System,
            surface,
            deployment_generation: None,
        }
    }

    pub(crate) fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub(crate) fn reauthorize_application(
        &self,
        principal: PrincipalContext,
        surface: &'static str,
    ) -> Self {
        let mut context = Self::application(self.tenant_id.clone(), principal, surface);
        if let Some(generation) = self.deployment_generation {
            context = context.with_deployment_generation(generation);
        }
        context
    }

    pub(crate) fn with_deployment_generation(mut self, generation: u64) -> Self {
        self.deployment_generation = Some(generation);
        self
    }

    pub(crate) fn for_service(
        &self,
        service_name: impl Into<String>,
    ) -> TenantServiceIsolationContext {
        TenantServiceIsolationContext {
            tenant: self.clone(),
            service_name: service_name.into(),
        }
    }

    pub(crate) fn ensure_tenant_matches(&self, actual: &TenantId, context: &str) -> Result<()> {
        if actual == &self.tenant_id {
            return Ok(());
        }
        Err(Error::InvalidInput(format!(
            "tenant isolation context for {} on {} authorized tenant {}, but {context} referenced tenant {}",
            self.authority.describe(),
            self.surface,
            self.tenant_id,
            actual
        )))
    }

    pub(crate) fn ensure_runtime_bundle_matches(
        &self,
        bundle: &RuntimeBundle,
        context: &str,
    ) -> Result<()> {
        let Some(tenant_label) = bundle.identity().tenant_label() else {
            return Ok(());
        };
        let actual = TenantId::new(tenant_label.to_string())?;
        self.ensure_tenant_matches(&actual, context)
    }

    pub(crate) fn ensure_deployment_generation_matches(
        &self,
        actual_generation: u64,
        context: &str,
    ) -> Result<()> {
        let Some(expected_generation) = self.deployment_generation else {
            return Ok(());
        };
        if expected_generation == actual_generation {
            return Ok(());
        }
        Err(Error::InvalidInput(format!(
            "tenant isolation context for {} on {} authorized deployment generation {}, but {context} referenced deployment generation {}",
            self.authority.describe(),
            self.surface,
            expected_generation,
            actual_generation
        )))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TenantServiceIsolationContext {
    tenant: TenantIsolationContext,
    service_name: String,
}

impl TenantServiceIsolationContext {
    pub(crate) fn tenant_id(&self) -> &TenantId {
        self.tenant.tenant_id()
    }

    pub(crate) fn ensure_sandbox_launch_matches(
        &self,
        launch: &SandboxServiceLaunch,
        actual_backend: SandboxBackendKind,
    ) -> Result<()> {
        let spec = launch.spec();
        self.ensure_sandbox_spec_matches(spec, actual_backend)
    }

    pub(crate) fn ensure_sandbox_spec_matches(
        &self,
        spec: &SandboxSpec,
        actual_backend: SandboxBackendKind,
    ) -> Result<()> {
        if spec.backend != actual_backend {
            return Err(Error::InvalidInput(format!(
                "sandbox service {} for tenant {} requested backend {:?}, but the configured manager backend is {:?}",
                self.service_name,
                self.tenant_id(),
                spec.backend,
                actual_backend
            )));
        }
        if spec.name != self.service_name {
            return Err(Error::InvalidInput(format!(
                "sandbox service catalog returned launch spec name {} for requested service {}",
                spec.name, self.service_name
            )));
        }
        self.tenant
            .ensure_tenant_matches(&spec.tenant_id, "sandbox service launch spec")
    }
}

#[cfg(test)]
mod tests {
    use nimbus_sandbox::{
        SandboxBackendKind, SandboxFilesystemSpec, SandboxImageLaunchSpec, SandboxProcessSpec,
    };

    use super::*;

    fn sparse_spec(tenant: &str, name: &str, backend: SandboxBackendKind) -> SandboxSpec {
        SandboxSpec::new(
            TenantId::new(tenant).expect("tenant id should parse"),
            name,
            backend,
            SandboxFilesystemSpec::new(""),
            SandboxProcessSpec::new(Vec::<String>::new()),
        )
    }

    fn tenant_labeled_bundle(tenant: &str) -> RuntimeBundle {
        RuntimeBundle::for_tenant(
            "bundle.mjs",
            "0000000000000000000000000000000000000000000000000000000000000000",
            tenant,
        )
        .expect("test runtime bundle should build")
    }

    #[test]
    fn tenant_context_rejects_mismatched_tenant_before_launch() {
        let context = TenantIsolationContext::operator(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            "test",
        )
        .for_service("db");
        let launch = SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
            sparse_spec("tenant-b", "db", SandboxBackendKind::Krun),
            "postgres:16",
        ));

        let error = context
            .ensure_sandbox_launch_matches(&launch, SandboxBackendKind::Krun)
            .expect_err("mismatched tenant must be rejected before sandbox launch");
        assert!(
            error.to_string().contains("authorized tenant tenant-a"),
            "error should name the authorized tenant: {error}"
        );
        assert!(
            error.to_string().contains("referenced tenant tenant-b"),
            "error should name the rejected tenant: {error}"
        );
    }

    #[test]
    fn tenant_context_rejects_mismatched_service_before_launch() {
        let context = TenantIsolationContext::operator(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            "test",
        )
        .for_service("db");
        let launch = SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
            sparse_spec("tenant-a", "cache", SandboxBackendKind::Krun),
            "redis:7",
        ));

        let error = context
            .ensure_sandbox_launch_matches(&launch, SandboxBackendKind::Krun)
            .expect_err("mismatched service name must be rejected before sandbox launch");
        assert!(
            error
                .to_string()
                .contains("returned launch spec name cache"),
            "error should name the rejected service: {error}"
        );
    }

    #[test]
    fn tenant_context_rejects_mismatched_backend_before_launch() {
        let context = TenantIsolationContext::operator(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            "test",
        )
        .for_service("db");
        let launch = SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
            sparse_spec("tenant-a", "db", SandboxBackendKind::Container),
            "postgres:16",
        ));

        let error = context
            .ensure_sandbox_launch_matches(&launch, SandboxBackendKind::Krun)
            .expect_err("mismatched backend must be rejected before sandbox launch");
        assert!(
            error.to_string().contains("requested backend Container"),
            "error should name the rejected backend: {error}"
        );
    }

    #[test]
    fn tenant_context_rejects_mismatched_runtime_bundle_before_invocation() {
        let context = TenantIsolationContext::application(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            PrincipalContext::anonymous(),
            "test",
        );
        let bundle = tenant_labeled_bundle("tenant-b");

        let error = context
            .ensure_runtime_bundle_matches(&bundle, "runtime bundle")
            .expect_err("mismatched runtime bundle tenant must be rejected before invocation");
        assert!(
            error.to_string().contains("authorized tenant tenant-a"),
            "error should name the authorized tenant: {error}"
        );
        assert!(
            error.to_string().contains("referenced tenant tenant-b"),
            "error should name the rejected tenant: {error}"
        );
    }

    #[test]
    fn tenant_context_rejects_mismatched_deployment_before_invocation() {
        let context = TenantIsolationContext::application(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            PrincipalContext::anonymous(),
            "test",
        )
        .with_deployment_generation(7);

        let error = context
            .ensure_deployment_generation_matches(8, "runtime invocation")
            .expect_err("mismatched deployment generation must be rejected");
        assert!(
            error
                .to_string()
                .contains("authorized deployment generation 7"),
            "error should name the authorized deployment generation: {error}"
        );
        assert!(
            error
                .to_string()
                .contains("referenced deployment generation 8"),
            "error should name the rejected deployment generation: {error}"
        );
    }

    #[test]
    fn reauthorized_application_context_preserves_tenant_and_deployment_generation() {
        let context = TenantIsolationContext::application(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            PrincipalContext::anonymous(),
            "convex websocket route",
        )
        .with_deployment_generation(42);

        let derived =
            context.reauthorize_application(PrincipalContext::system(), "convex subscription");

        derived
            .ensure_tenant_matches(
                &TenantId::new("tenant-a").expect("tenant id should parse"),
                "derived context tenant",
            )
            .expect("derived context should preserve tenant identity");
        derived
            .ensure_deployment_generation_matches(42, "derived context deployment")
            .expect("derived context should preserve deployment generation");
        let error = derived
            .ensure_deployment_generation_matches(43, "stale subscription runtime")
            .expect_err("derived context must still reject stale deployment generations");
        assert!(
            error
                .to_string()
                .contains("authorized deployment generation 42"),
            "error should name the preserved deployment generation: {error}"
        );
    }
}
