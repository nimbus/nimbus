use std::sync::{Arc, Mutex};
use std::time::Duration;

use nimbus_sandbox::SandboxBackend;
use tokio::sync::Notify;

mod activation;
mod catalog;
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
mod tests {
    use std::collections::BTreeMap;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use nimbus_core::{Error, TenantId};
    use nimbus_runtime::HostCallCancellation;
    use nimbus_sandbox::{
        PublishedEndpoint, PublishedEndpointProtocol, SandboxBackend, SandboxBackendKind,
        SandboxEgressPolicy, SandboxEgressRule, SandboxError, SandboxFuture, SandboxHandle,
        SandboxId, SandboxMountSpec, SandboxOciBuildSpec, SandboxOciImageSource, SandboxOwnerSpec,
        SandboxProcessSpec, SandboxRootSpec, SandboxSpec, SandboxStatus,
    };

    use crate::{
        ExternalAuthPolicy, HealthCheckPolicy, RuntimeServiceRegistry, ServiceBackend,
        ServiceDefinitionCatalog, SessionTarget,
    };
    use nimbus_tenant::{
        TenantImageVerificationEvidence, TenantImageVerificationProvider, TenantIsolationContext,
        TenantIsolationPolicyInput, TenantServiceGrantPolicyDecision, TenantVolumePolicyDecision,
        WorkloadAttributes,
    };

    use super::*;

    mod definitions;

    struct StubServiceDefinitionCatalog {
        launches: BTreeMap<String, ServiceBackend>,
    }

    impl ServiceDefinitionCatalog for StubServiceDefinitionCatalog {
        fn service_backend_for_tenant(
            &self,
            _tenant_id: &TenantId,
            service_name: &str,
        ) -> Option<ServiceBackend> {
            self.launches.get(service_name).cloned()
        }
    }

    struct StubSandboxBackend {
        image_starts: AtomicUsize,
        build_starts: AtomicUsize,
        stop_calls: AtomicUsize,
        artifact_cleanup_calls: AtomicUsize,
        inspect_calls: AtomicUsize,
        egress_reloads: Mutex<Vec<(String, SandboxEgressPolicy)>>,
        ready_after_inspects: usize,
        handle_tenant_override: Option<TenantId>,
        handles: Mutex<BTreeMap<String, SandboxHandle>>,
    }

    impl StubSandboxBackend {
        fn new(ready_after_inspects: usize) -> Self {
            Self {
                image_starts: AtomicUsize::new(0),
                build_starts: AtomicUsize::new(0),
                stop_calls: AtomicUsize::new(0),
                artifact_cleanup_calls: AtomicUsize::new(0),
                inspect_calls: AtomicUsize::new(0),
                egress_reloads: Mutex::new(Vec::new()),
                ready_after_inspects,
                handle_tenant_override: None,
                handles: Mutex::new(BTreeMap::new()),
            }
        }

        fn with_handle_tenant_override(mut self, tenant_id: TenantId) -> Self {
            self.handle_tenant_override = Some(tenant_id);
            self
        }

        fn sandbox_handle(
            &self,
            tenant_id: &TenantId,
            service_name: &str,
            status: SandboxStatus,
        ) -> SandboxHandle {
            let endpoints = if status == SandboxStatus::Ready {
                vec![
                    PublishedEndpoint::new(
                        "postgres",
                        PublishedEndpointProtocol::Tcp,
                        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 15432),
                    )
                    .with_guest_port(5432),
                ]
            } else {
                Vec::new()
            };
            let handle_tenant_id = self
                .handle_tenant_override
                .as_ref()
                .unwrap_or(tenant_id)
                .clone();
            SandboxHandle::new(
                handle_tenant_id.clone(),
                SandboxId::new(format!("sandbox-{handle_tenant_id}-{service_name}")),
                service_name,
                SandboxBackendKind::Krun,
                status,
                endpoints,
            )
        }
    }

    struct RecordingImageVerifier {
        evidence: TenantImageVerificationEvidence,
        calls: AtomicUsize,
        references: Mutex<Vec<String>>,
    }

    impl RecordingImageVerifier {
        fn with_evidence(evidence: TenantImageVerificationEvidence) -> Self {
            Self {
                evidence,
                calls: AtomicUsize::new(0),
                references: Mutex::new(Vec::new()),
            }
        }
    }

    impl TenantImageVerificationProvider for RecordingImageVerifier {
        fn verify_registry_image(
            &self,
            request: &nimbus_tenant::TenantImageVerificationRequest,
        ) -> nimbus_core::Result<TenantImageVerificationEvidence> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.references
                .lock()
                .expect("image verifier references should not be poisoned")
                .push(request.image_reference().to_string());
            Ok(self.evidence.clone())
        }
    }

    impl SandboxBackend for StubSandboxBackend {
        fn kind(&self) -> SandboxBackendKind {
            SandboxBackendKind::Krun
        }

        fn start(&self, spec: SandboxSpec) -> SandboxFuture<SandboxHandle> {
            match &spec.root {
                SandboxRootSpec::Rootfs(_) => {
                    let message = format!("rootfs launch unsupported for {}", spec.display_name());
                    return Box::pin(async move { Err(SandboxError::InvalidSpec { message }) });
                }
                SandboxRootSpec::OciImage(image) => match &image.source {
                    SandboxOciImageSource::Reference(_) => {
                        self.image_starts.fetch_add(1, Ordering::SeqCst);
                    }
                    SandboxOciImageSource::Build(_) => {
                        self.build_starts.fetch_add(1, Ordering::SeqCst);
                    }
                },
            }
            let handle = self.sandbox_handle(
                &spec.tenant_id,
                spec.display_name(),
                SandboxStatus::Starting,
            );
            self.handles
                .lock()
                .expect("backend lock should not be poisoned")
                .insert(handle.id.as_str().to_owned(), handle.clone());
            Box::pin(async move { Ok(handle) })
        }

        fn inspect(&self, id: &SandboxId) -> SandboxFuture<Option<SandboxHandle>> {
            let inspect_call = self.inspect_calls.fetch_add(1, Ordering::SeqCst) + 1;
            let mut handles = self
                .handles
                .lock()
                .expect("backend lock should not be poisoned");
            let handle = handles.get_mut(id.as_str()).cloned().map(|mut handle| {
                if inspect_call >= self.ready_after_inspects {
                    handle =
                        self.sandbox_handle(&handle.tenant_id, &handle.name, SandboxStatus::Ready);
                    handles.insert(id.as_str().to_owned(), handle.clone());
                }
                handle
            });
            Box::pin(async move { Ok(handle) })
        }

        fn stop(&self, id: &SandboxId) -> SandboxFuture<()> {
            self.stop_calls.fetch_add(1, Ordering::SeqCst);
            self.handles
                .lock()
                .expect("backend lock should not be poisoned")
                .remove(id.as_str());
            Box::pin(async move { Ok(()) })
        }

        fn reload_egress_policy(
            &self,
            id: &SandboxId,
            egress: SandboxEgressPolicy,
        ) -> SandboxFuture<()> {
            self.egress_reloads
                .lock()
                .expect("backend lock should not be poisoned")
                .push((id.as_str().to_owned(), egress));
            Box::pin(async move { Ok(()) })
        }

        fn remove_tenant_artifacts(&self, _tenant_id: TenantId) -> SandboxFuture<()> {
            self.artifact_cleanup_calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { Ok(()) })
        }
    }

    fn sparse_image_spec(name: &str) -> SandboxSpec {
        sparse_image_spec_with_reference(name, "postgres:16")
    }

    fn sparse_image_spec_with_reference(
        name: &str,
        image_reference: impl Into<String>,
    ) -> SandboxSpec {
        SandboxSpec::new(
            TenantId::new("tenant").expect("tenant id should be valid"),
            SandboxOwnerSpec::service(name),
            SandboxBackendKind::Krun,
            SandboxRootSpec::oci_image_reference(image_reference),
            SandboxProcessSpec::new(Vec::<String>::new()),
        )
    }

    fn sparse_build_spec(
        name: &str,
        image_name: impl Into<String>,
        dockerfile_path: impl Into<std::path::PathBuf>,
        context_path: impl Into<std::path::PathBuf>,
    ) -> SandboxSpec {
        SandboxSpec::new(
            TenantId::new("tenant").expect("tenant id should be valid"),
            SandboxOwnerSpec::service(name),
            SandboxBackendKind::Krun,
            SandboxRootSpec::oci_image(SandboxOciImageSource::Build(SandboxOciBuildSpec::new(
                image_name,
                dockerfile_path,
                context_path,
            ))),
            SandboxProcessSpec::new(Vec::<String>::new()),
        )
    }

    fn standalone_resource_spec(tenant_id: &TenantId, display_name: &str) -> SandboxSpec {
        SandboxSpec::new(
            tenant_id.clone(),
            SandboxOwnerSpec::standalone_named(display_name),
            SandboxBackendKind::Krun,
            SandboxRootSpec::oci_image_reference("registry.example.com/task:latest"),
            SandboxProcessSpec::new(vec!["task".to_owned()]),
        )
    }

    fn image_service_backend(name: &str, image_reference: impl Into<String>) -> ServiceBackend {
        ServiceBackend::sandbox(sparse_image_spec_with_reference(name, image_reference))
    }

    fn build_service_backend(
        name: &str,
        image_name: impl Into<String>,
        dockerfile_path: impl Into<std::path::PathBuf>,
        context_path: impl Into<std::path::PathBuf>,
    ) -> ServiceBackend {
        ServiceBackend::sandbox(sparse_build_spec(
            name,
            image_name,
            dockerfile_path,
            context_path,
        ))
    }

    #[tokio::test]
    async fn start_service_for_decision_rejects_built_in_backend_before_launch() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(1));
        let manager = ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::from([(
                    "browser".to_owned(),
                    ServiceBackend::built_in("browser"),
                )]),
            }),
            backend.clone(),
        );
        let isolation =
            TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
        let decision = service_lifecycle_decision(&isolation, "browser")
            .expect("browser service activation decision should build");

        let error = manager
            .start_service_for_decision_async(&decision, "browser", HostCallCancellation::default())
            .await
            .expect_err("sandbox manager must reject built-in service backends");

        assert!(
            error.to_string().contains("built-in backend"),
            "error should name unsupported backing: {error}"
        );
        assert_eq!(
            backend.image_starts.load(Ordering::SeqCst),
            0,
            "built-in services must not reach image launch"
        );
        assert_eq!(
            backend.build_starts.load(Ordering::SeqCst),
            0,
            "built-in services must not reach build launch"
        );
    }

    #[tokio::test]
    async fn start_service_for_decision_rejects_unadmitted_service_before_launch() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(1));
        let manager = ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::from([(
                    "cache".to_owned(),
                    image_service_backend("cache", "redis:7"),
                )]),
            }),
            backend.clone(),
        );
        let isolation =
            TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
        let decision = service_lifecycle_decision(&isolation, "db")
            .expect("db service activation decision should build");

        let error = manager
            .start_service_for_decision_async(&decision, "cache", HostCallCancellation::default())
            .await
            .expect_err("decision must reject a forged lower-seam service name");

        assert!(
            error.to_string().contains("permission denied"),
            "error should map to permission denial: {error}"
        );
        assert!(
            error
                .to_string()
                .contains("did not authorize service `cache`"),
            "error should name the rejected service: {error}"
        );
        assert_eq!(
            backend.image_starts.load(Ordering::SeqCst),
            0,
            "unadmitted service should fail before the sandbox backend is called"
        );
    }

    #[tokio::test]
    async fn start_service_for_decision_rejects_unadmitted_sandbox_egress_before_launch() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(1));
        let egress = SandboxEgressPolicy::new([SandboxEgressRule::new(
            "stripe",
            PublishedEndpointProtocol::Https,
            "api.stripe.com",
            443,
        )]);
        let manager = ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    ServiceBackend::sandbox(sparse_image_spec("db").with_egress_policy(egress)),
                )]),
            }),
            backend.clone(),
        );
        let isolation =
            TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
        let decision = service_lifecycle_decision(&isolation, "db")
            .expect("db service activation decision should build");

        let error = manager
            .start_service_for_decision_async(&decision, "db", HostCallCancellation::default())
            .await
            .expect_err("decision must reject unadmitted sandbox egress policy");

        assert!(
            error
                .to_string()
                .contains("did not authorize sandbox egress policy"),
            "error should name the egress-policy mismatch: {error}"
        );
        assert_eq!(
            backend.image_starts.load(Ordering::SeqCst),
            0,
            "unadmitted egress should fail before the sandbox backend is called"
        );
    }

    #[tokio::test]
    async fn start_service_for_decision_accepts_matching_sandbox_egress_policy() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(1));
        let egress = SandboxEgressPolicy::new([SandboxEgressRule::new(
            "stripe",
            PublishedEndpointProtocol::Https,
            "api.stripe.com",
            443,
        )]);
        let manager = ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    ServiceBackend::sandbox(
                        sparse_image_spec("db").with_egress_policy(egress.clone()),
                    ),
                )]),
            }),
            backend.clone(),
        )
        .with_activation_poll_interval(Duration::from_millis(1))
        .with_activation_timeout(Duration::from_secs(1));
        let isolation =
            TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
        let decision = isolation
            .admit_decision(
                TenantIsolationPolicyInput::new(WorkloadAttributes::service("db"))
                    .with_services(TenantServiceGrantPolicyDecision::new(["db"]))
                    .with_network(
                        nimbus_tenant::TenantNetworkPolicyDecision::default()
                            .with_sandbox_egress(egress)
                            .expect("test egress policy should compile"),
                    ),
            )
            .expect("decision with matching egress should admit");

        manager
            .start_service_for_decision_async(&decision, "db", HostCallCancellation::default())
            .await
            .expect("matching egress policy should start")
            .expect("handle should be returned");

        assert_eq!(backend.image_starts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn start_service_for_decision_rejects_unverified_image_before_materialization() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let image = "registry.example.com/nimbus/api@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let backend = Arc::new(StubSandboxBackend::new(1));
        let manager = ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::from([("api".to_owned(), image_service_backend("api", image))]),
            }),
            backend.clone(),
        );
        let isolation =
            TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
        let decision = isolation
            .admit_decision(
                TenantIsolationPolicyInput::new(WorkloadAttributes::service("api"))
                    .with_services(TenantServiceGrantPolicyDecision::new(["api"]))
                    .with_image(
                        nimbus_tenant::TenantImagePolicyDecision::digest_pinned(image)
                            .require_signature("https://issuer.example.com", "repo:nimbus/api"),
                    ),
            )
            .expect("image policy decision should admit");

        let error = manager
            .start_service_for_decision_async(&decision, "api", HostCallCancellation::default())
            .await
            .expect_err("missing signature evidence should fail before image materialization");

        assert!(
            error.to_string().contains("requires a matching signature"),
            "image admission failure should be visible: {error}"
        );
        assert_eq!(
            backend.image_starts.load(Ordering::SeqCst),
            0,
            "unverified image must not reach sandbox materialization"
        );
    }

    #[tokio::test]
    async fn start_service_for_decision_admits_verified_image_before_materialization() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let image = "registry.example.com/nimbus/api@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let backend = Arc::new(StubSandboxBackend::new(1));
        let verifier = Arc::new(RecordingImageVerifier::with_evidence(
            TenantImageVerificationEvidence::new()
                .with_signature("https://issuer.example.com", "repo:nimbus/api"),
        ));
        let manager = ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::from([("api".to_owned(), image_service_backend("api", image))]),
            }),
            backend.clone(),
        )
        .with_image_verification_provider_arc(verifier.clone())
        .with_activation_poll_interval(Duration::from_millis(1))
        .with_activation_timeout(Duration::from_secs(1));
        let isolation =
            TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
        let decision = isolation
            .admit_decision(
                TenantIsolationPolicyInput::new(WorkloadAttributes::service("api"))
                    .with_services(TenantServiceGrantPolicyDecision::new(["api"]))
                    .with_image(
                        nimbus_tenant::TenantImagePolicyDecision::digest_pinned(image)
                            .require_signature("https://issuer.example.com", "repo:nimbus/api"),
                    ),
            )
            .expect("image policy decision should admit");

        manager
            .start_service_for_decision_async(&decision, "api", HostCallCancellation::default())
            .await
            .expect("verified image should start")
            .expect("handle should be returned");

        assert_eq!(backend.image_starts.load(Ordering::SeqCst), 1);
        assert_eq!(verifier.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            verifier
                .references
                .lock()
                .expect("image verifier references should not be poisoned")
                .as_slice(),
            [image]
        );
    }

    #[tokio::test]
    async fn reload_service_egress_for_decision_updates_active_backend_policy() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(1));
        let manager = ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    image_service_backend("db", "postgres:16"),
                )]),
            }),
            backend.clone(),
        )
        .with_activation_poll_interval(Duration::from_millis(1))
        .with_activation_timeout(Duration::from_secs(1));
        let isolation =
            TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
        let start_decision = service_lifecycle_decision(&isolation, "db")
            .expect("db service activation decision should build");
        let handle = manager
            .start_service_for_decision_async(
                &start_decision,
                "db",
                HostCallCancellation::default(),
            )
            .await
            .expect("service should start")
            .expect("handle should exist");
        let egress = SandboxEgressPolicy::new([SandboxEgressRule::new(
            "stripe",
            PublishedEndpointProtocol::Https,
            "api.stripe.com",
            443,
        )]);
        let reload_decision = isolation
            .admit_decision(
                TenantIsolationPolicyInput::new(WorkloadAttributes::service("db"))
                    .with_services(TenantServiceGrantPolicyDecision::new(["db"]))
                    .with_network(
                        nimbus_tenant::TenantNetworkPolicyDecision::default()
                            .with_sandbox_egress(egress.clone())
                            .expect("test egress policy should compile"),
                    ),
            )
            .expect("reload decision with egress should admit");

        let reloaded = manager
            .reload_service_egress_for_decision_async(&tenant_id, &reload_decision, "db")
            .await
            .expect("egress reload should apply")
            .expect("active handle should remain");

        assert_eq!(reloaded.id, handle.id);
        let reloads = backend
            .egress_reloads
            .lock()
            .expect("backend lock should not be poisoned");
        assert_eq!(reloads.len(), 1);
        assert_eq!(reloads[0].0, handle.id.as_str());
        assert_eq!(reloads[0].1, egress);
    }

    #[tokio::test]
    async fn ensure_service_binding_async_starts_declared_image_service_once() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(2));
        let manager = ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    image_service_backend("db", "postgres:16"),
                )]),
            }),
            backend.clone(),
        )
        .with_activation_poll_interval(Duration::from_millis(1))
        .with_activation_timeout(Duration::from_secs(1));

        let binding = manager
            .ensure_service_binding_async(&tenant_id, "db", HostCallCancellation::default())
            .await
            .expect("image-backed service activation should succeed")
            .expect("db binding should exist");

        assert_eq!(binding.host, "127.0.0.1");
        assert_eq!(binding.port, 15432);
        assert_eq!(backend.image_starts.load(Ordering::SeqCst), 1);
        assert_eq!(backend.build_starts.load(Ordering::SeqCst), 0);

        let second = manager
            .ensure_service_binding_async(&tenant_id, "db", HostCallCancellation::default())
            .await
            .expect("cached service activation should succeed")
            .expect("db binding should still exist");
        assert_eq!(second.port, 15432);
        assert_eq!(
            backend.image_starts.load(Ordering::SeqCst),
            1,
            "existing active handle should prevent duplicate starts"
        );

        let snapshot = manager.snapshot_for_tenant(&tenant_id);
        assert_eq!(
            snapshot
                .get("db")
                .expect("db binding should be in snapshot")
                .port,
            15432
        );
    }

    #[tokio::test]
    async fn resolve_service_binding_refreshes_cached_handle_before_projecting_endpoint() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(1));
        let manager = ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    image_service_backend("db", "postgres:16"),
                )]),
            }),
            backend.clone(),
        )
        .with_activation_poll_interval(Duration::from_millis(1))
        .with_activation_timeout(Duration::from_secs(1));

        manager
            .ensure_service_binding_async(&tenant_id, "db", HostCallCancellation::default())
            .await
            .expect("service activation should succeed")
            .expect("db binding should exist");
        assert!(manager.snapshot_for_tenant(&tenant_id).contains_key("db"));

        let sandbox_id = backend
            .handles
            .lock()
            .expect("backend lock should not be poisoned")
            .keys()
            .next()
            .expect("backend should have a started sandbox")
            .clone();
        backend
            .handles
            .lock()
            .expect("backend lock should not be poisoned")
            .remove(&sandbox_id);
        let inspect_calls_before = backend.inspect_calls.load(Ordering::SeqCst);

        let binding = manager
            .resolve_service_binding(&tenant_id, "db")
            .expect("service binding refresh should not fail");

        assert!(
            binding.is_none(),
            "runtime binding resolution must not hand out endpoints for vanished sandboxes"
        );
        assert_eq!(
            backend.inspect_calls.load(Ordering::SeqCst),
            inspect_calls_before + 1,
            "resolve_service_binding should verify cached handles with the sandbox backend"
        );
        assert!(
            manager.snapshot_for_tenant(&tenant_id).is_empty(),
            "stale handle should be removed from future snapshots"
        );
    }

    #[tokio::test]
    async fn stop_service_for_context_async_stops_active_handle_and_clears_snapshot() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(1));
        let manager = ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    image_service_backend("db", "postgres:16"),
                )]),
            }),
            backend.clone(),
        )
        .with_activation_poll_interval(Duration::from_millis(1))
        .with_activation_timeout(Duration::from_secs(1));
        let isolation =
            TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");

        manager
            .start_service_for_context_async(&isolation, "db", HostCallCancellation::default())
            .await
            .expect("service should start")
            .expect("active handle should exist");
        let stopped = manager
            .stop_service_for_context_async(&isolation, "db")
            .await
            .expect("service should stop")
            .expect("stopped handle should be returned");

        assert_eq!(stopped.status, SandboxStatus::Stopped);
        assert!(stopped.published_endpoints.is_empty());
        assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 1);
        assert!(
            manager.snapshot_for_tenant(&tenant_id).is_empty(),
            "stopped service should not remain in runtime service snapshots"
        );
    }

    #[tokio::test]
    async fn stop_service_for_decision_async_requires_exact_service_grant() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(1));
        let manager = ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    image_service_backend("db", "postgres:16"),
                )]),
            }),
            backend.clone(),
        )
        .with_activation_poll_interval(Duration::from_millis(1))
        .with_activation_timeout(Duration::from_secs(1));
        let isolation =
            TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
        manager
            .start_service_for_context_async(&isolation, "db", HostCallCancellation::default())
            .await
            .expect("service should start")
            .expect("active handle should exist");
        let denied_decision = isolation
            .admit_decision(
                TenantIsolationPolicyInput::new(WorkloadAttributes::service("db")).with_image(
                    nimbus_tenant::TenantImagePolicyDecision::default().allow_local_build(),
                ),
            )
            .expect("decision without service grant should still build");

        let error = manager
            .stop_service_for_decision_async(&denied_decision, "db")
            .await
            .expect_err("stop must require an exact service grant");

        assert!(
            error.to_string().contains("db"),
            "service grant error should name the denied service: {error}"
        );
        assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 0);
        assert!(
            !manager.snapshot_for_tenant(&tenant_id).is_empty(),
            "denied stop must leave the active service snapshot intact"
        );
    }

    #[tokio::test]
    async fn restart_service_for_context_async_stops_then_starts_service() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(1));
        let manager = ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    image_service_backend("db", "postgres:16"),
                )]),
            }),
            backend.clone(),
        )
        .with_activation_poll_interval(Duration::from_millis(1))
        .with_activation_timeout(Duration::from_secs(1));
        let isolation =
            TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");

        manager
            .start_service_for_context_async(&isolation, "db", HostCallCancellation::default())
            .await
            .expect("initial service start should succeed")
            .expect("initial handle should exist");
        let restarted = manager
            .restart_service_for_context_async(&isolation, "db", HostCallCancellation::default())
            .await
            .expect("restart should succeed")
            .expect("restarted handle should exist");

        assert_eq!(restarted.status, SandboxStatus::Ready);
        assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            backend.image_starts.load(Ordering::SeqCst),
            2,
            "restart should materialize a fresh sandbox-backed service"
        );
    }

    #[tokio::test]
    async fn restart_service_for_decision_async_requires_exact_service_grant_before_stop() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(1));
        let manager = ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    image_service_backend("db", "postgres:16"),
                )]),
            }),
            backend.clone(),
        )
        .with_activation_poll_interval(Duration::from_millis(1))
        .with_activation_timeout(Duration::from_secs(1));
        let isolation =
            TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
        manager
            .start_service_for_context_async(&isolation, "db", HostCallCancellation::default())
            .await
            .expect("service should start")
            .expect("active handle should exist");
        let denied_decision = isolation
            .admit_decision(
                TenantIsolationPolicyInput::new(WorkloadAttributes::service("db")).with_image(
                    nimbus_tenant::TenantImagePolicyDecision::default().allow_local_build(),
                ),
            )
            .expect("decision without service grant should still build");

        manager
            .restart_service_for_decision_async(
                &denied_decision,
                "db",
                HostCallCancellation::default(),
            )
            .await
            .expect_err("restart must require an exact service grant before stopping");

        assert_eq!(
            backend.stop_calls.load(Ordering::SeqCst),
            0,
            "denied restart must not stop the active sandbox first"
        );
        assert_eq!(
            backend.image_starts.load(Ordering::SeqCst),
            1,
            "denied restart must not materialize a replacement sandbox"
        );
    }

    #[tokio::test]
    async fn ensure_service_binding_async_rejects_backend_handle_for_wrong_tenant() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(1).with_handle_tenant_override(
            TenantId::new("tenant-b").expect("tenant id should be valid"),
        ));
        let manager = ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    image_service_backend("db", "postgres:16"),
                )]),
            }),
            backend,
        )
        .with_activation_poll_interval(Duration::from_millis(1))
        .with_activation_timeout(Duration::from_secs(1));

        let error = manager
            .ensure_service_binding_async(&tenant_id, "db", HostCallCancellation::default())
            .await
            .expect_err("backend handle from another tenant should be rejected");

        assert!(
            error
                .to_string()
                .contains("backend returned handle for tenant tenant-b"),
            "error should name the backend tenant mismatch: {error}"
        );
    }

    #[tokio::test]
    async fn ensure_service_binding_async_uses_build_launch_for_build_backed_service() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(1));
        let manager = ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::from([(
                    "api".to_owned(),
                    build_service_backend(
                        "api",
                        "nimbus-api",
                        "/workspace/Dockerfile",
                        "/workspace",
                    ),
                )]),
            }),
            backend.clone(),
        )
        .with_activation_poll_interval(Duration::from_millis(1))
        .with_activation_timeout(Duration::from_secs(1));

        let binding = manager
            .ensure_service_binding_async(&tenant_id, "api", HostCallCancellation::default())
            .await
            .expect("build-backed service activation should succeed")
            .expect("api binding should exist");

        assert_eq!(binding.port, 15432);
        assert_eq!(backend.image_starts.load(Ordering::SeqCst), 0);
        assert_eq!(backend.build_starts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn ensure_service_binding_sync_lookup_stays_snapshot_only_for_missing_service() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(1));
        let manager = ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    image_service_backend("db", "postgres:16"),
                )]),
            }),
            backend.clone(),
        );

        let binding = manager
            .resolve_service_binding(&tenant_id, "db")
            .expect("sync lookup should not fail");
        assert!(
            binding.is_none(),
            "missing in-memory bindings stay unresolved"
        );
        assert_eq!(
            backend.image_starts.load(Ordering::SeqCst),
            0,
            "sync lookup should not trigger sandbox activation"
        );
    }

    #[tokio::test]
    async fn ensure_service_binding_async_can_be_cancelled_while_waiting_for_readiness() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(usize::MAX));
        let manager = ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    image_service_backend("db", "postgres:16"),
                )]),
            }),
            backend.clone(),
        )
        .with_activation_poll_interval(Duration::from_millis(5))
        .with_activation_timeout(Duration::from_secs(1));
        let cancellation = HostCallCancellation::default();
        let cancellation_handle = cancellation.clone();

        let task = tokio::spawn(async move {
            manager
                .ensure_service_binding_async(&tenant_id, "db", cancellation)
                .await
        });

        tokio::time::sleep(Duration::from_millis(20)).await;
        cancellation_handle.cancel();

        let result = task
            .await
            .expect("cancellation task should join")
            .expect_err("cancellation should interrupt activation");
        assert!(matches!(result, Error::Cancelled));
        assert_eq!(
            backend.image_starts.load(Ordering::SeqCst),
            1,
            "activation should still start before the readiness wait is canceled"
        );
    }

    #[tokio::test]
    async fn create_sandbox_resource_stops_backend_after_post_start_validation_errors() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let other_tenant_id = TenantId::new("other").expect("tenant id should be valid");
        let backend = Arc::new(
            StubSandboxBackend::new(1).with_handle_tenant_override(other_tenant_id.clone()),
        );
        let manager = ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::new(),
            }),
            backend.clone(),
        );
        let result = manager
            .create_sandbox_resource_async(
                &tenant_id,
                "worker",
                standalone_resource_spec(&tenant_id, "task"),
                BTreeMap::new(),
            )
            .await;

        assert!(
            matches!(&result, Err(Error::InvalidInput(message)) if message.contains(other_tenant_id.as_str())),
            "mismatched post-start handle should return validation error, got {result:?}"
        );
        assert_eq!(backend.image_starts.load(Ordering::SeqCst), 1);
        assert_eq!(
            backend.stop_calls.load(Ordering::SeqCst),
            1,
            "post-start validation failure must stop the returned untracked sandbox"
        );
        assert!(
            manager
                .list_sandbox_resources_for_tenant(&tenant_id)
                .is_empty(),
            "failed post-start validation must not record a sandbox resource"
        );
        assert!(
            backend
                .handles
                .lock()
                .expect("backend lock should not be poisoned")
                .is_empty(),
            "cleanup should remove the mismatched started handle from the backend"
        );
    }

    #[tokio::test]
    async fn create_sandbox_resource_preserves_existing_backend_after_duplicate_started_id() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(1));
        let manager = ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::new(),
            }),
            backend.clone(),
        );

        manager
            .create_sandbox_resource_async(
                &tenant_id,
                "worker",
                standalone_resource_spec(&tenant_id, "task"),
                BTreeMap::new(),
            )
            .await
            .expect("first standalone sandbox should start");
        let duplicate = manager
            .create_sandbox_resource_async(
                &tenant_id,
                "worker",
                standalone_resource_spec(&tenant_id, "task"),
                BTreeMap::new(),
            )
            .await;

        assert!(
            matches!(&duplicate, Err(Error::Conflict(message)) if message.contains("duplicate sandbox id")),
            "duplicate post-start id should return conflict, got {duplicate:?}"
        );
        assert_eq!(backend.image_starts.load(Ordering::SeqCst), 2);
        assert_eq!(
            backend.stop_calls.load(Ordering::SeqCst),
            0,
            "duplicate-id failure must not stop a tracked sandbox through the create path"
        );
        assert_eq!(
            manager.list_sandbox_resources_for_tenant(&tenant_id).len(),
            1,
            "duplicate-id failure must not insert a second sandbox resource"
        );
        assert!(
            backend
                .handles
                .lock()
                .expect("backend lock should not be poisoned")
                .contains_key("sandbox-tenant-task"),
            "duplicate-id failure must leave the tracked backend handle intact"
        );
    }

    #[tokio::test]
    async fn open_session_rejects_not_ready_sandbox_targets() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(usize::MAX));
        let manager = ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::new(),
            }),
            backend,
        );
        let sandbox = manager
            .create_sandbox_resource_async(
                &tenant_id,
                "worker",
                standalone_resource_spec(&tenant_id, "task"),
                BTreeMap::new(),
            )
            .await
            .expect("standalone sandbox should start in a non-ready state");

        let error = manager
            .open_session_async(
                &tenant_id,
                SessionTarget::Sandbox {
                    id: sandbox.id.clone(),
                },
                vec!["stdio".to_owned()],
                Some(60_000),
            )
            .await
            .expect_err("sessions must not attach to a not-ready sandbox");

        assert!(
            error
                .to_string()
                .contains("session open requires a ready sandbox target"),
            "session open should explain ready-state requirement: {error}"
        );
        assert!(
            manager.list_sessions_for_tenant(&tenant_id).is_empty(),
            "rejected not-ready sandbox session must not create a session resource"
        );
    }

    #[tokio::test]
    async fn teardown_tenant_stops_tracked_sandboxes_and_clears_tenant_resources() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(1));
        let manager = ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    image_service_backend("db", "postgres:16"),
                )]),
            }),
            backend.clone(),
        )
        .with_activation_poll_interval(Duration::from_millis(1))
        .with_activation_timeout(Duration::from_secs(1));

        manager
            .ensure_service_binding_async(&tenant_id, "db", HostCallCancellation::default())
            .await
            .expect("service activation should succeed")
            .expect("db binding should exist");
        manager
            .create_service_definition(
                &tenant_id,
                "browser",
                ServiceBackend::built_in("browser"),
                BTreeMap::new(),
            )
            .expect("dynamic built-in definition should be recorded");
        let standalone = manager
            .create_sandbox_resource_async(
                &tenant_id,
                "worker",
                standalone_resource_spec(&tenant_id, "task"),
                BTreeMap::new(),
            )
            .await
            .expect("standalone sandbox should start");
        manager
            .open_session_async(
                &tenant_id,
                SessionTarget::Sandbox {
                    id: standalone.id.clone(),
                },
                vec!["stdio".to_owned()],
                Some(60_000),
            )
            .await
            .expect("standalone sandbox session should open");
        assert!(manager.snapshot_for_tenant(&tenant_id).contains_key("db"));
        assert!(
            manager
                .service_definition_for_tenant(&tenant_id, "browser")
                .is_some()
        );
        assert_eq!(
            manager.list_sandbox_resources_for_tenant(&tenant_id).len(),
            1
        );
        assert_eq!(manager.list_sessions_for_tenant(&tenant_id).len(), 1);

        manager
            .teardown_tenant(&tenant_id)
            .expect("tenant teardown should stop tracked resources");

        assert_eq!(
            backend.stop_calls.load(Ordering::SeqCst),
            2,
            "tenant teardown should stop service-backed and standalone sandboxes"
        );
        assert_eq!(
            backend.artifact_cleanup_calls.load(Ordering::SeqCst),
            1,
            "tenant teardown should remove tenant-owned sandbox artifact roots"
        );
        assert!(
            manager.snapshot_for_tenant(&tenant_id).is_empty(),
            "tenant teardown should clear manager snapshots"
        );
        assert!(
            manager
                .service_definition_for_tenant(&tenant_id, "browser")
                .is_none(),
            "tenant teardown should purge dynamic service definitions"
        );
        assert!(
            manager
                .list_sandbox_resources_for_tenant(&tenant_id)
                .is_empty(),
            "tenant teardown should purge standalone sandbox resources"
        );
        assert!(
            manager.list_sessions_for_tenant(&tenant_id).is_empty(),
            "tenant teardown should purge tenant sessions"
        );
    }
}
