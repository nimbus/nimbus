use std::path::PathBuf;

use nimbus_core::TenantId;
use serde::Deserialize;

use crate::artifact_paths;
use crate::error::{Result, SandboxError};
use crate::instance::SandboxStatus;
use crate::spec::{SandboxResourceCharge, SandboxResourceQuotaPolicy, SandboxSpec};

#[derive(Debug, Clone)]
pub(crate) struct ResourceQuotaManager {
    state_root: PathBuf,
    policy: SandboxResourceQuotaPolicy,
}

impl ResourceQuotaManager {
    pub(crate) fn new(state_root: impl Into<PathBuf>, policy: SandboxResourceQuotaPolicy) -> Self {
        Self {
            state_root: state_root.into(),
            policy,
        }
    }

    pub(crate) fn ensure_launch_quota(&self, spec: &SandboxSpec) -> Result<()> {
        let existing = self.read_reserved_resource_charge_for_tenant(&spec.tenant_id)?;
        let launch = self.policy.charge_for(&spec.resources);
        self.ensure_within_policy(&spec.tenant_id, existing.plus(launch))
    }

    fn read_reserved_resource_charge_for_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<SandboxResourceCharge> {
        let mut charge = SandboxResourceCharge::default();
        for manifest_path in artifact_paths::manifest_paths_for_tenant(&self.state_root, tenant_id)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to read sandbox tenant state directory {} for tenant {tenant_id}: {error}",
                    self.state_root.display()
                ),
            })?
        {
            let contents =
                std::fs::read(&manifest_path).map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to read sandbox manifest {} for resource quota admission: {error}",
                        manifest_path.display()
                    ),
                })?;
            let manifest: ResourceQuotaManifest =
                serde_json::from_slice(&contents).map_err(|error| {
                    SandboxError::OperationFailed {
                        message: format!(
                            "failed to parse sandbox manifest {} for resource quota admission: {error}",
                            manifest_path.display()
                        ),
                    }
                })?;
            if manifest.status.reserves_resources() {
                charge = charge.plus(self.policy.charge_for(&manifest.spec.resources));
            }
        }
        Ok(charge)
    }

    fn ensure_within_policy(
        &self,
        tenant_id: &TenantId,
        requested: SandboxResourceCharge,
    ) -> Result<()> {
        ensure_limit(
            tenant_id,
            "active sandbox",
            requested.active_sandboxes as u64,
            self.policy
                .max_active_sandboxes_per_tenant
                .map(|limit| limit as u64),
        )?;
        ensure_limit(
            tenant_id,
            "sandbox vCPU",
            requested.vcpus,
            self.policy.max_vcpus_per_tenant,
        )?;
        ensure_limit(
            tenant_id,
            "sandbox memory byte",
            requested.memory_bytes,
            self.policy.max_memory_bytes_per_tenant,
        )?;
        ensure_limit(
            tenant_id,
            "sandbox disk byte",
            requested.disk_bytes,
            self.policy.max_disk_bytes_per_tenant,
        )?;
        ensure_limit(
            tenant_id,
            "sandbox log byte",
            requested.log_bytes,
            self.policy.max_log_bytes_per_tenant,
        )
    }
}

fn ensure_limit(
    tenant_id: &TenantId,
    label: &str,
    requested: u64,
    limit: Option<u64>,
) -> Result<()> {
    let Some(limit) = limit else {
        return Ok(());
    };
    if requested <= limit {
        return Ok(());
    }
    Err(SandboxError::OperationFailed {
        message: format!(
            "{label} quota exceeded for tenant {tenant_id}: {requested} requested/reserved exceeds limit {limit}"
        ),
    })
}

#[derive(Debug, Deserialize)]
struct ResourceQuotaManifest {
    status: SandboxStatus,
    spec: SandboxSpec,
}

impl SandboxStatus {
    fn reserves_resources(self) -> bool {
        matches!(
            self,
            Self::Starting | Self::Ready | Self::NotReady | Self::Stopping
        )
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use nimbus_core::TenantId;
    use serde_json::json;

    use super::*;
    use crate::backend::SandboxBackendKind;
    use crate::instance::{SandboxHandle, SandboxId};
    use crate::spec::{
        SandboxOwnerSpec, SandboxProcessSpec, SandboxResourceLimits, SandboxRootSpec,
        SandboxRootfsSpec,
    };

    fn tenant_id(value: &str) -> TenantId {
        TenantId::new(value).expect("tenant id should parse")
    }

    fn sample_spec(tenant: &str, service: &str) -> SandboxSpec {
        SandboxSpec::new(
            tenant_id(tenant),
            SandboxOwnerSpec::service(service),
            SandboxBackendKind::Krun,
            SandboxRootSpec::Rootfs(SandboxRootfsSpec::new("")),
            SandboxProcessSpec::new(Vec::<String>::new()),
        )
    }

    fn write_manifest(
        state_root: &std::path::Path,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        status: SandboxStatus,
    ) {
        let manifest_path =
            crate::artifact_paths::manifest_path(state_root, &spec.tenant_id, sandbox_id);
        let parent = manifest_path
            .parent()
            .expect("manifest path should have a parent");
        fs::create_dir_all(parent).expect("manifest parent should create");
        fs::write(
            &manifest_path,
            serde_json::to_vec(&json!({
                "handle": SandboxHandle::new(
                    spec.tenant_id.clone(),
                    sandbox_id.clone(),
                    spec.display_name().to_owned(),
                    spec.backend,
                    status,
                    Vec::new(),
                ),
                "spec": spec,
                "status": status,
            }))
            .expect("manifest should serialize"),
        )
        .expect("manifest should write");
    }

    #[test]
    fn resource_quota_counts_only_same_tenant_active_manifests() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let tenant = tenant_id("tenant-a");
        let policy = SandboxResourceQuotaPolicy::default()
            .with_max_active_sandboxes_per_tenant(Some(2))
            .with_max_vcpus_per_tenant(Some(2));
        let manager = ResourceQuotaManager::new(temp.path(), policy);
        let first = sample_spec("tenant-a", "db")
            .with_resource_limits(SandboxResourceLimits::default().with_cpu_count(1));
        write_manifest(
            temp.path(),
            &first,
            &SandboxId::new("db-existing"),
            SandboxStatus::Ready,
        );
        let other_tenant = sample_spec("tenant-b", "db")
            .with_resource_limits(SandboxResourceLimits::default().with_cpu_count(64));
        write_manifest(
            temp.path(),
            &other_tenant,
            &SandboxId::new("tenant-b-db"),
            SandboxStatus::Ready,
        );

        let launch = sample_spec("tenant-a", "api")
            .with_resource_limits(SandboxResourceLimits::default().with_cpu_count(1));

        manager
            .ensure_launch_quota(&launch)
            .expect("other tenant usage should not consume tenant-a quota");

        assert_eq!(launch.tenant_id, tenant);
    }

    #[test]
    fn resource_quota_rejects_same_tenant_cpu_memory_disk_and_log_exhaustion() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let policy = SandboxResourceQuotaPolicy::default()
            .with_max_vcpus_per_tenant(Some(2))
            .with_max_memory_bytes_per_tenant(Some(1024))
            .with_max_disk_bytes_per_tenant(Some(2048))
            .with_max_log_bytes_per_tenant(Some(256));
        let manager = ResourceQuotaManager::new(temp.path(), policy);
        let first = sample_spec("tenant-a", "db").with_resource_limits(
            SandboxResourceLimits::default()
                .with_cpu_count(1)
                .with_memory_limit_bytes(512)
                .with_disk_limit_bytes(1024)
                .with_log_limit_bytes(128),
        );
        write_manifest(
            temp.path(),
            &first,
            &SandboxId::new("db-existing"),
            SandboxStatus::Ready,
        );
        let launch = sample_spec("tenant-a", "api").with_resource_limits(
            SandboxResourceLimits::default()
                .with_cpu_count(2)
                .with_memory_limit_bytes(512)
                .with_disk_limit_bytes(1024)
                .with_log_limit_bytes(128),
        );

        let error = manager
            .ensure_launch_quota(&launch)
            .expect_err("same-tenant launch should exceed vCPU quota first");

        assert!(
            error.to_string().contains("sandbox vCPU quota exceeded"),
            "expected vCPU quota rejection, got: {error}"
        );
    }
}
