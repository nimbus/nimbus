use std::collections::BTreeMap;
use std::sync::Arc;

use nimbus_core::{Error, TenantId};
use nimbus_network::{EndpointProtocol, PublishedEndpoint};
use nimbus_runtime::{
    InvocationServiceBinding, InvocationServiceEndpoint, InvocationServiceProtocol,
    InvocationServices,
};
use nimbus_sandbox::{SandboxHandle, SandboxStatus};
use nimbus_tenant::TenantServiceAccessDecision;

use crate::ServiceInstanceCatalog;

/// Effect-free runtime service naming projection.
///
/// Provision and tenant retirement deliberately use separate capabilities so
/// a snapshot or lookup holder cannot start, inspect, stop, or clean up a
/// provider.
pub trait RuntimeServiceRegistry: Send + Sync + 'static {
    fn snapshot_for_tenant(&self, tenant_id: &TenantId) -> InvocationServices;

    fn resolve_service_binding(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Result<Option<InvocationServiceBinding>, Error>;

    fn resolve_service_binding_for_decision(
        &self,
        service_access: &TenantServiceAccessDecision,
    ) -> Result<Option<InvocationServiceBinding>, Error> {
        self.resolve_service_binding(service_access.tenant_id(), service_access.service_name())
    }
}

pub struct ServiceInstanceBindingRegistry {
    service_instances: Arc<dyn ServiceInstanceCatalog>,
}

impl ServiceInstanceBindingRegistry {
    pub fn new(service_instances: Arc<dyn ServiceInstanceCatalog>) -> Self {
        Self { service_instances }
    }
}

impl RuntimeServiceRegistry for ServiceInstanceBindingRegistry {
    fn snapshot_for_tenant(&self, tenant_id: &TenantId) -> InvocationServices {
        self.service_instances
            .service_instances_for_tenant(tenant_id)
            .into_iter()
            .filter_map(|(service_name, handle)| {
                if &handle.tenant_id != tenant_id {
                    return None;
                }
                service_binding_from_handle(&handle).map(|binding| (service_name, binding))
            })
            .collect()
    }

    fn resolve_service_binding(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Result<Option<InvocationServiceBinding>, Error> {
        let Some(handle) = self
            .service_instances
            .service_instance_for_name(tenant_id, service_name)
        else {
            return Ok(None);
        };
        if handle.tenant_id != *tenant_id {
            return Err(Error::PermissionDenied(format!(
                "sandbox catalog returned service {service_name} for tenant {}, but runtime lookup requested tenant {tenant_id}",
                handle.tenant_id
            )));
        }
        Ok(service_binding_from_handle(&handle))
    }
}

pub fn service_binding_from_handle(handle: &SandboxHandle) -> Option<InvocationServiceBinding> {
    if handle.status != SandboxStatus::Ready {
        return None;
    }

    let primary = select_primary_endpoint(&handle.published_endpoints)?;
    let endpoints = handle
        .published_endpoints
        .iter()
        .map(|endpoint| {
            (
                endpoint.name.clone(),
                service_endpoint_from_published(endpoint),
            )
        })
        .collect::<BTreeMap<_, _>>();

    Some(InvocationServiceBinding {
        host: primary.address.ip().to_string(),
        port: primary.address.port(),
        protocol: service_protocol_from_published(primary.protocol),
        endpoints,
    })
}

fn select_primary_endpoint(endpoints: &[PublishedEndpoint]) -> Option<&PublishedEndpoint> {
    endpoints.iter().min_by_key(|endpoint| {
        (
            primary_protocol_rank(endpoint.protocol),
            endpoint.name.as_str(),
            endpoint.address,
        )
    })
}

fn primary_protocol_rank(protocol: EndpointProtocol) -> u8 {
    match protocol {
        EndpointProtocol::Tcp => 0,
        EndpointProtocol::Http => 1,
        EndpointProtocol::Https => 2,
    }
}

fn service_endpoint_from_published(endpoint: &PublishedEndpoint) -> InvocationServiceEndpoint {
    InvocationServiceEndpoint {
        host: endpoint.address.ip().to_string(),
        port: endpoint.address.port(),
        protocol: service_protocol_from_published(endpoint.protocol),
    }
}

fn service_protocol_from_published(protocol: EndpointProtocol) -> InvocationServiceProtocol {
    match protocol {
        EndpointProtocol::Tcp => InvocationServiceProtocol::Tcp,
        EndpointProtocol::Http => InvocationServiceProtocol::Http,
        EndpointProtocol::Https => InvocationServiceProtocol::Https,
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use nimbus_core::TenantId;
    use nimbus_sandbox::{SandboxBackendKind, SandboxId};

    use super::*;

    struct StubServiceInstanceCatalog {
        sandboxes: BTreeMap<String, SandboxHandle>,
    }

    impl ServiceInstanceCatalog for StubServiceInstanceCatalog {
        fn service_instances_for_tenant(
            &self,
            _tenant_id: &TenantId,
        ) -> BTreeMap<String, SandboxHandle> {
            self.sandboxes.clone()
        }
    }

    #[test]
    fn snapshot_selects_tcp_as_primary_endpoint() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let registry = ServiceInstanceBindingRegistry::new(Arc::new(StubServiceInstanceCatalog {
            sandboxes: BTreeMap::from([(
                "db".to_string(),
                SandboxHandle::new(
                    tenant_id.clone(),
                    SandboxId::new("sandbox-db"),
                    "db",
                    SandboxBackendKind::Krun,
                    SandboxStatus::Ready,
                    vec![
                        PublishedEndpoint::new(
                            "health",
                            EndpointProtocol::Http,
                            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 18080),
                        ),
                        PublishedEndpoint::new(
                            "postgres",
                            EndpointProtocol::Tcp,
                            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 15432),
                        ),
                    ],
                ),
            )]),
        }));

        let services = registry.snapshot_for_tenant(&tenant_id);
        let db = services.get("db").expect("db service should be projected");

        assert_eq!(db.host, "127.0.0.1");
        assert_eq!(db.port, 15432);
        assert_eq!(db.protocol, InvocationServiceProtocol::Tcp);
        assert_eq!(
            db.endpoints
                .get("health")
                .expect("health endpoint should be present")
                .port,
            18080
        );
    }

    #[test]
    fn snapshot_skips_sandboxes_without_ready_endpoints() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let registry = ServiceInstanceBindingRegistry::new(Arc::new(StubServiceInstanceCatalog {
            sandboxes: BTreeMap::from([(
                "db".to_string(),
                SandboxHandle::new(
                    tenant_id.clone(),
                    SandboxId::new("sandbox-db"),
                    "db",
                    SandboxBackendKind::Krun,
                    SandboxStatus::Starting,
                    vec![PublishedEndpoint::new(
                        "postgres",
                        EndpointProtocol::Tcp,
                        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 15432),
                    )],
                ),
            )]),
        }));

        assert!(
            registry.snapshot_for_tenant(&tenant_id).is_empty(),
            "non-ready sandboxes should stay hidden from invocation service bindings"
        );
    }

    #[test]
    fn not_ready_endpoint_withdrawal_cannot_materialize_a_service_binding() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let handle = SandboxHandle::new(
            tenant_id,
            SandboxId::new("sandbox-withdrawn"),
            "db",
            SandboxBackendKind::Krun,
            SandboxStatus::NotReady,
            Vec::new(),
        );

        assert!(
            service_binding_from_handle(&handle).is_none(),
            "a backend-withdrawn NotReady projection must not become a logical service binding"
        );
    }

    #[test]
    fn snapshot_skips_sandboxes_for_a_different_tenant() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let other_tenant = TenantId::new("tenant-b").expect("tenant id should be valid");
        let registry = ServiceInstanceBindingRegistry::new(Arc::new(StubServiceInstanceCatalog {
            sandboxes: BTreeMap::from([(
                "db".to_string(),
                SandboxHandle::new(
                    other_tenant,
                    SandboxId::new("sandbox-db"),
                    "db",
                    SandboxBackendKind::Krun,
                    SandboxStatus::Ready,
                    vec![PublishedEndpoint::new(
                        "postgres",
                        EndpointProtocol::Tcp,
                        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 15432),
                    )],
                ),
            )]),
        }));

        assert!(
            registry.snapshot_for_tenant(&tenant_id).is_empty(),
            "tenant-scoped service snapshots must not project another tenant's handle"
        );
    }

    #[test]
    fn resolve_service_binding_returns_binding_for_named_service() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let registry = ServiceInstanceBindingRegistry::new(Arc::new(StubServiceInstanceCatalog {
            sandboxes: BTreeMap::from([(
                "db".to_string(),
                SandboxHandle::new(
                    tenant_id.clone(),
                    SandboxId::new("sandbox-db"),
                    "db",
                    SandboxBackendKind::Krun,
                    SandboxStatus::Ready,
                    vec![PublishedEndpoint::new(
                        "postgres",
                        EndpointProtocol::Tcp,
                        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 15432),
                    )],
                ),
            )]),
        }));

        let binding = registry
            .resolve_service_binding(&tenant_id, "db")
            .expect("service lookup should succeed")
            .expect("db binding should exist");

        assert_eq!(binding.port, 15432);
        assert_eq!(binding.protocol, InvocationServiceProtocol::Tcp);
    }

    #[test]
    fn resolve_service_binding_rejects_handle_for_a_different_tenant() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let other_tenant = TenantId::new("tenant-b").expect("tenant id should be valid");
        let registry = ServiceInstanceBindingRegistry::new(Arc::new(StubServiceInstanceCatalog {
            sandboxes: BTreeMap::from([(
                "db".to_string(),
                SandboxHandle::new(
                    other_tenant,
                    SandboxId::new("sandbox-db"),
                    "db",
                    SandboxBackendKind::Krun,
                    SandboxStatus::Ready,
                    vec![PublishedEndpoint::new(
                        "postgres",
                        EndpointProtocol::Tcp,
                        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 15432),
                    )],
                ),
            )]),
        }));

        let error = registry
            .resolve_service_binding(&tenant_id, "db")
            .expect_err("mismatched handle tenant should fail closed");

        assert!(
            error
                .to_string()
                .contains("returned service db for tenant tenant-b"),
            "error should name the rejected handle tenant: {error}"
        );
    }
}
