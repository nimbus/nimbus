use super::*;

pub(super) fn running_status(manifest: &KrunSandboxManifest) -> SandboxStatus {
    match readiness_probe_target(manifest) {
        Some(target) if probe_target_ready(target, readiness_probe_timeout(manifest)) => {
            SandboxStatus::Ready
        }
        Some(_)
            if matches!(
                manifest.status,
                SandboxStatus::Ready | SandboxStatus::NotReady
            ) =>
        {
            SandboxStatus::NotReady
        }
        Some(_) => SandboxStatus::Starting,
        None => SandboxStatus::Ready,
    }
}

pub(super) fn readiness_probe_target(
    manifest: &KrunSandboxManifest,
) -> Option<ReadinessProbeTarget> {
    let endpoints = published_endpoints(&manifest.spec);
    endpoints
        .iter()
        .find_map(|endpoint| match endpoint.protocol {
            PublishedEndpointProtocol::Http => Some(ReadinessProbeTarget::Http(endpoint.address)),
            PublishedEndpointProtocol::Https => Some(ReadinessProbeTarget::Tcp(endpoint.address)),
            PublishedEndpointProtocol::Tcp => None,
        })
        .or_else(|| {
            endpoints
                .iter()
                .find_map(|endpoint| match endpoint.protocol {
                    PublishedEndpointProtocol::Tcp | PublishedEndpointProtocol::Https => {
                        Some(ReadinessProbeTarget::Tcp(endpoint.address))
                    }
                    PublishedEndpointProtocol::Http => None,
                })
        })
}

fn readiness_probe_timeout(manifest: &KrunSandboxManifest) -> Duration {
    manifest
        .image_metadata
        .healthcheck
        .as_ref()
        .and_then(|healthcheck| healthcheck.timeout)
        .map(Duration::from_nanos)
        .unwrap_or_else(|| Duration::from_millis(DEFAULT_READINESS_PROBE_TIMEOUT_MILLIS))
}

pub(super) fn probe_target_ready(target: ReadinessProbeTarget, timeout: Duration) -> bool {
    match target {
        ReadinessProbeTarget::Tcp(address) => TcpStream::connect_timeout(&address, timeout).is_ok(),
        ReadinessProbeTarget::Http(address) => probe_http_ready(address, timeout),
    }
}

fn probe_http_ready(address: SocketAddr, timeout: Duration) -> bool {
    let Ok(mut stream) = TcpStream::connect_timeout(&address, timeout) else {
        return false;
    };
    if stream.set_read_timeout(Some(timeout)).is_err() {
        return false;
    }
    if stream
        .write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n")
        .is_err()
    {
        return false;
    }

    let mut response = [0_u8; 256];
    match stream.read(&mut response) {
        Ok(read) if read > 0 => String::from_utf8_lossy(&response[..read]).starts_with("HTTP/"),
        _ => false,
    }
}

pub(super) fn visible_published_endpoints(
    launch_mode: KrunLaunchMode,
    spec: &SandboxSpec,
    status: SandboxStatus,
) -> Vec<PublishedEndpoint> {
    let endpoints = published_endpoints(spec);
    if launch_mode == KrunLaunchMode::Execute && status != SandboxStatus::Ready {
        Vec::new()
    } else {
        endpoints
    }
}

pub(super) fn synchronize_handle_status(manifest: &mut KrunSandboxManifest, status: SandboxStatus) {
    manifest.status = status;
    manifest.handle.status = status;
    manifest.handle.published_endpoints =
        visible_published_endpoints(manifest.launch_mode, &manifest.spec, status);
}

fn published_endpoints(spec: &SandboxSpec) -> Vec<PublishedEndpoint> {
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
