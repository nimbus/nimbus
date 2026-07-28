//! Restart decision state machine for supervised containers.

use crate::backends::conmon::lifecycle::{
    delete_runtime_and_confirm_absent, read_exit_code, remove_if_exists, restart_backoff_delay,
    restart_policy_allows_restart,
};
use crate::backends::oci::network::{
    authenticate_container_network_generation, remove_persistent_network_namespace,
    teardown_container_network,
};
use crate::error::{Result, SandboxError};
use crate::instance::SandboxStatus;

use super::manifest::ContainerSandboxManifest;
use super::status::synchronize_handle_status;
use super::{ContainerSandboxBackend, ContainerStartMode, hostname_for};

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
        authenticate_container_network_generation(
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
        let netavark_cleanup = if manifest.start_mode == ContainerStartMode::Execute
            && manifest.runner_config.machine_port_forwarder.is_none()
        {
            let port_lease_coordinator = self.port_lease_coordinator_for_manifest(manifest)?;
            match port_lease_coordinator.classify_netavark_cleanup_batch(
                &manifest.spec.tenant_id,
                &manifest.handle.id,
                &manifest.spec.port_bindings,
                &manifest.port_leases,
                manifest.launch_reservation_claim.as_ref(),
            )? {
                crate::backends::oci::port_lifecycle::LaunchPortBatchState::ProviderOwned => {
                    let cleanup = port_lease_coordinator.begin_netavark_cleanup(
                        &self.netavark_port_lifetimes,
                        &manifest.spec.tenant_id,
                        &manifest.handle.id,
                        &manifest.spec.port_bindings,
                        &manifest.port_leases,
                    )?;
                    Some((port_lease_coordinator, cleanup))
                }
                crate::backends::oci::port_lifecycle::LaunchPortBatchState::RestartRetained
                | crate::backends::oci::port_lifecycle::LaunchPortBatchState::TerminalNoEffect => {
                    None
                }
                crate::backends::oci::port_lifecycle::LaunchPortBatchState::NeverBound
                    if manifest.port_leases.is_empty() =>
                {
                    None
                }
                state => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "container restart cannot detach Netavark from published-listener \
                             authority {state:?}"
                        ),
                    });
                }
            }
        } else {
            None
        };
        let mut errors = Vec::new();
        if let Err(error) = self.egress_proxies.stop_for_restart(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            manifest.egress_proxy.as_ref(),
        ) {
            errors.push(error.to_string());
        }
        let machine_port_cleanup = if manifest.start_mode == ContainerStartMode::Execute
            && manifest.runner_config.machine_port_forwarder.is_some()
        {
            match self.begin_machine_port_proxy_restart_for_manifest(manifest) {
                Ok(cleanup) => cleanup,
                Err(error) => {
                    errors.push(error.to_string());
                    None
                }
            }
        } else {
            None
        };
        let mut netavark_detach_confirmed = match teardown_container_network(
            &manifest.network_layout,
            network_config,
            &manifest.handle.id,
            manifest.spec.display_name(),
            &hostname_for(&manifest.spec),
            &manifest.spec.port_bindings,
            (manifest.start_mode == ContainerStartMode::Execute)
                .then_some(manifest.runner_config.machine_port_forwarder.as_ref())
                .flatten(),
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
            netavark_detach_confirmed = false;
            errors.push(error.to_string());
        }
        if netavark_detach_confirmed
            && let Some((port_lease_coordinator, cleanup)) = &netavark_cleanup
            && let Err(error) = port_lease_coordinator.complete_netavark_cleanup(
                &manifest.port_leases,
                cleanup.as_ref(),
                false,
            )
        {
            netavark_detach_confirmed = false;
            errors.push(error.to_string());
        }
        if !netavark_detach_confirmed
            && let Some((port_lease_coordinator, cleanup)) = netavark_cleanup
            && let Err(error) = port_lease_coordinator.retain_ambiguous_netavark_cleanup(
                &self.netavark_port_lifetimes,
                &manifest.spec.tenant_id,
                &manifest.handle.id,
                cleanup,
            )
        {
            errors.push(error.to_string());
        }
        if manifest.start_mode == ContainerStartMode::Execute
            && let Some(forwarder) = manifest.runner_config.machine_port_forwarder.as_ref()
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
            // The exit receipt is the durable restart checkpoint. Consume it
            // only after every provider/network teardown and all other stale
            // runtime artifacts are acknowledged, so a failed reset retains
            // enough exact evidence for a bounded retry.
            remove_if_exists(&manifest.conmon_layout.pidfile)?;
            remove_if_exists(&manifest.conmon_layout.conmon_pidfile)?;
            remove_if_exists(&manifest.conmon_layout.exit_status_file)?;
            Ok(())
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
