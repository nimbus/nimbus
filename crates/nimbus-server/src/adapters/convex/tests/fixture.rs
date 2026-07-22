use super::*;
use nimbus_services::ServiceInstanceBindingRegistry;

pub(in crate::adapters::convex::tests) fn host_bridge_fixture()
-> (TempDir, Arc<Engine>, TenantId, ConvexHostBridge) {
    let tempdir = tempdir().expect("runtime action tempdir should build");
    let engine = Arc::new(Engine::new(tempdir.path()).expect("engine should build"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should be created");
    let registry = Arc::new(ConvexRegistry::empty());
    let runtime_manager = nimbus_compute::runtime_manager::RuntimeManager::new(
        engine.clone(),
        nimbus_compute::config::runtime::RuntimeGovernorConfig::default(),
    );
    let runtime_lane = runtime_manager.lane_for_limits(registry.runtime_limits());
    let invocation_lease = runtime_manager
        .acquire_invocation_lease_blocking(&tenant_id, 0)
        .expect("fixture runtime authority should build");
    let isolation = nimbus_tenant::TenantIsolationContext::application(
        tenant_id.clone(),
        nimbus_core::PrincipalContext::anonymous(),
        "convex_fixture",
    );
    let decision = nimbus_tenant::admit_runtime_invocation_decision(
        &isolation,
        "convex_fixture",
        None,
        &runtime_lane.policy(),
        nimbus_tenant::RuntimeIsolationTier::InProcessUntrusted,
        nimbus_tenant::TenantIsolationMode::LocalDevelopment,
        std::iter::empty::<String>(),
    )
    .expect("fixture tenant isolation decision should build");
    let bridge = ConvexHostBridge::new(
        ConvexHostBridgeScope::new(
            engine.clone(),
            registry,
            decision,
            Arc::new(ServiceInstanceBindingRegistry::new(Arc::new(
                nimbus_services::EmptyServiceInstanceCatalog,
            ))),
            runtime_manager,
            invocation_lease.authority(),
            runtime_lane.policy().limits().clone(),
        ),
        ConvexHostBridgeInvocation::new(
            None,
            Default::default(),
            nimbus_core::PrincipalContext::anonymous(),
            None,
            InvocationKind::Query,
            "convex_fixture_test",
        ),
    );
    (tempdir, engine, tenant_id, bridge)
}
