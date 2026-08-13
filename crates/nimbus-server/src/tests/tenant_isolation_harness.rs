use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

use super::managed_workload::{TestSandboxActivation, managed_router_config};
use super::*;
use crate::local_server::{
    LocalServerPaths, LocalServerSecurityState, load_or_create_local_admin_token,
};
use nimbus_network::{EndpointProtocol, PublishedEndpoint};
use nimbus_runtime::{InvocationServiceBinding, InvocationServiceProtocol, RuntimeLimits};
use nimbus_sandbox::{
    SandboxBackend, SandboxBackendKind, SandboxError, SandboxFuture, SandboxHandle, SandboxId,
    SandboxInspection, SandboxMountSpec, SandboxOciImageSource, SandboxOwnerSpec,
    SandboxProcessSpec, SandboxRootSpec, SandboxSpec, SandboxStatus,
};
use nimbus_services::{RuntimeServiceRegistry, ServiceBackend, ServiceDefinition, ServiceManager};

struct HarnessServiceDefinitionCatalog;

impl nimbus_services::ServiceDefinitionCatalog for HarnessServiceDefinitionCatalog {
    fn service_definition_for_tenant(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<ServiceDefinition> {
        if service_name != "db" {
            return None;
        }
        let spec = SandboxSpec::new(
            tenant_id.clone(),
            SandboxOwnerSpec::service("db"),
            SandboxBackendKind::Krun,
            SandboxRootSpec::oci_image_reference(
                "registry.example.com/nimbus/postgres@sha256:0000000000000000000000000000000000000000000000000000000000000000",
            ),
            SandboxProcessSpec::new(Vec::<String>::new()),
        )
        .with_mount(SandboxMountSpec::tenant_volume("data", "/var/lib/db"))
        .with_port_binding(nimbus_sandbox::SandboxPortBinding::new(
            "postgres",
            EndpointProtocol::Tcp,
            tenant_service_port(tenant_id.as_str()),
            5432,
        ));
        Some(ServiceDefinition::static_catalog(
            tenant_id.clone(),
            service_name,
            ServiceBackend::sandbox(spec),
        ))
    }

    fn service_definitions_for_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> BTreeMap<String, ServiceDefinition> {
        self.service_definition_for_tenant(tenant_id, "db")
            .map(|definition| BTreeMap::from([("db".to_owned(), definition)]))
            .unwrap_or_default()
    }

    fn service_volume_policy_for_tenant(
        &self,
        _tenant_id: &TenantId,
        service_name: &str,
    ) -> nimbus_tenant::TenantVolumePolicyDecision {
        if service_name == "db" {
            return nimbus_tenant::TenantVolumePolicyDecision::new(["data"]);
        }
        nimbus_tenant::TenantVolumePolicyDecision::default()
    }
}

#[derive(Debug, Clone)]
struct HarnessSandboxRecord {
    handle: SandboxHandle,
    image_reference: String,
    bundle_path: PathBuf,
    rootfs_path: PathBuf,
    state_dir: PathBuf,
    log_path: PathBuf,
    volume_path: PathBuf,
}

#[derive(Debug)]
struct HarnessSandboxBackend {
    root: PathBuf,
    handles: Mutex<BTreeMap<String, SandboxHandle>>,
    records: Mutex<BTreeMap<(String, String), HarnessSandboxRecord>>,
    start_calls: AtomicUsize,
    stop_calls: AtomicUsize,
}

impl HarnessSandboxBackend {
    fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            handles: Mutex::new(BTreeMap::new()),
            records: Mutex::new(BTreeMap::new()),
            start_calls: AtomicUsize::new(0),
            stop_calls: AtomicUsize::new(0),
        }
    }

    fn record_for(&self, tenant_id: &str, service_name: &str) -> HarnessSandboxRecord {
        self.records
            .lock()
            .expect("sandbox records lock should not be poisoned")
            .get(&(tenant_id.to_owned(), service_name.to_owned()))
            .cloned()
            .unwrap_or_else(|| panic!("missing sandbox record for {tenant_id}/{service_name}"))
    }

    fn tenant_artifact_root(&self, kind: &str, tenant_id: &str) -> PathBuf {
        self.root.join(kind).join("tenants").join(tenant_id)
    }

    fn release_exact_artifacts(&self, execution_id: &SandboxId) -> Result<(), SandboxError> {
        let record = self
            .records
            .lock()
            .expect("sandbox records lock should not be poisoned")
            .values()
            .find(|record| record.handle.id == *execution_id)
            .cloned()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!("missing sandbox record for exact release {execution_id}"),
            })?;
        let bundle_root = record
            .bundle_path
            .parent()
            .and_then(Path::parent)
            .expect("harness bundle path should retain its sandbox root");
        let state_root = record
            .state_dir
            .parent()
            .expect("harness state path should retain its sandbox root");
        for root in [bundle_root, state_root, record.volume_path.as_path()] {
            if root.exists() {
                std::fs::remove_dir_all(root).map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to release harness sandbox artifact root {}: {error}",
                        root.display()
                    ),
                })?;
            }
        }
        let tenant = record.handle.tenant_id.as_str();
        for root in [
            self.tenant_artifact_root("bundles", tenant),
            self.tenant_artifact_root("state", tenant),
        ] {
            for candidate in [root.join("sandboxes"), root.join("volumes"), root] {
                match std::fs::remove_dir(&candidate) {
                    Ok(()) => {}
                    Err(error)
                        if error.kind() == std::io::ErrorKind::NotFound
                            || error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
                    Err(error) => {
                        return Err(SandboxError::OperationFailed {
                            message: format!(
                                "failed to prune harness sandbox artifact root {}: {error}",
                                candidate.display()
                            ),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    fn materialize_record(
        &self,
        spec: &SandboxSpec,
        sandbox_id: SandboxId,
        image_reference: &str,
    ) -> Result<SandboxHandle, SandboxError> {
        let tenant = spec.tenant_id.as_str();
        let service = spec
            .service_name()
            .ok_or_else(|| SandboxError::InvalidSpec {
                message: "harness service sandbox spec must be service-owned".to_owned(),
            })?;
        let sandbox_root = |kind: &str| {
            self.tenant_artifact_root(kind, tenant)
                .join("sandboxes")
                .join(sandbox_id.as_str())
        };
        let bundle_path = sandbox_root("bundles").join("bundle").join("config.json");
        let state_dir = sandbox_root("state").join("state");
        let rootfs_path = sandbox_root("state").join("rootfs").join("rootfs");
        let log_path = state_dir
            .join("containers")
            .join(sandbox_id.as_str())
            .join("ctr.log");
        let volume_path = self
            .tenant_artifact_root("state", tenant)
            .join("volumes")
            .join("data");

        for path in [&bundle_path, &rootfs_path, &log_path] {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to create harness sandbox artifact directory {}: {error}",
                        parent.display()
                    ),
                })?;
            }
        }
        std::fs::create_dir_all(&volume_path).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to create harness tenant volume {}: {error}",
                volume_path.display()
            ),
        })?;
        std::fs::write(&bundle_path, "{}").map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to write harness bundle {}: {error}",
                bundle_path.display()
            ),
        })?;
        std::fs::write(&rootfs_path, "rootfs").map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to write harness rootfs marker {}: {error}",
                rootfs_path.display()
            ),
        })?;
        std::fs::write(&log_path, "").map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to write harness log {}: {error}",
                log_path.display()
            ),
        })?;

        let handle = SandboxHandle::new(
            spec.tenant_id.clone(),
            sandbox_id,
            service,
            SandboxBackendKind::Krun,
            SandboxStatus::Ready,
            vec![PublishedEndpoint::new(
                "postgres",
                EndpointProtocol::Tcp,
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), tenant_service_port(tenant)),
            )],
        );
        self.handles
            .lock()
            .expect("sandbox handles lock should not be poisoned")
            .insert(handle.id.as_str().to_owned(), handle.clone());
        self.records
            .lock()
            .expect("sandbox records lock should not be poisoned")
            .insert(
                (tenant.to_owned(), service.to_owned()),
                HarnessSandboxRecord {
                    handle: handle.clone(),
                    image_reference: image_reference.to_owned(),
                    bundle_path,
                    rootfs_path,
                    state_dir,
                    log_path,
                    volume_path,
                },
            );
        Ok(handle)
    }
}

impl SandboxBackend for HarnessSandboxBackend {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Krun
    }

    fn inspect(&self, id: &SandboxId) -> SandboxFuture<Option<SandboxInspection>> {
        let handle = self
            .handles
            .lock()
            .expect("sandbox handles lock should not be poisoned")
            .get(id.as_str())
            .cloned();
        Box::pin(async move { Ok(handle.map(SandboxInspection::provider_reported)) })
    }

    fn remove_tenant_artifacts(&self, tenant_id: TenantId) -> SandboxFuture<()> {
        let tenant = tenant_id.as_str().to_owned();
        for kind in ["bundles", "state"] {
            let root = self.tenant_artifact_root(kind, &tenant);
            if root.exists()
                && let Err(error) = std::fs::remove_dir_all(&root)
            {
                return Box::pin(async move {
                    Err(SandboxError::OperationFailed {
                        message: format!(
                            "failed to remove harness tenant root {}: {error}",
                            root.display()
                        ),
                    })
                });
            }
        }
        self.records
            .lock()
            .expect("sandbox records lock should not be poisoned")
            .retain(|(record_tenant, _), _| record_tenant != &tenant);
        Box::pin(async { Ok(()) })
    }
}

impl TestSandboxActivation for HarnessSandboxBackend {
    fn activate_for_test(
        &self,
        spec: SandboxSpec,
        execution_id: SandboxId,
    ) -> Result<SandboxHandle, SandboxError> {
        self.start_calls.fetch_add(1, Ordering::SeqCst);
        let image_reference = match &spec.root {
            SandboxRootSpec::OciImage(image) => match &image.source {
                SandboxOciImageSource::Reference(reference) => Ok(reference.reference.as_str()),
                SandboxOciImageSource::Build(_) => Err(SandboxError::InvalidSpec {
                    message: format!(
                        "harness service sandbox {} must use an image reference",
                        spec.display_name()
                    ),
                }),
            },
            SandboxRootSpec::Rootfs(_) => Err(SandboxError::InvalidSpec {
                message: format!(
                    "harness service sandbox {} must use an OCI image root",
                    spec.display_name()
                ),
            }),
        };
        image_reference
            .and_then(|reference| self.materialize_record(&spec, execution_id, reference))
    }

    fn activated_handle_for_test(&self, execution_id: &SandboxId) -> Option<SandboxHandle> {
        self.handles
            .lock()
            .expect("sandbox handles lock should not be poisoned")
            .get(execution_id.as_str())
            .cloned()
    }

    fn teardown_for_test(
        &self,
        step: nimbus_workloads::WorkloadTeardownStep,
        execution_id: &SandboxId,
    ) -> SandboxFuture<()> {
        if step == nimbus_workloads::WorkloadTeardownStep::StopExecution {
            self.stop_calls.fetch_add(1, Ordering::SeqCst);
            self.handles
                .lock()
                .expect("sandbox handles lock should not be poisoned")
                .remove(execution_id.as_str());
        }
        let result = if step == nimbus_workloads::WorkloadTeardownStep::ReleaseNetwork {
            self.release_exact_artifacts(execution_id)
        } else {
            Ok(())
        };
        Box::pin(async move { result })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConformanceExpectation {
    Allowed,
    Denied,
}

#[derive(Debug)]
struct ConformanceScenario {
    id: &'static str,
    expectation: ConformanceExpectation,
    evidence: String,
}

#[derive(Debug, Default)]
struct TenantIsolationConformanceReport {
    scenarios: Vec<ConformanceScenario>,
}

impl TenantIsolationConformanceReport {
    fn allowed(&mut self, id: &'static str, evidence: impl Into<String>) {
        self.record(id, ConformanceExpectation::Allowed, evidence);
    }

    fn denied(&mut self, id: &'static str, evidence: impl Into<String>) {
        self.record(id, ConformanceExpectation::Denied, evidence);
    }

    fn record(
        &mut self,
        id: &'static str,
        expectation: ConformanceExpectation,
        evidence: impl Into<String>,
    ) {
        assert!(
            self.scenarios.iter().all(|scenario| scenario.id != id),
            "tenant isolation conformance scenario {id} recorded twice"
        );
        self.scenarios.push(ConformanceScenario {
            id,
            expectation,
            evidence: evidence.into(),
        });
    }

    fn assert_counts(&self, allowed: usize, denied: usize) {
        let actual_allowed = self
            .scenarios
            .iter()
            .filter(|scenario| scenario.expectation == ConformanceExpectation::Allowed)
            .count();
        let actual_denied = self
            .scenarios
            .iter()
            .filter(|scenario| scenario.expectation == ConformanceExpectation::Denied)
            .count();
        println!(
            "tenant isolation conformance: {} scenarios, {} allowed, {} denied",
            self.scenarios.len(),
            actual_allowed,
            actual_denied
        );
        for scenario in &self.scenarios {
            println!(
                "  PASS {:?} {} - {}",
                scenario.expectation, scenario.id, scenario.evidence
            );
        }
        assert_eq!(
            actual_allowed, allowed,
            "allowed conformance scenario count changed"
        );
        assert_eq!(
            actual_denied, denied,
            "denied conformance scenario count changed"
        );
    }
}

#[test]
fn tenant_isolation_conformance_suite_covers_runtime_services_storage_and_system_control() {
    std::thread::Builder::new()
        .name("tenant-isolation-conformance".to_owned())
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tenant-isolation conformance runtime should build")
                .block_on(async {
    let mut conformance = TenantIsolationConformanceReport::default();
    let _guard = auth::auth_test_guard().await;
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, local_admin_token) = local_server_security(temp.path());
    let issuer = "https://issuer.example.com";
    let issuer_a = "https://issuer-a.example.com";
    let application_id = "nimbus-test";
    let (tenant_b_jwt, jwks_data_url) = auth::issue_es256_test_token(
        issuer,
        application_id,
        "user-tenant-b",
        json!({ "tenant_id": "tenant-b", "email": "tenant-b@example.com" }),
    );
    // A separately keyed tenant-a verifier proves the cross-tenant swap below
    // is rejected by cryptographic verifier selection, even though both tokens
    // carry otherwise parallel identity shapes.
    let (tenant_a_jwt, jwks_a_data_url) = auth::issue_es256_test_token(
        issuer_a,
        application_id,
        "user-tenant-a",
        json!({ "tenant_id": "tenant-a", "email": "tenant-a@example.com" }),
    );
    let tenant_b_auth_registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([]),
        json!([]),
        None,
        Some(json!({
            "providers": [{
                "type": "customJwt",
                "issuer": issuer,
                "jwks": jwks_data_url.clone(),
                "algorithm": "ES256",
                "applicationID": application_id
            }]
        })),
    );
    let tenant_a_auth_registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([]),
        json!([]),
        None,
        Some(json!({
            "providers": [{
                "type": "customJwt",
                "issuer": issuer_a,
                "jwks": jwks_a_data_url.clone(),
                "algorithm": "ES256",
                "applicationID": application_id
            }]
        })),
    );
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([
            {
                "name": "services:proof",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx, { probeUrl }, request) => { let deniedFetch = null; try { await fetch(probeUrl); deniedFetch = \"allowed\"; } catch (error) { deniedFetch = String(error && error.message ? error.message : error); } return { ctxServicesType: typeof ctx.services, hasCtxServices: Object.prototype.hasOwnProperty.call(ctx, \"services\"), requestServicesType: typeof request.services, deniedFetch }; }"
            },
            {
                "name": "messages:list",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx) => await ctx.db.query(\"messages\").take(20)"
            },
            {
                "name": "system:routes",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx) => await ctx.db.query(\"routes\").take(20)"
            }
        ]),
        json!([]),
        Some(HARNESS_RUNTIME_BUNDLE),
        Some(json!({
            "providers": [
                {
                    "type": "customJwt",
                    "issuer": issuer,
                    "jwks": jwks_data_url,
                    "algorithm": "ES256",
                    "applicationID": application_id
                },
                {
                    "type": "customJwt",
                    "issuer": issuer_a,
                    "jwks": jwks_a_data_url,
                    "algorithm": "ES256",
                    "applicationID": application_id
                }
            ]
        })),
    )
    .with_runtime_limits(runtime_limits_with_db_service_grant());
    let system_registry = convex_registry(json!([query_function("routes:list", "routes")]));
    let sandbox_backend = Arc::new(HarnessSandboxBackend::new(temp.path().join("sandbox")));
    let service_manager = Arc::new(ServiceManager::new(
        Arc::new(HarnessServiceDefinitionCatalog),
        sandbox_backend.kind(),
    ));
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let service = fixture.engine();
    crate::system_tenant::prepare_system_tenant_async(&service, None)
        .await
        .expect("system tenant should prepare");
    // Anonymous policy metadata remains separate from authenticated admission.
    let tenant_isolation_tenancy = {
        let team_a = nimbus_convex::TeamId::new("team-a").expect("team id");
        let team_b = nimbus_convex::TeamId::new("team-b").expect("team id");
        let silos = nimbus_convex::SiloTeamRegistry::new()
            .bind(
                &TenantId::new("tenant-a").expect("tenant id"),
                team_a.clone(),
            )
            .bind(&TenantId::new("tenant-b").expect("tenant id"), team_b);
        nimbus_convex::ConvexTenancyConfig::new().with_silo_teams(silos)
    };
    let server = ServerFixture::start(
        managed_router_config(service, service_manager.clone(), sandbox_backend.clone())
            .with_convex_silo_auth_verifier(
                &TenantId::new("tenant-a").expect("tenant id"),
                crate::router::convex_application_auth_verifier(&tenant_a_auth_registry),
            )
            .with_convex_silo_auth_verifier(
                &TenantId::new("tenant-b").expect("tenant id"),
                crate::router::convex_application_auth_verifier(&tenant_b_auth_registry),
            )
            .with_convex(registry)
            .with_convex_tenancy(tenant_isolation_tenancy)
            .with_system_convex_registry(system_registry)
            .with_local_server_security(local_server_security)
            .build(),
    )
    .await;

    assert_eq!(
        create_tenant_with_admin(&server, &local_admin_token.token, "tenant-a")
            .await
            .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        create_tenant_with_admin(&server, &local_admin_token.token, "tenant-b")
            .await
            .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        insert_document_with_admin(
            &server,
            &local_admin_token.token,
            "tenant-a",
            "messages",
            json!({ "body": "tenant-a message" }),
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        insert_document_with_admin(
            &server,
            &local_admin_token.token,
            "tenant-b",
            "messages",
            json!({ "body": "tenant-b message" }),
        )
        .await
        .status(),
        StatusCode::CREATED
    );

    let native_without_operator = server
        .client()
        .get(server.http_url("/api/tenants/tenant-a/documents/messages"))
        .send()
        .await
        .expect("unauthenticated native list should send");
    assert_eq!(native_without_operator.status(), StatusCode::UNAUTHORIZED);
    conformance.denied(
        "native.path_without_operator_denied",
        "tenant document path rejects missing operator bearer",
    );
    let tenant_a_native =
        list_documents_with_admin(&server, &local_admin_token.token, "tenant-a", "messages").await;
    assert_eq!(
        tenant_a_native["data"][0]["body"],
        json!("tenant-a message")
    );
    conformance.allowed(
        "native.storage.tenant_a_admin_read_allowed",
        "operator bearer reads tenant-a document path",
    );
    let tenant_b_native =
        list_documents_with_admin(&server, &local_admin_token.token, "tenant-b", "messages").await;
    assert_eq!(
        tenant_b_native["data"][0]["body"],
        json!("tenant-b message")
    );
    conformance.allowed(
        "native.storage.tenant_b_admin_read_allowed",
        "operator bearer reads tenant-b document path",
    );

    let tenant_a_id = TenantId::new("tenant-a").expect("tenant id should parse");
    let tenant_b_id = TenantId::new("tenant-b").expect("tenant id should parse");
    for tenant_id in [&tenant_a_id, &tenant_b_id] {
        let start = server
            .client()
            .post(server.http_url(&format!(
                "/api/tenants/{}/services/db/start",
                tenant_id.as_str()
            )))
            .bearer_auth(&local_admin_token.token)
            .send()
            .await
            .expect("service start request should send");
        assert_eq!(start.status(), StatusCode::OK);
    }
    let tenant_a_binding = service_manager
        .resolve_service_binding(&tenant_a_id, "db")
        .expect("tenant-a service binding should resolve")
        .expect("tenant-a db service should exist");
    let tenant_b_binding = service_manager
        .resolve_service_binding(&tenant_b_id, "db")
        .expect("tenant-b service binding should resolve")
        .expect("tenant-b db service should exist");
    assert_service_manager_binding(&tenant_a_binding, tenant_service_port("tenant-a"));
    conformance.allowed(
        "service_manager.tenant_a_db_binding_allowed",
        "tenant-a resolves db service through Rust-owned service manager",
    );
    assert_service_manager_binding(&tenant_b_binding, tenant_service_port("tenant-b"));
    conformance.allowed(
        "service_manager.tenant_b_db_binding_allowed",
        "tenant-b resolves db service through Rust-owned service manager",
    );
    assert_ne!(
        tenant_a_binding.port, tenant_b_binding.port,
        "same service name must resolve to each tenant's own sandbox binding"
    );
    conformance.allowed(
        "service_manager.same_service_name_is_tenant_scoped",
        "tenant-a/db and tenant-b/db resolve to distinct tenant-scoped ports",
    );

    let tenant_a_services = convex_query_json(
        &server,
        "tenant-a",
        Some(&tenant_a_jwt),
        "services:proof",
        json!({ "probeUrl": format!("http://127.0.0.1:{}/", tenant_service_port("tenant-a")) }),
    )
    .await;
    let tenant_b_services = convex_query_json(
        &server,
        "tenant-b",
        Some(&tenant_b_jwt),
        "services:proof",
        json!({ "probeUrl": format!("http://127.0.0.1:{}/", tenant_service_port("tenant-b")) }),
    )
    .await;
    assert_adapter_service_absence_proof(&tenant_a_services);
    conformance.denied(
        "runtime.service.tenant_a_adapter_shortcut_absent",
        "tenant-a Convex adapter ctx exposes no Nimbus service shortcut",
    );
    conformance.denied(
        "runtime.network.tenant_a_generic_localhost_denied",
        "tenant-a service grant does not imply generic localhost fetch",
    );
    assert_adapter_service_absence_proof(&tenant_b_services);
    conformance.denied(
        "runtime.service.tenant_b_adapter_shortcut_absent",
        "tenant-b Convex adapter ctx exposes no Nimbus service shortcut",
    );
    conformance.denied(
        "runtime.network.tenant_b_generic_localhost_denied",
        "tenant-b service grant does not imply generic localhost fetch",
    );

    let tenant_a_record = sandbox_backend.record_for("tenant-a", "db");
    let tenant_b_record = sandbox_backend.record_for("tenant-b", "db");
    assert_eq!(
        sandbox_backend.start_calls.load(Ordering::SeqCst),
        2,
        "each tenant should start exactly one sandbox for the shared service name"
    );
    assert_ne!(tenant_a_record.handle.id, tenant_b_record.handle.id);
    conformance.allowed(
        "sandbox.handle.same_service_name_is_tenant_scoped",
        "same service name materializes distinct sandbox handles per tenant",
    );
    assert_distinct_tenant_path_pair(&tenant_a_record.bundle_path, &tenant_b_record.bundle_path);
    assert_distinct_tenant_path_pair(&tenant_a_record.rootfs_path, &tenant_b_record.rootfs_path);
    assert_distinct_tenant_path_pair(&tenant_a_record.state_dir, &tenant_b_record.state_dir);
    assert_distinct_tenant_path_pair(&tenant_a_record.log_path, &tenant_b_record.log_path);
    assert_distinct_tenant_path_pair(&tenant_a_record.volume_path, &tenant_b_record.volume_path);
    conformance.allowed(
        "sandbox.volume.same_named_volume_is_tenant_scoped",
        "same named volume data materializes under distinct tenant roots",
    );
    assert!(tenant_a_record.image_reference.contains("@sha256:"));
    assert!(tenant_b_record.image_reference.contains("@sha256:"));
    conformance.allowed(
        "sandbox.image.digest_pinned_service_launch_allowed",
        "service catalog uses digest-pinned image references for production provenance floor",
    );

    let tenant_b_messages = convex_query_json(
        &server,
        "tenant-b",
        Some(&tenant_b_jwt),
        "messages:list",
        json!({}),
    )
    .await;
    assert_eq!(tenant_b_messages[0]["body"], json!("tenant-b message"));
    conformance.allowed(
        "runtime.storage.tenant_b_messages_allowed",
        "tenant-b runtime HostBridge reads only tenant-b messages",
    );
    let swapped = post_convex_query(
        &server,
        "tenant-a",
        Some(&tenant_b_jwt),
        "messages:list",
        json!({}),
    )
    .await;
    let swapped_status = swapped.status();
    let swapped_body = swapped
        .text()
        .await
        .expect("swapped tenant body should read");
    assert_eq!(
        swapped_status,
        StatusCode::UNAUTHORIZED,
        "swapped tenant body: {swapped_body}"
    );
    assert!(
        swapped_body.contains("no auth provider matched this token"),
        "tenant-a's verifier must reject tenant-b's independently signed token: {swapped_body}"
    );
    conformance.denied(
        "runtime.auth.bearer_tenant_claim_swap_denied",
        "tenant-b bearer cannot target tenant-a path",
    );

    let application_system_routes = convex_query_json(
        &server,
        "tenant-a",
        Some(&tenant_a_jwt),
        "system:routes",
        json!({}),
    )
    .await;
    assert_eq!(
        application_system_routes,
        json!([]),
        "application runtime HostBridge reads must stay in the application tenant, not _nimbus"
    );
    conformance.denied(
        "runtime.system.application_nimbus_access_denied",
        "application runtime sees no _nimbus system routes through HostBridge storage",
    );
    let missing_system_auth =
        post_convex_query(&server, "_nimbus", None, "routes:list", json!({})).await;
    assert_eq!(missing_system_auth.status(), StatusCode::UNAUTHORIZED);
    conformance.denied(
        "system.missing_operator_nimbus_access_denied",
        "_nimbus query requires operator bearer",
    );
    let operator_system_routes = post_convex_query(
        &server,
        "_nimbus",
        Some(&local_admin_token.token),
        "routes:list",
        json!({}),
    )
    .await;
    assert_eq!(operator_system_routes.status(), StatusCode::OK);
    let routes = operator_system_routes
        .json::<serde_json::Value>()
        .await
        .expect("operator system routes should parse");
    assert!(
        routes.as_array().is_some_and(|routes| routes
            .iter()
            .any(|route| route["path"] == "/health" && route["adapter"] == "native")),
        "operator-authenticated _nimbus query should see system route inventory: {routes}"
    );
    conformance.allowed(
        "system.operator_nimbus_routes_allowed",
        "operator bearer can read _nimbus route inventory",
    );

    let delete_tenant_a = server
        .client()
        .delete(server.http_url("/api/tenants/tenant-a"))
        .bearer_auth(&local_admin_token.token)
        .send()
        .await
        .expect("tenant delete should send");
    let delete_tenant_a_status = delete_tenant_a.status();
    let delete_tenant_a_body = delete_tenant_a
        .text()
        .await
        .expect("tenant delete body should read");
    assert_eq!(
        delete_tenant_a_status,
        StatusCode::NO_CONTENT,
        "tenant delete body: {delete_tenant_a_body}"
    );
    assert_eq!(
        sandbox_backend.stop_calls.load(Ordering::SeqCst),
        1,
        "deleting tenant-a should stop only tenant-a's active sandbox"
    );
    assert!(
        service_manager
            .snapshot_for_tenant(&TenantId::new("tenant-a").expect("tenant id should parse"))
            .is_empty()
    );
    assert!(
        service_manager
            .snapshot_for_tenant(&TenantId::new("tenant-b").expect("tenant id should parse"))
            .contains_key("db")
    );
    assert!(
        !sandbox_backend
            .tenant_artifact_root("state", "tenant-a")
            .exists(),
        "tenant-a state artifacts should be removed"
    );
    conformance.denied(
        "cleanup.tenant_a_sandbox_artifacts_removed",
        "tenant-a deletion removes tenant-a sandbox artifacts",
    );
    assert!(
        sandbox_backend
            .tenant_artifact_root("state", "tenant-b")
            .exists(),
        "tenant-b state artifacts should remain"
    );
    conformance.allowed(
        "cleanup.tenant_b_sandbox_artifacts_survive_tenant_a_delete",
        "tenant-a deletion leaves tenant-b sandbox artifacts",
    );
    let tenant_b_after_delete = convex_query_json(
        &server,
        "tenant-b",
        Some(&tenant_b_jwt),
        "messages:list",
        json!({}),
    )
    .await;
    assert_eq!(tenant_b_after_delete[0]["body"], json!("tenant-b message"));
    conformance.allowed(
        "cleanup.tenant_b_runtime_storage_survives_tenant_a_delete",
        "tenant-b runtime storage remains readable after tenant-a cleanup",
    );

                    conformance.assert_counts(12, 9);
                });
        })
        .expect("tenant-isolation conformance thread should start")
        .join()
        .expect("tenant-isolation conformance thread should complete");
}

const HARNESS_RUNTIME_BUNDLE: &str = r#"
const definitions = new Map([
  ["services:proof", {
    name: "services:proof",
    kind: "query",
    runtime_handler: "async (ctx, { probeUrl }, request) => { let deniedFetch = null; try { await fetch(probeUrl); deniedFetch = \"allowed\"; } catch (error) { deniedFetch = String(error && error.message ? error.message : error); } return { ctxServicesType: typeof ctx.services, hasCtxServices: Object.prototype.hasOwnProperty.call(ctx, \"services\"), requestServicesType: typeof request.services, deniedFetch }; }",
  }],
  ["messages:list", {
    name: "messages:list",
    kind: "query",
    runtime_handler: "async (ctx) => await ctx.db.query(\"messages\").take(20)",
  }],
  ["system:routes", {
    name: "system:routes",
    kind: "query",
    runtime_handler: "async (ctx) => await ctx.db.query(\"routes\").take(20)",
  }],
]);

function compileRuntimeHandler(definition) {
  return new Function(
    "ctx",
    "args",
    "request",
    "return (" + definition.runtime_handler + ")(ctx, args, request);",
  );
}

const handlers = new Map(
  [...definitions.values()].map((definition) => [
    definition.name,
    compileRuntimeHandler(definition),
  ]),
);

globalThis.__nimbusInvoke = async function(request) {
  try {
    const handler = handlers.get(request.function_name);
    return {
      status: "ok",
      value: await handler(
        globalThis.__nimbusCreateContext({
          request,
          hostCallSessionId: `${request.kind}:${request.function_name}`,
        }),
        request.args ?? {},
        request,
      ),
    };
  } catch (error) {
    if (error && typeof error === "object" && "nimbusHostError" in error) {
      return { status: "error", error: error.nimbusHostError };
    }
    throw error;
  }
};

export {};
"#;

fn runtime_limits_with_db_service_grant() -> RuntimeLimits {
    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.grants.service = vec!["db".to_owned()];
    limits
}

fn tenant_service_port(tenant_id: &str) -> u16 {
    match tenant_id {
        "tenant-a" => 15_432,
        "tenant-b" => 25_432,
        other => panic!("unexpected harness tenant id: {other}"),
    }
}

fn local_server_security(
    root: &Path,
) -> (
    Arc<LocalServerSecurityState>,
    crate::local_server::LocalAdminTokenRecord,
) {
    let paths = LocalServerPaths {
        auth_token_path: root.join("auth").join("token"),
        server_discovery_path: root.join("run").join("server.json"),
        audit_log_path: root.join("logs").join("access.jsonl"),
    };
    let token = load_or_create_local_admin_token(&paths).expect("token should exist");
    (
        Arc::new(LocalServerSecurityState::new(paths, token.clone())),
        token,
    )
}

fn query_function(name: &str, table: &str) -> serde_json::Value {
    json!({
        "name": name,
        "kind": "query",
        "plan": {
            "table": table,
            "filters": [],
            "order": null,
            "limit": null
        }
    })
}

async fn create_tenant_with_admin(
    server: &ServerFixture,
    admin_token: &str,
    tenant_id: &str,
) -> reqwest::Response {
    server
        .client()
        .post(server.http_url("/api/tenants"))
        .bearer_auth(admin_token)
        .json(&json!({ "id": tenant_id }))
        .send()
        .await
        .expect("admin tenant create should send")
}

async fn insert_document_with_admin(
    server: &ServerFixture,
    admin_token: &str,
    tenant_id: &str,
    table: &str,
    fields: serde_json::Value,
) -> reqwest::Response {
    server
        .client()
        .post(server.http_url(&format!("/api/tenants/{tenant_id}/documents")))
        .bearer_auth(admin_token)
        .json(&json!({ "table": table, "fields": fields }))
        .send()
        .await
        .expect("admin document insert should send")
}

async fn list_documents_with_admin(
    server: &ServerFixture,
    admin_token: &str,
    tenant_id: &str,
    table: &str,
) -> serde_json::Value {
    let response = server
        .client()
        .get(server.http_url(&format!("/api/tenants/{tenant_id}/documents/{table}")))
        .bearer_auth(admin_token)
        .send()
        .await
        .expect("admin document list should send");
    assert_eq!(response.status(), StatusCode::OK);
    response
        .json::<serde_json::Value>()
        .await
        .expect("admin document list body should parse")
}

async fn post_convex_query(
    server: &ServerFixture,
    tenant_id: &str,
    bearer: Option<&str>,
    name: &str,
    args: serde_json::Value,
) -> reqwest::Response {
    let mut request = server
        .client()
        .post(server.http_url(&format!("/convex/{tenant_id}/query")))
        .json(&json!({ "name": name, "args": args }));
    if let Some(bearer) = bearer {
        request = request.bearer_auth(bearer);
    }
    request.send().await.expect("convex query should send")
}

async fn convex_query_json(
    server: &ServerFixture,
    tenant_id: &str,
    bearer: Option<&str>,
    name: &str,
    args: serde_json::Value,
) -> serde_json::Value {
    let response = post_convex_query(server, tenant_id, bearer, name, args).await;
    let status = response.status();
    let body = response
        .text()
        .await
        .expect("convex query body should read");
    assert_eq!(status, StatusCode::OK, "convex query body: {body}");
    serde_json::from_str(&body).expect("convex query body should parse")
}

fn assert_service_manager_binding(binding: &InvocationServiceBinding, expected_port: u16) {
    assert_eq!(binding.host, "127.0.0.1");
    assert_eq!(binding.port, expected_port);
    assert_eq!(binding.protocol, InvocationServiceProtocol::Tcp);
    let postgres = binding
        .endpoints
        .get("postgres")
        .expect("postgres endpoint should exist");
    assert_eq!(postgres.host, "127.0.0.1");
    assert_eq!(postgres.port, expected_port);
    assert_eq!(postgres.protocol, InvocationServiceProtocol::Tcp);
}

fn assert_adapter_service_absence_proof(body: &serde_json::Value) {
    assert_eq!(body["ctxServicesType"], json!("undefined"));
    assert_eq!(body["hasCtxServices"], json!(false));
    assert_eq!(body["requestServicesType"], json!("undefined"));
    assert!(
        body["deniedFetch"]
            .as_str()
            .is_some_and(|message| message != "allowed" && !message.is_empty()),
        "Convex adapter ctx.services must stay absent and generic localhost fetch denied: {body}"
    );
}

fn assert_distinct_tenant_path_pair(tenant_a: &Path, tenant_b: &Path) {
    assert_ne!(
        tenant_a,
        tenant_b,
        "tenant artifact paths must not collide: {}",
        tenant_a.display()
    );
    assert!(
        tenant_a.to_string_lossy().contains(&format!(
            "{}tenant-a{}",
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR
        )),
        "tenant-a artifact path should include tenant root: {}",
        tenant_a.display()
    );
    assert!(
        tenant_b.to_string_lossy().contains(&format!(
            "{}tenant-b{}",
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR
        )),
        "tenant-b artifact path should include tenant root: {}",
        tenant_b.display()
    );
}
