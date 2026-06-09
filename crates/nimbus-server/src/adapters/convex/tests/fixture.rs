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
    let isolation = nimbus_tenant::TenantIsolationContext::application(
        tenant_id.clone(),
        nimbus_core::PrincipalContext::anonymous(),
        "convex_fixture",
    );
    let decision = nimbus_tenant::admit_runtime_invocation_decision(
        &isolation,
        "convex_fixture",
        None,
        &registry.runtime_policy(),
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
                crate::EmptyServiceInstanceCatalog,
            ))),
        ),
        ConvexHostBridgeInvocation::new(
            None,
            Default::default(),
            nimbus_core::PrincipalContext::anonymous(),
            None,
            InvocationKind::Query,
        ),
    );
    (tempdir, engine, tenant_id, bridge)
}
