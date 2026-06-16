use std::sync::{Arc, Mutex};
use std::time::Duration;

use nimbus_sandbox::SandboxBackend;
use tokio::sync::Notify;

mod activation;
mod catalog;
mod clock;
mod definitions;
mod handles;
mod launch;
mod registry;
mod sandboxes;
mod sessions;
mod system_state;
mod types;
mod verification;

#[cfg(test)]
use activation::service_lifecycle_decision;

use crate::ServiceDefinitionCatalog;
use nimbus_tenant::TenantImageVerificationProvider;

use types::ServiceManagerState;
use verification::DefaultTenantImageVerificationProvider;

pub use system_state::{NoopServiceEvidenceWriter, ServiceEvidenceFuture, ServiceEvidenceWriter};

const DEFAULT_ACTIVATION_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_ACTIVATION_POLL_INTERVAL: Duration = Duration::from_millis(25);

pub struct ServiceManager {
    service_definitions: Arc<dyn ServiceDefinitionCatalog>,
    sandbox_backend: Arc<dyn SandboxBackend>,
    image_verification_provider: Arc<dyn TenantImageVerificationProvider>,
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
            activation_timeout: DEFAULT_ACTIVATION_TIMEOUT,
            activation_poll_interval: DEFAULT_ACTIVATION_POLL_INTERVAL,
            state: Mutex::new(ServiceManagerState::default()),
            service_evidence_writer: Mutex::new(Arc::new(NoopServiceEvidenceWriter)),
            activation_notify: Notify::new(),
            #[cfg(test)]
            activation_wait_observer: Mutex::new(None),
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
