use std::sync::Arc;

use nimbus_core::TenantId;
use nimbus_engine::Engine;
use nimbus_sandbox::SandboxHandle;
use nimbus_services::{ServiceEvidenceFuture, ServiceEvidenceWriter, ServiceManager};

struct SystemTenantServiceEvidenceWriter {
    engine: Arc<Engine>,
}

impl SystemTenantServiceEvidenceWriter {
    fn new(engine: Arc<Engine>) -> Self {
        Self { engine }
    }
}

impl ServiceEvidenceWriter for SystemTenantServiceEvidenceWriter {
    fn record_service_handle<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        handle: &'a SandboxHandle,
    ) -> ServiceEvidenceFuture<'a> {
        Box::pin(async move {
            nimbus_system::record_service_handle_async(&self.engine, tenant_id, handle).await
        })
    }
}

pub(crate) fn attach_system_state_engine(manager: &ServiceManager, engine: Arc<Engine>) {
    manager
        .set_service_evidence_writer_arc(Arc::new(SystemTenantServiceEvidenceWriter::new(engine)));
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::http::StatusCode;
    use futures::future::BoxFuture;
    use nimbus_auth::{ApplicationAuthError, ApplicationAuthVerifier};
    use nimbus_core::{TableName, TenantId};
    use nimbus_engine::Engine;
    use nimbus_runtime::{
        HostCallCancellation, InvocationAuth, RuntimeUserIdentity, VerifiedUserIdentity,
        VerifiedUserIdentityKind,
    };
    use nimbus_sandbox::{
        PublishedEndpoint, PublishedEndpointProtocol, SandboxBackend, SandboxBackendKind,
        SandboxError, SandboxFilesystemSpec, SandboxFuture, SandboxHandle, SandboxId,
        SandboxImageLaunchSpec, SandboxProcessSpec, SandboxSpec, SandboxStatus,
    };
    use nimbus_services::{
        RuntimeServiceRegistry, ServiceDefinitionCatalog, ServiceImplementation, ServiceManager,
    };
    use nimbus_testing::ServerFixture;
    use serde_json::{Map, Value, json};

    use super::attach_system_state_engine;
    use crate::local_server::{
        LocalServerAuditRecord, LocalServerPaths, LocalServerSecurityState,
        load_or_create_local_admin_token,
    };

    struct StubServiceDefinitionCatalog {
        image_launches: BTreeMap<String, String>,
    }

    impl ServiceDefinitionCatalog for StubServiceDefinitionCatalog {
        fn service_implementation_for_tenant(
            &self,
            tenant_id: &TenantId,
            service_name: &str,
        ) -> Option<ServiceImplementation> {
            self.image_launches.get(service_name).map(|image| {
                ServiceImplementation::sandbox_image(SandboxImageLaunchSpec::new(
                    sparse_image_spec(tenant_id, service_name),
                    image,
                ))
            })
        }
    }

    struct ReadySandboxBackend {
        image_starts: AtomicUsize,
        stop_calls: AtomicUsize,
    }

    impl ReadySandboxBackend {
        fn handle(
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
            SandboxHandle::new(
                tenant_id.clone(),
                SandboxId::new(format!("sandbox-{tenant_id}-{service_name}")),
                service_name,
                SandboxBackendKind::Krun,
                status,
                endpoints,
            )
        }
    }

    impl SandboxBackend for ReadySandboxBackend {
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
            Box::pin(async move {
                Ok(Self::handle(
                    &launch.spec.tenant_id,
                    &launch.spec.name,
                    SandboxStatus::Ready,
                ))
            })
        }

        fn inspect(&self, id: &SandboxId) -> SandboxFuture<Option<SandboxHandle>> {
            let id = id.as_str().to_owned();
            Box::pin(async move {
                let parts = id.strip_prefix("sandbox-").unwrap_or(&id);
                let (tenant, service) = parts.rsplit_once('-').unwrap_or(("tenant", "db"));
                let tenant_id = TenantId::new(tenant).expect("test tenant id should parse");
                Ok(Some(Self::handle(
                    &tenant_id,
                    service,
                    SandboxStatus::Ready,
                )))
            })
        }

        fn stop(&self, _id: &SandboxId) -> SandboxFuture<()> {
            self.stop_calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        }

        fn remove_tenant_artifacts(&self, _tenant_id: TenantId) -> SandboxFuture<()> {
            Box::pin(async { Ok(()) })
        }
    }

    fn sparse_image_spec(tenant_id: &TenantId, name: &str) -> SandboxSpec {
        SandboxSpec::new(
            tenant_id.clone(),
            name,
            SandboxBackendKind::Krun,
            SandboxFilesystemSpec::new(""),
            SandboxProcessSpec::new(Vec::<String>::new()),
        )
    }

    fn service_manager(backend: Arc<ReadySandboxBackend>) -> Arc<ServiceManager> {
        Arc::new(
            ServiceManager::new(
                Arc::new(StubServiceDefinitionCatalog {
                    image_launches: BTreeMap::from([("db".to_owned(), "postgres:16".to_owned())]),
                }),
                backend,
            )
            .with_activation_poll_interval(std::time::Duration::from_millis(1))
            .with_activation_timeout(std::time::Duration::from_secs(1)),
        )
    }

    fn sample_paths(root: &std::path::Path) -> LocalServerPaths {
        LocalServerPaths {
            auth_token_path: root.join("auth").join("token"),
            server_discovery_path: root.join("run").join("server.json"),
            audit_log_path: root.join("logs").join("access.jsonl"),
        }
    }

    fn local_server_security(
        root: &std::path::Path,
    ) -> (
        Arc<LocalServerSecurityState>,
        crate::local_server::LocalAdminTokenRecord,
    ) {
        let paths = sample_paths(root);
        let token = load_or_create_local_admin_token(&paths).expect("token should exist");
        (
            Arc::new(LocalServerSecurityState::new(paths, token.clone())),
            token,
        )
    }

    fn read_audit_records(path: &std::path::Path) -> Vec<LocalServerAuditRecord> {
        fs::read_to_string(path)
            .expect("audit log should be readable")
            .lines()
            .map(|line| serde_json::from_str(line).expect("audit record should parse"))
            .collect()
    }

    struct StaticServiceRouteAuthVerifier;

    impl ApplicationAuthVerifier for StaticServiceRouteAuthVerifier {
        fn verify_bearer_token<'a>(
            &'a self,
            token: &'a str,
        ) -> BoxFuture<'a, Result<InvocationAuth, ApplicationAuthError>> {
            Box::pin(async move {
                match token {
                    "tenant-a-db" => Ok(service_route_auth(
                        token,
                        "tenanta",
                        Some("tenant"),
                        &["db"],
                    )),
                    "tenant-a-none" => {
                        Ok(service_route_auth(token, "tenanta", Some("tenant"), &[]))
                    }
                    "tenant-a-wildcard" => {
                        Ok(service_route_auth(token, "tenanta", Some("tenant"), &["*"]))
                    }
                    "tenantless-db" => Ok(service_route_auth_without_tenant(
                        token,
                        Some("tenant"),
                        &["db"],
                    )),
                    "tenant-a-spawned-db" => Ok(service_route_auth(
                        token,
                        "tenanta",
                        Some("spawned_workload"),
                        &["db"],
                    )),
                    "tenant-a-operator-db" => Ok(service_route_auth(
                        token,
                        "tenanta",
                        Some("operator"),
                        &["db"],
                    )),
                    _ => Err(ApplicationAuthError::unauthorized("unknown test token")),
                }
            })
        }
    }

    fn service_route_auth(
        token: &str,
        tenant_id: &str,
        principal_class: Option<&str>,
        grants: &[&str],
    ) -> InvocationAuth {
        let claims = service_route_claims(tenant_id, principal_class, grants);
        InvocationAuth::with_identities(
            runtime_identity(token, claims.clone()),
            verified_identity(token, claims),
            false,
        )
    }

    fn service_route_auth_without_tenant(
        token: &str,
        principal_class: Option<&str>,
        grants: &[&str],
    ) -> InvocationAuth {
        let mut claims = Map::new();
        if let Some(principal_class) = principal_class {
            claims.insert("nimbus_principal_class".to_string(), json!(principal_class));
        }
        claims.insert("nimbus_service_grants".to_string(), json!(grants));
        InvocationAuth::with_identities(
            runtime_identity(token, claims.clone()),
            verified_identity(token, claims),
            false,
        )
    }

    fn service_route_claims(
        tenant_id: &str,
        principal_class: Option<&str>,
        grants: &[&str],
    ) -> Map<String, Value> {
        let mut claims = Map::new();
        claims.insert("nimbus_tenant_id".to_string(), json!(tenant_id));
        if let Some(principal_class) = principal_class {
            claims.insert("nimbus_principal_class".to_string(), json!(principal_class));
        }
        claims.insert("nimbus_service_grants".to_string(), json!(grants));
        claims
    }

    fn runtime_identity(token: &str, custom_claims: Map<String, Value>) -> RuntimeUserIdentity {
        RuntimeUserIdentity {
            token_identifier: format!("test|{token}"),
            subject: token.to_string(),
            issuer: "test-issuer".to_string(),
            name: None,
            given_name: None,
            family_name: None,
            nickname: None,
            preferred_username: None,
            profile_url: None,
            picture_url: None,
            email: None,
            email_verified: None,
            gender: None,
            birthday: None,
            timezone: None,
            language: None,
            phone_number: None,
            phone_number_verified: None,
            address: None,
            updated_at: None,
            custom_claims,
        }
    }

    fn verified_identity(token: &str, custom_claims: Map<String, Value>) -> VerifiedUserIdentity {
        VerifiedUserIdentity {
            kind: VerifiedUserIdentityKind::Oidc,
            token_identifier: format!("test|{token}"),
            subject: token.to_string(),
            issuer: "test-issuer".to_string(),
            name: None,
            given_name: None,
            family_name: None,
            nickname: None,
            preferred_username: None,
            profile_url: None,
            picture_url: None,
            email: None,
            email_verified: None,
            gender: None,
            birthday: None,
            timezone: None,
            language: None,
            phone_number: None,
            phone_number_verified: None,
            address: None,
            updated_at: None,
            custom_claims,
        }
    }

    #[tokio::test]
    async fn service_evidence_writer_records_observed_state_to_system_tenant() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
        nimbus_system::prepare_system_tenant_async(&engine, None)
            .await
            .expect("system tenant should prepare");
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(ReadySandboxBackend {
            image_starts: AtomicUsize::new(0),
            stop_calls: AtomicUsize::new(0),
        });
        let manager = service_manager(backend);
        attach_system_state_engine(&manager, engine.clone());

        manager
            .ensure_service_binding_async(&tenant_id, "db", HostCallCancellation::default())
            .await
            .expect("service activation should succeed")
            .expect("db binding should exist");

        let documents = engine
            .list_documents_async(
                nimbus_system::system_tenant_id().expect("system id should parse"),
                TableName::new("services").expect("table should parse"),
            )
            .await
            .expect("service state documents should list");
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].fields.get("name"), Some(&json!("db")));
        assert_eq!(documents[0].fields.get("state"), Some(&json!("ready")));

        let ports = engine
            .list_documents_async(
                nimbus_system::system_tenant_id().expect("system id should parse"),
                TableName::new("ports").expect("table should parse"),
            )
            .await
            .expect("service ports should list");
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].fields.get("tenantId"), Some(&json!("tenant")));
        assert_eq!(ports[0].fields.get("serviceName"), Some(&json!("db")));
        assert_eq!(ports[0].fields.get("hostPort"), Some(&json!(15432)));
        assert_eq!(ports[0].fields.get("guestPort"), Some(&json!(5432)));
    }

    #[tokio::test]
    async fn service_lifecycle_routes_reject_unauthenticated_when_local_security_absent() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
        let backend = Arc::new(ReadySandboxBackend {
            image_starts: AtomicUsize::new(0),
            stop_calls: AtomicUsize::new(0),
        });
        let server = ServerFixture::start(
            crate::router::RouterBuildConfig::core(engine.clone())
                .with_service_manager(service_manager(backend.clone()))
                .without_deploy_admin_token()
                .build(),
        )
        .await;

        let start = server
            .client()
            .post(server.http_url("/api/tenants/tenant/services/db/start"))
            .send()
            .await
            .expect("service start request should send");
        assert_eq!(start.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(backend.image_starts.load(Ordering::SeqCst), 0);

        let get = server
            .client()
            .get(server.http_url("/api/tenants/tenant/services/db"))
            .send()
            .await
            .expect("service get request should send");
        assert_eq!(get.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn operator_service_lifecycle_routes_support_explicit_verbs_and_get() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let (local_server_security, token) = local_server_security(temp.path());
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
        let backend = Arc::new(ReadySandboxBackend {
            image_starts: AtomicUsize::new(0),
            stop_calls: AtomicUsize::new(0),
        });
        let server = ServerFixture::start(
            crate::router::RouterBuildConfig::core(engine.clone())
                .with_service_manager(service_manager(backend.clone()))
                .with_local_server_security(local_server_security)
                .without_deploy_admin_token()
                .build(),
        )
        .await;

        let before_start = server
            .client()
            .get(server.http_url("/api/tenants/tenant/services/db"))
            .bearer_auth(&token.token)
            .send()
            .await
            .expect("service get request should send");
        assert_eq!(before_start.status(), StatusCode::OK);
        let before_start_body = before_start
            .json::<serde_json::Value>()
            .await
            .expect("service get response should parse");
        assert_eq!(before_start_body["tenantId"], json!("tenant"));
        assert_eq!(before_start_body["name"], json!("db"));
        assert_eq!(before_start_body["state"], json!("stopped"));
        assert!(before_start_body.get("sandboxId").is_none());

        let start = server
            .client()
            .post(server.http_url("/api/tenants/tenant/services/db/start"))
            .bearer_auth(&token.token)
            .send()
            .await
            .expect("service start request should send");
        assert_eq!(start.status(), StatusCode::OK);
        let start_body = start
            .json::<serde_json::Value>()
            .await
            .expect("service start response should parse");
        assert_eq!(start_body["tenantId"], json!("tenant"));
        assert_eq!(start_body["name"], json!("db"));
        assert_eq!(start_body["state"], json!("ready"));
        assert_eq!(start_body["readiness"], json!("ready"));
        assert_eq!(start_body["health"], json!("healthy"));
        assert_eq!(backend.image_starts.load(Ordering::SeqCst), 1);

        let get = server
            .client()
            .get(server.http_url("/api/tenants/tenant/services/db"))
            .bearer_auth(&token.token)
            .send()
            .await
            .expect("service get request should send");
        assert_eq!(get.status(), StatusCode::OK);
        let get_body = get
            .json::<serde_json::Value>()
            .await
            .expect("service get response should parse");
        assert_eq!(get_body["tenantId"], json!("tenant"));
        assert_eq!(get_body["name"], json!("db"));
        assert_eq!(get_body["state"], json!("ready"));
        assert_eq!(get_body["readiness"], json!("ready"));
        assert_eq!(get_body["health"], json!("healthy"));
        assert_eq!(get_body["endpoints"][0]["name"], json!("postgres"));

        let stop = server
            .client()
            .post(server.http_url("/api/tenants/tenant/services/db/stop"))
            .bearer_auth(&token.token)
            .send()
            .await
            .expect("service stop request should send");
        assert_eq!(stop.status(), StatusCode::OK);
        let stop_body = stop
            .json::<serde_json::Value>()
            .await
            .expect("service stop response should parse");
        assert_eq!(stop_body["state"], json!("stopped"));
        assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn tenant_workload_service_routes_do_not_require_operator_security() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
        let backend = Arc::new(ReadySandboxBackend {
            image_starts: AtomicUsize::new(0),
            stop_calls: AtomicUsize::new(0),
        });
        let server = ServerFixture::start(
            crate::router::RouterBuildConfig::core(engine.clone())
                .with_service_manager(service_manager(backend.clone()))
                .with_application_auth_verifier(Arc::new(StaticServiceRouteAuthVerifier))
                .without_deploy_admin_token()
                .build(),
        )
        .await;

        let start = server
            .client()
            .post(server.http_url("/api/tenants/tenanta/services/db/start"))
            .bearer_auth("tenant-a-db")
            .send()
            .await
            .expect("tenant service route request should send");
        assert_eq!(start.status(), StatusCode::OK);
        assert_eq!(backend.image_starts.load(Ordering::SeqCst), 1);

        let get = server
            .client()
            .get(server.http_url("/api/tenants/tenanta/services/db"))
            .bearer_auth("tenant-a-db")
            .send()
            .await
            .expect("tenant service get request should send");
        assert_eq!(get.status(), StatusCode::OK);
        let get_body = get
            .json::<serde_json::Value>()
            .await
            .expect("service get response should parse");
        assert_eq!(get_body["tenantId"], json!("tenanta"));
        assert_eq!(get_body["name"], json!("db"));
        assert_eq!(get_body["state"], json!("ready"));
    }

    #[tokio::test]
    async fn spawned_workload_service_routes_use_own_tenant_and_exact_grants() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
        let backend = Arc::new(ReadySandboxBackend {
            image_starts: AtomicUsize::new(0),
            stop_calls: AtomicUsize::new(0),
        });
        let server = ServerFixture::start(
            crate::router::RouterBuildConfig::core(engine.clone())
                .with_service_manager(service_manager(backend.clone()))
                .with_application_auth_verifier(Arc::new(StaticServiceRouteAuthVerifier))
                .without_deploy_admin_token()
                .build(),
        )
        .await;

        let start = server
            .client()
            .post(server.http_url("/api/tenants/tenanta/services/db/start"))
            .bearer_auth("tenant-a-spawned-db")
            .send()
            .await
            .expect("spawned service route request should send");
        assert_eq!(start.status(), StatusCode::OK);
        let start_body = start
            .json::<serde_json::Value>()
            .await
            .expect("spawned service route response should parse");
        assert_eq!(start_body["tenantId"], json!("tenanta"));
        assert_eq!(start_body["name"], json!("db"));
        assert_eq!(start_body["state"], json!("ready"));
        assert_eq!(backend.image_starts.load(Ordering::SeqCst), 1);

        let get = server
            .client()
            .get(server.http_url("/api/tenants/tenanta/services/db"))
            .bearer_auth("tenant-a-spawned-db")
            .send()
            .await
            .expect("spawned service get request should send");
        assert_eq!(get.status(), StatusCode::OK);
        let get_body = get
            .json::<serde_json::Value>()
            .await
            .expect("spawned service get response should parse");
        assert_eq!(get_body["tenantId"], json!("tenanta"));
        assert_eq!(get_body["name"], json!("db"));
        assert_eq!(get_body["state"], json!("ready"));

        let cross_tenant = server
            .client()
            .post(server.http_url("/api/tenants/tenantb/services/db/start"))
            .bearer_auth("tenant-a-spawned-db")
            .send()
            .await
            .expect("spawned cross-tenant service request should send");
        assert_eq!(cross_tenant.status(), StatusCode::FORBIDDEN);

        let different_service = server
            .client()
            .post(server.http_url("/api/tenants/tenanta/services/cache/start"))
            .bearer_auth("tenant-a-spawned-db")
            .send()
            .await
            .expect("spawned different-service request should send");
        assert_eq!(different_service.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            backend.image_starts.load(Ordering::SeqCst),
            1,
            "denied spawned requests must not start additional services"
        );
    }

    #[tokio::test]
    async fn principal_class_service_route_policy_allows_operator_cross_tenant_and_audits() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let (local_server_security, token) = local_server_security(temp.path());
        let audit_log_path = local_server_security.paths().audit_log_path.clone();
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
        let backend = Arc::new(ReadySandboxBackend {
            image_starts: AtomicUsize::new(0),
            stop_calls: AtomicUsize::new(0),
        });
        let server = ServerFixture::start(
            crate::router::RouterBuildConfig::core(engine.clone())
                .with_service_manager(service_manager(backend.clone()))
                .with_local_server_security(local_server_security)
                .without_deploy_admin_token()
                .build(),
        )
        .await;

        let response = server
            .client()
            .post(server.http_url("/api/tenants/tenantb/services/db/start"))
            .bearer_auth(&token.token)
            .send()
            .await
            .expect("operator cross-tenant service request should send");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(backend.image_starts.load(Ordering::SeqCst), 1);
        let records = read_audit_records(&audit_log_path);
        assert!(records.iter().any(|record| {
            record.success
                && record.tenant_id.as_deref() == Some("tenantb")
                && record.auth_scope == "service_principal_class"
                && record.reason.contains("principal_class=operator")
        }));
    }

    #[tokio::test]
    async fn principal_class_service_route_policy_rejects_tenant_cross_tenant() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let (local_server_security, _token) = local_server_security(temp.path());
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
        let backend = Arc::new(ReadySandboxBackend {
            image_starts: AtomicUsize::new(0),
            stop_calls: AtomicUsize::new(0),
        });
        let server = ServerFixture::start(
            crate::router::RouterBuildConfig::core(engine.clone())
                .with_service_manager(service_manager(backend.clone()))
                .with_local_server_security(local_server_security)
                .with_application_auth_verifier(Arc::new(StaticServiceRouteAuthVerifier))
                .without_deploy_admin_token()
                .build(),
        )
        .await;

        let response = server
            .client()
            .post(server.http_url("/api/tenants/tenantb/services/db/start"))
            .bearer_auth("tenant-a-db")
            .send()
            .await
            .expect("tenant cross-tenant service request should send");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(backend.image_starts.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn principal_class_service_route_policy_requires_exact_service_grant() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let (local_server_security, _token) = local_server_security(temp.path());
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
        let backend = Arc::new(ReadySandboxBackend {
            image_starts: AtomicUsize::new(0),
            stop_calls: AtomicUsize::new(0),
        });
        let server = ServerFixture::start(
            crate::router::RouterBuildConfig::core(engine.clone())
                .with_service_manager(service_manager(backend.clone()))
                .with_local_server_security(local_server_security)
                .with_application_auth_verifier(Arc::new(StaticServiceRouteAuthVerifier))
                .without_deploy_admin_token()
                .build(),
        )
        .await;

        let tenantless = server
            .client()
            .post(server.http_url("/api/tenants/tenanta/services/db/start"))
            .bearer_auth("tenantless-db")
            .send()
            .await
            .expect("tenantless service request should send");
        assert_eq!(tenantless.status(), StatusCode::FORBIDDEN);

        let ungranted = server
            .client()
            .post(server.http_url("/api/tenants/tenanta/services/db/start"))
            .bearer_auth("tenant-a-none")
            .send()
            .await
            .expect("ungranted exact service request should send");
        assert_eq!(ungranted.status(), StatusCode::FORBIDDEN);

        let wildcard = server
            .client()
            .post(server.http_url("/api/tenants/tenanta/services/db/start"))
            .bearer_auth("tenant-a-wildcard")
            .send()
            .await
            .expect("wildcard service request should send");
        assert_eq!(wildcard.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            backend.image_starts.load(Ordering::SeqCst),
            0,
            "tenantless, ungranted, and wildcard service requests must not materialize sandboxes"
        );

        let granted = server
            .client()
            .post(server.http_url("/api/tenants/tenanta/services/db/start"))
            .bearer_auth("tenant-a-db")
            .send()
            .await
            .expect("exact service grant request should send");
        assert_eq!(granted.status(), StatusCode::OK);

        let other_service = server
            .client()
            .post(server.http_url("/api/tenants/tenanta/services/cache/start"))
            .bearer_auth("tenant-a-db")
            .send()
            .await
            .expect("different service request should send");
        assert_eq!(other_service.status(), StatusCode::FORBIDDEN);

        let operator_claim = server
            .client()
            .post(server.http_url("/api/tenants/tenanta/services/db/start"))
            .bearer_auth("tenant-a-operator-db")
            .send()
            .await
            .expect("operator principal class claim request should send");
        assert_eq!(operator_claim.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn principal_class_service_route_policy_rejects_spawned_admin_routes() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let (local_server_security, _token) = local_server_security(temp.path());
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
        let server = ServerFixture::start(
            crate::router::RouterBuildConfig::core(engine.clone())
                .with_local_server_security(local_server_security)
                .with_application_auth_verifier(Arc::new(StaticServiceRouteAuthVerifier))
                .without_deploy_admin_token()
                .build(),
        )
        .await;

        let response = server
            .client()
            .post(server.http_url("/api/tenants"))
            .bearer_auth("tenant-a-spawned-db")
            .json(&json!({ "id": "admin-route-denied" }))
            .send()
            .await
            .expect("spawned admin route request should send");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
