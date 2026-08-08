use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use nimbus_core::{Error, TenantId};
use nimbus_engine::Engine;
use nimbus_runtime::{InvocationServiceBinding, InvocationServiceProtocol, InvocationServices};
use nimbus_services::RuntimeServiceRegistry;
use nimbus_tenant::{TenantIsolationContext, TenantIsolationMode};
use tempfile::tempdir;

use super::RuntimeInvocationContext;
use crate::adapters::convex::ConvexRegistry;

struct ReadOnlySnapshotRegistry {
    snapshot_calls: AtomicUsize,
    forbidden_effect_calls: AtomicUsize,
}

impl RuntimeServiceRegistry for ReadOnlySnapshotRegistry {
    fn snapshot_for_tenant(&self, tenant_id: &TenantId) -> InvocationServices {
        assert_eq!(tenant_id.as_str(), "tenant-read-only");
        self.snapshot_calls.fetch_add(1, Ordering::SeqCst);
        InvocationServices::from([(
            "db".to_owned(),
            InvocationServiceBinding {
                host: "127.0.0.1".to_owned(),
                port: 15432,
                protocol: InvocationServiceProtocol::Tcp,
                endpoints: BTreeMap::new(),
            },
        )])
    }

    fn resolve_service_binding(
        &self,
        _tenant_id: &TenantId,
        _service_name: &str,
    ) -> Result<Option<InvocationServiceBinding>, Error> {
        self.forbidden_effect_calls.fetch_add(1, Ordering::SeqCst);
        panic!("invocation snapshot must not resolve or activate a service")
    }
}

#[test]
fn convex_sync_and_invocation_snapshots_are_read_only_for_invocation_snapshot() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let engine = Arc::new(Engine::new(data_dir.path()).expect("engine should build"));
    let tenant_id = TenantId::new("tenant-read-only").expect("tenant id should parse");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    let registry = Arc::new(ConvexRegistry::empty());
    let runtime_manager = nimbus_compute::runtime_manager::RuntimeManager::new(
        engine.clone(),
        nimbus_compute::config::runtime::RuntimeGovernorConfig::default(),
    );
    let service_registry = Arc::new(ReadOnlySnapshotRegistry {
        snapshot_calls: AtomicUsize::new(0),
        forbidden_effect_calls: AtomicUsize::new(0),
    });
    let service_registry_trait: Arc<dyn RuntimeServiceRegistry> = service_registry.clone();
    let context = RuntimeInvocationContext::new(
        &engine,
        &registry,
        &service_registry_trait,
        &runtime_manager,
        None,
        TenantIsolationContext::application(
            tenant_id,
            nimbus_core::PrincipalContext::anonymous(),
            "convex_read_only_invocation_snapshot_test",
        ),
        TenantIsolationMode::LocalDevelopment,
    );

    let snapshot = context.runtime_services();

    let binding = snapshot.get("db").expect("db snapshot should be present");
    assert_eq!(binding.host, "127.0.0.1");
    assert_eq!(binding.port, 15432);
    assert_eq!(service_registry.snapshot_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        service_registry
            .forbidden_effect_calls
            .load(Ordering::SeqCst),
        0,
        "invocation snapshot must perform no lookup, activation, or teardown effect"
    );
}
