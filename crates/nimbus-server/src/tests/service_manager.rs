use std::collections::BTreeMap;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::http::StatusCode;
use futures::future::BoxFuture;
use nimbus_auth::{ApplicationAuthError, ApplicationAuthVerifier};
use nimbus_core::{
    InvocationAuth, RuntimeUserIdentity, TableName, TenantId, VerifiedUserIdentity,
    VerifiedUserIdentityKind,
};
use nimbus_engine::Engine;
use nimbus_runtime::HostCallCancellation;
use nimbus_sandbox::{
    PublishedEndpoint, PublishedEndpointProtocol, SandboxBackend, SandboxBackendKind, SandboxError,
    SandboxFuture, SandboxHandle, SandboxId, SandboxOciImageSource, SandboxOwnerSpec,
    SandboxProcessSpec, SandboxRootSpec, SandboxSpec, SandboxStatus,
};
use nimbus_services::{
    RuntimeServiceRegistry, ServiceBackend, ServiceDefinitionCatalog, ServiceManager,
};
use nimbus_testing::ServerFixture;
use serde_json::{Map, Value, json};

use crate::local_server::{
    LocalServerAuditRecord, LocalServerPaths, LocalServerSecurityState,
    load_or_create_local_admin_token,
};
use crate::service_manager::attach_system_state_engine;

#[path = "service_manager/definitions.rs"]
mod definitions;
#[path = "service_manager/redaction.rs"]
mod redaction;
#[path = "service_manager/sandboxes.rs"]
mod sandboxes;
#[path = "service_manager/sessions.rs"]
mod sessions;

struct StubServiceDefinitionCatalog {
    image_launches: BTreeMap<String, String>,
    custom_backends: BTreeMap<String, ServiceBackend>,
}

impl ServiceDefinitionCatalog for StubServiceDefinitionCatalog {
    fn service_backend_for_tenant(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<ServiceBackend> {
        if let Some(backend) = self.custom_backends.get(service_name) {
            return Some(backend.clone());
        }
        self.image_launches.get(service_name).map(|image| {
            let mut spec = sparse_image_spec(tenant_id, service_name);
            spec.root = SandboxRootSpec::oci_image_reference(image.as_str());
            ServiceBackend::sandbox(spec)
        })
    }

    fn service_backends_for_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> BTreeMap<String, ServiceBackend> {
        let mut backends = self.custom_backends.clone();
        for (service_name, image) in &self.image_launches {
            backends.entry(service_name.clone()).or_insert_with(|| {
                let mut spec = sparse_image_spec(tenant_id, service_name);
                spec.root = SandboxRootSpec::oci_image_reference(image.as_str());
                ServiceBackend::sandbox(spec)
            });
        }
        backends
    }
}

struct ReadySandboxBackend {
    image_starts: AtomicUsize,
    stop_calls: AtomicUsize,
}

impl ReadySandboxBackend {
    fn handle(tenant_id: &TenantId, service_name: &str, status: SandboxStatus) -> SandboxHandle {
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
        let is_image_start = matches!(
            &spec.root,
            SandboxRootSpec::OciImage(image)
                if matches!(&image.source, SandboxOciImageSource::Reference(_))
        );
        if is_image_start {
            self.image_starts.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok(Self::handle(
                    &spec.tenant_id,
                    spec.display_name(),
                    SandboxStatus::Ready,
                ))
            })
        } else {
            let service_name = spec.display_name().to_owned();
            Box::pin(async move {
                Err(SandboxError::InvalidSpec {
                    message: format!("non-image service backend unsupported for {service_name}"),
                })
            })
        }
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
        SandboxOwnerSpec::service(name),
        SandboxBackendKind::Krun,
        SandboxRootSpec::rootfs(""),
        SandboxProcessSpec::new(Vec::<String>::new()),
    )
}

fn standalone_sandbox_spec(tenant_id: &TenantId, display_name: &str) -> SandboxSpec {
    SandboxSpec::new(
        tenant_id.clone(),
        SandboxOwnerSpec::standalone_named(display_name),
        SandboxBackendKind::Krun,
        SandboxRootSpec::oci_image_reference("registry.example.com/task:latest"),
        SandboxProcessSpec::new(vec!["task".to_owned()]),
    )
}

fn sandbox_spec_body(tenant_id: &str, owner: Value) -> Value {
    json!({
        "tenantId": tenant_id,
        "owner": owner,
        "backend": "krun",
        "root": {
            "kind": "oci_image",
            "source": {
                "kind": "reference",
                "reference": "registry.example.com/task:latest",
            },
        },
        "process": {
            "argv": ["task", "--password=launch-secret"],
            "env": ["NIMBUS_SECRET=launch-secret"],
        },
    })
}

fn sandbox_rootfs_spec_body(tenant_id: &str, owner: Value) -> Value {
    json!({
        "tenantId": tenant_id,
        "owner": owner,
        "backend": "krun",
        "root": {
            "kind": "rootfs",
            "rootfs": "/private/host/rootfs",
        },
        "process": {
            "argv": ["task"],
        },
    })
}

fn sandbox_build_spec_body(tenant_id: &str, owner: Value) -> Value {
    json!({
        "tenantId": tenant_id,
        "owner": owner,
        "backend": "krun",
        "root": {
            "kind": "oci_image",
            "source": {
                "kind": "build",
                "imageName": "registry.example.com/task:local",
                "dockerfilePath": "/private/host/Dockerfile",
                "contextPath": "/private/host/context",
            },
        },
        "process": {
            "argv": ["task"],
        },
    })
}

fn sandbox_service_definition_body(tenant_id: &str, service_name: &str) -> Value {
    json!({
        "metadata": {
            "tenantId": tenant_id,
            "name": service_name,
            "labels": {
                "app": "test",
            },
        },
        "spec": {
            "backend": {
                "kind": "sandbox",
                "sandbox": sandbox_spec_body(
                    tenant_id,
                    json!({ "kind": "service", "serviceName": service_name }),
                ),
            },
        },
    })
}

fn built_in_service_definition_body(
    tenant_id: &str,
    service_name: &str,
    generation: u64,
    provider: &str,
) -> Value {
    json!({
        "metadata": {
            "tenantId": tenant_id,
            "name": service_name,
            "generation": generation,
            "labels": {
                "app": "test",
            },
        },
        "spec": {
            "backend": {
                "kind": "builtIn",
                "provider": provider,
            },
        },
    })
}

fn external_service_definition_body(tenant_id: &str, service_name: &str, endpoint: &str) -> Value {
    json!({
        "metadata": {
            "tenantId": tenant_id,
            "name": service_name,
        },
        "spec": {
            "backend": {
                "kind": "external",
                "endpoint": { "url": endpoint },
                "auth": { "kind": "none" },
                "health": { "kind": "http", "path": "/health" },
            },
        },
    })
}

fn sandbox_create_body(tenant_id: &str, display_name: &str) -> Value {
    json!({
        "profile": "worker",
        "spec": sandbox_spec_body(
            tenant_id,
            json!({ "kind": "standalone", "displayName": display_name }),
        ),
        "labels": {
            "app": "task",
        },
    })
}

fn service_owned_sandbox_create_body(tenant_id: &str, service_name: &str) -> Value {
    json!({
        "profile": "worker",
        "spec": sandbox_spec_body(
            tenant_id,
        json!({ "kind": "service", "serviceName": service_name }),
        ),
    })
}

fn sandbox_create_body_with_spec(spec: Value) -> Value {
    json!({
        "profile": "worker",
        "spec": spec,
    })
}

fn service_manager(backend: Arc<ReadySandboxBackend>) -> Arc<ServiceManager> {
    service_manager_with_catalog(backend, BTreeMap::new())
}

fn service_manager_with_catalog(
    backend: Arc<ReadySandboxBackend>,
    custom_backends: BTreeMap<String, ServiceBackend>,
) -> Arc<ServiceManager> {
    Arc::new(
        ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                image_launches: BTreeMap::from([("db".to_owned(), "postgres:16".to_owned())]),
                custom_backends,
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
                "tenant-a-none" => Ok(service_route_auth(token, "tenanta", Some("tenant"), &[])),
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
                "tenant-a-worker-definition" => Ok(service_definition_route_auth(
                    token,
                    "tenanta",
                    Some("tenant"),
                    &[],
                    &["create", "list", "update", "delete"],
                    "worker",
                )),
                "tenant-a-worker-definition-list" => Ok(service_definition_route_auth(
                    token,
                    "tenanta",
                    Some("tenant"),
                    &[],
                    &["list"],
                    "worker",
                )),
                "tenant-a-worker-definition-inspect" => Ok(service_definition_route_auth(
                    token,
                    "tenanta",
                    Some("tenant"),
                    &[],
                    &["list", "inspect"],
                    "worker",
                )),
                "tenant-a-worker-definition-force" => Ok(service_definition_route_auth(
                    token,
                    "tenanta",
                    Some("tenant"),
                    &["worker"],
                    &["delete", "forceDelete"],
                    "worker",
                )),
                "tenant-a-browser-definition-force" => Ok(service_definition_route_auth(
                    token,
                    "tenanta",
                    Some("tenant"),
                    &["browser"],
                    &["delete", "forceDelete"],
                    "browser",
                )),
                "tenant-a-sandbox" => Ok(sandbox_route_auth(
                    token,
                    "tenanta",
                    Some("tenant"),
                    &["create", "list", "get", "stop"],
                )),
                "tenant-a-sandbox-task-list" => Ok(sandbox_route_auth_with_scope(
                    token,
                    "tenanta",
                    Some("tenant"),
                    &["list"],
                    json!({
                        "kind": "exactId",
                        "id": "sandbox-tenanta-task",
                    }),
                )),
                "tenant-a-sandbox-task-prefix-list" => Ok(sandbox_route_auth_with_scope(
                    token,
                    "tenanta",
                    Some("tenant"),
                    &["list"],
                    json!({
                        "kind": "idPrefix",
                        "prefix": "sandbox-tenanta-t",
                    }),
                )),
                "tenant-b-sandbox" => Ok(sandbox_route_auth(
                    token,
                    "tenantb",
                    Some("tenant"),
                    &["create", "list", "get", "stop"],
                )),
                "tenant-a-browser-session" => Ok(session_route_auth(
                    token,
                    "tenanta",
                    Some("tenant"),
                    &["browser"],
                    &["open", "list", "get", "close"],
                    &["cdp", "page"],
                    false,
                )),
                "tenant-a-browser-session-service-scope" => Ok(session_route_auth_with_scope(
                    token,
                    "tenanta",
                    Some("tenant"),
                    &["browser"],
                    &["open", "list", "get", "close"],
                    &["cdp", "page"],
                    SessionRouteScopeOptions {
                        scope: json!({
                            "kind": "service",
                            "name": "browser",
                        }),
                        include_sandbox_reach: false,
                    },
                )),
                "tenant-b-browser-session" => Ok(session_route_auth(
                    token,
                    "tenantb",
                    Some("tenant"),
                    &["browser"],
                    &["open", "list", "get", "close"],
                    &["cdp", "page"],
                    false,
                )),
                "tenant-a-browser-session-no-grant" => Ok(session_route_auth(
                    token,
                    "tenanta",
                    Some("tenant"),
                    &[],
                    &["open", "list", "get", "close"],
                    &["cdp", "page"],
                    false,
                )),
                "tenant-a-browser-session-wildcard-grant" => Ok(session_route_auth(
                    token,
                    "tenanta",
                    Some("tenant"),
                    &["browser", "services:*"],
                    &["open", "list", "get", "close"],
                    &["cdp", "page"],
                    false,
                )),
                "tenant-a-browser-session-stdio" => Ok(session_route_auth(
                    token,
                    "tenanta",
                    Some("tenant"),
                    &["browser"],
                    &["open"],
                    &["stdio"],
                    false,
                )),
                "tenant-a-sandbox-session" => Ok(session_route_auth(
                    token,
                    "tenanta",
                    Some("tenant"),
                    &[],
                    &["open", "list", "get", "close"],
                    &["stdio", "files"],
                    true,
                )),
                "tenant-a-sandbox-session-sandbox-scope" => Ok(session_route_auth_with_scope(
                    token,
                    "tenanta",
                    Some("tenant"),
                    &[],
                    &["list", "get", "close"],
                    &["stdio", "files"],
                    SessionRouteScopeOptions {
                        scope: json!({
                            "kind": "sandbox",
                            "id": "sandbox-tenanta-task",
                        }),
                        include_sandbox_reach: true,
                    },
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

fn sandbox_route_auth(
    token: &str,
    tenant_id: &str,
    principal_class: Option<&str>,
    actions: &[&str],
) -> InvocationAuth {
    sandbox_route_auth_with_scope(
        token,
        tenant_id,
        principal_class,
        actions,
        json!({
            "kind": "tenant",
        }),
    )
}

fn sandbox_route_auth_with_scope(
    token: &str,
    tenant_id: &str,
    principal_class: Option<&str>,
    actions: &[&str],
    scope: Value,
) -> InvocationAuth {
    let mut claims = service_route_claims(tenant_id, principal_class, &[]);
    claims.insert(
        "nimbus_sandbox_permissions".to_string(),
        json!([{
            "actions": actions,
            "scope": scope,
        }]),
    );
    InvocationAuth::with_identities(
        runtime_identity(token, claims.clone()),
        verified_identity(token, claims),
        false,
    )
}

fn service_definition_route_auth(
    token: &str,
    tenant_id: &str,
    principal_class: Option<&str>,
    grants: &[&str],
    actions: &[&str],
    service_name: &str,
) -> InvocationAuth {
    let mut claims = service_route_claims(tenant_id, principal_class, grants);
    claims.insert(
        "nimbus_service_definition_permissions".to_string(),
        json!([{
            "actions": actions,
            "scope": {
                "kind": "exactName",
                "name": service_name,
            },
        }]),
    );
    InvocationAuth::with_identities(
        runtime_identity(token, claims.clone()),
        verified_identity(token, claims),
        false,
    )
}

fn session_route_auth(
    token: &str,
    tenant_id: &str,
    principal_class: Option<&str>,
    service_grants: &[&str],
    session_actions: &[&str],
    channels: &[&str],
    include_sandbox_reach: bool,
) -> InvocationAuth {
    session_route_auth_with_scope(
        token,
        tenant_id,
        principal_class,
        service_grants,
        session_actions,
        channels,
        SessionRouteScopeOptions {
            scope: json!({
                "kind": "tenant",
            }),
            include_sandbox_reach,
        },
    )
}

struct SessionRouteScopeOptions {
    scope: Value,
    include_sandbox_reach: bool,
}

fn session_route_auth_with_scope(
    token: &str,
    tenant_id: &str,
    principal_class: Option<&str>,
    service_grants: &[&str],
    session_actions: &[&str],
    channels: &[&str],
    options: SessionRouteScopeOptions,
) -> InvocationAuth {
    let mut claims = service_route_claims(tenant_id, principal_class, service_grants);
    claims.insert(
        "nimbus_session_permissions".to_string(),
        json!([{
            "actions": session_actions,
            "channels": channels,
            "scope": options.scope,
        }]),
    );
    if options.include_sandbox_reach {
        claims.insert(
            "nimbus_sandbox_permissions".to_string(),
            json!([{
                "actions": ["get"],
                "scope": {
                    "kind": "tenant",
                },
            }]),
        );
    }
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
async fn operator_service_definition_routes_are_resource_shaped_and_preconditioned() {
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

    let create = server
        .client()
        .post(server.http_url("/api/tenants/tenant/services"))
        .bearer_auth(&token.token)
        .json(&sandbox_service_definition_body("tenant", "worker"))
        .send()
        .await
        .expect("service definition create should send");
    assert_eq!(create.status(), StatusCode::CREATED);
    let create_body = create
        .json::<Value>()
        .await
        .expect("create response should parse");
    assert_eq!(create_body["metadata"]["tenantId"], json!("tenant"));
    assert_eq!(create_body["metadata"]["name"], json!("worker"));
    assert_eq!(create_body["metadata"]["generation"], json!(1));
    assert_eq!(create_body["metadata"]["source"], json!("dynamic"));
    assert_eq!(create_body["spec"]["backend"]["kind"], json!("sandbox"));
    assert_eq!(
        create_body["status"]["conditions"][0]["observedGeneration"],
        json!(1)
    );

    let list = server
        .client()
        .get(server.http_url("/api/tenants/tenant/services?limit=10"))
        .bearer_auth(&token.token)
        .send()
        .await
        .expect("service definition list should send");
    assert_eq!(list.status(), StatusCode::OK);
    let list_body = list.json::<Value>().await.expect("list should parse");
    assert_eq!(list_body["metadata"]["tenantId"], json!("tenant"));
    assert_eq!(list_body["metadata"]["limit"], json!(10));
    let worker = list_body["items"]
        .as_array()
        .expect("items should be an array")
        .iter()
        .find(|item| item["metadata"]["name"] == json!("worker"))
        .expect("worker service definition should list");
    assert_eq!(worker["metadata"]["name"], json!("worker"));

    let stale_update = server
        .client()
        .put(server.http_url("/api/tenants/tenant/services/worker"))
        .bearer_auth(&token.token)
        .json(&built_in_service_definition_body(
            "tenant", "worker", 99, "browser",
        ))
        .send()
        .await
        .expect("stale service definition update should send");
    assert_eq!(stale_update.status(), StatusCode::PRECONDITION_FAILED);

    let update = server
        .client()
        .put(server.http_url("/api/tenants/tenant/services/worker"))
        .bearer_auth(&token.token)
        .json(&built_in_service_definition_body(
            "tenant", "worker", 1, "browser",
        ))
        .send()
        .await
        .expect("service definition update should send");
    assert_eq!(update.status(), StatusCode::OK);
    let update_body = update.json::<Value>().await.expect("update should parse");
    assert_eq!(update_body["metadata"]["generation"], json!(2));
    assert_eq!(update_body["spec"]["backend"]["kind"], json!("builtIn"));

    let delete_without_precondition = server
        .client()
        .delete(server.http_url("/api/tenants/tenant/services/worker"))
        .bearer_auth(&token.token)
        .send()
        .await
        .expect("service definition delete should send");
    assert_eq!(
        delete_without_precondition.status(),
        StatusCode::BAD_REQUEST
    );

    let stale_delete = server
        .client()
        .delete(server.http_url("/api/tenants/tenant/services/worker?ifMatchGeneration=1"))
        .bearer_auth(&token.token)
        .send()
        .await
        .expect("stale service definition delete should send");
    assert_eq!(stale_delete.status(), StatusCode::PRECONDITION_FAILED);

    let delete = server
        .client()
        .delete(server.http_url("/api/tenants/tenant/services/worker?ifMatchGeneration=2"))
        .bearer_auth(&token.token)
        .send()
        .await
        .expect("service definition delete should send");
    assert_eq!(delete.status(), StatusCode::NO_CONTENT);

    let external = server
        .client()
        .post(server.http_url("/api/tenants/tenant/services"))
        .bearer_auth(&token.token)
        .json(&external_service_definition_body(
            "tenant",
            "api",
            "https://api.example.com",
        ))
        .send()
        .await
        .expect("external service definition create should send");
    assert_eq!(external.status(), StatusCode::CREATED);
    let external_body = external
        .json::<Value>()
        .await
        .expect("external should parse");
    assert_eq!(external_body["spec"]["backend"]["kind"], json!("external"));
    assert_eq!(
        external_body["spec"]["backend"]["endpoint"]["url"],
        json!("https://api.example.com")
    );
    assert_eq!(
        external_body["spec"]["backend"]["auth"]["kind"],
        json!("none")
    );
    assert_eq!(
        external_body["spec"]["backend"]["health"]["path"],
        json!("/health")
    );
}

fn assert_sandbox_resource_response_redacts_launch_details(response: &Value) {
    let rendered = serde_json::to_string(response).expect("response should serialize");
    for forbidden in ["launch-secret", "NIMBUS_SECRET"] {
        assert!(
            !rendered.contains(forbidden),
            "sandbox resource response leaked forbidden launch detail `{forbidden}`: {rendered}"
        );
    }

    let process = &response["spec"]["sandbox"]["process"];
    assert!(
        process.get("env").is_none(),
        "sandbox resource response must not expose raw env values"
    );
    assert_eq!(process["argv"]["redacted"], json!(true));
    assert_eq!(process["environment"]["redacted"], json!(true));
}

#[tokio::test]
async fn service_definition_permissions_do_not_imply_service_lifecycle_grants() {
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

    let create = server
        .client()
        .post(server.http_url("/api/tenants/tenanta/services"))
        .bearer_auth("tenant-a-worker-definition")
        .json(&sandbox_service_definition_body("tenanta", "worker"))
        .send()
        .await
        .expect("tenant service definition create should send");
    assert_eq!(create.status(), StatusCode::CREATED);

    let start = server
        .client()
        .post(server.http_url("/api/tenants/tenanta/services/worker/start"))
        .bearer_auth("tenant-a-worker-definition")
        .send()
        .await
        .expect("tenant service start should send");
    assert_eq!(start.status(), StatusCode::FORBIDDEN);
    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 0);

    let exact_grant_create = server
        .client()
        .post(server.http_url("/api/tenants/tenanta/services"))
        .bearer_auth("tenant-a-db")
        .json(&sandbox_service_definition_body("tenanta", "other"))
        .send()
        .await
        .expect("exact service grant create should send");
    assert_eq!(exact_grant_create.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn service_definition_list_only_permission_redacts_inspect_details() {
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
            .with_application_auth_verifier(Arc::new(StaticServiceRouteAuthVerifier))
            .without_deploy_admin_token()
            .build(),
    )
    .await;

    let create = server
        .client()
        .post(server.http_url("/api/tenants/tenanta/services"))
        .bearer_auth(&token.token)
        .json(&external_service_definition_body(
            "tenanta",
            "worker",
            "https://private-control.example.com",
        ))
        .send()
        .await
        .expect("operator service definition create should send");
    assert_eq!(create.status(), StatusCode::CREATED);

    let list_only = server
        .client()
        .get(server.http_url("/api/tenants/tenanta/services"))
        .bearer_auth("tenant-a-worker-definition-list")
        .send()
        .await
        .expect("list-only service definition list should send");
    assert_eq!(list_only.status(), StatusCode::OK);
    let list_only_body = list_only.json::<Value>().await.expect("list should parse");
    let list_only_items = list_only_body["items"]
        .as_array()
        .expect("list-only items should be an array");
    assert_eq!(
        list_only_items.len(),
        1,
        "list-only exact-name scope should list only the scoped service"
    );
    assert_eq!(list_only_items[0]["metadata"]["name"], json!("worker"));
    assert_eq!(
        list_only_items[0]["spec"]["backend"]["kind"],
        json!("redacted")
    );
    assert_eq!(
        list_only_items[0]["spec"]["backend"]["backend"],
        json!("external")
    );
    assert_eq!(
        list_only_items[0]["spec"]["backend"]["reason"],
        json!("requiresInspectPermission")
    );
    assert!(
        !serde_json::to_string(&list_only_body)
            .expect("list-only body should serialize")
            .contains("private-control.example.com"),
        "list-only permission must not expose inspect-only endpoint configuration"
    );

    let inspect = server
        .client()
        .get(server.http_url("/api/tenants/tenanta/services"))
        .bearer_auth("tenant-a-worker-definition-inspect")
        .send()
        .await
        .expect("inspect-capable service definition list should send");
    assert_eq!(inspect.status(), StatusCode::OK);
    let inspect_body = inspect.json::<Value>().await.expect("inspect should parse");
    let inspect_item = inspect_body["items"]
        .as_array()
        .expect("inspect items should be an array")
        .iter()
        .find(|item| item["metadata"]["name"] == json!("worker"))
        .expect("worker should be listed for inspect principal");
    assert_eq!(inspect_item["spec"]["backend"]["kind"], json!("external"));
    assert_eq!(
        inspect_item["spec"]["backend"]["endpoint"]["url"],
        json!("https://private-control.example.com")
    );
}

#[tokio::test]
async fn service_definition_force_delete_requires_separate_policy_and_exact_service_grant() {
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

    let create = server
        .client()
        .post(server.http_url("/api/tenants/tenanta/services"))
        .bearer_auth("tenant-a-worker-definition")
        .json(&sandbox_service_definition_body("tenanta", "worker"))
        .send()
        .await
        .expect("tenant service definition create should send");
    assert_eq!(create.status(), StatusCode::CREATED);

    let start = server
        .client()
        .post(server.http_url("/api/tenants/tenanta/services/worker/start"))
        .bearer_auth("tenant-a-worker-definition-force")
        .send()
        .await
        .expect("worker start should send");
    assert_eq!(start.status(), StatusCode::OK);
    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 1);

    let unauthorized_force = server
        .client()
        .delete(
            server.http_url("/api/tenants/tenanta/services/worker?ifMatchGeneration=1&force=true"),
        )
        .bearer_auth("tenant-a-worker-definition")
        .send()
        .await
        .expect("unauthorized force delete should send");
    assert_eq!(unauthorized_force.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        backend.stop_calls.load(Ordering::SeqCst),
        0,
        "unauthorized force delete must not stop the service backend"
    );

    let force_delete = server
        .client()
        .delete(
            server.http_url("/api/tenants/tenanta/services/worker?ifMatchGeneration=1&force=true"),
        )
        .bearer_auth("tenant-a-worker-definition-force")
        .send()
        .await
        .expect("authorized force delete should send");
    assert_eq!(force_delete.status(), StatusCode::NO_CONTENT);
    assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn service_definition_update_rejects_active_backend_until_stopped() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let (local_server_security, token) = local_server_security(temp.path());
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
    let backend = Arc::new(ReadySandboxBackend {
        image_starts: AtomicUsize::new(0),
        stop_calls: AtomicUsize::new(0),
    });
    let manager = service_manager(backend.clone());
    let server = ServerFixture::start(
        crate::router::RouterBuildConfig::core(engine.clone())
            .with_service_manager(manager.clone())
            .with_local_server_security(local_server_security)
            .without_deploy_admin_token()
            .build(),
    )
    .await;

    let create = server
        .client()
        .post(server.http_url("/api/tenants/tenant/services"))
        .bearer_auth(&token.token)
        .json(&sandbox_service_definition_body("tenant", "worker"))
        .send()
        .await
        .expect("service definition create should send");
    assert_eq!(create.status(), StatusCode::CREATED);

    let start = server
        .client()
        .post(server.http_url("/api/tenants/tenant/services/worker/start"))
        .bearer_auth(&token.token)
        .send()
        .await
        .expect("worker start should send");
    assert_eq!(start.status(), StatusCode::OK);
    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 1);

    let active_update = server
        .client()
        .put(server.http_url("/api/tenants/tenant/services/worker"))
        .bearer_auth(&token.token)
        .json(&built_in_service_definition_body(
            "tenant", "worker", 1, "browser",
        ))
        .send()
        .await
        .expect("active service update should send");
    assert_eq!(active_update.status(), StatusCode::CONFLICT);
    let definition = manager
        .service_definition_for_tenant(
            &TenantId::new("tenant").expect("tenant id should parse"),
            "worker",
        )
        .expect("definition should still exist");
    assert_eq!(definition.generation, 1);
    assert!(
        matches!(definition.backend, ServiceBackend::Sandbox(_)),
        "rejected update must preserve the running service's sandbox backend definition"
    );

    let stop = server
        .client()
        .post(server.http_url("/api/tenants/tenant/services/worker/stop"))
        .bearer_auth(&token.token)
        .send()
        .await
        .expect("worker stop should send");
    assert_eq!(stop.status(), StatusCode::OK);

    let stopped_update = server
        .client()
        .put(server.http_url("/api/tenants/tenant/services/worker"))
        .bearer_auth(&token.token)
        .json(&built_in_service_definition_body(
            "tenant", "worker", 1, "browser",
        ))
        .send()
        .await
        .expect("stopped service update should send");
    assert_eq!(stopped_update.status(), StatusCode::OK);
    let stopped_update_body = stopped_update
        .json::<Value>()
        .await
        .expect("stopped update body should parse");
    assert_eq!(stopped_update_body["metadata"]["generation"], json!(2));
    assert_eq!(
        stopped_update_body["spec"]["backend"]["kind"],
        json!("builtIn")
    );
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
