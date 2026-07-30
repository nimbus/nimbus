//! Restart decision state machine for supervised containers.

use crate::backends::conmon::lifecycle::{
    delete_runtime_and_confirm_absent, read_exit_code, remove_if_exists, restart_backoff_delay,
    restart_policy_allows_restart,
};
use crate::backends::oci::network::{
    AttachmentAuxiliaryDisposition, AttachmentTeardownMode, OciNetavarkOperation,
    authenticate_container_network_generation, remove_persistent_network_namespace,
    teardown_container_network,
};
use crate::error::{Result, SandboxError};
use crate::instance::SandboxStatus;

use super::manifest::ContainerSandboxManifest;
use super::status::synchronize_handle_status;
use super::{ContainerSandboxBackend, hostname_for};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContainerRestartDecision {
    NotRestarting,
    WaitingForBackoff,
    RestartNow,
}

pub(super) fn mark_restart_decision_after_exit(
    manifest: &mut ContainerSandboxManifest,
    now_millis: u64,
) -> Result<ContainerRestartDecision> {
    if manifest.shutdown_requested || !manifest.conmon_layout.exit_status_file.exists() {
        return Ok(ContainerRestartDecision::NotRestarting);
    }

    let exit_code = read_exit_code(&manifest.conmon_layout.exit_status_file)?;
    if !restart_policy_allows_restart(
        manifest.spec.lifecycle.restart_policy,
        exit_code,
        manifest.restart_count,
    ) {
        return Ok(ContainerRestartDecision::NotRestarting);
    }

    manifest.last_exit_code = Some(exit_code);
    let next_restart_at_millis = manifest.next_restart_at_millis.get_or_insert_with(|| {
        now_millis.saturating_add(restart_backoff_delay(manifest.restart_count).as_millis() as u64)
    });
    if now_millis < *next_restart_at_millis {
        synchronize_handle_status(manifest, SandboxStatus::Starting);
        return Ok(ContainerRestartDecision::WaitingForBackoff);
    }

    manifest.restart_count += 1;
    manifest.next_restart_at_millis = None;
    synchronize_handle_status(manifest, SandboxStatus::Starting);
    Ok(ContainerRestartDecision::RestartNow)
}

impl ContainerSandboxBackend {
    pub(super) fn reset_runtime_for_restart(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<()> {
        self.validate_manifest_execution_context(manifest)?;
        let network_config = manifest.require_network_config()?;
        if manifest.runner_config.machine_port_forwarder.is_none() {
            return self.reset_host_managed_runtime_for_restart(manifest, network_config);
        }
        self.reset_machine_forwarded_runtime_for_restart(manifest, network_config)
    }

    fn reset_host_managed_runtime_for_restart(
        &self,
        manifest: &ContainerSandboxManifest,
        network_config: &crate::backends::oci::network::OciNetworkConfig,
    ) -> Result<()> {
        let ports = self.port_lease_coordinator_for_manifest(manifest)?;
        let hostname = hostname_for(&manifest.spec);
        let lifecycle = self.attachment_lifecycle(&ports);
        self.attachment_adapter(manifest, network_config, &hostname, None)
            .detach_host_managed(&lifecycle, AttachmentTeardownMode::Restart, |auxiliary| {
                delete_runtime_and_confirm_absent(
                    &manifest.conmon_launch.delete_command,
                    &manifest.conmon_launch.state_command,
                    manifest.handle.id.as_str(),
                )
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to reset container sandbox {} for restart before provider detach: \
                         {error}",
                        manifest.handle.id
                    ),
                })?;
                if auxiliary == AttachmentAuxiliaryDisposition::ProviderOwned {
                    self.egress_proxies.stop_for_restart(
                        &manifest.spec.tenant_id,
                        &manifest.handle.id,
                        manifest.egress_proxy.as_ref(),
                    )?;
                }
                Ok(())
            })?;
        clear_restart_receipts(manifest)
    }

    fn reset_machine_forwarded_runtime_for_restart(
        &self,
        manifest: &ContainerSandboxManifest,
        network_config: &crate::backends::oci::network::OciNetworkConfig,
    ) -> Result<()> {
        authenticate_container_network_generation(
            &self.ipam_authority,
            &manifest.network_layout,
            network_config,
            &manifest.handle.id,
        )?;
        delete_runtime_and_confirm_absent(
            &manifest.conmon_launch.delete_command,
            &manifest.conmon_launch.state_command,
            manifest.handle.id.as_str(),
        )
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to reset container sandbox {} for restart before provider detach: \
                 {error}",
                manifest.handle.id
            ),
        })?;
        let mut errors = Vec::new();
        if let Err(error) = self.egress_proxies.stop_for_restart(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            manifest.egress_proxy.as_ref(),
        ) {
            errors.push(error.to_string());
        }
        let machine_port_cleanup =
            match self.begin_machine_port_proxy_restart_for_manifest(manifest) {
                Ok(cleanup) => cleanup,
                Err(error) => {
                    errors.push(error.to_string());
                    None
                }
            };
        let netavark_detach_confirmed = match teardown_container_network(
            &self.ipam_authority,
            &OciNetavarkOperation::new(
                &manifest.network_layout,
                network_config,
                &manifest.handle.id,
                manifest.spec.display_name(),
                &hostname_for(&manifest.spec),
                &manifest.spec.port_bindings,
                manifest.runner_config.machine_port_forwarder.as_ref(),
            ),
        ) {
            Ok(()) => true,
            Err(error) => {
                errors.push(error.to_string());
                false
            }
        };
        if netavark_detach_confirmed
            && let Err(error) =
                remove_persistent_network_namespace(&manifest.network_layout.netns_path)
        {
            errors.push(error.to_string());
        }
        if let Some(forwarder) = manifest.runner_config.machine_port_forwarder.as_ref()
            && let Some(cleanup) = machine_port_cleanup.as_ref()
            && let Err(error) = self.unexpose_machine_port_proxy_publications(cleanup, forwarder)
        {
            errors.push(error.to_string());
        }
        if errors.is_empty()
            && let Some(cleanup) = machine_port_cleanup.as_ref()
            && let Err(error) = self.complete_machine_port_proxy_cleanup(cleanup)
        {
            errors.push(error.to_string());
        }
        if errors.is_empty() {
            clear_restart_receipts(manifest)
        } else {
            Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to reset container sandbox {} for restart: {}",
                    manifest.handle.id,
                    errors.join("; ")
                ),
            })
        }
    }
}

fn clear_restart_receipts(manifest: &ContainerSandboxManifest) -> Result<()> {
    // The exit receipt is the durable restart checkpoint. Consume it only
    // after every provider/network teardown and all other stale runtime
    // artifacts are acknowledged, so a failed reset retains enough exact
    // evidence for a bounded retry.
    remove_if_exists(&manifest.conmon_layout.pidfile)?;
    remove_if_exists(&manifest.conmon_layout.conmon_pidfile)?;
    remove_if_exists(&manifest.conmon_layout.exit_status_file)
}
