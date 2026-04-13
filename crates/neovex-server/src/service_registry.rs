use std::collections::BTreeMap;
use std::sync::Arc;

use neovex_core::{Error, TenantId};
use neovex_runtime::{
    InvocationServiceBinding, InvocationServiceEndpoint, InvocationServiceProtocol,
    InvocationServices,
};
use neovex_sandbox::{PublishedEndpoint, PublishedEndpointProtocol, SandboxHandle, SandboxStatus};

use crate::sandbox::SandboxCatalog;

pub trait RuntimeServiceRegistry: Send + Sync + 'static {
    fn snapshot_for_tenant(&self, tenant_id: &TenantId) -> InvocationServices;

    fn resolve_service_binding(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Result<Option<InvocationServiceBinding>, Error>;

    fn ensure_service_binding(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Result<Option<InvocationServiceBinding>, Error> {
        self.resolve_service_binding(tenant_id, service_name)
    }

    fn teardown_tenant(&self, _tenant_id: &TenantId) -> Result<(), Error> {
        Ok(())
    }
}

pub(crate) struct SandboxCatalogRuntimeServiceRegistry {
    sandbox_catalog: Arc<dyn SandboxCatalog>,
}

impl SandboxCatalogRuntimeServiceRegistry {
    pub(crate) fn new(sandbox_catalog: Arc<dyn SandboxCatalog>) -> Self {
        Self { sandbox_catalog }
    }
}

impl RuntimeServiceRegistry for SandboxCatalogRuntimeServiceRegistry {
    fn snapshot_for_tenant(&self, tenant_id: &TenantId) -> InvocationServices {
        self.sandbox_catalog
            .sandboxes_for_tenant(tenant_id)
            .into_iter()
            .filter_map(|(service_name, handle)| {
                service_binding_from_handle(&handle).map(|binding| (service_name, binding))
            })
            .collect()
    }

    fn resolve_service_binding(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Result<Option<InvocationServiceBinding>, Error> {
        Ok(self
            .sandbox_catalog
            .sandbox_for_service(tenant_id, service_name)
            .and_then(|handle| service_binding_from_handle(&handle)))
    }
}

pub(crate) fn service_binding_from_handle(
    handle: &SandboxHandle,
) -> Option<InvocationServiceBinding> {
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

fn primary_protocol_rank(protocol: PublishedEndpointProtocol) -> u8 {
    match protocol {
        PublishedEndpointProtocol::Tcp => 0,
        PublishedEndpointProtocol::Http => 1,
        PublishedEndpointProtocol::Https => 2,
    }
}

fn service_endpoint_from_published(endpoint: &PublishedEndpoint) -> InvocationServiceEndpoint {
    InvocationServiceEndpoint {
        host: endpoint.address.ip().to_string(),
        port: endpoint.address.port(),
        protocol: service_protocol_from_published(endpoint.protocol),
    }
}

fn service_protocol_from_published(
    protocol: PublishedEndpointProtocol,
) -> InvocationServiceProtocol {
    match protocol {
        PublishedEndpointProtocol::Tcp => InvocationServiceProtocol::Tcp,
        PublishedEndpointProtocol::Http => InvocationServiceProtocol::Http,
        PublishedEndpointProtocol::Https => InvocationServiceProtocol::Https,
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use neovex_core::TenantId;
    use neovex_sandbox::{SandboxBackendKind, SandboxId};

    use super::*;

    struct StubSandboxCatalog {
        sandboxes: BTreeMap<String, SandboxHandle>,
    }

    impl SandboxCatalog for StubSandboxCatalog {
        fn sandboxes_for_tenant(&self, _tenant_id: &TenantId) -> BTreeMap<String, SandboxHandle> {
            self.sandboxes.clone()
        }
    }

    #[test]
    fn snapshot_selects_tcp_as_primary_endpoint() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let registry = SandboxCatalogRuntimeServiceRegistry::new(Arc::new(StubSandboxCatalog {
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
        let registry = SandboxCatalogRuntimeServiceRegistry::new(Arc::new(StubSandboxCatalog {
            sandboxes: BTreeMap::from([(
                "db".to_string(),
                SandboxHandle::new(
                    SandboxId::new("sandbox-db"),
                    "db",
                    SandboxBackendKind::Krun,
                    SandboxStatus::Starting,
                    vec![PublishedEndpoint::new(
                        "postgres",
                        PublishedEndpointProtocol::Tcp,
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
    fn resolve_service_binding_returns_binding_for_named_service() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let registry = SandboxCatalogRuntimeServiceRegistry::new(Arc::new(StubSandboxCatalog {
            sandboxes: BTreeMap::from([(
                "db".to_string(),
                SandboxHandle::new(
                    SandboxId::new("sandbox-db"),
                    "db",
                    SandboxBackendKind::Krun,
                    SandboxStatus::Ready,
                    vec![PublishedEndpoint::new(
                        "postgres",
                        PublishedEndpointProtocol::Tcp,
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
}
