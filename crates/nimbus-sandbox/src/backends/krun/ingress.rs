//! Provider-private ingress realization for libkrun TSI workloads.
//!
//! The server remains the host-listener and forwarding owner. This module
//! describes only the private listener that libkrun realizes inside the
//! sandbox network namespace and the port used to reach it.

use std::net::Ipv4Addr;

use crate::SandboxPortBinding;
use nimbus_network::PublishedEndpoint;

pub(super) fn format_private_tsi_port_map(
    port_bindings: &[SandboxPortBinding],
    bind_address: Ipv4Addr,
) -> String {
    port_bindings
        .iter()
        .map(|binding| {
            format!(
                "{}:{}:{}",
                bind_address, binding.host_port, binding.guest_port
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

pub(super) const fn private_tsi_upstream_port(binding: &SandboxPortBinding) -> u16 {
    binding.host_port
}

pub(super) fn private_tsi_readiness_endpoints(
    port_bindings: &[SandboxPortBinding],
    assigned_ip: Ipv4Addr,
) -> Vec<PublishedEndpoint> {
    port_bindings
        .iter()
        .map(|binding| {
            PublishedEndpoint::new(
                binding.name.clone(),
                binding.protocol,
                std::net::SocketAddr::new(assigned_ip.into(), private_tsi_upstream_port(binding)),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use super::{
        format_private_tsi_port_map, private_tsi_readiness_endpoints, private_tsi_upstream_port,
    };
    use crate::SandboxPortBinding;

    #[test]
    fn private_tsi_port_map_binds_the_provider_namespace_wildcard() {
        let bindings = [
            SandboxPortBinding::tcp("loopback", 18_080, 8_080)
                .with_host_address(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            SandboxPortBinding::tcp("private", 18_443, 8_443)
                .with_host_address(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5))),
        ];

        assert_eq!(
            format_private_tsi_port_map(&bindings, Ipv4Addr::UNSPECIFIED),
            "0.0.0.0:18080:8080,0.0.0.0:18443:8443"
        );
    }

    #[test]
    fn private_tsi_port_map_uses_the_authenticated_attachment_address() {
        let bindings = [SandboxPortBinding::tcp("http", 18_080, 8_080)];

        assert_eq!(
            format_private_tsi_port_map(&bindings, Ipv4Addr::new(10, 0, 0, 2)),
            "10.0.0.2:18080:8080"
        );
    }

    #[test]
    fn private_tsi_upstream_uses_the_reserved_bridge_port() {
        let binding = SandboxPortBinding::tcp("http", 18_080, 8_080);

        assert_eq!(private_tsi_upstream_port(&binding), 18_080);
    }

    #[test]
    fn private_tsi_readiness_uses_attachment_address_and_reserved_bridge_port() {
        let binding = SandboxPortBinding::tcp("http", 18_080, 8_080)
            .with_host_address(IpAddr::V4(Ipv4Addr::LOCALHOST));

        let endpoints = private_tsi_readiness_endpoints(&[binding], Ipv4Addr::new(10, 0, 0, 9));

        assert_eq!(endpoints.len(), 1);
        assert_eq!(
            endpoints[0].address,
            SocketAddr::from(([10, 0, 0, 9], 18_080))
        );
        assert_eq!(endpoints[0].guest_port, None);
    }
}
