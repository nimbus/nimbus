use std::collections::BTreeSet;
use std::ops::RangeInclusive;
use std::path::PathBuf;

use serde::Deserialize;

use super::buildah::{OciExposedPort, OciExposedPortProtocol};
use crate::error::{Result, SandboxError};
use crate::instance::SandboxStatus;
use crate::spec::SandboxPortBinding;

#[derive(Debug, Clone)]
pub(crate) struct PortManager {
    range: RangeInclusive<u16>,
    state_root: PathBuf,
}

impl PortManager {
    pub(crate) fn new(state_root: impl Into<PathBuf>, range: RangeInclusive<u16>) -> Self {
        Self {
            range,
            state_root: state_root.into(),
        }
    }

    pub(crate) fn allocate_missing_bindings(
        &self,
        existing_bindings: &[SandboxPortBinding],
        exposed_ports: &[OciExposedPort],
    ) -> Result<Vec<SandboxPortBinding>> {
        let mut used_host_ports = self.read_used_host_ports()?;
        used_host_ports.extend(existing_bindings.iter().map(|binding| binding.host_port));

        let mut mapped_guest_ports: BTreeSet<u16> = existing_bindings
            .iter()
            .map(|binding| binding.guest_port)
            .collect();
        let mut allocated = Vec::new();

        for exposed_port in exposed_ports {
            if exposed_port.protocol != OciExposedPortProtocol::Tcp {
                continue;
            }
            if !mapped_guest_ports.insert(exposed_port.port) {
                continue;
            }

            let host_port = self.next_available_host_port(&used_host_ports)?;
            used_host_ports.insert(host_port);
            allocated.push(SandboxPortBinding::tcp(
                auto_binding_name(exposed_port.port),
                host_port,
                exposed_port.port,
            ));
        }

        Ok(allocated)
    }

    fn next_available_host_port(&self, used_host_ports: &BTreeSet<u16>) -> Result<u16> {
        self.range
            .clone()
            .find(|port| !used_host_ports.contains(port))
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "published port range {}-{} is exhausted",
                    self.range.start(),
                    self.range.end()
                ),
            })
    }

    fn read_used_host_ports(&self) -> Result<BTreeSet<u16>> {
        let containers_root = self.state_root.join("containers");
        if !containers_root.exists() {
            return Ok(BTreeSet::new());
        }

        let mut used_host_ports = BTreeSet::new();
        for entry in
            std::fs::read_dir(&containers_root).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to read port-manager state directory {}: {error}",
                    containers_root.display()
                ),
            })?
        {
            let entry = entry.map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to iterate port-manager state directory {}: {error}",
                    containers_root.display()
                ),
            })?;
            let manifest_path = entry.path().join("manifest.json");
            if !manifest_path.exists() {
                continue;
            }

            let contents =
                std::fs::read(&manifest_path).map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to read sandbox manifest {}: {error}",
                        manifest_path.display()
                    ),
                })?;
            let manifest: PortLeaseManifest =
                serde_json::from_slice(&contents).map_err(|error| {
                    SandboxError::OperationFailed {
                        message: format!(
                            "failed to parse sandbox manifest {} for port leasing: {error}",
                            manifest_path.display()
                        ),
                    }
                })?;

            if !manifest.status.reserves_ports() {
                continue;
            }

            used_host_ports.extend(
                manifest
                    .spec
                    .port_bindings
                    .into_iter()
                    .map(|binding| binding.host_port),
            );
        }

        Ok(used_host_ports)
    }
}

fn auto_binding_name(guest_port: u16) -> String {
    format!("tcp-{guest_port}")
}

#[derive(Debug, Deserialize)]
struct PortLeaseManifest {
    status: SandboxStatus,
    spec: PortLeaseSpec,
}

#[derive(Debug, Deserialize)]
struct PortLeaseSpec {
    port_bindings: Vec<SandboxPortBinding>,
}

impl SandboxStatus {
    fn reserves_ports(self) -> bool {
        matches!(self, Self::Starting | Self::Ready | Self::Stopping)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::TempDir;

    use super::PortManager;
    use crate::backends::krun::buildah::{OciExposedPort, OciExposedPortProtocol};
    use crate::instance::SandboxStatus;
    use crate::spec::SandboxPortBinding;

    #[test]
    fn allocate_missing_bindings_uses_range_and_skips_existing_guest_ports() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let manager = PortManager::new(temp_dir.path(), 15000..=15005);
        let existing = vec![SandboxPortBinding::tcp("http", 18080, 8080)];
        let exposed = vec![
            tcp_exposed_port(8080),
            tcp_exposed_port(5432),
            udp_exposed_port(5353),
        ];

        let allocated = manager
            .allocate_missing_bindings(&existing, &exposed)
            .expect("port allocation should succeed");

        assert_eq!(
            allocated,
            vec![SandboxPortBinding::tcp("tcp-5432", 15000, 5432)]
        );
    }

    #[test]
    fn allocate_missing_bindings_ignores_stopped_manifests_and_reserves_active_ones() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        write_manifest(
            temp_dir.path(),
            "active",
            SandboxStatus::Ready,
            &[(15000, 5432)],
        );
        write_manifest(
            temp_dir.path(),
            "stopped",
            SandboxStatus::Stopped,
            &[(15001, 5432)],
        );

        let manager = PortManager::new(temp_dir.path(), 15000..=15002);
        let allocated = manager
            .allocate_missing_bindings(&[], &[tcp_exposed_port(8080), tcp_exposed_port(8443)])
            .expect("port allocation should succeed");

        assert_eq!(
            allocated,
            vec![
                SandboxPortBinding::tcp("tcp-8080", 15001, 8080),
                SandboxPortBinding::tcp("tcp-8443", 15002, 8443),
            ]
        );
    }

    fn write_manifest(
        state_root: &std::path::Path,
        sandbox_id: &str,
        status: SandboxStatus,
        host_guest_ports: &[(u16, u16)],
    ) {
        let container_dir = state_root.join("containers").join(sandbox_id);
        fs::create_dir_all(&container_dir).expect("container manifest directory should exist");
        let manifest = json!({
            "status": status,
            "spec": {
                "port_bindings": host_guest_ports
                    .iter()
                    .map(|(host_port, guest_port)| json!({
                        "name": format!("tcp-{guest_port}"),
                        "protocol": "tcp",
                        "host_address": "127.0.0.1",
                        "host_port": host_port,
                        "guest_port": guest_port,
                    }))
                    .collect::<Vec<_>>(),
            },
        });
        fs::write(
            container_dir.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).expect("manifest JSON should serialize"),
        )
        .expect("manifest JSON should be written");
    }

    fn tcp_exposed_port(port: u16) -> OciExposedPort {
        OciExposedPort {
            port,
            protocol: OciExposedPortProtocol::Tcp,
            raw: format!("{port}/tcp"),
        }
    }

    fn udp_exposed_port(port: u16) -> OciExposedPort {
        OciExposedPort {
            port,
            protocol: OciExposedPortProtocol::Udp,
            raw: format!("{port}/udp"),
        }
    }
}
