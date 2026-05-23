use std::collections::BTreeSet;
use std::ops::RangeInclusive;
use std::path::PathBuf;

use nimbus_core::TenantId;
use serde::Deserialize;

use super::buildah::{OciExposedPort, OciExposedPortProtocol};
use crate::artifact_paths;
use crate::error::{Result, SandboxError};
use crate::instance::SandboxStatus;
use crate::spec::SandboxPortBinding;

pub(crate) const DEFAULT_MAX_PORTS_PER_TENANT: usize = 128;

#[derive(Debug, Clone)]
pub(crate) struct PortManager {
    range: RangeInclusive<u16>,
    state_root: PathBuf,
    max_ports_per_tenant: Option<usize>,
}

impl PortManager {
    pub(crate) fn new(state_root: impl Into<PathBuf>, range: RangeInclusive<u16>) -> Self {
        Self {
            range,
            state_root: state_root.into(),
            max_ports_per_tenant: None,
        }
    }

    pub(crate) fn with_max_ports_per_tenant(mut self, max_ports_per_tenant: Option<usize>) -> Self {
        self.max_ports_per_tenant = max_ports_per_tenant;
        self
    }

    pub(crate) fn allocate_missing_bindings_for_tenant(
        &self,
        tenant_id: &TenantId,
        existing_bindings: &[SandboxPortBinding],
        exposed_ports: &[OciExposedPort],
    ) -> Result<Vec<SandboxPortBinding>> {
        let mut used_host_ports = self.read_used_host_ports()?;
        used_host_ports.extend(existing_bindings.iter().map(|binding| binding.host_port));

        let mut mapped_guest_ports: BTreeSet<u16> = existing_bindings
            .iter()
            .map(|binding| binding.guest_port)
            .collect();
        let mut unmapped_tcp_guest_ports = Vec::new();

        for exposed_port in exposed_ports {
            if exposed_port.protocol != OciExposedPortProtocol::Tcp {
                continue;
            }
            if !mapped_guest_ports.insert(exposed_port.port) {
                continue;
            }
            unmapped_tcp_guest_ports.push(exposed_port.port);
        }

        self.ensure_tenant_port_quota(
            tenant_id,
            existing_bindings
                .len()
                .saturating_add(unmapped_tcp_guest_ports.len()),
        )?;

        let mut allocated = Vec::new();
        for guest_port in unmapped_tcp_guest_ports {
            let host_port = self.next_available_host_port(&used_host_ports)?;
            used_host_ports.insert(host_port);
            allocated.push(SandboxPortBinding::tcp(
                auto_binding_name(guest_port),
                host_port,
                guest_port,
            ));
        }

        Ok(allocated)
    }

    pub(crate) fn allocate_internal_host_port(
        &self,
        existing_bindings: &[SandboxPortBinding],
    ) -> Result<u16> {
        let mut used_host_ports = self.read_used_host_ports()?;
        used_host_ports.extend(existing_bindings.iter().map(|binding| binding.host_port));
        self.next_available_host_port(&used_host_ports)
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
        let mut used_host_ports = BTreeSet::new();
        for manifest_path in
            artifact_paths::all_manifest_paths(&self.state_root).map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!(
                        "failed to read port-manager tenant state directory {}: {error}",
                        self.state_root.display()
                    ),
                }
            })?
        {
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
            if let Some(egress_proxy) = manifest.egress_proxy {
                used_host_ports.insert(egress_proxy.port);
            }
        }

        Ok(used_host_ports)
    }

    fn ensure_tenant_port_quota(&self, tenant_id: &TenantId, launch_ports: usize) -> Result<()> {
        let Some(max_ports_per_tenant) = self.max_ports_per_tenant else {
            return Ok(());
        };
        let active_ports = self.read_reserved_port_count_for_tenant(tenant_id)?;
        let requested_ports = active_ports.saturating_add(launch_ports);
        if requested_ports <= max_ports_per_tenant {
            return Ok(());
        }
        Err(SandboxError::OperationFailed {
            message: format!(
                "published port quota exceeded for tenant {tenant_id}: {requested_ports} requested/reserved ports exceeds limit {max_ports_per_tenant}"
            ),
        })
    }

    fn read_reserved_port_count_for_tenant(&self, tenant_id: &TenantId) -> Result<usize> {
        let mut reserved_ports = 0usize;
        for manifest_path in artifact_paths::manifest_paths_for_tenant(&self.state_root, tenant_id)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to read port-manager tenant state directory {} for tenant {tenant_id}: {error}",
                    self.state_root.display()
                ),
            })?
        {
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
                            "failed to parse sandbox manifest {} for tenant port quota: {error}",
                            manifest_path.display()
                        ),
                    }
                })?;

            if manifest.status.reserves_ports() {
                reserved_ports =
                    reserved_ports.saturating_add(manifest.spec.port_bindings.len());
            }
        }
        Ok(reserved_ports)
    }
}

fn auto_binding_name(guest_port: u16) -> String {
    format!("tcp-{guest_port}")
}

#[derive(Debug, Deserialize)]
struct PortLeaseManifest {
    status: SandboxStatus,
    spec: PortLeaseSpec,
    egress_proxy: Option<PortLeaseEgressProxy>,
}

#[derive(Debug, Deserialize)]
struct PortLeaseSpec {
    port_bindings: Vec<SandboxPortBinding>,
}

#[derive(Debug, Deserialize)]
struct PortLeaseEgressProxy {
    port: u16,
}

impl SandboxStatus {
    fn reserves_ports(self) -> bool {
        matches!(
            self,
            Self::Starting | Self::Ready | Self::NotReady | Self::Stopping
        )
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::TempDir;

    use super::PortManager;
    use crate::artifact_paths;
    use crate::backends::oci::buildah::{OciExposedPort, OciExposedPortProtocol};
    use crate::instance::{SandboxId, SandboxStatus};
    use crate::spec::SandboxPortBinding;
    use nimbus_core::TenantId;

    #[test]
    fn allocate_missing_bindings_uses_range_and_skips_existing_guest_ports() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let tenant_id = tenant_id("tenant-a");
        let manager = PortManager::new(temp_dir.path(), 15000..=15005);
        let existing = vec![SandboxPortBinding::tcp("http", 18080, 8080)];
        let exposed = vec![
            tcp_exposed_port(8080),
            tcp_exposed_port(5432),
            udp_exposed_port(5353),
        ];

        let allocated = manager
            .allocate_missing_bindings_for_tenant(&tenant_id, &existing, &exposed)
            .expect("port allocation should succeed");

        assert_eq!(
            allocated,
            vec![SandboxPortBinding::tcp("tcp-5432", 15000, 5432)]
        );
    }

    #[test]
    fn allocate_missing_bindings_ignores_stopped_manifests_and_reserves_active_ones() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let tenant_id = tenant_id("tenant-a");
        write_manifest(
            temp_dir.path(),
            &tenant_id,
            "active",
            SandboxStatus::Ready,
            &[(15000, 5432)],
        );
        write_manifest(
            temp_dir.path(),
            &tenant_id,
            "stopped",
            SandboxStatus::Stopped,
            &[(15001, 5432)],
        );

        let manager = PortManager::new(temp_dir.path(), 15000..=15002);
        let allocated = manager
            .allocate_missing_bindings_for_tenant(
                &tenant_id,
                &[],
                &[tcp_exposed_port(8080), tcp_exposed_port(8443)],
            )
            .expect("port allocation should succeed");

        assert_eq!(
            allocated,
            vec![
                SandboxPortBinding::tcp("tcp-8080", 15001, 8080),
                SandboxPortBinding::tcp("tcp-8443", 15002, 8443),
            ]
        );
    }

    #[test]
    fn allocate_missing_bindings_keeps_not_ready_ports_reserved() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let tenant_id = tenant_id("tenant-a");
        write_manifest(
            temp_dir.path(),
            &tenant_id,
            "not-ready",
            SandboxStatus::NotReady,
            &[(15000, 5432)],
        );

        let manager = PortManager::new(temp_dir.path(), 15000..=15001);
        let allocated = manager
            .allocate_missing_bindings_for_tenant(&tenant_id, &[], &[tcp_exposed_port(8080)])
            .expect("port allocation should succeed");

        assert_eq!(
            allocated,
            vec![SandboxPortBinding::tcp("tcp-8080", 15001, 8080)]
        );
    }

    #[test]
    fn allocate_internal_host_port_skips_active_egress_proxy_leases() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let tenant_id = tenant_id("tenant-a");
        write_manifest_with_egress_proxy(
            temp_dir.path(),
            &tenant_id,
            "active",
            SandboxStatus::Ready,
            15000,
        );

        let manager = PortManager::new(temp_dir.path(), 15000..=15001);
        let allocated = manager
            .allocate_internal_host_port(&[])
            .expect("internal port allocation should skip active proxy leases");

        assert_eq!(allocated, 15001);
    }

    #[test]
    fn allocate_internal_host_port_ignores_stopped_egress_proxy_leases() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let tenant_id = tenant_id("tenant-a");
        write_manifest_with_egress_proxy(
            temp_dir.path(),
            &tenant_id,
            "stopped",
            SandboxStatus::Stopped,
            15000,
        );

        let manager = PortManager::new(temp_dir.path(), 15000..=15001);
        let allocated = manager
            .allocate_internal_host_port(&[])
            .expect("stopped proxy lease should not reserve a host port");

        assert_eq!(allocated, 15000);
    }

    #[test]
    fn tenant_port_quota_rejects_explicit_bindings_that_exceed_same_tenant_limit() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let tenant_id = tenant_id("tenant-a");
        write_manifest(
            temp_dir.path(),
            &tenant_id,
            "active",
            SandboxStatus::Ready,
            &[(15000, 5432)],
        );

        let manager =
            PortManager::new(temp_dir.path(), 15000..=15002).with_max_ports_per_tenant(Some(1));
        let existing = vec![SandboxPortBinding::tcp("http", 18080, 8080)];
        let error = manager
            .allocate_missing_bindings_for_tenant(&tenant_id, &existing, &[])
            .expect_err("explicit bindings should still count against the tenant port quota");

        assert!(
            error.to_string().contains("published port quota exceeded")
                && error.to_string().contains("tenant-a")
                && error.to_string().contains("limit 1"),
            "expected tenant quota error, got: {error}"
        );
    }

    #[test]
    fn tenant_port_quota_counts_only_same_tenant_but_reserves_host_ports_globally() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let tenant_a = tenant_id("tenant-a");
        let tenant_b = tenant_id("tenant-b");
        write_manifest(
            temp_dir.path(),
            &tenant_a,
            "active-a",
            SandboxStatus::Ready,
            &[(15000, 5432)],
        );
        write_manifest(
            temp_dir.path(),
            &tenant_b,
            "active-b",
            SandboxStatus::Ready,
            &[(15001, 6379)],
        );

        let manager =
            PortManager::new(temp_dir.path(), 15000..=15002).with_max_ports_per_tenant(Some(2));
        let allocated = manager
            .allocate_missing_bindings_for_tenant(&tenant_a, &[], &[tcp_exposed_port(8080)])
            .expect("other tenant leases should not consume tenant-a quota");

        assert_eq!(
            allocated,
            vec![SandboxPortBinding::tcp("tcp-8080", 15002, 8080)],
            "other tenant leases should still reserve host ports globally"
        );
    }

    fn write_manifest(
        state_root: &std::path::Path,
        tenant_id: &TenantId,
        sandbox_id: &str,
        status: SandboxStatus,
        host_guest_ports: &[(u16, u16)],
    ) {
        let sandbox_id = SandboxId::new(sandbox_id);
        let manifest_path = artifact_paths::manifest_path(state_root, tenant_id, &sandbox_id);
        let container_dir = manifest_path
            .parent()
            .expect("manifest path should have a parent directory");
        fs::create_dir_all(container_dir).expect("container manifest directory should exist");
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
            manifest_path,
            serde_json::to_vec_pretty(&manifest).expect("manifest JSON should serialize"),
        )
        .expect("manifest JSON should be written");
    }

    fn write_manifest_with_egress_proxy(
        state_root: &std::path::Path,
        tenant_id: &TenantId,
        sandbox_id: &str,
        status: SandboxStatus,
        egress_proxy_port: u16,
    ) {
        let sandbox_id = SandboxId::new(sandbox_id);
        let manifest_path = artifact_paths::manifest_path(state_root, tenant_id, &sandbox_id);
        let container_dir = manifest_path
            .parent()
            .expect("manifest path should have a parent directory");
        fs::create_dir_all(container_dir).expect("container manifest directory should exist");
        let manifest = json!({
            "status": status,
            "egress_proxy": {
                "host": "10.89.0.1",
                "port": egress_proxy_port,
            },
            "spec": {
                "port_bindings": [],
            },
        });
        fs::write(
            manifest_path,
            serde_json::to_vec_pretty(&manifest).expect("manifest JSON should serialize"),
        )
        .expect("manifest JSON should be written");
    }

    fn tenant_id(value: &str) -> TenantId {
        TenantId::new(value).expect("tenant id should parse")
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
