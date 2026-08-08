use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use nimbus_core::{Error, TenantId};
use nimbus_engine::Engine;
use nimbus_runtime::{
    HostCallCancellation, InvocationKind, InvocationServiceBinding, InvocationServiceProtocol,
    InvocationServices, NimbusRuntimeError,
};
use nimbus_services::RuntimeServiceRegistry;
use nimbus_tenant::{
    RuntimeIsolationTier, TenantIsolationContext, TenantIsolationMode,
    admit_runtime_invocation_decision,
};
use serde_json::{Value, json};
use tempfile::tempdir;

use crate::adapters::convex::ConvexRegistry;
use crate::adapters::convex::host_bridge::{
    ConvexHostBridge, ConvexHostBridgeInvocation, ConvexHostBridgeScope,
    ConvexRuntimeResponseEnvelope, ConvexServiceProvisionPort,
};
use nimbus_compute::WorkloadProvisionCancellation;

struct ReadOnlyLookupRegistry {
    resolve_calls: AtomicUsize,
    forbidden_effect_calls: AtomicUsize,
    binding: InvocationServiceBinding,
}

struct OrderedProjectedBindingRegistry {
    order: Arc<AtomicUsize>,
    binding: InvocationServiceBinding,
}

impl RuntimeServiceRegistry for OrderedProjectedBindingRegistry {
    fn snapshot_for_tenant(&self, _tenant_id: &TenantId) -> InvocationServices {
        InvocationServices::new()
    }

    fn resolve_service_binding(
        &self,
        _tenant_id: &TenantId,
        service_name: &str,
    ) -> Result<Option<InvocationServiceBinding>, Error> {
        assert_eq!(service_name, "db");
        self.order
            .compare_exchange(1, 2, Ordering::SeqCst, Ordering::SeqCst)
            .expect("binding resolution must follow compute dispatch exactly once");
        Ok(Some(self.binding.clone()))
    }
}

struct RecordingProvisionPort {
    order: Arc<AtomicUsize>,
    calls: AtomicUsize,
    wait_for_cancellation: bool,
    cancellation_observed: AtomicUsize,
}

impl ConvexServiceProvisionPort for RecordingProvisionPort {
    fn provision_sandbox_service<'a>(
        &'a self,
        context: &'a TenantIsolationContext,
        access: &'a nimbus_tenant::TenantServiceAccessDecision,
        cancellation: &'a WorkloadProvisionCancellation,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>> {
        Box::pin(async move {
            assert_eq!(context.tenant_id().as_str(), "tenant-read-only");
            assert_eq!(access.tenant_id(), context.tenant_id());
            assert_eq!(access.service_name(), "db");
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.wait_for_cancellation {
                while !cancellation.is_cancelled() {
                    tokio::task::yield_now().await;
                }
                self.cancellation_observed.store(1, Ordering::SeqCst);
                return Err(Error::Cancelled);
            }
            self.order
                .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
                .expect("compute dispatch must be the first activation step");
            Ok(())
        })
    }
}

impl RuntimeServiceRegistry for ReadOnlyLookupRegistry {
    fn snapshot_for_tenant(&self, _tenant_id: &TenantId) -> InvocationServices {
        panic!("synchronous service lookup must not request an invocation snapshot")
    }

    fn resolve_service_binding(
        &self,
        _tenant_id: &TenantId,
        service_name: &str,
    ) -> Result<Option<InvocationServiceBinding>, Error> {
        self.resolve_calls.fetch_add(1, Ordering::SeqCst);
        Ok((service_name == "db").then(|| self.binding.clone()))
    }
}

#[test]
fn convex_sync_and_invocation_snapshots_are_read_only_for_sync_present_and_missing_lookups() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let engine = Arc::new(Engine::new(data_dir.path()).expect("engine should build"));
    let tenant_id = TenantId::new("tenant-read-only").expect("tenant id should parse");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    let registry = Arc::new(ConvexRegistry::empty());
    let runtime_policy = Arc::new(nimbus_runtime::RuntimePolicy::new(
        registry.runtime_limits(),
    ));
    let isolation = TenantIsolationContext::application(
        tenant_id.clone(),
        nimbus_core::PrincipalContext::anonymous(),
        "convex_read_only_service_lookup_test",
    );
    let decision = admit_runtime_invocation_decision(
        &isolation,
        "convex_read_only_service_lookup_test",
        None,
        &runtime_policy,
        RuntimeIsolationTier::InProcessUntrusted,
        TenantIsolationMode::LocalDevelopment,
        ["db".to_owned(), "missing".to_owned()],
    )
    .expect("service grants should admit");
    let service_registry = Arc::new(ReadOnlyLookupRegistry {
        resolve_calls: AtomicUsize::new(0),
        forbidden_effect_calls: AtomicUsize::new(0),
        binding: InvocationServiceBinding {
            host: "127.0.0.1".to_owned(),
            port: 15432,
            protocol: InvocationServiceProtocol::Tcp,
            endpoints: BTreeMap::new(),
        },
    });
    let service_registry_trait: Arc<dyn RuntimeServiceRegistry> = service_registry.clone();
    let bridge = ConvexHostBridge::build(
        ConvexHostBridgeScope::new_for_test(engine, registry, decision, service_registry_trait),
        ConvexHostBridgeInvocation::new(
            None,
            InvocationServices::new(),
            nimbus_core::PrincipalContext::anonymous(),
            None,
            InvocationKind::Query,
            "convex_read_only_service_lookup_test",
        ),
    )
    .expect("bridge should build");

    let present = decode_result(
        bridge
            .invoke_ctx_service_lookup(json!({
                "service_name": "db",
                "host_call_session_id": bridge.host_call_session_id(),
            }))
            .expect("present lookup should return an envelope"),
    )
    .expect("present lookup should resolve");
    assert_eq!(present["host"], json!("127.0.0.1"));
    assert_eq!(present["port"], json!(15432));

    let missing = decode_result(
        bridge
            .invoke_ctx_service_lookup(json!({
                "service_name": "missing",
                "host_call_session_id": bridge.host_call_session_id(),
            }))
            .expect("missing lookup should return an envelope"),
    )
    .expect("a granted but absent service should be a successful null lookup");
    assert_eq!(missing, Value::Null);
    assert_eq!(service_registry.resolve_calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        service_registry
            .forbidden_effect_calls
            .load(Ordering::SeqCst),
        0,
        "synchronous lookup must perform no activation or teardown effect"
    );
}

#[tokio::test]
async fn convex_async_activation_uses_compute_dispatch() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let engine = Arc::new(Engine::new(data_dir.path()).expect("engine should build"));
    let tenant_id = TenantId::new("tenant-read-only").expect("tenant id should parse");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    let registry = Arc::new(ConvexRegistry::empty());
    let isolation = TenantIsolationContext::application(
        tenant_id,
        nimbus_core::PrincipalContext::anonymous(),
        "convex_async_service_lookup_test",
    );
    let decision = admit_runtime_invocation_decision(
        &isolation,
        "convex_async_service_lookup_test",
        None,
        &nimbus_runtime::RuntimePolicy::new(registry.runtime_limits()),
        RuntimeIsolationTier::InProcessUntrusted,
        TenantIsolationMode::LocalDevelopment,
        ["db".to_owned()],
    )
    .expect("service grant should admit");
    let order = Arc::new(AtomicUsize::new(0));
    let service_registry: Arc<dyn RuntimeServiceRegistry> =
        Arc::new(OrderedProjectedBindingRegistry {
            order: Arc::clone(&order),
            binding: InvocationServiceBinding {
                host: "127.0.0.1".to_owned(),
                port: 15432,
                protocol: InvocationServiceProtocol::Tcp,
                endpoints: BTreeMap::new(),
            },
        });
    let provisioner = Arc::new(RecordingProvisionPort {
        order: Arc::clone(&order),
        calls: AtomicUsize::new(0),
        wait_for_cancellation: false,
        cancellation_observed: AtomicUsize::new(0),
    });
    let provisioner_trait: Arc<dyn ConvexServiceProvisionPort> = provisioner.clone();
    let scope = ConvexHostBridgeScope::new_for_test(engine, registry, decision, service_registry)
        .with_service_provisioning(isolation, provisioner_trait)
        .expect("exact service provision scope should build");
    let bridge = ConvexHostBridge::build(
        scope,
        ConvexHostBridgeInvocation::new(
            None,
            InvocationServices::new(),
            nimbus_core::PrincipalContext::anonymous(),
            None,
            InvocationKind::Query,
            "convex_async_service_lookup_test",
        ),
    )
    .expect("bridge should build");

    let binding = decode_result(
        bridge
            .invoke_ctx_service_lookup_async_cancellable(
                json!({
                    "service_name": "db",
                    "host_call_session_id": bridge.host_call_session_id(),
                }),
                &HostCallCancellation::default(),
            )
            .await
            .expect("async lookup should return an envelope"),
    )
    .expect("compute-dispatched activation should resolve its projected binding");

    assert_eq!(binding["port"], json!(15432));
    assert_eq!(provisioner.calls.load(Ordering::SeqCst), 1);
    assert_eq!(order.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn convex_async_activation_translates_host_cancellation_without_dropping_dispatch() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let engine = Arc::new(Engine::new(data_dir.path()).expect("engine should build"));
    let tenant_id = TenantId::new("tenant-read-only").expect("tenant id should parse");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    let registry = Arc::new(ConvexRegistry::empty());
    let isolation = TenantIsolationContext::application(
        tenant_id,
        nimbus_core::PrincipalContext::anonymous(),
        "convex_async_service_cancellation_test",
    );
    let decision = admit_runtime_invocation_decision(
        &isolation,
        "convex_async_service_cancellation_test",
        None,
        &nimbus_runtime::RuntimePolicy::new(registry.runtime_limits()),
        RuntimeIsolationTier::InProcessUntrusted,
        TenantIsolationMode::LocalDevelopment,
        ["db".to_owned()],
    )
    .expect("service grant should admit");
    let provisioner = Arc::new(RecordingProvisionPort {
        order: Arc::new(AtomicUsize::new(0)),
        calls: AtomicUsize::new(0),
        wait_for_cancellation: true,
        cancellation_observed: AtomicUsize::new(0),
    });
    let provisioner_trait: Arc<dyn ConvexServiceProvisionPort> = provisioner.clone();
    let service_registry: Arc<dyn RuntimeServiceRegistry> = Arc::new(ReadOnlyLookupRegistry {
        resolve_calls: AtomicUsize::new(0),
        forbidden_effect_calls: AtomicUsize::new(0),
        binding: InvocationServiceBinding {
            host: "127.0.0.1".to_owned(),
            port: 15432,
            protocol: InvocationServiceProtocol::Tcp,
            endpoints: BTreeMap::new(),
        },
    });
    let scope = ConvexHostBridgeScope::new_for_test(engine, registry, decision, service_registry)
        .with_service_provisioning(isolation, provisioner_trait)
        .expect("exact service provision scope should build");
    let bridge = ConvexHostBridge::build(
        scope,
        ConvexHostBridgeInvocation::new(
            None,
            InvocationServices::new(),
            nimbus_core::PrincipalContext::anonymous(),
            None,
            InvocationKind::Query,
            "convex_async_service_cancellation_test",
        ),
    )
    .expect("bridge should build");
    let cancellation = HostCallCancellation::default();
    cancellation.cancel();

    let result = bridge
        .invoke_ctx_service_lookup_async_cancellable(
            json!({
                "service_name": "db",
                "host_call_session_id": bridge.host_call_session_id(),
            }),
            &cancellation,
        )
        .await;

    assert!(matches!(result, Err(NimbusRuntimeError::Cancelled)));
    assert_eq!(provisioner.calls.load(Ordering::SeqCst), 1);
    assert_eq!(provisioner.cancellation_observed.load(Ordering::SeqCst), 1);
}

fn decode_result(value: Value) -> Result<Value, Error> {
    let envelope: ConvexRuntimeResponseEnvelope =
        serde_json::from_value(value).expect("runtime envelope should decode");
    envelope.into_core_result()
}
