//! Runtime status, readiness probing, and endpoint publication.

use std::net::Ipv4Addr;
use std::time::Duration;

use crate::backends::readiness_probe::DEFAULT_READINESS_PROBE_TIMEOUT;
#[cfg(test)]
use crate::backends::readiness_probe::{ReadinessProbeProvider, application_readiness_status};
use crate::instance::SandboxStatus;
use crate::spec::SandboxSpec;
use nimbus_network::PublishedEndpoint;

use super::config::ContainerStartMode;
use super::manifest::ContainerSandboxManifest;

#[cfg(test)]
pub(super) fn running_status(
    manifest: &ContainerSandboxManifest,
    provider: &dyn ReadinessProbeProvider,
) -> SandboxStatus {
    application_readiness_status(
        manifest.status,
        &published_endpoints(&manifest.spec),
        readiness_probe_timeout(manifest),
        provider,
    )
}

pub(super) fn readiness_probe_timeout(manifest: &ContainerSandboxManifest) -> Duration {
    manifest
        .image_metadata
        .healthcheck
        .as_ref()
        .and_then(|healthcheck| healthcheck.timeout)
        .map(Duration::from_nanos)
        .unwrap_or(DEFAULT_READINESS_PROBE_TIMEOUT)
}

pub(super) fn visible_published_endpoints(
    start_mode: ContainerStartMode,
    spec: &SandboxSpec,
    status: SandboxStatus,
) -> Vec<PublishedEndpoint> {
    let endpoints = published_endpoints(spec);
    if start_mode == ContainerStartMode::Execute && status != SandboxStatus::Ready {
        Vec::new()
    } else {
        endpoints
    }
}

pub(super) fn synchronize_handle_status(
    manifest: &mut ContainerSandboxManifest,
    status: SandboxStatus,
) {
    manifest.status = status;
    manifest.handle.status = status;
    manifest.handle.published_endpoints =
        visible_published_endpoints(manifest.start_mode, &manifest.spec, status);
}

pub(super) fn published_endpoints(spec: &SandboxSpec) -> Vec<PublishedEndpoint> {
    spec.port_bindings
        .iter()
        .map(|port_binding| {
            PublishedEndpoint::new(
                port_binding.name.clone(),
                port_binding.protocol,
                port_binding.host_socket_addr(),
            )
            .with_guest_port(port_binding.guest_port)
        })
        .collect()
}

pub(super) fn private_readiness_endpoints(
    spec: &SandboxSpec,
    assigned_ip: Ipv4Addr,
) -> Vec<PublishedEndpoint> {
    spec.port_bindings
        .iter()
        .map(|binding| {
            PublishedEndpoint::new(
                binding.name.clone(),
                binding.protocol,
                std::net::SocketAddr::new(assigned_ip.into(), binding.guest_port),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use super::private_readiness_endpoints;
    use crate::SandboxPortBinding;
    use crate::backends::container::runtime::support::sample_spec;

    #[test]
    fn private_readiness_uses_attachment_address_and_guest_port() {
        let spec = sample_spec().with_port_binding(
            SandboxPortBinding::tcp("http", 18_080, 8_080)
                .with_host_address(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        );

        let endpoints = private_readiness_endpoints(&spec, Ipv4Addr::new(10, 0, 0, 9));

        assert_eq!(endpoints.len(), 1);
        assert_eq!(
            endpoints[0].address,
            SocketAddr::from(([10, 0, 0, 9], 8_080))
        );
        assert_eq!(endpoints[0].guest_port, None);
    }
}
