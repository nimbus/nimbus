//! Container-specific teardown prerequisites and machine-provider cleanup.
//!
//! Host-managed Netavark, netns, IPAM, segment, and port-lifetime ordering is
//! delegated to the shared OCI attachment lifecycle. Machine-forwarded
//! publication remains explicit here because it has a distinct provider and
//! durable receipt contract.

use super::machine_ports::MachinePortProxyCleanup;
use super::*;
use crate::backends::conmon::lifecycle::delete_runtime_and_confirm_absent as delete_conmon_runtime_and_confirm_absent;
use crate::backends::oci::network::{
    AttachmentAuxiliaryDisposition, AttachmentDetachFailure, AttachmentDetachFailureStage,
    AttachmentTeardownMode,
};
use crate::backends::oci::port_lifecycle::LaunchPortBatchState;
use nimbus_network::{NetworkReservationClaim, PortLeaseRequest};

struct MachineForwardedFinalization {
    published_batch_state: LaunchPortBatchState,
    pep_batch_state: LaunchPortBatchState,
    launch_claim: Option<NetworkReservationClaim>,
    pep_requests: Vec<PortLeaseRequest>,
    machine_port_cleanup: Option<MachinePortProxyCleanup>,
}

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
        match manifest
            .runner_config
            .validated_machine_port_forwarder(&manifest.handle.id)?
        {
            None => self.release_host_managed_execution_artifacts(manifest, &network_config),
            Some(forwarder) => self.release_machine_forwarded_execution_artifacts(
                manifest,
                &network_config,
                forwarder.clone(),
            ),
        }
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
        forwarder: crate::backends::oci::network::OciMachinePortForwarderConfig,
    ) -> Result<()> {
        let mut errors = Vec::new();
        if let Err(error) = self.remove_runner_manifest_pointer(manifest) {
            errors.push(error.to_string());
        }
        let port_lease_coordinator = self.port_lease_coordinator_for_manifest(manifest)?;
        let hostname = hostname_for(&manifest.spec);
        let lifecycle = self.attachment_lifecycle(&port_lease_coordinator);
        let detach = self
            .attachment_adapter(manifest, network_config, &hostname, Some(&forwarder))
            .detach_machine_forwarded(
                &lifecycle,
                AttachmentTeardownMode::Final,
                || {
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
                        |claim| {
                            port_lease_coordinator
                                .classify_launch_port_batch(&manifest.port_leases, claim)
                        },
                    )?;
                    let pep_requests = manifest
                        .egress_proxy
                        .as_ref()
                        .map(|assignment| vec![assignment.port_lease.clone()])
                        .unwrap_or_default();
                    let pep_batch_state = launch_claim.as_ref().map_or(
                        Ok(LaunchPortBatchState::ProviderOwned),
                        |claim| {
                            port_lease_coordinator.classify_launch_port_batch(&pep_requests, claim)
                        },
                    )?;
                    let machine_port_cleanup =
                        if published_batch_state == LaunchPortBatchState::ProviderOwned {
                            self.begin_machine_port_proxy_release_for_manifest(manifest)?
                        } else {
                            None
                        };
                    if let Some(cleanup) = machine_port_cleanup.as_ref() {
                        self.unexpose_machine_port_proxy_publications(cleanup, &forwarder)?;
                    }
                    delete_runtime_and_confirm_absent(manifest)?;
                    if pep_batch_state == LaunchPortBatchState::ProviderOwned {
                        self.stop_egress_proxy(
                            &manifest.spec.tenant_id,
                            &manifest.handle.id,
                            manifest.egress_proxy.as_ref(),
                        )?;
                    }
                    Ok(MachineForwardedFinalization {
                        published_batch_state,
                        pep_batch_state,
                        launch_claim,
                        pep_requests,
                        machine_port_cleanup,
                    })
                },
                |finalization| {
                    match finalization.published_batch_state {
                        LaunchPortBatchState::NeverBound => {
                            if let Some(claim) = finalization.launch_claim.as_ref() {
                                port_lease_coordinator
                                    .release_never_bound_requests(&manifest.port_leases, claim)?;
                            }
                        }
                        LaunchPortBatchState::NetavarkClaimed(_) => {}
                        LaunchPortBatchState::RestartRetained => {
                            port_lease_coordinator.release_restart_retained_machine_bindings(
                                &manifest.spec.tenant_id,
                                &manifest.handle.id,
                                &manifest.spec.port_bindings,
                                &manifest.port_leases,
                            )?;
                        }
                        LaunchPortBatchState::TerminalNoEffect => {}
                        LaunchPortBatchState::ProviderOwned => {
                            if let Some(cleanup) = finalization.machine_port_cleanup.as_ref() {
                                self.complete_machine_port_proxy_cleanup(cleanup)?;
                            }
                        }
                    }
                    if finalization.pep_batch_state == LaunchPortBatchState::NeverBound
                        && let Some(claim) = finalization.launch_claim.as_ref()
                    {
                        port_lease_coordinator
                            .release_never_bound_requests(&finalization.pep_requests, claim)?;
                    }
                    Ok(())
                },
            );
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
