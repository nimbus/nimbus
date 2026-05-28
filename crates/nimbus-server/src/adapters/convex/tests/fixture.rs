use super::*;
use nimbus_services::SandboxCatalogRuntimeServiceRegistry;

pub(in crate::adapters::convex::tests) fn host_bridge_fixture()
-> (TempDir, Arc<Service>, TenantId, ConvexHostBridge) {
    let tempdir = tempdir().expect("runtime action tempdir should build");
    let service = Arc::new(Service::new(tempdir.path()).expect("service should build"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
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
            service.clone(),
            registry,
            decision,
            Arc::new(SandboxCatalogRuntimeServiceRegistry::new(Arc::new(
                crate::EmptySandboxCatalog,
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
    (tempdir, service, tenant_id, bridge)
}
