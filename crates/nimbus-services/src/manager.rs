use std::sync::{Arc, Mutex};
use std::time::Duration;

use nimbus_sandbox::SandboxBackend;
use tokio::sync::Notify;

mod activation;
mod catalog;
mod clock;
mod definitions;
mod handles;
mod registry;
mod sandboxes;
mod service_start;
mod sessions;
mod system_state;
mod types;
mod verification;

use crate::ServiceDefinitionCatalog;
use nimbus_tenant::{TenantImagePolicyDecision, TenantImageVerificationProvider};

use types::ServiceManagerState;
use verification::DefaultTenantImageVerificationProvider;

pub use system_state::{NoopServiceEvidenceWriter, ServiceEvidenceFuture, ServiceEvidenceWriter};

const DEFAULT_ACTIVATION_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_ACTIVATION_POLL_INTERVAL: Duration = Duration::from_millis(25);

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
    activation_timeout: Duration,
    activation_poll_interval: Duration,
    state: Mutex<ServiceManagerState>,
    service_evidence_writer: Mutex<Arc<dyn ServiceEvidenceWriter>>,
    activation_notify: Notify,
    #[cfg(test)]
    activation_wait_observer: Mutex<Option<Arc<Notify>>>,
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
            activation_timeout: DEFAULT_ACTIVATION_TIMEOUT,
            activation_poll_interval: DEFAULT_ACTIVATION_POLL_INTERVAL,
            state: Mutex::new(ServiceManagerState::default()),
            service_evidence_writer: Mutex::new(Arc::new(NoopServiceEvidenceWriter)),
            activation_notify: Notify::new(),
            #[cfg(test)]
            activation_wait_observer: Mutex::new(None),
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

    pub fn with_activation_timeout(mut self, activation_timeout: Duration) -> Self {
        self.activation_timeout = activation_timeout;
        self
    }

    pub fn with_activation_poll_interval(mut self, activation_poll_interval: Duration) -> Self {
        self.activation_poll_interval = activation_poll_interval;
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
    fn set_activation_wait_observer(&self, observer: Arc<Notify>) {
        *self
            .activation_wait_observer
            .lock()
            .expect("activation wait observer lock should not be poisoned") = Some(observer);
    }

    #[cfg(test)]
    fn notify_activation_wait_observer(&self) {
        if let Some(observer) = self
            .activation_wait_observer
            .lock()
            .expect("activation wait observer lock should not be poisoned")
            .as_ref()
        {
            observer.notify_waiters();
        }
    }

    #[cfg(not(test))]
    fn notify_activation_wait_observer(&self) {}
}

#[cfg(test)]
#[path = "manager/tests/mod.rs"]
mod tests;
