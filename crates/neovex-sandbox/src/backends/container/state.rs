use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use neovex_core::TenantId;
use serde::{Deserialize, Serialize};

use super::runtime::ContainerSandboxBackendConfig;
use crate::endpoint::PublishedEndpoint;
use crate::error::{Result, SandboxError};
use crate::instance::{SandboxHandle, SandboxId, SandboxStatus};
use crate::spec::{SandboxLifecycleSpec, SandboxPortBinding, SandboxResourceLimits, SandboxSpec};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerSandboxStateView {
    state_root: PathBuf,
}

impl ContainerSandboxStateView {
    pub fn new(state_root: impl Into<PathBuf>) -> Self {
        Self {
            state_root: state_root.into(),
        }
    }

    pub fn from_config(config: &ContainerSandboxBackendConfig) -> Self {
        Self::new(config.state_root.clone())
    }

    pub fn state_root(&self) -> &Path {
        &self.state_root
    }

    pub fn list(&self) -> Result<Vec<ContainerSandboxSummary>> {
        let mut summaries = self
            .read_all_records()?
            .into_iter()
            .map(ContainerPersistedSandboxRecord::into_summary)
            .collect::<Vec<_>>();
        summaries.sort_by(compare_summary_order);
        Ok(summaries)
    }

    pub fn list_for_tenant(&self, tenant_id: &TenantId) -> Result<Vec<ContainerSandboxSummary>> {
        let mut summaries = self
            .list()?
            .into_iter()
            .filter(|summary| &summary.tenant_id == tenant_id)
            .collect::<Vec<_>>();
        summaries.sort_by(compare_summary_order);
        Ok(summaries)
    }

    pub fn inspect(&self, sandbox_id: &SandboxId) -> Result<Option<ContainerSandboxDetails>> {
        self.read_record(&self.manifest_path(sandbox_id))
            .map(|record| record.map(ContainerPersistedSandboxRecord::into_details))
    }

    pub fn inspect_service(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Result<Option<ContainerSandboxDetails>> {
        let selected = self
            .read_all_records()?
            .into_iter()
            .filter(|record| {
                record.manifest.spec.tenant_id == *tenant_id
                    && record.manifest.spec.name == service_name
            })
            .max_by(compare_service_identity_preference);

        Ok(selected.map(ContainerPersistedSandboxRecord::into_details))
    }

    pub fn log_paths(&self, sandbox_id: &SandboxId) -> Result<Option<ContainerSandboxLogPaths>> {
        self.inspect(sandbox_id)
            .map(|details| details.map(|details| details.log_paths))
    }

    fn read_all_records(&self) -> Result<Vec<ContainerPersistedSandboxRecord>> {
        let containers_root = self.state_root.join("containers");
        if !containers_root.exists() {
            return Ok(Vec::new());
        }

        let mut records = Vec::new();
        for entry in
            std::fs::read_dir(&containers_root).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to read container state directory {}: {error}",
                    containers_root.display()
                ),
            })?
        {
            let entry = entry.map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to iterate container state directory {}: {error}",
                    containers_root.display()
                ),
            })?;
            let manifest_path = entry.path().join("manifest.json");
            let Some(record) = self.read_record(&manifest_path)? else {
                continue;
            };
            records.push(record);
        }

        Ok(records)
    }

    fn read_record(&self, manifest_path: &Path) -> Result<Option<ContainerPersistedSandboxRecord>> {
        if !manifest_path.exists() {
            return Ok(None);
        }

        let contents =
            std::fs::read(manifest_path).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to read container sandbox manifest {}: {error}",
                    manifest_path.display()
                ),
            })?;
        let manifest =
            serde_json::from_slice::<ContainerPersistedManifest>(&contents).map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!(
                        "failed to parse container sandbox manifest {}: {error}",
                        manifest_path.display()
                    ),
                }
            })?;

        Ok(Some(ContainerPersistedSandboxRecord {
            manifest,
            manifest_path: manifest_path.to_path_buf(),
        }))
    }

    fn manifest_path(&self, sandbox_id: &SandboxId) -> PathBuf {
        self.state_root
            .join("containers")
            .join(sandbox_id.as_str())
            .join("manifest.json")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContainerSandboxSummary {
    pub sandbox_id: SandboxId,
    pub tenant_id: TenantId,
    pub service_name: String,
    pub status: SandboxStatus,
    pub published_endpoints: Vec<PublishedEndpoint>,
    pub restart_count: u32,
    pub last_exit_code: Option<i32>,
    pub shutdown_requested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContainerSandboxDetails {
    pub summary: ContainerSandboxSummary,
    pub resources: SandboxResourceLimits,
    pub lifecycle: SandboxLifecycleSpec,
    pub port_bindings: Vec<SandboxPortBinding>,
    pub log_paths: ContainerSandboxLogPaths,
    pub state_dir: PathBuf,
    pub manifest_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContainerSandboxLogPaths {
    pub ctr_log: PathBuf,
    pub oci_log: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContainerPersistedSandboxRecord {
    manifest: ContainerPersistedManifest,
    manifest_path: PathBuf,
}

impl ContainerPersistedSandboxRecord {
    fn into_summary(self) -> ContainerSandboxSummary {
        ContainerSandboxSummary {
            sandbox_id: self.manifest.handle.id,
            tenant_id: self.manifest.spec.tenant_id,
            service_name: self.manifest.spec.name,
            status: self.manifest.status,
            published_endpoints: self.manifest.handle.published_endpoints,
            restart_count: 0,
            last_exit_code: self.manifest.last_exit_code,
            shutdown_requested: self.manifest.shutdown_requested,
        }
    }

    fn into_details(self) -> ContainerSandboxDetails {
        let summary = ContainerSandboxSummary {
            sandbox_id: self.manifest.handle.id,
            tenant_id: self.manifest.spec.tenant_id.clone(),
            service_name: self.manifest.spec.name.clone(),
            status: self.manifest.status,
            published_endpoints: self.manifest.handle.published_endpoints.clone(),
            restart_count: 0,
            last_exit_code: self.manifest.last_exit_code,
            shutdown_requested: self.manifest.shutdown_requested,
        };

        ContainerSandboxDetails {
            summary,
            resources: self.manifest.spec.resources,
            lifecycle: self.manifest.spec.lifecycle,
            port_bindings: self.manifest.spec.port_bindings,
            log_paths: ContainerSandboxLogPaths {
                ctr_log: self.manifest.conmon_layout.ctr_log,
                oci_log: self.manifest.conmon_layout.oci_log,
            },
            state_dir: self.manifest.conmon_layout.container_state_dir,
            manifest_path: self.manifest_path,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct ContainerPersistedManifest {
    handle: SandboxHandle,
    spec: SandboxSpec,
    conmon_layout: ContainerPersistedConmonLayout,
    last_exit_code: Option<i32>,
    shutdown_requested: bool,
    status: SandboxStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct ContainerPersistedConmonLayout {
    container_state_dir: PathBuf,
    ctr_log: PathBuf,
    oci_log: PathBuf,
}

fn compare_summary_order(
    left: &ContainerSandboxSummary,
    right: &ContainerSandboxSummary,
) -> Ordering {
    left.tenant_id
        .cmp(&right.tenant_id)
        .then_with(|| left.service_name.cmp(&right.service_name))
        .then_with(|| left.sandbox_id.as_str().cmp(right.sandbox_id.as_str()))
}

fn compare_service_identity_preference(
    left: &ContainerPersistedSandboxRecord,
    right: &ContainerPersistedSandboxRecord,
) -> Ordering {
    live_status(left.manifest.status)
        .cmp(&live_status(right.manifest.status))
        .then_with(|| {
            left.manifest
                .handle
                .id
                .as_str()
                .cmp(right.manifest.handle.id.as_str())
        })
}

fn live_status(status: SandboxStatus) -> bool {
    matches!(
        status,
        SandboxStatus::Starting
            | SandboxStatus::Ready
            | SandboxStatus::NotReady
            | SandboxStatus::Stopping
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::path::Path;

    use neovex_core::TenantId;
    use serde_json::json;
    use tempfile::TempDir;

    use super::ContainerSandboxStateView;
    use crate::endpoint::{PublishedEndpoint, PublishedEndpointProtocol};
    use crate::instance::{SandboxHandle, SandboxId, SandboxStatus};
    use crate::spec::{SandboxPortBinding, SandboxResourceLimits};

    #[test]
    fn state_view_lists_manifest_backed_summaries_and_skips_missing_manifest_dirs() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        write_manifest(
            temp_dir.path(),
            "db-01aaa",
            "svc-demo",
            "db",
            SandboxStatus::Ready,
            Some(137),
        );
        write_manifest(
            temp_dir.path(),
            "cache-01aaa",
            "svc-demo",
            "cache",
            SandboxStatus::Stopped,
            Some(0),
        );
        fs::create_dir_all(temp_dir.path().join("containers").join("missing-only-dir"))
            .expect("missing-only-dir should build");

        let view = ContainerSandboxStateView::new(temp_dir.path());
        let summaries = view.list().expect("manifest list should load");

        assert_eq!(summaries.len(), 2);
        assert_eq!(
            summaries
                .iter()
                .map(|summary| summary.service_name.as_str())
                .collect::<Vec<_>>(),
            vec!["cache", "db"]
        );
        assert_eq!(summaries[0].status, SandboxStatus::Stopped);
        assert_eq!(summaries[1].status, SandboxStatus::Ready);
        assert_eq!(summaries[1].last_exit_code, Some(137));
        assert_eq!(summaries[1].restart_count, 0);
    }

    #[test]
    fn inspect_service_prefers_live_sandbox_before_newer_terminal_one() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        write_manifest(
            temp_dir.path(),
            "db-01aaa",
            "svc-demo",
            "db",
            SandboxStatus::Ready,
            None,
        );
        write_manifest(
            temp_dir.path(),
            "db-01bbb",
            "svc-demo",
            "db",
            SandboxStatus::Stopped,
            Some(0),
        );

        let view = ContainerSandboxStateView::new(temp_dir.path());
        let details = view
            .inspect_service(
                &TenantId::new("svc-demo").expect("tenant id should be valid"),
                "db",
            )
            .expect("inspect should succeed")
            .expect("service should resolve");

        assert_eq!(details.summary.sandbox_id.as_str(), "db-01aaa");
        assert_eq!(details.summary.status, SandboxStatus::Ready);
        assert!(
            details
                .log_paths
                .ctr_log
                .ends_with("containers/db-01aaa/ctr.log"),
            "ctr log should come from the selected live sandbox"
        );
    }

    #[test]
    fn inspect_service_falls_back_to_newest_terminal_sandbox_when_no_live_match_exists() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        write_manifest(
            temp_dir.path(),
            "db-01aaa",
            "svc-demo",
            "db",
            SandboxStatus::Failed,
            Some(1),
        );
        write_manifest(
            temp_dir.path(),
            "db-01bbb",
            "svc-demo",
            "db",
            SandboxStatus::Stopped,
            Some(0),
        );

        let view = ContainerSandboxStateView::new(temp_dir.path());
        let details = view
            .inspect_service(
                &TenantId::new("svc-demo").expect("tenant id should be valid"),
                "db",
            )
            .expect("inspect should succeed")
            .expect("service should resolve");

        assert_eq!(details.summary.sandbox_id.as_str(), "db-01bbb");
        assert_eq!(details.summary.status, SandboxStatus::Stopped);
    }

    #[test]
    fn state_view_returns_empty_results_for_missing_roots_and_unknown_services() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let view = ContainerSandboxStateView::new(temp_dir.path());
        let tenant_id = TenantId::new("svc-demo").expect("tenant id should be valid");

        assert!(
            view.list().expect("list should succeed").is_empty(),
            "missing state roots should list as empty"
        );
        assert!(
            view.inspect(&SandboxId::new("db-01aaa"))
                .expect("inspect should succeed")
                .is_none(),
            "unknown sandbox ids should return none"
        );
        assert!(
            view.inspect_service(&tenant_id, "db")
                .expect("service inspect should succeed")
                .is_none(),
            "unknown service identities should return none"
        );
        assert!(
            view.log_paths(&SandboxId::new("db-01aaa"))
                .expect("log path lookup should succeed")
                .is_none(),
            "unknown log path lookups should return none"
        );
    }

    fn write_manifest(
        state_root: &Path,
        sandbox_id: &str,
        tenant_id: &str,
        service_name: &str,
        status: SandboxStatus,
        last_exit_code: Option<i32>,
    ) {
        let container_dir = state_root.join("containers").join(sandbox_id);
        fs::create_dir_all(&container_dir).expect("container manifest directory should exist");

        let handle = SandboxHandle::new(
            SandboxId::new(sandbox_id),
            service_name,
            crate::backend::SandboxBackendKind::Container,
            status,
            vec![PublishedEndpoint::new(
                "http",
                PublishedEndpointProtocol::Tcp,
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 18080),
            )],
        );
        let manifest = json!({
            "handle": handle,
            "spec": {
                "tenant_id": tenant_id,
                "name": service_name,
                "backend": "container",
                "filesystem": {
                    "rootfs": "/tmp/rootfs",
                    "readonly": true
                },
                "process": {
                    "args": ["/bin/server"],
                    "env": ["PATH=/usr/bin"],
                    "cwd": "/",
                    "terminal": false
                },
                "resources": SandboxResourceLimits::default(),
                "lifecycle": {
                    "restart_policy": "never"
                },
                "port_bindings": [SandboxPortBinding::tcp("http", 18080, 8080)]
            },
            "conmon_layout": {
                "container_state_dir": container_dir,
                "ctr_log": container_dir.join("ctr.log"),
                "oci_log": container_dir.join("oci.log")
            },
            "last_exit_code": last_exit_code,
            "shutdown_requested": matches!(status, SandboxStatus::Stopped),
            "status": status
        });

        fs::write(
            container_dir.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).expect("manifest should serialize"),
        )
        .expect("manifest should write");
    }
}
