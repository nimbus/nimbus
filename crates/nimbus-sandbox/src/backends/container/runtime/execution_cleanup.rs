//! Provider-backed container teardown and launch-failure compensation.
//!
//! This module owns the ordered withdraw/stop/detach/release state machine.
//! The runtime composition root delegates here so lifecycle fencing and retry
//! evidence remain colocated and independently testable.

use super::*;
use crate::backends::conmon::lifecycle::delete_runtime_and_confirm_absent as delete_conmon_runtime_and_confirm_absent;
use crate::backends::oci::network::{
    deallocate_container_ips_after_confirmed_detach, quarantine_network_segment_hold,
    release_network_segment_hold, remove_persistent_network_namespace,
};
use crate::backends::oci::port_manager::LaunchPortBatchState;

impl ContainerSandboxBackend {
    pub(super) fn release_execution_artifacts(
        &self,
        manifest: &mut ContainerSandboxManifest,
    ) -> Result<()> {
        if manifest.start_mode == ContainerStartMode::PlanOnly {
            return self.release_plan_only_execution_artifacts(manifest);
        }
        if !manifest.creator_handoff.authorizes_runtime_cleanup() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container creator for {} remains pending; runtime absence cannot authorize \
                     provider cleanup or network authority release",
                    manifest.handle.id
                ),
            });
        }
        self.validate_manifest_execution_context(manifest)?;
        let network_config = manifest.require_network_config()?;
        let adoption_receipt = network_config.reservation_claim.clone();
        authenticate_container_network_generation_for_cleanup(
            &manifest.network_layout,
            network_config,
            &manifest.handle.id,
        )?;
        let mut errors = Vec::new();
        let mut detach_confirmed = true;
        if let Err(error) = self.remove_runner_manifest_pointer(manifest) {
            errors.push(error.to_string());
        }
        let port_manager = self.port_manager_for_manifest(manifest)?;
        let launch_claim = manifest.launch_reservation_claim.clone();
        let machine_port_mode = manifest.start_mode == ContainerStartMode::Execute
            && manifest.runner_config.machine_port_forwarder.is_some();
        let published_batch_state = if machine_port_mode {
            launch_claim.as_ref().map_or_else(
                || {
                    port_manager.classify_machine_cleanup_batch(
                        &manifest.spec.tenant_id,
                        &manifest.handle.id,
                        &manifest.spec.port_bindings,
                        &manifest.port_leases,
                    )
                },
                |claim| port_manager.classify_launch_port_batch(&manifest.port_leases, claim),
            )
        } else {
            port_manager.classify_netavark_cleanup_batch(
                &manifest.spec.tenant_id,
                &manifest.handle.id,
                &manifest.spec.port_bindings,
                &manifest.port_leases,
                launch_claim.as_ref(),
            )
        };
        let pep_requests = manifest
            .egress_proxy
            .as_ref()
            .map(|assignment| vec![assignment.port_lease.clone()])
            .unwrap_or_default();
        let pep_batch_state = launch_claim
            .as_ref()
            .map_or(Ok(LaunchPortBatchState::ProviderOwned), |claim| {
                port_manager.classify_launch_port_batch(&pep_requests, claim)
            });
        if let Err(error) = &published_batch_state {
            detach_confirmed = false;
            errors.push(error.to_string());
        }
        if let Err(error) = &pep_batch_state {
            detach_confirmed = false;
            errors.push(error.to_string());
        }
        if let Err(error) = quarantine_network_segment_hold(
            self.segment_allocator.as_ref(),
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &adoption_receipt,
        ) {
            detach_confirmed = false;
            errors.push(error.to_string());
        }
        let mut machine_port_cleanup = None;
        if manifest.start_mode == ContainerStartMode::Execute
            && matches!(
                &published_batch_state,
                Ok(LaunchPortBatchState::ProviderOwned)
            )
        {
            if machine_port_mode {
                match self.begin_machine_port_proxy_release_for_manifest(manifest) {
                    Ok(cleanup) => machine_port_cleanup = cleanup,
                    Err(error) => {
                        detach_confirmed = false;
                        errors.push(error.to_string());
                    }
                }
            } else if let Err(error) = port_manager.withdraw_bindings(
                &manifest.spec.tenant_id,
                &manifest.handle.id,
                &manifest.spec.port_bindings,
                &manifest.port_leases,
            ) {
                detach_confirmed = false;
                errors.push(error.to_string());
            }
        }
        if manifest.start_mode == ContainerStartMode::Execute
            && let Some(forwarder) = manifest.runner_config.machine_port_forwarder.as_ref()
            && let Some(cleanup) = machine_port_cleanup.as_ref()
            && let Err(error) = self.unexpose_machine_port_proxy_publications(cleanup, forwarder)
        {
            detach_confirmed = false;
            errors.push(error.to_string());
        }
        if let Err(error) = delete_runtime_and_confirm_absent(manifest) {
            detach_confirmed = false;
            errors.push(error.to_string());
        }
        if !detach_confirmed {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to clean up container sandbox {} before provider detach: {}",
                    manifest.handle.id,
                    errors.join("; ")
                ),
            });
        }
        if matches!(&pep_batch_state, Ok(LaunchPortBatchState::ProviderOwned))
            && let Err(error) = self.stop_egress_proxy(
                &manifest.spec.tenant_id,
                &manifest.handle.id,
                manifest.egress_proxy.as_ref(),
            )
        {
            detach_confirmed = false;
            errors.push(error.to_string());
        }
        let netavark_detach_confirmed = match teardown_container_network(
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
                detach_confirmed = false;
                errors.push(error.to_string());
                false
            }
        };
        if netavark_detach_confirmed
            && let Err(error) =
                remove_persistent_network_namespace(&manifest.network_layout.netns_path)
        {
            detach_confirmed = false;
            errors.push(error.to_string());
        }
        // Final teardown (not restart): release the quarantined hold only after
        // provider and persistent-netns deletion are confirmed. On the last
        // hold, bridge cleanup must also succeed before allocation finalization.
        if detach_confirmed && manifest.start_mode == ContainerStartMode::Execute {
            match published_batch_state {
                Ok(LaunchPortBatchState::NeverBound) => {
                    if let Some(claim) = launch_claim.as_ref()
                        && let Err(error) =
                            port_manager.release_never_bound_requests(&manifest.port_leases, claim)
                    {
                        detach_confirmed = false;
                        errors.push(error.to_string());
                    }
                }
                Ok(LaunchPortBatchState::NetavarkClaimed(claims)) => {
                    if let Err(error) = port_manager.abandon_netavark_bind_claims_without_effect(
                        &manifest.spec.tenant_id,
                        &manifest.handle.id,
                        &manifest.spec.port_bindings,
                        &manifest.port_leases,
                        &claims,
                        launch_claim.as_ref(),
                    ) {
                        detach_confirmed = false;
                        errors.push(error.to_string());
                    }
                    if detach_confirmed {
                        let release = launch_claim.as_ref().map_or_else(
                            || {
                                port_manager.release_restart_retained_bindings(
                                    &manifest.spec.tenant_id,
                                    &manifest.handle.id,
                                    &manifest.spec.port_bindings,
                                    &manifest.port_leases,
                                )
                            },
                            |claim| {
                                port_manager
                                    .release_never_bound_requests(&manifest.port_leases, claim)
                            },
                        );
                        if let Err(error) = release {
                            detach_confirmed = false;
                            errors.push(error.to_string());
                        }
                    }
                }
                Ok(LaunchPortBatchState::RestartRetained) => {
                    let release = if machine_port_mode {
                        port_manager.release_restart_retained_machine_bindings(
                            &manifest.spec.tenant_id,
                            &manifest.handle.id,
                            &manifest.spec.port_bindings,
                            &manifest.port_leases,
                        )
                    } else {
                        port_manager.release_restart_retained_bindings(
                            &manifest.spec.tenant_id,
                            &manifest.handle.id,
                            &manifest.spec.port_bindings,
                            &manifest.port_leases,
                        )
                    };
                    if let Err(error) = release {
                        detach_confirmed = false;
                        errors.push(error.to_string());
                    }
                }
                Ok(LaunchPortBatchState::TerminalNoEffect) => {}
                Ok(LaunchPortBatchState::ProviderOwned) => {
                    let release_result = if machine_port_mode {
                        machine_port_cleanup.as_ref().map_or(Ok(()), |cleanup| {
                            self.complete_machine_port_proxy_cleanup(cleanup)
                        })
                    } else {
                        port_manager.release_bindings(
                            &manifest.spec.tenant_id,
                            &manifest.handle.id,
                            &manifest.spec.port_bindings,
                            &manifest.port_leases,
                        )
                    };
                    if let Err(error) = release_result {
                        detach_confirmed = false;
                        errors.push(error.to_string());
                    }
                }
                Err(_) => {}
            }
            if detach_confirmed
                && matches!(&pep_batch_state, Ok(LaunchPortBatchState::NeverBound))
                && let Some(claim) = launch_claim.as_ref()
                && let Err(error) = port_manager.release_never_bound_requests(&pep_requests, claim)
            {
                detach_confirmed = false;
                errors.push(error.to_string());
            }
        }
        if detach_confirmed
            && let Ok(network_config) = manifest.require_network_config()
            && let Err(error) = deallocate_container_ips_after_confirmed_detach(
                &manifest.network_layout,
                &manifest.handle.id,
                &network_config.reservation_claim,
            )
        {
            detach_confirmed = false;
            errors.push(error.to_string());
        }
        if detach_confirmed {
            errors.extend(
                release_network_segment_hold(
                    self.segment_allocator.as_ref(),
                    &manifest.spec.tenant_id,
                    &manifest.handle.id,
                    &adoption_receipt,
                )
                .into_iter()
                .map(|error| error.to_string()),
            );
        }
        // The launch claim is the only durable retry capability for a
        // never-bound batch. Retain it whenever network convergence failed.
        if detach_confirmed && errors.is_empty() {
            manifest.launch_reservation_claim = None;
        }
        match self.cleanup_manifest_launch_artifacts(manifest) {
            Ok(()) => manifest.launch_artifact = None,
            Err(error) => errors.push(error.to_string()),
        }
        if errors.is_empty() {
            manifest.network_cleanup_complete = true;
            Ok(())
        } else {
            Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to clean up container sandbox {}: {}",
                    manifest.handle.id,
                    errors.join("; ")
                ),
            })
        }
    }

    fn stop_egress_proxy(
        &self,
        tenant_id: &nimbus_core::TenantId,
        id: &SandboxId,
        assignment: Option<&EgressProxyAssignment>,
    ) -> Result<()> {
        self.egress_proxies
            .stop_with_assignment(tenant_id, id, assignment)
    }
}

fn delete_runtime_and_confirm_absent(manifest: &ContainerSandboxManifest) -> Result<()> {
    delete_conmon_runtime_and_confirm_absent(
        &manifest.conmon_launch.delete_command,
        &manifest.conmon_launch.state_command,
        manifest.handle.id.as_str(),
    )
}
