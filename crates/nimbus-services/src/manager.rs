use std::sync::{Arc, Mutex};
use std::time::Duration;

use nimbus_sandbox::SandboxBackend;
use tokio::sync::Notify;

mod activation;
mod catalog;
mod handles;
mod launch;
mod registry;
mod system_state;
mod types;
mod verification;

#[cfg(test)]
use activation::service_activation_decision;

use crate::SandboxServiceCatalog;
use nimbus_tenant::TenantImageVerificationProvider;

use types::SandboxServiceManagerState;
use verification::DefaultTenantImageVerificationProvider;

pub use system_state::{NoopServiceEvidenceWriter, ServiceEvidenceFuture, ServiceEvidenceWriter};

const DEFAULT_ACTIVATION_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_ACTIVATION_POLL_INTERVAL: Duration = Duration::from_millis(25);

pub struct SandboxServiceManager {
    service_catalog: Arc<dyn SandboxServiceCatalog>,
    sandbox_backend: Arc<dyn SandboxBackend>,
    image_verification_provider: Arc<dyn TenantImageVerificationProvider>,
    activation_timeout: Duration,
    activation_poll_interval: Duration,
    state: Mutex<SandboxServiceManagerState>,
    service_evidence_writer: Mutex<Arc<dyn ServiceEvidenceWriter>>,
    activation_notify: Notify,
}

impl SandboxServiceManager {
    pub fn new(
        service_catalog: Arc<dyn SandboxServiceCatalog>,
        sandbox_backend: Arc<dyn SandboxBackend>,
    ) -> Self {
        Self {
            service_catalog,
            sandbox_backend,
            image_verification_provider: Arc::new(DefaultTenantImageVerificationProvider),
            activation_timeout: DEFAULT_ACTIVATION_TIMEOUT,
            activation_poll_interval: DEFAULT_ACTIVATION_POLL_INTERVAL,
            state: Mutex::new(SandboxServiceManagerState::default()),
            service_evidence_writer: Mutex::new(Arc::new(NoopServiceEvidenceWriter)),
            activation_notify: Notify::new(),
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
        SandboxBuildLaunchSpec, SandboxEgressPolicy, SandboxEgressRule, SandboxError,
        SandboxFilesystemSpec, SandboxFuture, SandboxHandle, SandboxId, SandboxImageLaunchSpec,
        SandboxProcessSpec, SandboxSpec, SandboxStatus,
    };

    use crate::{RuntimeServiceRegistry, SandboxServiceCatalog, SandboxServiceLaunch};
    use nimbus_tenant::{
        TenantImageVerificationEvidence, TenantImageVerificationProvider, TenantIsolationContext,
        TenantIsolationPolicyInput, TenantServiceGrantPolicyDecision, TenantWorkloadIdentity,
    };

    use super::*;

    struct StubSandboxServiceCatalog {
        launches: BTreeMap<String, SandboxServiceLaunch>,
    }

    impl SandboxServiceCatalog for StubSandboxServiceCatalog {
        fn sandbox_service_for_tenant(
            &self,
            _tenant_id: &TenantId,
            service_name: &str,
        ) -> Option<SandboxServiceLaunch> {
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
            Box::pin(async move {
                Err(SandboxError::InvalidSpec {
                    message: format!("rootfs launch unsupported for {}", spec.name),
                })
            })
        }

        fn start_from_image(&self, launch: SandboxImageLaunchSpec) -> SandboxFuture<SandboxHandle> {
            self.image_starts.fetch_add(1, Ordering::SeqCst);
            let handle = self.sandbox_handle(
                &launch.spec.tenant_id,
                &launch.spec.name,
                SandboxStatus::Starting,
            );
            self.handles
                .lock()
                .expect("backend lock should not be poisoned")
                .insert(handle.id.as_str().to_owned(), handle.clone());
            Box::pin(async move { Ok(handle) })
        }

        fn start_from_build(&self, launch: SandboxBuildLaunchSpec) -> SandboxFuture<SandboxHandle> {
            self.build_starts.fetch_add(1, Ordering::SeqCst);
            let handle = self.sandbox_handle(
                &launch.spec.tenant_id,
                &launch.spec.name,
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
        SandboxSpec::new(
            TenantId::new("tenant").expect("tenant id should be valid"),
            name,
            SandboxBackendKind::Krun,
            SandboxFilesystemSpec::new(""),
            SandboxProcessSpec::new(Vec::<String>::new()),
        )
    }

    #[tokio::test]
    async fn start_service_for_decision_rejects_unadmitted_service_before_launch() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(1));
        let manager = SandboxServiceManager::new(
            Arc::new(StubSandboxServiceCatalog {
                launches: BTreeMap::from([(
                    "cache".to_owned(),
                    SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
                        sparse_image_spec("cache"),
                        "redis:7",
                    )),
                )]),
            }),
            backend.clone(),
        );
        let isolation =
            TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
        let decision = service_activation_decision(&isolation, "db")
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
        let manager = SandboxServiceManager::new(
            Arc::new(StubSandboxServiceCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
                        sparse_image_spec("db").with_egress_policy(egress),
                        "postgres:16",
                    )),
                )]),
            }),
            backend.clone(),
        );
        let isolation =
            TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
        let decision = service_activation_decision(&isolation, "db")
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
        let manager = SandboxServiceManager::new(
            Arc::new(StubSandboxServiceCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
                        sparse_image_spec("db").with_egress_policy(egress.clone()),
                        "postgres:16",
                    )),
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
                TenantIsolationPolicyInput::new(TenantWorkloadIdentity::sandbox_service(
                    "db",
                    "activation",
                ))
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
        let manager = SandboxServiceManager::new(
            Arc::new(StubSandboxServiceCatalog {
                launches: BTreeMap::from([(
                    "api".to_owned(),
                    SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
                        sparse_image_spec("api"),
                        image,
                    )),
                )]),
            }),
            backend.clone(),
        );
        let isolation =
            TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
        let decision = isolation
            .admit_decision(
                TenantIsolationPolicyInput::new(TenantWorkloadIdentity::sandbox_service(
                    "api",
                    "activation",
                ))
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
        let manager = SandboxServiceManager::new(
            Arc::new(StubSandboxServiceCatalog {
                launches: BTreeMap::from([(
                    "api".to_owned(),
                    SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
                        sparse_image_spec("api"),
                        image,
                    )),
                )]),
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
                TenantIsolationPolicyInput::new(TenantWorkloadIdentity::sandbox_service(
                    "api",
                    "activation",
                ))
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
        let manager = SandboxServiceManager::new(
            Arc::new(StubSandboxServiceCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
                        sparse_image_spec("db"),
                        "postgres:16",
                    )),
                )]),
            }),
            backend.clone(),
        )
        .with_activation_poll_interval(Duration::from_millis(1))
        .with_activation_timeout(Duration::from_secs(1));
        let isolation =
            TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
        let start_decision = service_activation_decision(&isolation, "db")
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
                TenantIsolationPolicyInput::new(TenantWorkloadIdentity::sandbox_service(
                    "db",
                    "egress-reload",
                ))
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
        let manager = SandboxServiceManager::new(
            Arc::new(StubSandboxServiceCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
                        sparse_image_spec("db"),
                        "postgres:16",
                    )),
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
    async fn stop_service_for_context_async_stops_active_handle_and_clears_snapshot() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(1));
        let manager = SandboxServiceManager::new(
            Arc::new(StubSandboxServiceCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
                        sparse_image_spec("db"),
                        "postgres:16",
                    )),
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
    async fn restart_service_for_context_async_stops_then_starts_service() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(1));
        let manager = SandboxServiceManager::new(
            Arc::new(StubSandboxServiceCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
                        sparse_image_spec("db"),
                        "postgres:16",
                    )),
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
            "restart should materialize a fresh sandbox service"
        );
    }

    #[tokio::test]
    async fn ensure_service_binding_async_rejects_backend_handle_for_wrong_tenant() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(1).with_handle_tenant_override(
            TenantId::new("tenant-b").expect("tenant id should be valid"),
        ));
        let manager = SandboxServiceManager::new(
            Arc::new(StubSandboxServiceCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
                        sparse_image_spec("db"),
                        "postgres:16",
                    )),
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
        let manager = SandboxServiceManager::new(
            Arc::new(StubSandboxServiceCatalog {
                launches: BTreeMap::from([(
                    "api".to_owned(),
                    SandboxServiceLaunch::build(SandboxBuildLaunchSpec::new(
                        sparse_image_spec("api"),
                        "nimbus-api",
                        "/workspace/Dockerfile",
                        "/workspace",
                    )),
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
        let manager = SandboxServiceManager::new(
            Arc::new(StubSandboxServiceCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
                        sparse_image_spec("db"),
                        "postgres:16",
                    )),
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
        let manager = SandboxServiceManager::new(
            Arc::new(StubSandboxServiceCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
                        sparse_image_spec("db"),
                        "postgres:16",
                    )),
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
    async fn teardown_tenant_stops_tracked_sandboxes_and_clears_snapshot() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(1));
        let manager = SandboxServiceManager::new(
            Arc::new(StubSandboxServiceCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
                        sparse_image_spec("db"),
                        "postgres:16",
                    )),
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

        manager
            .teardown_tenant(&tenant_id)
            .expect("tenant teardown should stop tracked sandboxes");

        assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            backend.artifact_cleanup_calls.load(Ordering::SeqCst),
            1,
            "tenant teardown should remove tenant-owned sandbox artifact roots"
        );
        assert!(
            manager.snapshot_for_tenant(&tenant_id).is_empty(),
            "tenant teardown should clear manager snapshots"
        );
    }
}
