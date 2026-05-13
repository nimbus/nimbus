use std::collections::BTreeMap;
use std::env;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nimbus_core::TenantId;
use nimbus_runtime::{
    HostCallCancellation, InvocationServiceBinding, InvocationServiceEndpoint,
    InvocationServiceProtocol, InvocationServices,
};
use nimbus_sandbox::{
    PublishedEndpoint, PublishedEndpointProtocol, SandboxBackend, SandboxBackendKind, SandboxError,
    SandboxFilesystemSpec, SandboxFuture, SandboxHandle, SandboxId, SandboxImageLaunchSpec,
    SandboxImageProcessOverrides, SandboxPortBinding, SandboxProcessSpec, SandboxSpec,
    SandboxStatus,
    backends::krun::{KrunLaunchMode, KrunSandboxBackend, KrunSandboxBackendConfig},
};
use serde_json::json;

use super::*;
use crate::service_registry::{RuntimeServiceBindingFuture, RuntimeServiceRegistry};
use crate::{SandboxCatalog, SandboxServiceCatalog, SandboxServiceLaunch, SandboxServiceManager};

struct StubSandboxCatalog {
    sandboxes: BTreeMap<String, SandboxHandle>,
}

impl SandboxCatalog for StubSandboxCatalog {
    fn sandboxes_for_tenant(&self, _tenant_id: &TenantId) -> BTreeMap<String, SandboxHandle> {
        self.sandboxes.clone()
    }

    fn sandbox_for_service(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<SandboxHandle> {
        self.sandboxes_for_tenant(tenant_id).remove(service_name)
    }
}

struct StubRuntimeServiceRegistry {
    binding: InvocationServiceBinding,
}

impl RuntimeServiceRegistry for StubRuntimeServiceRegistry {
    fn snapshot_for_tenant(&self, _tenant_id: &TenantId) -> InvocationServices {
        InvocationServices::default()
    }

    fn resolve_service_binding(
        &self,
        _tenant_id: &TenantId,
        service_name: &str,
    ) -> Result<Option<InvocationServiceBinding>, nimbus_core::Error> {
        Ok((service_name == "db").then(|| self.binding.clone()))
    }
}

struct ActivatingRuntimeServiceRegistry {
    binding: InvocationServiceBinding,
    ensure_calls: AtomicUsize,
    delay: Duration,
}

impl RuntimeServiceRegistry for ActivatingRuntimeServiceRegistry {
    fn snapshot_for_tenant(&self, _tenant_id: &TenantId) -> InvocationServices {
        InvocationServices::default()
    }

    fn resolve_service_binding(
        &self,
        _tenant_id: &TenantId,
        _service_name: &str,
    ) -> Result<Option<InvocationServiceBinding>, nimbus_core::Error> {
        Ok(None)
    }

    fn ensure_service_binding_async<'a>(
        &'a self,
        _tenant_id: &'a TenantId,
        service_name: &'a str,
        cancellation: HostCallCancellation,
    ) -> RuntimeServiceBindingFuture<'a> {
        self.ensure_calls.fetch_add(1, Ordering::SeqCst);
        let binding = self.binding.clone();
        let delay = self.delay;
        Box::pin(async move {
            tokio::select! {
                _ = cancellation.cancelled() => Err(nimbus_core::Error::Cancelled),
                _ = tokio::time::sleep(delay) => Ok((service_name == "db").then_some(binding)),
            }
        })
    }
}

struct DeclaredSandboxServiceCatalog {
    launches: BTreeMap<String, SandboxServiceLaunch>,
}

impl SandboxServiceCatalog for DeclaredSandboxServiceCatalog {
    fn sandbox_service_for_tenant(
        &self,
        _tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<SandboxServiceLaunch> {
        self.launches.get(service_name).cloned()
    }
}

fn runtime_limits_with_service_grant(service_name: &str) -> nimbus_runtime::RuntimeLimits {
    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.grants.service = vec![service_name.to_owned()];
    limits
}

struct ManagedSandboxBackend {
    start_calls: AtomicUsize,
    stop_calls: AtomicUsize,
    inspect_calls: AtomicUsize,
    ready_after_inspects: usize,
    handles: Mutex<BTreeMap<String, SandboxHandle>>,
}

impl ManagedSandboxBackend {
    fn new(ready_after_inspects: usize) -> Self {
        Self {
            start_calls: AtomicUsize::new(0),
            stop_calls: AtomicUsize::new(0),
            inspect_calls: AtomicUsize::new(0),
            ready_after_inspects,
            handles: Mutex::new(BTreeMap::new()),
        }
    }

    fn starting_handle(&self, service_name: &str) -> SandboxHandle {
        SandboxHandle::new(
            SandboxId::new(format!("sandbox-{service_name}")),
            service_name,
            SandboxBackendKind::Krun,
            SandboxStatus::Starting,
            Vec::new(),
        )
    }

    fn ready_handle(&self, service_name: &str) -> SandboxHandle {
        SandboxHandle::new(
            SandboxId::new(format!("sandbox-{service_name}")),
            service_name,
            SandboxBackendKind::Krun,
            SandboxStatus::Ready,
            vec![PublishedEndpoint::new(
                "postgres",
                PublishedEndpointProtocol::Tcp,
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 15432),
            )],
        )
    }
}

impl SandboxBackend for ManagedSandboxBackend {
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
        self.start_calls.fetch_add(1, Ordering::SeqCst);
        let handle = self.starting_handle(&launch.spec.name);
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
        let handle = handles.get(id.as_str()).cloned().map(|mut handle| {
            if inspect_call >= self.ready_after_inspects {
                handle = self.ready_handle(&handle.name);
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
}

#[tokio::test]
async fn convex_runtime_query_exposes_service_bindings_and_preserves_them_for_nested_calls() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "services:binding",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx) => ctx.services.db"
            },
            {
                "name": "services:nested",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx) => ctx.runQuery({ name: \"services:binding\", visibility: \"public\" })"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["services:binding", {
    name: "services:binding",
    kind: "query",
    runtime_handler: "async (ctx) => ctx.services.db",
  }],
  ["services:nested", {
    name: "services:nested",
    kind: "query",
    runtime_handler: "async (ctx) => ctx.runQuery({ name: \"services:binding\", visibility: \"public\" })",
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
          sessionId: `${request.kind}:${request.function_name}`,
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
"#,
        ),
    );
    let sandbox_catalog = Arc::new(StubSandboxCatalog {
        sandboxes: BTreeMap::from([(
            "db".to_string(),
            SandboxHandle::new(
                SandboxId::new("sandbox-db"),
                "db",
                SandboxBackendKind::Krun,
                SandboxStatus::Ready,
                vec![
                    PublishedEndpoint::new(
                        "health",
                        PublishedEndpointProtocol::Http,
                        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 18080),
                    ),
                    PublishedEndpoint::new(
                        "postgres",
                        PublishedEndpointProtocol::Tcp,
                        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 15432),
                    ),
                ],
            ),
        )]),
    });
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(crate::build_router_with_convex_and_sandbox_catalog(
        fixture.service(),
        registry.clone(),
        sandbox_catalog,
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let binding = api
        .convex_named_query("demo", "services:binding", json!({}))
        .await;
    assert_eq!(binding.status(), StatusCode::OK);
    let binding_body = binding
        .json::<serde_json::Value>()
        .await
        .expect("service binding response should parse");
    assert_eq!(binding_body["host"], json!("127.0.0.1"));
    assert_eq!(binding_body["port"], json!(15432));
    assert_eq!(binding_body["protocol"], json!("tcp"));
    assert_eq!(binding_body["endpoints"]["health"]["port"], json!(18080));
    assert_eq!(binding_body["endpoints"]["postgres"]["port"], json!(15432));

    let nested = api
        .convex_named_query("demo", "services:nested", json!({}))
        .await;
    assert_eq!(nested.status(), StatusCode::OK);
    let nested_body = nested
        .json::<serde_json::Value>()
        .await
        .expect("nested service binding response should parse");
    assert_eq!(nested_body, binding_body);
}

#[tokio::test]
async fn convex_runtime_query_resolves_missing_service_bindings_via_services_get() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "services:lazy",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx) => ({ namesBefore: Object.keys(ctx.services).sort(), binding: await ctx.services.get(\"db\"), namesAfter: Object.keys(ctx.services).sort() })"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["services:lazy", {
    name: "services:lazy",
    kind: "query",
    runtime_handler: "async (ctx) => ({ namesBefore: Object.keys(ctx.services).sort(), binding: await ctx.services.get(\"db\"), namesAfter: Object.keys(ctx.services).sort() })",
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
          sessionId: `${request.kind}:${request.function_name}`,
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
	"#,
        ),
    )
    .with_runtime_limits(runtime_limits_with_service_grant("db"));
    let runtime_service_registry: Arc<dyn RuntimeServiceRegistry> =
        Arc::new(StubRuntimeServiceRegistry {
            binding: InvocationServiceBinding {
                host: "127.0.0.1".to_string(),
                port: 15432,
                protocol: InvocationServiceProtocol::Tcp,
                endpoints: BTreeMap::from([
                    (
                        "health".to_string(),
                        InvocationServiceEndpoint {
                            host: "127.0.0.1".to_string(),
                            port: 18080,
                            protocol: InvocationServiceProtocol::Http,
                        },
                    ),
                    (
                        "postgres".to_string(),
                        InvocationServiceEndpoint {
                            host: "127.0.0.1".to_string(),
                            port: 15432,
                            protocol: InvocationServiceProtocol::Tcp,
                        },
                    ),
                ]),
            },
        });
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(
        crate::router::build_router_with_convex_and_runtime_service_registry(
            fixture.service(),
            registry.clone(),
            runtime_service_registry,
        ),
    )
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_query("demo", "services:lazy", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("lazy service binding response should parse");

    assert_eq!(body["namesBefore"], json!([]));
    assert_eq!(body["namesAfter"], json!(["db"]));
    assert_eq!(body["binding"]["host"], json!("127.0.0.1"));
    assert_eq!(body["binding"]["port"], json!(15432));
    assert_eq!(body["binding"]["protocol"], json!("tcp"));
    assert_eq!(body["binding"]["endpoints"]["health"]["port"], json!(18080));
    assert_eq!(
        body["binding"]["endpoints"]["postgres"]["port"],
        json!(15432)
    );
}

#[tokio::test]
async fn convex_runtime_query_waits_for_activation_capable_services_get_once() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "services:activate",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx) => { const first = await ctx.services.get(\"db\"); return ({ first: first.port, second: ctx.services.db.port }); }"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["services:activate", {
    name: "services:activate",
    kind: "query",
    runtime_handler: "async (ctx) => { const first = await ctx.services.get(\"db\"); return ({ first: first.port, second: ctx.services.db.port }); }",
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
          sessionId: `${request.kind}:${request.function_name}`,
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
	"#,
        ),
    )
    .with_runtime_limits(runtime_limits_with_service_grant("db"));
    let runtime_service_registry = Arc::new(ActivatingRuntimeServiceRegistry {
        binding: InvocationServiceBinding {
            host: "127.0.0.1".to_string(),
            port: 15432,
            protocol: InvocationServiceProtocol::Tcp,
            endpoints: BTreeMap::default(),
        },
        ensure_calls: AtomicUsize::new(0),
        delay: Duration::from_millis(40),
    });
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(
        crate::router::build_router_with_convex_and_runtime_service_registry(
            fixture.service(),
            registry.clone(),
            runtime_service_registry.clone(),
        ),
    )
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let started_at = Instant::now();
    let response = api
        .convex_named_query("demo", "services:activate", json!({}))
        .await;
    let elapsed = started_at.elapsed();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("activation service binding response should parse");
    assert_eq!(body, json!({ "first": 15432, "second": 15432 }));
    assert!(
        elapsed >= Duration::from_millis(30),
        "activation-capable lookup should wait for the registry-provided binding"
    );
    assert_eq!(
        runtime_service_registry.ensure_calls.load(Ordering::SeqCst),
        1,
        "cached ctx.services access should not re-run activation within one invocation"
    );
}

#[tokio::test]
async fn convex_runtime_query_starts_declared_service_on_first_services_get() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "services:activate",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx) => { const first = await ctx.services.get(\"db\"); return ({ first: first.port, second: ctx.services.db.port }); }"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["services:activate", {
    name: "services:activate",
    kind: "query",
    runtime_handler: "async (ctx) => { const first = await ctx.services.get(\"db\"); return ({ first: first.port, second: ctx.services.db.port }); }",
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
          sessionId: `${request.kind}:${request.function_name}`,
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
	"#,
        ),
    )
    .with_runtime_limits(runtime_limits_with_service_grant("db"));
    let sandbox_backend = Arc::new(ManagedSandboxBackend::new(2));
    let sandbox_service_manager = Arc::new(
        SandboxServiceManager::new(
            Arc::new(DeclaredSandboxServiceCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
                        SandboxSpec::new(
                            TenantId::new("demo").expect("tenant id should be valid"),
                            "db",
                            SandboxBackendKind::Krun,
                            SandboxFilesystemSpec::new(""),
                            SandboxProcessSpec::new(Vec::<String>::new()),
                        ),
                        "postgres:16",
                    )),
                )]),
            }),
            sandbox_backend.clone(),
        )
        .with_activation_poll_interval(Duration::from_millis(20))
        .with_activation_timeout(Duration::from_secs(1)),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(crate::build_router_with_convex_and_sandbox_service_manager(
        fixture.service(),
        registry.clone(),
        sandbox_service_manager.clone(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let started_at = Instant::now();
    let first = api
        .convex_named_query("demo", "services:activate", json!({}))
        .await;
    let elapsed = started_at.elapsed();

    assert_eq!(first.status(), StatusCode::OK);
    let first_body = first
        .json::<serde_json::Value>()
        .await
        .expect("activation response should parse");
    assert_eq!(first_body, json!({ "first": 15432, "second": 15432 }));
    assert!(
        elapsed >= Duration::from_millis(20),
        "first lookup should wait for the sandbox service manager to observe readiness"
    );
    assert_eq!(
        sandbox_backend.start_calls.load(Ordering::SeqCst),
        1,
        "first lookup should start the declared sandbox exactly once"
    );

    let second = api
        .convex_named_query("demo", "services:activate", json!({}))
        .await;
    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(
        sandbox_backend.start_calls.load(Ordering::SeqCst),
        1,
        "subsequent lookups should reuse the active sandbox handle"
    );

    let snapshot = sandbox_service_manager
        .snapshot_for_tenant(&TenantId::new("demo").expect("tenant id should be valid"));
    assert_eq!(
        snapshot
            .get("db")
            .expect("db binding should be projected")
            .port,
        15432
    );
}

#[tokio::test]
async fn delete_tenant_stops_manager_owned_sandbox_services() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "services:activate",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx) => (await ctx.services.get(\"db\")).port"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["services:activate", {
    name: "services:activate",
    kind: "query",
    runtime_handler: "async (ctx) => (await ctx.services.get(\"db\")).port",
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
          sessionId: `${request.kind}:${request.function_name}`,
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
	"#,
        ),
    )
    .with_runtime_limits(runtime_limits_with_service_grant("db"));
    let sandbox_backend = Arc::new(ManagedSandboxBackend::new(1));
    let sandbox_service_manager = Arc::new(
        SandboxServiceManager::new(
            Arc::new(DeclaredSandboxServiceCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
                        SandboxSpec::new(
                            TenantId::new("demo").expect("tenant id should be valid"),
                            "db",
                            SandboxBackendKind::Krun,
                            SandboxFilesystemSpec::new(""),
                            SandboxProcessSpec::new(Vec::<String>::new()),
                        ),
                        "postgres:16",
                    )),
                )]),
            }),
            sandbox_backend.clone(),
        )
        .with_activation_poll_interval(Duration::from_millis(20))
        .with_activation_timeout(Duration::from_secs(1)),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(crate::build_router_with_convex_and_sandbox_service_manager(
        fixture.service(),
        registry.clone(),
        sandbox_service_manager.clone(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.convex_named_query("demo", "services:activate", json!({}))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(sandbox_backend.start_calls.load(Ordering::SeqCst), 1);
    assert!(
        sandbox_service_manager
            .snapshot_for_tenant(&TenantId::new("demo").expect("tenant id should be valid"))
            .contains_key("db")
    );

    let delete = api.delete_tenant("demo").await;
    assert_eq!(delete.status(), StatusCode::NO_CONTENT);
    assert_eq!(sandbox_backend.stop_calls.load(Ordering::SeqCst), 1);
    assert!(
        sandbox_service_manager
            .snapshot_for_tenant(&TenantId::new("demo").expect("tenant id should be valid"))
            .is_empty(),
        "tenant deletion should clear manager-owned sandbox bindings"
    );
}

#[tokio::test]
#[ignore = "requires a Linux host with KVM, buildah, conmon, and network access"]
#[allow(clippy::field_reassign_with_default)]
async fn convex_runtime_query_starts_real_krun_service_under_manager_and_tears_it_down() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "services:activate",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx) => (await ctx.services.get(\"db\")).port"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["services:activate", {
    name: "services:activate",
    kind: "query",
    runtime_handler: "async (ctx) => (await ctx.services.get(\"db\")).port",
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
          sessionId: `${request.kind}:${request.function_name}`,
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
	"#,
        ),
    )
    .with_runtime_limits(runtime_limits_with_service_grant("db"));
    let tenant_id = TenantId::new("demo").expect("tenant id should be valid");
    let host_port = env_u16("NIMBUS_KRUN_SMOKE_M4_HOST_PORT").unwrap_or(18090);
    let guest_port = env_u16("NIMBUS_KRUN_SMOKE_M4_GUEST_PORT").unwrap_or(8090);
    let guest_port_str = guest_port.to_string();

    let base_dir = env_path("NIMBUS_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("m4-manager-bundles");
    let state_root = base_dir.join("m4-manager-state");

    let mut config = KrunSandboxBackendConfig::default();
    config.bundle_root = bundle_root;
    config.state_root = state_root;
    config.launch_mode = KrunLaunchMode::Execute;
    if let Some(runtime_path) = env::var_os("NIMBUS_KRUN_SMOKE_RUNTIME") {
        config.runtime_path = runtime_path.into();
    }
    if let Some(conmon_path) = env::var_os("NIMBUS_KRUN_SMOKE_CONMON") {
        config.conmon_path = conmon_path.into();
    }
    if let Some(buildah_path) = env::var_os("NIMBUS_KRUN_SMOKE_BUILDAH") {
        config.buildah_path = buildah_path.into();
    }

    let sandbox_service_manager = Arc::new(
        SandboxServiceManager::new(
            Arc::new(DeclaredSandboxServiceCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    SandboxServiceLaunch::image(
                        SandboxImageLaunchSpec::new(
                            SandboxSpec::new(
                                tenant_id.clone(),
                                "db",
                                SandboxBackendKind::Krun,
                                SandboxFilesystemSpec::new(""),
                                SandboxProcessSpec::new(Vec::<String>::new()),
                            )
                            .with_port_binding(
                                SandboxPortBinding::new(
                                    "http",
                                    PublishedEndpointProtocol::Http,
                                    host_port,
                                    guest_port,
                                ),
                            ),
                            "docker://busybox:latest",
                        )
                        .with_process_overrides(
                            SandboxImageProcessOverrides {
                                cmd: Some(vec![
                                    "/bin/busybox".into(),
                                    "httpd".into(),
                                    "-f".into(),
                                    "-p".into(),
                                    guest_port_str,
                                ]),
                                ..Default::default()
                            },
                        ),
                    ),
                )]),
            }),
            Arc::new(KrunSandboxBackend::new(config)),
        )
        .with_activation_poll_interval(Duration::from_millis(50))
        .with_activation_timeout(Duration::from_secs(30)),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(crate::build_router_with_convex_and_sandbox_service_manager(
        fixture.service(),
        registry.clone(),
        sandbox_service_manager.clone(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_query("demo", "services:activate", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let port = response
        .json::<serde_json::Value>()
        .await
        .expect("activation response should parse")
        .as_u64()
        .expect("port should be numeric");
    assert_eq!(port, u64::from(host_port));

    let http_response = wait_for_http_response(host_port, Duration::from_secs(15)).await;
    assert!(
        http_response.starts_with("HTTP/1.") || http_response.contains("404"),
        "expected HTTP response from real krun-backed service, got: {http_response}"
    );
    assert!(
        sandbox_service_manager
            .snapshot_for_tenant(&tenant_id)
            .contains_key("db")
    );

    let delete = api.delete_tenant("demo").await;
    assert_eq!(delete.status(), StatusCode::NO_CONTENT);
    wait_for_condition(
        "manager-backed krun service should disappear after tenant deletion",
        Duration::from_secs(10),
        Duration::from_millis(100),
        || async {
            reqwest::get(format!("http://127.0.0.1:{host_port}/"))
                .await
                .is_err()
                && sandbox_service_manager
                    .snapshot_for_tenant(&tenant_id)
                    .is_empty()
        },
    )
    .await;
}

fn env_path(name: &str) -> PathBuf {
    PathBuf::from(env::var_os(name).unwrap_or_else(|| panic!("missing env var {name}")))
}

fn env_u16(name: &str) -> Option<u16> {
    env::var(name).ok().map(|value| {
        value
            .parse::<u16>()
            .unwrap_or_else(|error| panic!("invalid {name} value {value:?}: {error}"))
    })
}

async fn wait_for_http_response(host_port: u16, timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(response) = reqwest::get(format!("http://127.0.0.1:{host_port}/")).await {
            let status = response.status();
            if let Ok(body) = response.text().await {
                return format!("HTTP/1.1 {status}\n{body}");
            }
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for HTTP response on port {host_port}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
