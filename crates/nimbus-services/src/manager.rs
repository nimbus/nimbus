use std::sync::{Arc, Mutex};
use std::time::Duration;

use nimbus_sandbox::SandboxBackend;
use tokio::sync::Notify;

mod catalog;
mod clock;
mod definition_mutation;
mod definitions;
mod handles;
mod registry;
mod retirement;
mod sandboxes;
mod session_channels;
mod sessions;
mod source;
mod system_state;
mod types;
mod verification;

use crate::ServiceDefinitionCatalog;
use nimbus_tenant::{TenantImagePolicyDecision, TenantImageVerificationProvider};

use types::ServiceManagerState;
use verification::DefaultTenantImageVerificationProvider;

pub use retirement::{TenantServiceRetirement, TenantServiceRetirementFuture};
pub use source::{SandboxServiceProvisionSource, StandaloneSandboxProvisionSource};
pub use system_state::{NoopServiceEvidenceWriter, ServiceEvidenceFuture, ServiceEvidenceWriter};

const DEFAULT_DEFINITION_MUTATION_TIMEOUT: Duration = Duration::from_secs(10);

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
    sandbox_backend: Arc<dyn SandboxBackend>,
    image_verification_provider: Arc<dyn TenantImageVerificationProvider>,
    local_build_admission: LocalBuildAdmission,
    definition_mutation_timeout: Duration,
    state: Mutex<ServiceManagerState>,
    service_evidence_writer: Mutex<Arc<dyn ServiceEvidenceWriter>>,
    definition_mutation_notify: Notify,
    #[cfg(test)]
    definition_mutation_wait_observer: Mutex<Option<Arc<Notify>>>,
}

impl ServiceManager {
    pub fn new(
        service_definitions: Arc<dyn ServiceDefinitionCatalog>,
        sandbox_backend: Arc<dyn SandboxBackend>,
    ) -> Self {
        Self {
            service_definitions,
            sandbox_backend,
            image_verification_provider: Arc::new(DefaultTenantImageVerificationProvider),
            local_build_admission: LocalBuildAdmission::Denied,
            definition_mutation_timeout: DEFAULT_DEFINITION_MUTATION_TIMEOUT,
            state: Mutex::new(ServiceManagerState::default()),
            service_evidence_writer: Mutex::new(Arc::new(NoopServiceEvidenceWriter)),
            definition_mutation_notify: Notify::new(),
            #[cfg(test)]
            definition_mutation_wait_observer: Mutex::new(None),
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

    pub fn with_definition_mutation_timeout(
        mut self,
        definition_mutation_timeout: Duration,
    ) -> Self {
        self.definition_mutation_timeout = definition_mutation_timeout;
        self
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

    pub fn set_service_evidence_writer_arc(&self, writer: Arc<dyn ServiceEvidenceWriter>) {
        *self
            .service_evidence_writer
            .lock()
            .expect("service evidence writer lock should not be poisoned") = writer;
    }

    #[cfg(test)]
    fn set_definition_mutation_wait_observer(&self, observer: Arc<Notify>) {
        *self
            .definition_mutation_wait_observer
            .lock()
            .expect("definition mutation wait observer lock should not be poisoned") =
            Some(observer);
    }

    #[cfg(test)]
    fn notify_definition_mutation_wait_observer(&self) {
        if let Some(observer) = self
            .definition_mutation_wait_observer
            .lock()
            .expect("definition mutation wait observer lock should not be poisoned")
            .as_ref()
        {
            observer.notify_waiters();
        }
    }

    #[cfg(not(test))]
    fn notify_definition_mutation_wait_observer(&self) {}
}

#[cfg(test)]
#[path = "manager/tests/mod.rs"]
mod tests;
