//! Durable root-witness authentication for reopened krun workloads.

use super::*;

impl KrunSandboxBackend {
    /// Authenticate a durable manifest against this exact composition before
    /// any caller can perform restart, detach, or cleanup effects.
    pub(super) fn validate_manifest_roots(
        &self,
        expected_id: &SandboxId,
        manifest: &KrunSandboxManifest,
    ) -> Result<()> {
        if manifest.handle.id != *expected_id {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun manifest identity mismatch: expected sandbox {}, actual sandbox {}",
                    expected_id, manifest.handle.id
                ),
            });
        }
        if manifest.handle.tenant_id != manifest.spec.tenant_id {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun manifest tenant mismatch for sandbox {}: handle tenant {}, spec tenant {}",
                    manifest.handle.id, manifest.handle.tenant_id, manifest.spec.tenant_id
                ),
            });
        }

        let expected_network_layout = OciNetworkLayout::with_roots(
            &self.config.workload_state_root,
            &self.config.network_state_root,
            &manifest.spec.tenant_id,
            &manifest.handle.id,
        );
        if manifest.network_layout != expected_network_layout {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun manifest network-root mismatch for sandbox {}: expected workload root {}, actual workload root {}, expected network root {}, actual network root {}",
                    manifest.handle.id,
                    expected_network_layout.workload_state_root.display(),
                    manifest.network_layout.workload_state_root.display(),
                    expected_network_layout.network_state_root.display(),
                    manifest.network_layout.network_state_root.display(),
                ),
            });
        }

        let expected_conmon_layout = OciConmonLayout::new_for_tenant(
            &self.config.workload_state_root,
            &manifest.spec.tenant_id,
            &manifest.handle.id,
        );
        if manifest.conmon_layout != expected_conmon_layout {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun manifest workload-root mismatch for sandbox {}: expected conmon layout \
                     {expected_conmon_layout:?}, actual conmon layout {:?}",
                    manifest.handle.id, manifest.conmon_layout,
                ),
            });
        }
        Ok(())
    }
}
