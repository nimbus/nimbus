use nimbus_sandbox::SandboxBackendKind;
use std::sync::{Arc, Mutex};

mod catalog;
mod clock;
mod definitions;
mod handles;
mod registry;
mod sandboxes;
mod session_channels;
mod sessions;
mod source;
mod source_retirement;
mod tenant_retirement;
mod types;
mod verification;
mod workload_namespace;

use crate::ServiceDefinitionCatalog;
use nimbus_tenant::{TenantImagePolicyDecision, TenantImageVerificationProvider};

use types::ServiceManagerState;
use verification::DefaultTenantImageVerificationProvider;

pub use source::{SandboxServiceProvisionSource, StandaloneSandboxProvisionSource};
pub use source_retirement::{
    WorkloadSourceRetirementClaim, WorkloadSourceRetirementIdentity,
    WorkloadSourceRetirementOperation,
};
pub use tenant_retirement::{TenantSourceRetirementClaim, TenantSourceRetirementSnapshot};

/// Whether the service manager admits local-build sandbox roots.
///
/// This is a manager runtime posture, not a durable catalog type, so it stays
/// descriptive and verb-free rather than following the catalog `Spec`/`Kind`
/// naming rules. It defaults fail-closed (`Denied`, the production posture):
/// local builds carry no signature/provenance/SBOM evidence, so the manager
/// rejects them unless the operator explicitly opts in, mirroring the compose
/// and HTTP layers that only admit builds in local development.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LocalBuildAdmission {
    /// Reject local-build sandbox roots (production posture, fail-closed).
    #[default]
    Denied,
    /// Admit local-build sandbox roots (operator opt-in, local development).
    Allowed,
}

pub struct ServiceManager {
    service_definitions: Arc<dyn ServiceDefinitionCatalog>,
    sandbox_backend_kind: SandboxBackendKind,
    image_verification_provider: Arc<dyn TenantImageVerificationProvider>,
    local_build_admission: LocalBuildAdmission,
    state: Mutex<ServiceManagerState>,
}

impl ServiceManager {
    pub fn new(
        service_definitions: Arc<dyn ServiceDefinitionCatalog>,
        sandbox_backend_kind: SandboxBackendKind,
    ) -> Self {
        Self {
            service_definitions,
            sandbox_backend_kind,
            image_verification_provider: Arc::new(DefaultTenantImageVerificationProvider),
            local_build_admission: LocalBuildAdmission::Denied,
            state: Mutex::new(ServiceManagerState::default()),
        }
    }

    /// Sets the manager's local-build admission posture.
    ///
    /// Defaults fail-closed to [`LocalBuildAdmission::Denied`]; callers that
    /// run in local development opt in with [`LocalBuildAdmission::Allowed`].
    pub fn with_local_build_admission(mut self, admission: LocalBuildAdmission) -> Self {
        self.local_build_admission = admission;
        self
    }

    /// Returns the tenant image policy decision the manager layers onto every
    /// lifecycle and sandbox admission decision.
    ///
    /// Production (`Denied`) layers the default fail-closed image policy, which
    /// rejects local builds. Local development (`Allowed`) additionally admits
    /// local builds. Registry-image admission floors (digest/signature/
    /// provenance/SBOM) are unaffected by this posture.
    pub(super) fn manager_image_policy(&self) -> TenantImagePolicyDecision {
        match self.local_build_admission {
            LocalBuildAdmission::Denied => TenantImagePolicyDecision::default(),
            LocalBuildAdmission::Allowed => {
                TenantImagePolicyDecision::default().allow_local_build()
            }
        }
    }

    pub fn with_image_verification_provider(
        mut self,
        provider: impl TenantImageVerificationProvider + 'static,
    ) -> Self {
        self.image_verification_provider = Arc::new(provider);
        self
    }

    pub fn with_image_verification_provider_arc(
        mut self,
        provider: Arc<dyn TenantImageVerificationProvider>,
    ) -> Self {
        self.image_verification_provider = provider;
        self
    }
}

#[cfg(test)]
#[path = "manager/tests/mod.rs"]
mod tests;
