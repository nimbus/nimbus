//! Adapter context for durable port authority and provider-family decisions.
//!
//! New plans snapshot current configuration into their manifest. Every later
//! effect reconstructs this adapter from that persisted launch-time context.

use crate::backends::oci::port_lifecycle::OciPortLeaseCoordinator;
use crate::backends::oci::{conmon::OciConmonLayout, network::OciNetworkLayout};
use crate::error::{Result, SandboxError};

use super::{
    ContainerRunnerExecutionConfig, ContainerSandboxBackend, ContainerSandboxBackendConfig,
    ContainerSandboxManifest, runner,
};

pub(super) fn validate_manifest_execution_context_for_config(
    config: &ContainerSandboxBackendConfig,
    manifest: &ContainerSandboxManifest,
) -> Result<()> {
    runner::validate_runner_authority_roots(manifest)?;
    if config.workload_state_root != manifest.runner_config.workload_state_root {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "container backend workload root {} does not match manifest launch-time \
                 workload root {}",
                config.workload_state_root.display(),
                manifest.runner_config.workload_state_root.display()
            ),
        });
    }
    if config.network_state_root != manifest.runner_config.network_state_root {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "container backend network authority root {} does not match manifest \
                 launch-time network authority root {}",
                config.network_state_root.display(),
                manifest.runner_config.network_state_root.display()
            ),
        });
    }
    if manifest.handle.tenant_id != manifest.spec.tenant_id {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "container manifest handle tenant {} does not match specification tenant {} \
                 for {}",
                manifest.handle.tenant_id, manifest.spec.tenant_id, manifest.handle.id
            ),
        });
    }
    let expected_network_layout = OciNetworkLayout::with_roots(
        &manifest.runner_config.workload_state_root,
        &manifest.runner_config.network_state_root,
        &manifest.spec.tenant_id,
        &manifest.handle.id,
    );
    if manifest.network_layout != expected_network_layout {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "container manifest network layout for {} does not match its tenant-qualified \
                 launch-time authority root",
                manifest.handle.id
            ),
        });
    }
    let expected_conmon_layout = OciConmonLayout::new_for_tenant(
        &manifest.runner_config.workload_state_root,
        &manifest.spec.tenant_id,
        &manifest.handle.id,
    );
    if manifest.conmon_layout != expected_conmon_layout {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "container manifest runtime layout for {} does not match its tenant-qualified \
                 launch-time authority root",
                manifest.handle.id
            ),
        });
    }
    Ok(())
}

impl ContainerSandboxBackend {
    pub(in crate::backends::container::runtime) fn validate_manifest_execution_context(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<()> {
        validate_manifest_execution_context_for_config(&self.config, manifest)
    }

    pub(in crate::backends::container::runtime) fn port_lease_coordinator(
        &self,
    ) -> OciPortLeaseCoordinator {
        self.port_lease_coordinator_for_execution_config(
            &ContainerRunnerExecutionConfig::from_backend_config(&self.config),
        )
    }

    pub(in crate::backends::container::runtime) fn port_lease_coordinator_for_manifest(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<OciPortLeaseCoordinator> {
        self.validate_manifest_execution_context(manifest)?;
        Ok(self.port_lease_coordinator_for_execution_config(&manifest.runner_config))
    }

    fn port_lease_coordinator_for_execution_config(
        &self,
        config: &ContainerRunnerExecutionConfig,
    ) -> OciPortLeaseCoordinator {
        let manager = self
            .port_lease_coordinator
            .clone()
            .with_range(config.published_port_range.clone())
            .with_max_ports_per_tenant(config.max_published_ports_per_tenant);
        match config.network_publication_mode {
            super::ContainerNetworkPublicationMode::HostManaged => manager,
            super::ContainerNetworkPublicationMode::MachineForwarded => {
                manager.with_machine_port_proxy_bindings()
            }
        }
    }
}
