//! Container cleanup context for exact startup-quarantined network orphans.

use nimbus_network::NetworkAttachmentReservationState;

use super::manifest::{reconcile_startup_manifest_publications, retained_startup_manifest_paths};
use super::teardown::state::ContainerStopProgress;
use super::*;
use crate::backends::conmon::lifecycle::inspect_runtime_artifact_presence;
use crate::backends::oci::network::{
    AttachmentAuxiliaryDisposition, AttachmentBackendKind, AttachmentTeardownMode,
    OciNetworkConfig, OciOrphanCleanupContext, OciOrphanCleanupDisposition, OciOrphanCleanupKind,
    OciOrphanCleanupSubject, ReservedNetworkLaunchAuthority, ReservedNetworkLaunchIdentity,
    reconcile_startup_network_state_with_cleanup,
    release_reserved_network_launch_after_ports_with_terminal_publication,
    retire_terminal_container_ipam_release,
};

impl ContainerSandboxBackend {
    pub(super) fn reconcile_container_startup_network_state(&self) -> Result<()> {
        let attachments = self
            .attachment_authority
            .as_ref()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: "container startup cannot reconcile network state without portable attachment authority"
                    .to_owned(),
            })?;
        reconcile_startup_manifest_publications(&self.config.workload_state_root)?;
        let retained_desired_manifests = retained_startup_manifest_paths(&self.config)?;
        reconcile_startup_network_state_with_cleanup(
            &self.config.workload_state_root,
            attachments,
            &self.ipam_authority,
            self.segment_allocator.as_ref(),
            &retained_desired_manifests,
            self,
        )
    }

    fn read_exact_startup_manifest(
        &self,
        subject: &OciOrphanCleanupSubject,
    ) -> Result<ContainerSandboxManifest> {
        let path = crate::artifact_paths::manifest_path(
            &self.config.workload_state_root,
            subject.tenant_id(),
            subject.sandbox_id(),
        );
        let bytes = std::fs::read(&path).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to read exact Container orphan-cleanup manifest {}: {error}",
                path.display()
            ),
        })?;
        let manifest: ContainerSandboxManifest =
            serde_json::from_slice(&bytes).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to parse exact Container orphan-cleanup manifest {}: {error}",
                    path.display()
                ),
            })?;
        self.validate_manifest_execution_context(&manifest)?;
        if manifest.conmon_layout.manifest_path != path
            || manifest.handle.id != *subject.sandbox_id()
            || manifest.spec.tenant_id != *subject.tenant_id()
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "Container orphan-cleanup manifest {} crossed its tenant-qualified subject",
                    path.display()
                ),
            });
        }
        Ok(manifest)
    }

    fn authenticate_container_orphan_subject(
        &self,
        manifest: &ContainerSandboxManifest,
        subject: &OciOrphanCleanupSubject,
    ) -> Result<OciNetworkConfig> {
        if subject.backend() != AttachmentBackendKind::Container {
            return Err(SandboxError::OperationFailed {
                message: "Container cleanup context rejected a non-Container provider subject"
                    .to_owned(),
            });
        }
        let network_config = manifest.require_network_config()?.clone();
        let desired_matches = subject.desired().is_none_or(|desired| {
            desired.association().segment_id() == subject.segment_id()
                && desired.association().reservation_claim() == subject.reservation_claim()
        });
        let manifest_authority_matches = match subject.kind() {
            OciOrphanCleanupKind::NeverEffected | OciOrphanCleanupKind::Effectful => {
                manifest.launch_reservation_claim.as_ref() == Some(subject.reservation_claim())
                    && !manifest.network_cleanup_complete
            }
            OciOrphanCleanupKind::TerminalPublication => {
                (manifest.network_cleanup_complete && manifest.launch_reservation_claim.is_none())
                    || (!manifest.network_cleanup_complete
                        && manifest.launch_reservation_claim.as_ref()
                            == Some(subject.reservation_claim()))
            }
        };
        let dead_never_effected = subject.kind() != OciOrphanCleanupKind::NeverEffected
            || (manifest.shutdown_requested
                && matches!(
                    manifest.status,
                    SandboxStatus::Stopping | SandboxStatus::Stopped | SandboxStatus::Failed
                ));
        let no_workload_teardown_owner = manifest.execution_teardown.admission_is_open()
            && matches!(
                manifest.execution_teardown.stop(),
                ContainerStopProgress::NotRequested
            );
        let exact = manifest.start_mode == ContainerStartMode::Execute
            && manifest.handle.id == *subject.sandbox_id()
            && manifest.handle.tenant_id == *subject.tenant_id()
            && manifest.spec.tenant_id == *subject.tenant_id()
            && subject.authenticates_network_config(&network_config)
            && desired_matches
            && manifest_authority_matches
            && dead_never_effected
            && no_workload_teardown_owner;
        if !exact {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "Container manifest for {} does not authenticate the exact quarantined network generation",
                    subject.sandbox_id()
                ),
            });
        }
        if !matches!(
            manifest.creator_handoff,
            ContainerCreatorHandoffState::NotSpawned
        ) {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "Container orphan {} has creator state {:?}; runtime absence is not proven",
                    subject.sandbox_id(),
                    manifest.creator_handoff
                ),
            });
        }
        for (path, label) in [
            (&manifest.conmon_layout.pidfile, "runtime pidfile"),
            (&manifest.conmon_layout.conmon_pidfile, "conmon pidfile"),
            (
                &manifest.conmon_layout.exit_status_file,
                "exit-status receipt",
            ),
        ] {
            if inspect_runtime_artifact_presence(path, label)? {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "Container orphan {} retains {label}; runtime absence is not proven",
                        subject.sandbox_id()
                    ),
                });
            }
        }
        if manifest
            .runner_config
            .validated_machine_port_forwarder(&manifest.handle.id)?
            .is_some()
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "Container orphan {} uses provider-managed machine forwarding; host-managed cleanup is not authorized",
                    subject.sandbox_id()
                ),
            });
        }
        Ok(network_config)
    }
}

impl OciOrphanCleanupContext for ContainerSandboxBackend {
    fn converge_quarantined_orphan(
        &self,
        subject: &OciOrphanCleanupSubject,
    ) -> Result<OciOrphanCleanupDisposition> {
        if subject.backend() != AttachmentBackendKind::Container {
            return Ok(OciOrphanCleanupDisposition::Retain);
        }
        let initial = self.read_exact_startup_manifest(subject)?;
        let (_lifecycle_guard, mut manifest) =
            runner::lock_current_provision_lifecycle_for_backend(self, &initial)?;
        let network_config = self.authenticate_container_orphan_subject(&manifest, subject)?;
        if subject.kind() == OciOrphanCleanupKind::TerminalPublication {
            if !manifest.network_cleanup_complete || manifest.launch_reservation_claim.is_some() {
                manifest.launch_reservation_claim = None;
                manifest.network_cleanup_complete = true;
                self.write_existing_workload_manifest(&manifest)?;
            }
            if subject.desired().is_none() {
                retire_terminal_container_ipam_release(
                    &self.ipam_authority,
                    &manifest.network_layout,
                    &manifest.handle.id,
                    &network_config.attachment_id,
                    &network_config.reservation_claim,
                    network_config.provider_kind(),
                )?;
            } else {
                self.reconcile_terminal_ipam_retirement(&manifest)?;
            }
            return Ok(OciOrphanCleanupDisposition::Converged);
        }
        let reservation = self.segment_allocator.inspect_attachment_reservation(
            subject.tenant_id(),
            subject.attachment_id(),
            subject.reservation_claim(),
        )?;
        let reservation_matches = match subject.kind() {
            OciOrphanCleanupKind::NeverEffected => {
                reservation.state() == NetworkAttachmentReservationState::ReservationCleanupPending
            }
            OciOrphanCleanupKind::Effectful => matches!(
                reservation.state(),
                NetworkAttachmentReservationState::ProviderCleanupPending
                    | NetworkAttachmentReservationState::Absent
            ),
            OciOrphanCleanupKind::TerminalPublication => {
                unreachable!("terminal publication returns before allocator cleanup")
            }
        };
        if !reservation_matches {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "Container orphan {} has an invalid exact segment cleanup state {:?}",
                    subject.sandbox_id(),
                    reservation.state()
                ),
            });
        }

        if subject.kind() == OciOrphanCleanupKind::NeverEffected {
            let ports = self.port_lease_coordinator_for_manifest(&manifest)?;
            let port_compensation =
                ports.release_never_bound_launch_claim(subject.reservation_claim());
            let layout = manifest.network_layout.clone();
            release_reserved_network_launch_after_ports_with_terminal_publication(
                ReservedNetworkLaunchAuthority::new(
                    self.segment_allocator.as_ref(),
                    &self.ipam_authority,
                    ReservedNetworkLaunchIdentity::new(
                        &layout,
                        subject.tenant_id(),
                        subject.sandbox_id(),
                        subject.attachment_id(),
                        subject.reservation_claim(),
                    ),
                    network_config.provider_kind(),
                ),
                port_compensation,
                || {
                    manifest.launch_reservation_claim = None;
                    manifest.network_cleanup_complete = true;
                    self.write_existing_workload_manifest(&manifest)
                },
            )?;
            return Ok(OciOrphanCleanupDisposition::Converged);
        }

        let ports = self.port_lease_coordinator_for_manifest(&manifest)?;
        let hostname = hostname_for(&manifest.spec);
        let lifecycle = self.attachment_lifecycle(&ports);
        let adapter = self.attachment_adapter(&manifest, &network_config, &hostname, None);
        adapter
            .detach_host_managed(&lifecycle, AttachmentTeardownMode::Final, |auxiliary| {
                match auxiliary {
                    AttachmentAuxiliaryDisposition::NoEffect => Ok(()),
                    AttachmentAuxiliaryDisposition::ProviderOwned => {
                        Err(SandboxError::OperationFailed {
                            message: format!(
                                "Container orphan {} retains a live auxiliary provider",
                                subject.sandbox_id()
                            ),
                        })
                    }
                    AttachmentAuxiliaryDisposition::Unknown => Err(SandboxError::OperationFailed {
                        message: format!(
                            "Container orphan {} has unknown auxiliary-provider authority",
                            subject.sandbox_id()
                        ),
                    }),
                }
            })
            .map_err(Into::<SandboxError>::into)?;

        manifest.launch_reservation_claim = None;
        manifest.network_cleanup_complete = true;
        self.write_existing_workload_manifest(&manifest)?;
        Ok(OciOrphanCleanupDisposition::Converged)
    }
}
