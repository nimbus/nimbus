//! Runtime status, readiness probing, and endpoint publication.

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
