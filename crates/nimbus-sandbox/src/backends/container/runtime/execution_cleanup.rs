//! Container-specific teardown prerequisites and machine-provider cleanup.
//!
//! Host-managed Netavark, netns, IPAM, segment, and port-lifetime ordering is
//! delegated to the shared OCI attachment lifecycle. Machine-forwarded
//! publication remains explicit here because it has a distinct provider and
//! durable receipt contract.

use super::*;
use crate::backends::conmon::lifecycle::delete_runtime_and_confirm_absent as delete_conmon_runtime_and_confirm_absent;
use crate::backends::oci::network::{
    AttachmentAuxiliaryDisposition, AttachmentDetachFailure, AttachmentDetachFailureStage,
    AttachmentTeardownMode, OciNetavarkOperation,
    authenticate_container_network_generation_for_cleanup,
    deallocate_container_ips_after_confirmed_detach, quarantine_network_segment_hold,
    release_network_segment_hold, remove_persistent_network_namespace, teardown_container_network,
};
use crate::backends::oci::port_lifecycle::LaunchPortBatchState;

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
        let network_config = manifest.require_network_config()?.clone();
        if manifest.runner_config.machine_port_forwarder.is_none() {
            return self.release_host_managed_execution_artifacts(manifest, &network_config);
        }
        self.release_machine_forwarded_execution_artifacts(manifest, &network_config)
    }

    fn release_host_managed_execution_artifacts(
        &self,
        manifest: &mut ContainerSandboxManifest,
        network_config: &crate::backends::oci::network::OciNetworkConfig,
    ) -> Result<()> {
        let mut errors = Vec::new();
        if let Err(error) = self.remove_runner_manifest_pointer(manifest) {
            errors.push(error.to_string());
        }
        let ports = self.port_lease_coordinator_for_manifest(manifest)?;
        let hostname = hostname_for(&manifest.spec);
        let lifecycle = self.attachment_lifecycle(&ports);
        let detach: std::result::Result<(), AttachmentDetachFailure> = self
            .attachment_adapter(manifest, network_config, &hostname, None)
            .detach_host_managed(&lifecycle, AttachmentTeardownMode::Final, |auxiliary| {
                delete_runtime_and_confirm_absent(manifest)?;
                if auxiliary == AttachmentAuxiliaryDisposition::ProviderOwned {
                    self.stop_egress_proxy(
                        &manifest.spec.tenant_id,
                        &manifest.handle.id,
                        manifest.egress_proxy.as_ref(),
                    )?;
                }
                Ok(())
            });
        match detach {
            Ok(()) => manifest.launch_reservation_claim = None,
            Err(failure) => {
                let stage = failure.stage();
                errors.push(failure.into_error().to_string());
                if stage == AttachmentDetachFailureStage::BeforeProviderDetach {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "failed to clean up container sandbox {} before provider detach: {}",
                            manifest.handle.id,
                            errors.join("; ")
                        ),
                    });
                }
            }
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

    fn release_machine_forwarded_execution_artifacts(
        &self,
        manifest: &mut ContainerSandboxManifest,
        network_config: &crate::backends::oci::network::OciNetworkConfig,
    ) -> Result<()> {
        let adoption_receipt = network_config.reservation_claim.clone();
        authenticate_container_network_generation_for_cleanup(
            &self.ipam_authority,
            &manifest.network_layout,
            network_config,
            &manifest.handle.id,
        )?;
        let mut errors = Vec::new();
        let mut detach_confirmed = true;
        if let Err(error) = self.remove_runner_manifest_pointer(manifest) {
            errors.push(error.to_string());
        }
        let port_lease_coordinator = self.port_lease_coordinator_for_manifest(manifest)?;
        let launch_claim = manifest.launch_reservation_claim.clone();
        let published_batch_state = launch_claim.as_ref().map_or_else(
            || {
                port_lease_coordinator.classify_machine_cleanup_batch(
                    &manifest.spec.tenant_id,
                    &manifest.handle.id,
                    &manifest.spec.port_bindings,
                    &manifest.port_leases,
                )
            },
            |claim| port_lease_coordinator.classify_launch_port_batch(&manifest.port_leases, claim),
        );
        let pep_requests = manifest
            .egress_proxy
            .as_ref()
            .map(|assignment| vec![assignment.port_lease.clone()])
            .unwrap_or_default();
        let pep_batch_state = launch_claim
            .as_ref()
            .map_or(Ok(LaunchPortBatchState::ProviderOwned), |claim| {
                port_lease_coordinator.classify_launch_port_batch(&pep_requests, claim)
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
        if matches!(
            &published_batch_state,
            Ok(LaunchPortBatchState::ProviderOwned)
        ) {
            match self.begin_machine_port_proxy_release_for_manifest(manifest) {
                Ok(cleanup) => machine_port_cleanup = cleanup,
                Err(error) => {
                    detach_confirmed = false;
                    errors.push(error.to_string());
                }
            }
        }
        if let Some(forwarder) = manifest.runner_config.machine_port_forwarder.as_ref()
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
            &self.ipam_authority,
            &OciNetavarkOperation::new(
                &manifest.network_layout,
                network_config,
                &manifest.handle.id,
                manifest.spec.display_name(),
                &hostname_for(&manifest.spec),
                &manifest.spec.port_bindings,
                (manifest.start_mode == ContainerStartMode::Execute)
                    .then_some(manifest.runner_config.machine_port_forwarder.as_ref())
                    .flatten(),
            ),
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
        if detach_confirmed {
            match published_batch_state {
                Ok(LaunchPortBatchState::NeverBound) => {
                    if let Some(claim) = launch_claim.as_ref()
                        && let Err(error) = port_lease_coordinator
                            .release_never_bound_requests(&manifest.port_leases, claim)
                    {
                        detach_confirmed = false;
                        errors.push(error.to_string());
                    }
                }
                Ok(LaunchPortBatchState::NetavarkClaimed(_)) => {
                    // Dead-owner recovery plus the exact Netavark absence
                    // receipt completed this terminal release above.
                }
                Ok(LaunchPortBatchState::RestartRetained) => {
                    let release = port_lease_coordinator.release_restart_retained_machine_bindings(
                        &manifest.spec.tenant_id,
                        &manifest.handle.id,
                        &manifest.spec.port_bindings,
                        &manifest.port_leases,
                    );
                    if let Err(error) = release {
                        detach_confirmed = false;
                        errors.push(error.to_string());
                    }
                }
                Ok(LaunchPortBatchState::TerminalNoEffect) => {}
                Ok(LaunchPortBatchState::ProviderOwned) => {
                    let release_result = machine_port_cleanup.as_ref().map_or(Ok(()), |cleanup| {
                        self.complete_machine_port_proxy_cleanup(cleanup)
                    });
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
                && let Err(error) =
                    port_lease_coordinator.release_never_bound_requests(&pep_requests, claim)
            {
                detach_confirmed = false;
                errors.push(error.to_string());
            }
        }
        if detach_confirmed
            && let Ok(network_config) = manifest.require_network_config()
            && let Err(error) = deallocate_container_ips_after_confirmed_detach(
                &self.ipam_authority,
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
