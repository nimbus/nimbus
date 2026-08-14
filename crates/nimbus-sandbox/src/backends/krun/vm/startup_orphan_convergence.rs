//! Krun cleanup context for exact startup-quarantined network orphans.

use std::collections::BTreeSet;

use nimbus_network::NetworkAttachmentReservationState;

use super::start::hostname_for;
use super::teardown::state::KrunStopProgress;
use super::*;
use crate::backends::conmon::lifecycle::inspect_runtime_artifact_presence;
use crate::backends::oci::network::{
    AttachmentAuxiliaryDisposition, AttachmentTeardownMode, OciOrphanCleanupContext,
    OciOrphanCleanupDisposition, OciOrphanCleanupKind, OciOrphanCleanupSubject,
    ReservedNetworkLaunchAuthority, ReservedNetworkLaunchIdentity,
    reconcile_startup_network_state_with_cleanup,
    release_reserved_network_launch_after_ports_with_terminal_publication,
    retire_terminal_container_ipam_release,
};

impl KrunSandboxBackend {
    fn retained_krun_startup_manifest_paths(&self) -> Result<BTreeSet<std::path::PathBuf>> {
        let paths = crate::artifact_paths::all_manifest_paths(&self.config.workload_state_root)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to enumerate Krun startup manifests under {}: {error}",
                    self.config.workload_state_root.display()
                ),
            })?;
        let mut retained = BTreeSet::new();
        for path in paths {
            let bytes = std::fs::read(&path).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to read Krun startup manifest {}: {error}",
                    path.display()
                ),
            })?;
            let Ok(manifest) = serde_json::from_slice::<KrunSandboxManifest>(&bytes) else {
                continue;
            };
            if self
                .validate_manifest_roots(&manifest.handle.id, &manifest)
                .is_err()
            {
                continue;
            }
            let authority_free = manifest.launch_authority == KrunLaunchAuthority::Released
                || (manifest.start_mode == KrunStartMode::PlanOnly
                    && manifest.launch_authority == KrunLaunchAuthority::PlanOnly
                    && manifest.network_config.is_none()
                    && manifest.port_leases.is_empty()
                    && manifest.egress_proxy.is_none());
            if authority_free && manifest.conmon_layout.manifest_path == path {
                retained.insert(path);
            }
        }
        Ok(retained)
    }

    pub(super) fn reconcile_krun_startup_network_state(&self) -> Result<()> {
        let attachments = self
            .attachment_authority
            .as_ref()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: "krun startup cannot reconcile network state without portable attachment authority"
                    .to_owned(),
            })?;
        let retained_manifests = self.retained_krun_startup_manifest_paths()?;
        reconcile_startup_network_state_with_cleanup(
            &self.config.workload_state_root,
            attachments,
            &self.ipam_authority,
            self.segment_allocator.as_ref(),
            &retained_manifests,
            self,
        )
    }

    fn authenticate_krun_orphan_subject(
        &self,
        manifest: &KrunSandboxManifest,
        subject: &OciOrphanCleanupSubject,
    ) -> Result<OciNetworkConfig> {
        self.validate_manifest_roots(subject.sandbox_id(), manifest)?;
        let network_config = manifest.require_network_config()?.clone();
        let desired_matches = subject.desired().is_none_or(|desired| {
            desired.association().segment_id() == subject.segment_id()
                && desired.association().reservation_claim() == subject.reservation_claim()
        });
        let launch_authority_matches = match subject.kind() {
            OciOrphanCleanupKind::NeverEffected => matches!(
                &manifest.launch_authority,
                KrunLaunchAuthority::Reserved { reservation_claim }
                    if reservation_claim == subject.reservation_claim()
            ),
            OciOrphanCleanupKind::Effectful => matches!(
                &manifest.launch_authority,
                KrunLaunchAuthority::Adopted { reservation_claim }
                    if reservation_claim == subject.reservation_claim()
            ),
            OciOrphanCleanupKind::TerminalPublication => {
                manifest.launch_authority == KrunLaunchAuthority::Released
                    || matches!(
                        &manifest.launch_authority,
                        KrunLaunchAuthority::Reserved { reservation_claim }
                            | KrunLaunchAuthority::Adopted { reservation_claim }
                            if reservation_claim == subject.reservation_claim()
                    )
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
                KrunStopProgress::NotRequested
            );
        let exact = subject.backend() == AttachmentBackendKind::Krun
            && manifest.start_mode == KrunStartMode::Execute
            && manifest.handle.id == *subject.sandbox_id()
            && manifest.handle.tenant_id == *subject.tenant_id()
            && manifest.spec.tenant_id == *subject.tenant_id()
            && subject.authenticates_network_config(&network_config)
            && desired_matches
            && launch_authority_matches
            && dead_never_effected
            && no_workload_teardown_owner
            && !manifest.provider_failure_cleanup.is_active();
        if !exact {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "Krun manifest for {} does not authenticate the exact quarantined network generation",
                    subject.sandbox_id()
                ),
            });
        }
        if !matches!(
            manifest.creator_handoff,
            KrunCreatorHandoffState::NotSpawned
        ) {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "Krun orphan {} has creator state {:?}; runtime absence is not proven",
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
                        "Krun orphan {} retains {label}; runtime absence is not proven",
                        subject.sandbox_id()
                    ),
                });
            }
        }
        Ok(network_config)
    }
}

impl OciOrphanCleanupContext for KrunSandboxBackend {
    fn converge_quarantined_orphan(
        &self,
        subject: &OciOrphanCleanupSubject,
    ) -> Result<OciOrphanCleanupDisposition> {
        if subject.backend() != AttachmentBackendKind::Krun {
            return Ok(OciOrphanCleanupDisposition::Retain);
        }
        let _lifecycle_guard =
            self.lock_launch_lifecycle_for(subject.tenant_id(), subject.sandbox_id())?;
        let mut manifest = self
            .read_exact_manifest(subject.tenant_id(), subject.sandbox_id())?
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "Krun orphan {} lost its exact manifest while the lifecycle lock was held",
                    subject.sandbox_id()
                ),
            })?;
        let network_config = self.authenticate_krun_orphan_subject(&manifest, subject)?;
        if subject.kind() == OciOrphanCleanupKind::TerminalPublication {
            if manifest.launch_authority != KrunLaunchAuthority::Released {
                manifest.launch_authority = KrunLaunchAuthority::Released;
                self.write_manifest(&manifest)?;
            }
            if subject.desired().is_none() || manifest.has_terminal_network_finality() {
                retire_terminal_container_ipam_release(
                    &self.ipam_authority,
                    &manifest.network_layout,
                    &manifest.handle.id,
                    &network_config.attachment_id,
                    &network_config.reservation_claim,
                    network_config.provider_kind(),
                )?;
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
                    "Krun orphan {} has an invalid exact segment cleanup state {:?}",
                    subject.sandbox_id(),
                    reservation.state()
                ),
            });
        }

        if subject.kind() == OciOrphanCleanupKind::NeverEffected {
            let ports = self.port_lease_coordinator();
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
                    manifest.launch_authority = KrunLaunchAuthority::Released;
                    self.write_manifest(&manifest)
                },
            )?;
            return Ok(OciOrphanCleanupDisposition::Converged);
        }

        let ports = self.port_lease_coordinator();
        let hostname = hostname_for(&manifest.spec);
        let lifecycle = self.attachment_lifecycle(&ports);
        let adapter = self.attachment_adapter(&manifest, &network_config, &hostname);
        adapter
            .detach_host_managed(&lifecycle, AttachmentTeardownMode::Final, |auxiliary| {
                match auxiliary {
                    AttachmentAuxiliaryDisposition::NoEffect => Ok(()),
                    AttachmentAuxiliaryDisposition::ProviderOwned => {
                        Err(SandboxError::OperationFailed {
                            message: format!(
                                "Krun orphan {} retains a live auxiliary provider",
                                subject.sandbox_id()
                            ),
                        })
                    }
                    AttachmentAuxiliaryDisposition::Unknown => Err(SandboxError::OperationFailed {
                        message: format!(
                            "Krun orphan {} has unknown auxiliary-provider authority",
                            subject.sandbox_id()
                        ),
                    }),
                }
            })
            .map_err(Into::<SandboxError>::into)?;

        manifest.launch_authority = KrunLaunchAuthority::Released;
        self.write_manifest(&manifest)?;
        Ok(OciOrphanCleanupDisposition::Converged)
    }
}
