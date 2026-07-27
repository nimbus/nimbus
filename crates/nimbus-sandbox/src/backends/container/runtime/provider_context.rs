//! Adapter context for durable port authority and provider-family decisions.
//!
//! New plans snapshot current configuration into their manifest. Every later
//! effect reconstructs this adapter from that persisted launch-time context.

use crate::backends::oci::port_manager::PortManager;
use crate::backends::oci::{conmon::OciConmonLayout, network::OciNetworkLayout};
use crate::error::{Result, SandboxError};

use super::{
    ContainerRunnerExecutionConfig, ContainerSandboxBackend, ContainerSandboxManifest, runner,
};

impl ContainerSandboxBackend {
    pub(in crate::backends::container::runtime) fn validate_manifest_execution_context(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<()> {
        runner::validate_runner_authority_root(manifest)?;
        if self.config.state_root != manifest.runner_config.state_root {
            return Err(SandboxError::InvalidSpec {
                message: format!(
                    "container backend authority root {} does not match manifest launch-time \
                     authority {}",
                    self.config.state_root.display(),
                    manifest.runner_config.state_root.display()
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
        let expected_network_layout = OciNetworkLayout::new(
            &manifest.runner_config.state_root,
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
            &manifest.runner_config.state_root,
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

    pub(in crate::backends::container::runtime) fn port_manager(&self) -> PortManager {
        Self::port_manager_for_execution_config(
            &ContainerRunnerExecutionConfig::from_backend_config(&self.config),
        )
    }

    pub(in crate::backends::container::runtime) fn port_manager_for_manifest(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<PortManager> {
        self.validate_manifest_execution_context(manifest)?;
        Ok(Self::port_manager_for_execution_config(
            &manifest.runner_config,
        ))
    }

    fn port_manager_for_execution_config(config: &ContainerRunnerExecutionConfig) -> PortManager {
        let manager = PortManager::new(&config.state_root, config.published_port_range.clone())
            .with_max_ports_per_tenant(config.max_published_ports_per_tenant);
        if config.machine_port_forwarder.is_some() {
            manager.with_machine_port_proxy_bindings()
        } else {
            manager
        }
    }
}
