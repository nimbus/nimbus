use super::*;
use crate::service_registry::SandboxCatalogRuntimeServiceRegistry;

pub(in crate::adapters::convex::tests) fn host_bridge_fixture()
-> (TempDir, Arc<Service>, TenantId, ConvexHostBridge) {
    let tempdir = tempdir().expect("runtime action tempdir should build");
    let service = Arc::new(Service::new(tempdir.path()).expect("service should build"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should be created");
    let registry = Arc::new(ConvexRegistry::empty());
    let bridge = ConvexHostBridge::new(
        ConvexHostBridgeScope::new(
            service.clone(),
            registry,
            tenant_id.clone(),
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
