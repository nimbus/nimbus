//! Proof-gated final release choreography.

use nimbus_network::{
    NetworkAttachmentReservationState, NetworkResourcePhase, NetworkTransitionEvidence,
};

use super::super::*;
use super::progress::{
    HostManagedAttachmentDetachedEvidence, HostManagedAttachmentDetachedProof,
    HostManagedAttachmentReleasePhase,
};
use super::retained_detach::{
    authenticate_exact_command_context, complete_port_plan_members, evidence_digest,
    require_retained_records, retained_attachment_authority_digest,
};
use crate::SandboxNetworkTeardownCommand;
use crate::backends::oci::network::ipam::{
    ContainerIpamAuthorityState, inspect_container_ipam_authority,
    inspect_netavark_provider_operation,
};
use crate::backends::oci::network::netns::ExactRegularArtifactObservation;

impl OciAttachmentAdapter<'_> {
    /// Release every authority retained by one exact detached proof.
    pub(crate) fn release_host_managed_detached(
        &self,
        lifecycle: &OciAttachmentLifecycle<'_>,
        command: &SandboxNetworkTeardownCommand,
        proof: &HostManagedAttachmentDetachedProof,
        current_phase: HostManagedAttachmentReleasePhase,
        record_phase: impl FnMut(HostManagedAttachmentReleasePhase) -> Result<()>,
        release_auxiliary: impl FnMut() -> Result<()>,
    ) -> Result<()> {
        lifecycle.release_host_managed_detached(
            &self.context,
            command,
            proof,
            current_phase,
            record_phase,
            release_auxiliary,
        )
    }
}

impl OciAttachmentLifecycle<'_> {
    fn release_host_managed_detached(
        &self,
        context: &OciAttachmentContext<'_>,
        command: &SandboxNetworkTeardownCommand,
        proof: &HostManagedAttachmentDetachedProof,
        current_phase: HostManagedAttachmentReleasePhase,
        mut record_phase: impl FnMut(HostManagedAttachmentReleasePhase) -> Result<()>,
        mut release_auxiliary: impl FnMut() -> Result<()>,
    ) -> Result<()> {
        authenticate_exact_command_context(context, command)?;
        proof.validate_release_command(command)?;
        let association = authority::authenticate_detach_association(
            self.attachments,
            self.allocator,
            context,
            AttachmentTeardownMode::Final,
        )?;
        if &association != proof.association() || association.lease_epoch() != proof.lease_epoch() {
            return Err(SandboxError::OperationFailed {
                message: "ReleaseNetwork crossed the exact detached segment association".to_owned(),
            });
        }
        let durable =
            state::OciAttachmentDurableState::compile(self.attachments, context, association)?;
        let plan_members = complete_port_plan_members(context);
        self.require_exact_detached_absence(context)?;

        if current_phase == HostManagedAttachmentReleasePhase::NotStarted {
            self.require_exact_detached_evidence(context, proof, &durable)?;
        }
        record_phase(HostManagedAttachmentReleasePhase::ReleaseAuthenticated)?;

        if current_phase < HostManagedAttachmentReleasePhase::PepReleased {
            let pep_requests = context
                .auxiliary_listener
                .map(OciAttachmentAuxiliaryListener::request)
                .map(std::slice::from_ref)
                .unwrap_or_default();
            let pep = self.ports.port_lease_plan_member_records_snapshot(
                &plan_members,
                pep_requests,
                "detached PEP listener",
            )?;
            if current_phase < HostManagedAttachmentReleasePhase::PepReleaseMayExist {
                require_retained_records(&pep, context.launch_claim, "PEP listener")?;
                record_phase(HostManagedAttachmentReleasePhase::PepReleaseMayExist)?;
            }
            release_auxiliary()?;
            record_phase(HostManagedAttachmentReleasePhase::PepReleased)?;
        }

        if current_phase < HostManagedAttachmentReleasePhase::ListenersReleased {
            let listeners = self.ports.port_lease_plan_member_records_snapshot(
                &plan_members,
                context.leases,
                "detached Netavark listener",
            )?;
            if current_phase < HostManagedAttachmentReleasePhase::ListenerReleaseMayExist {
                require_retained_records(&listeners, context.launch_claim, "published listener")?;
                record_phase(HostManagedAttachmentReleasePhase::ListenerReleaseMayExist)?;
            }
            match self.ports.classify_planned_netavark_cleanup_batch(
                &plan_members,
                context.tenant_id,
                context.bindings,
                context.leases,
                context.launch_claim,
            )? {
                LaunchPortBatchState::NeverBound => {
                    let claim =
                        context
                            .launch_claim
                            .ok_or_else(|| SandboxError::OperationFailed {
                                message: "never-bound detached listeners lost their launch claim"
                                    .to_owned(),
                            })?;
                    self.ports.release_never_bound_plan_members(
                        &plan_members,
                        context.leases,
                        claim,
                    )?;
                }
                LaunchPortBatchState::RestartRetained => {
                    self.ports
                        .release_planned_restart_retained_bindings(&plan_members, context.leases)?;
                }
                LaunchPortBatchState::TerminalNoEffect => {}
                LaunchPortBatchState::ProviderOwned | LaunchPortBatchState::NetavarkClaimed(_) => {
                    return Err(SandboxError::OperationFailed {
                        message:
                            "ReleaseNetwork found live or ambiguous Netavark listener authority"
                                .to_owned(),
                    });
                }
            }
            record_phase(HostManagedAttachmentReleasePhase::ListenersReleased)?;
        }

        if current_phase < HostManagedAttachmentReleasePhase::IpamReleased {
            let ipam = inspect_container_ipam_authority(
                self.ipam,
                context.layout,
                context.config,
                context.sandbox_id,
            )?;
            if current_phase < HostManagedAttachmentReleasePhase::IpamReleaseMayExist {
                if ipam != ContainerIpamAuthorityState::Live {
                    return Err(SandboxError::OperationFailed {
                        message: "ReleaseNetwork requires live exact IPAM authority before release"
                            .to_owned(),
                    });
                }
                record_phase(HostManagedAttachmentReleasePhase::IpamReleaseMayExist)?;
            }
            match ipam {
                ContainerIpamAuthorityState::Live => {
                    deallocate_container_ips_after_confirmed_detach(
                        self.ipam,
                        context.layout,
                        context.sandbox_id,
                        &context.config.attachment_id,
                        &context.config.reservation_claim,
                        context.config.provider_kind(),
                    )?;
                }
                ContainerIpamAuthorityState::Released
                    if current_phase >= HostManagedAttachmentReleasePhase::IpamReleaseMayExist => {}
                ContainerIpamAuthorityState::Released | ContainerIpamAuthorityState::Absent => {
                    return Err(SandboxError::OperationFailed {
                        message: "ReleaseNetwork found unauthenticated IPAM absence".to_owned(),
                    });
                }
            }
            record_phase(HostManagedAttachmentReleasePhase::IpamReleased)?;
        }

        if current_phase < HostManagedAttachmentReleasePhase::SegmentReleased {
            let segment = self.allocator.inspect_attachment_reservation(
                context.tenant_id,
                &context.config.attachment_id,
                &context.config.reservation_claim,
            )?;
            if current_phase < HostManagedAttachmentReleasePhase::SegmentReleaseMayExist {
                if segment.state() != NetworkAttachmentReservationState::ProviderCleanupPending
                    || segment.association() != Some(proof.association())
                {
                    return Err(SandboxError::OperationFailed {
                        message: "ReleaseNetwork requires the exact quarantined segment hold"
                            .to_owned(),
                    });
                }
                record_phase(HostManagedAttachmentReleasePhase::SegmentReleaseMayExist)?;
            }
            let errors =
                if segment.state() == NetworkAttachmentReservationState::ProviderCleanupPending {
                    release_network_segment_hold(
                        self.allocator,
                        context.tenant_id,
                        &context.config.attachment_id,
                        &context.config.reservation_claim,
                    )
                } else if segment.state() == NetworkAttachmentReservationState::Absent
                    && current_phase >= HostManagedAttachmentReleasePhase::SegmentReleaseMayExist
                {
                    Vec::new()
                } else {
                    return Err(SandboxError::OperationFailed {
                        message: "ReleaseNetwork found unauthenticated segment absence".to_owned(),
                    });
                };
            if !errors.is_empty() {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "ReleaseNetwork could not settle the exact segment hold: {}",
                        errors
                            .into_iter()
                            .map(|error| error.to_string())
                            .collect::<Vec<_>>()
                            .join("; ")
                    ),
                });
            }
            record_phase(HostManagedAttachmentReleasePhase::SegmentReleased)?;
        }

        record_phase(HostManagedAttachmentReleasePhase::AttachmentReleaseMayExist)?;
        let attachment = durable
            .inspect()?
            .ok_or_else(|| SandboxError::OperationFailed {
                message: "ReleaseNetwork lost portable attachment authority".to_owned(),
            })?;
        match attachment.resource().phase() {
            NetworkResourcePhase::Deleting | NetworkResourcePhase::CleanupPending => {
                durable.transition(
                    &attachment,
                    NetworkResourcePhase::Released,
                    NetworkTransitionEvidence::DeletionConfirmed,
                )?;
            }
            NetworkResourcePhase::Released => {}
            phase => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "ReleaseNetwork cannot publish terminal attachment from phase {phase:?}"
                    ),
                });
            }
        }
        record_phase(HostManagedAttachmentReleasePhase::Released)
    }

    fn require_exact_detached_evidence(
        &self,
        context: &OciAttachmentContext<'_>,
        proof: &HostManagedAttachmentDetachedProof,
        durable: &state::OciAttachmentDurableState<'_>,
    ) -> Result<()> {
        let attachment = durable
            .inspect()?
            .ok_or_else(|| SandboxError::OperationFailed {
                message: "ReleaseNetwork lost retained attachment authority".to_owned(),
            })?;
        if !matches!(
            attachment.resource().phase(),
            NetworkResourcePhase::Deleting | NetworkResourcePhase::CleanupPending
        ) || attachment.association() != proof.association()
            || attachment.selected_provider_id() != proof.selected_provider_id()
        {
            return Err(SandboxError::OperationFailed {
                message: "ReleaseNetwork crossed retained portable attachment authority".to_owned(),
            });
        }
        durable.authenticate_stable_handle(&attachment)?;
        let handle = attachment.resource().provider_handle().ok_or_else(|| {
            SandboxError::OperationFailed {
                message: "ReleaseNetwork attachment omitted its stable provider handle".to_owned(),
            }
        })?;
        let provider_operation = inspect_netavark_provider_operation(
            self.ipam,
            context.layout,
            context.config,
            context.sandbox_id,
        )?;
        if provider_operation.label() != "detached"
            || inspect_container_ipam_authority(
                self.ipam,
                context.layout,
                context.config,
                context.sandbox_id,
            )? != ContainerIpamAuthorityState::Live
        {
            return Err(SandboxError::OperationFailed {
                message: "ReleaseNetwork requires detached provider evidence and live IPAM"
                    .to_owned(),
            });
        }
        let segment = self.allocator.inspect_attachment_reservation(
            context.tenant_id,
            &context.config.attachment_id,
            &context.config.reservation_claim,
        )?;
        if segment.state() != NetworkAttachmentReservationState::ProviderCleanupPending
            || segment.association() != Some(proof.association())
        {
            return Err(SandboxError::OperationFailed {
                message: "ReleaseNetwork requires exact quarantined segment evidence".to_owned(),
            });
        }
        let (listeners, pep) = self.retained_port_plan_snapshot(context)?;
        require_retained_records(&listeners, context.launch_claim, "published listener")?;
        require_retained_records(&pep, context.launch_claim, "PEP listener")?;
        proof.require_current_evidence(HostManagedAttachmentDetachedEvidence {
            stable_handle_sha256: &evidence_digest("stable_handle", handle)?,
            provider_delete_evidence_sha256: &evidence_digest(
                "provider_delete",
                &provider_operation,
            )?,
            namespace_absence_evidence_sha256: &evidence_digest(
                "namespace_absence",
                &(
                    "explicitly_absent",
                    &context.layout.netns_root,
                    &context.layout.netns_path,
                ),
            )?,
            pep_retained_evidence_sha256: &evidence_digest("pep_retained", &pep)?,
            listener_retained_evidence_sha256: &evidence_digest("listeners_retained", &listeners)?,
            ipam_retained_evidence_sha256: &evidence_digest(
                "ipam_retained",
                &(
                    &context.config.attachment_id,
                    &context.config.reservation_claim,
                    &context.config.segment_id,
                    provider_operation.label(),
                ),
            )?,
            segment_quarantine_evidence_sha256: &evidence_digest(
                "segment_quarantined",
                &(segment.state() as u8, segment.association()),
            )?,
            attachment_retained_evidence_sha256: &retained_attachment_authority_digest(
                &attachment,
            )?,
        })
    }

    fn require_exact_detached_absence(&self, context: &OciAttachmentContext<'_>) -> Result<()> {
        if recovery::inspect_provider(self.ipam, context)
            != recovery::AttachmentProviderObservation::Absent
        {
            return Err(SandboxError::OperationFailed {
                message: "ReleaseNetwork requires exact provider and namespace absence".to_owned(),
            });
        }
        if recovery::inspect_namespace(context).map_err(|reason| SandboxError::OperationFailed {
            message: format!("ReleaseNetwork cannot prove exact namespace absence: {reason}"),
        })? != ExactRegularArtifactObservation::ExplicitlyAbsent
        {
            return Err(SandboxError::OperationFailed {
                message: "ReleaseNetwork requires explicit exact namespace absence".to_owned(),
            });
        }
        Ok(())
    }
}
