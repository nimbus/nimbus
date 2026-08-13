//! Proof-gated final release choreography.

use nimbus_network::{
    NetworkAttachmentReservationState, NetworkResourcePhase, NetworkTransitionEvidence,
};

use super::super::*;
use super::progress::{
    HostManagedAttachmentDetachedEvidence, HostManagedAttachmentDetachedProof,
    HostManagedAttachmentEffectDisposition, HostManagedAttachmentReleasePhase,
    NeverEffectedIpamAuthority, NeverEffectedPortAuthority, NeverEffectedSegmentAuthority,
    RetainedAttachmentPublicationEvidence,
};
use super::retained_detach::{
    authenticate_exact_command_context, classify_never_effected_port_records,
    complete_port_plan_members, evidence_digest, never_effected_attachment_authority_digest,
    never_effected_stable_handle_digest, require_retained_records,
    retained_attachment_authority_digest,
};
use crate::SandboxNetworkTeardownCommand;
use crate::backends::oci::network::ipam::{
    ContainerIpamAuthorityState, inspect_container_ipam_authority,
    inspect_netavark_provider_operation,
};
use crate::backends::oci::network::netns::ExactRegularArtifactObservation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReleasePublicationComposition {
    HostManaged,
    MachineForwarded,
}

impl ReleasePublicationComposition {
    fn from_context(context: &OciAttachmentContext<'_>) -> Self {
        if context.publication.owns_netavark_bindings() {
            Self::HostManaged
        } else {
            Self::MachineForwarded
        }
    }

    fn is_machine_forwarded(self) -> bool {
        self == Self::MachineForwarded
    }

    fn evidence_for_resume(
        self,
        current_phase: HostManagedAttachmentReleasePhase,
        require_publication_absent: &mut impl FnMut() -> Result<RetainedAttachmentPublicationEvidence>,
    ) -> Result<Option<RetainedAttachmentPublicationEvidence>> {
        if self.is_machine_forwarded()
            || current_phase == HostManagedAttachmentReleasePhase::NotStarted
        {
            require_publication_absent().map(Some)
        } else {
            Ok(None)
        }
    }

    fn release_listener_authority(
        self,
        release_host_managed: impl FnOnce() -> Result<()>,
        release_machine_forwarded: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        match self {
            Self::HostManaged => release_host_managed(),
            Self::MachineForwarded => release_machine_forwarded(),
        }
    }
}

pub(crate) struct AttachmentReleaseActions<
    RequirePublicationAbsent,
    ReleaseListeners,
    ReleaseAuxiliary,
> {
    require_publication_absent: RequirePublicationAbsent,
    release_listeners: ReleaseListeners,
    release_auxiliary: ReleaseAuxiliary,
}

impl<RequirePublicationAbsent, ReleaseListeners, ReleaseAuxiliary>
    AttachmentReleaseActions<RequirePublicationAbsent, ReleaseListeners, ReleaseAuxiliary>
{
    pub(crate) fn new(
        require_publication_absent: RequirePublicationAbsent,
        release_listeners: ReleaseListeners,
        release_auxiliary: ReleaseAuxiliary,
    ) -> Self {
        Self {
            require_publication_absent,
            release_listeners,
            release_auxiliary,
        }
    }
}

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
            AttachmentReleaseActions::new(
                || Ok(RetainedAttachmentPublicationEvidence::HostManaged),
                || Ok(()),
                release_auxiliary,
            ),
        )
    }

    /// Release a machine-forwarded attachment after the retained detach proof
    /// and the same exact machine-publication absence are authenticated.
    pub(crate) fn release_machine_forwarded_detached(
        &self,
        lifecycle: &OciAttachmentLifecycle<'_>,
        command: &SandboxNetworkTeardownCommand,
        proof: &HostManagedAttachmentDetachedProof,
        current_phase: HostManagedAttachmentReleasePhase,
        record_phase: impl FnMut(HostManagedAttachmentReleasePhase) -> Result<()>,
        actions: AttachmentReleaseActions<
            impl FnMut() -> Result<RetainedAttachmentPublicationEvidence>,
            impl FnMut() -> Result<()>,
            impl FnMut() -> Result<()>,
        >,
    ) -> Result<()> {
        lifecycle.release_host_managed_detached(
            &self.context,
            command,
            proof,
            current_phase,
            record_phase,
            actions,
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
        actions: AttachmentReleaseActions<
            impl FnMut() -> Result<RetainedAttachmentPublicationEvidence>,
            impl FnMut() -> Result<()>,
            impl FnMut() -> Result<()>,
        >,
    ) -> Result<()> {
        let AttachmentReleaseActions {
            mut require_publication_absent,
            release_listeners,
            mut release_auxiliary,
        } = actions;
        let composition = ReleasePublicationComposition::from_context(context);
        let machine_forwarded = composition.is_machine_forwarded();
        authenticate_exact_command_context(context, command, machine_forwarded)?;
        proof.validate_release_command(command)?;
        let association = authority::authenticate_detach_association_with_fallback(
            self.attachments,
            self.allocator,
            context,
            AttachmentTeardownMode::Final,
            Some(proof.association()),
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

        let publication_evidence =
            composition.evidence_for_resume(current_phase, &mut require_publication_absent)?;
        if let Some(publication_evidence) = publication_evidence.as_ref() {
            if machine_forwarded
                != matches!(
                    publication_evidence,
                    RetainedAttachmentPublicationEvidence::MachineForwarded { .. }
                )
            {
                return Err(SandboxError::OperationFailed {
                    message: "ReleaseNetwork crossed its retained publication composition"
                        .to_owned(),
                });
            }
            proof.require_publication_evidence(publication_evidence)?;
        }
        if current_phase == HostManagedAttachmentReleasePhase::NotStarted {
            let publication_evidence = publication_evidence
                .as_ref()
                .expect("initial release always authenticates publication evidence");
            match proof.effect_disposition() {
                HostManagedAttachmentEffectDisposition::ProviderEffectMayHaveExisted => self
                    .require_exact_detached_evidence(
                        context,
                        proof,
                        &durable,
                        publication_evidence,
                    )?,
                HostManagedAttachmentEffectDisposition::ConfirmedNoProviderEffect => self
                    .require_exact_never_effected_evidence(
                        context,
                        proof,
                        &durable,
                        publication_evidence,
                    )?,
            }
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
                require_releaseable_port_records(
                    &pep,
                    context.launch_claim,
                    proof,
                    "PEP listener",
                )?;
                record_phase(HostManagedAttachmentReleasePhase::PepReleaseMayExist)?;
            }
            release_auxiliary()?;
            record_phase(HostManagedAttachmentReleasePhase::PepReleased)?;
        }

        if current_phase < HostManagedAttachmentReleasePhase::ListenersReleased {
            let listeners = self.ports.port_lease_plan_member_records_snapshot(
                &plan_members,
                context.leases,
                "detached published listener",
            )?;
            if current_phase < HostManagedAttachmentReleasePhase::ListenerReleaseMayExist {
                require_releaseable_port_records(
                    &listeners,
                    context.launch_claim,
                    proof,
                    "published listener",
                )?;
                record_phase(HostManagedAttachmentReleasePhase::ListenerReleaseMayExist)?;
            }
            composition.release_listener_authority(
                || self.release_host_managed_listeners(context, &plan_members),
                release_listeners,
            )?;
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
                let expected = proof.never_effected_authority().map(|state| state.ipam());
                let authenticated = match expected {
                    Some(expected) => never_effected_ipam_authority(ipam) == expected,
                    None => ipam == ContainerIpamAuthorityState::Live,
                };
                if !authenticated {
                    return Err(SandboxError::OperationFailed {
                        message: "ReleaseNetwork crossed its exact IPAM authority before release"
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
                    if current_phase >= HostManagedAttachmentReleasePhase::IpamReleaseMayExist
                        || proof.never_effected_authority().is_some_and(|state| {
                            state.ipam() == NeverEffectedIpamAuthority::Released
                        }) => {}
                ContainerIpamAuthorityState::Absent
                    if proof.never_effected_authority().is_some_and(|state| {
                        state.ipam() == NeverEffectedIpamAuthority::Absent
                    }) => {}
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
                let exact_association = match segment.state() {
                    NetworkAttachmentReservationState::ProviderCleanupPending
                    | NetworkAttachmentReservationState::ReservationCleanupPending => {
                        segment.association() == Some(proof.association())
                    }
                    NetworkAttachmentReservationState::Absent => segment.association().is_none(),
                    _ => false,
                };
                let expected = proof
                    .never_effected_authority()
                    .map(|state| state.segment());
                let authenticated = match expected {
                    Some(expected) => {
                        never_effected_segment_authority(segment.state()) == Some(expected)
                            && exact_association
                    }
                    None => {
                        segment.state() == NetworkAttachmentReservationState::ProviderCleanupPending
                            && exact_association
                    }
                };
                if !authenticated {
                    return Err(SandboxError::OperationFailed {
                        message:
                            "ReleaseNetwork crossed its exact segment authority before release"
                                .to_owned(),
                    });
                }
                record_phase(HostManagedAttachmentReleasePhase::SegmentReleaseMayExist)?;
            }
            let errors = match segment.state() {
                NetworkAttachmentReservationState::ProviderCleanupPending => {
                    release_network_segment_hold(
                        self.allocator,
                        context.tenant_id,
                        &context.config.attachment_id,
                        &context.config.reservation_claim,
                    )
                }
                NetworkAttachmentReservationState::ReservationCleanupPending => {
                    release_reserved_network_launch_after_ports(
                        ReservedNetworkLaunchAuthority::new(
                            self.allocator,
                            self.ipam,
                            ReservedNetworkLaunchIdentity::new(
                                context.layout,
                                context.tenant_id,
                                context.sandbox_id,
                                &context.config.attachment_id,
                                &context.config.reservation_claim,
                            ),
                            context.config.provider_kind(),
                        ),
                        Ok(()),
                    )
                    .err()
                    .into_iter()
                    .collect()
                }
                NetworkAttachmentReservationState::Absent
                    if current_phase
                        >= HostManagedAttachmentReleasePhase::SegmentReleaseMayExist
                        || proof.never_effected_authority().is_some_and(|state| {
                            state.segment() == NeverEffectedSegmentAuthority::Absent
                        }) =>
                {
                    Vec::new()
                }
                _ => {
                    return Err(SandboxError::OperationFailed {
                        message: "ReleaseNetwork found unauthenticated segment authority"
                            .to_owned(),
                    });
                }
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
        let attachment = durable.inspect()?;
        match (
            attachment
                .as_ref()
                .map(|attachment| attachment.resource().phase()),
            proof.effect_disposition(),
        ) {
            (
                Some(NetworkResourcePhase::Deleting | NetworkResourcePhase::CleanupPending),
                HostManagedAttachmentEffectDisposition::ProviderEffectMayHaveExisted,
            ) => {
                durable.transition(
                    attachment
                        .as_ref()
                        .expect("effectful release phase requires attachment authority"),
                    NetworkResourcePhase::Released,
                    NetworkTransitionEvidence::DeletionConfirmed,
                )?;
            }
            (
                Some(NetworkResourcePhase::Reserved),
                HostManagedAttachmentEffectDisposition::ConfirmedNoProviderEffect,
            ) => {
                durable.transition(
                    attachment
                        .as_ref()
                        .expect("reserved no-effect release requires attachment authority"),
                    NetworkResourcePhase::Released,
                    NetworkTransitionEvidence::ConfirmedNoEffect,
                )?;
            }
            (None, HostManagedAttachmentEffectDisposition::ConfirmedNoProviderEffect)
            | (Some(NetworkResourcePhase::Released), _) => {}
            (phase, _) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "ReleaseNetwork cannot publish terminal attachment from phase {phase:?}"
                    ),
                });
            }
        }
        record_phase(HostManagedAttachmentReleasePhase::Released)
    }

    fn release_host_managed_listeners(
        &self,
        context: &OciAttachmentContext<'_>,
        plan_members: &[nimbus_network::PortLeaseRequest],
    ) -> Result<()> {
        match self.ports.classify_planned_netavark_cleanup_batch(
            plan_members,
            context.tenant_id,
            context.bindings,
            context.leases,
            context.launch_claim,
        )? {
            LaunchPortBatchState::NeverBound => {
                let claim = context
                    .launch_claim
                    .ok_or_else(|| SandboxError::OperationFailed {
                        message: "never-bound detached listeners lost their launch claim"
                            .to_owned(),
                    })?;
                self.ports
                    .release_never_bound_plan_members(plan_members, context.leases, claim)?;
            }
            LaunchPortBatchState::RestartRetained => {
                self.ports
                    .release_planned_restart_retained_bindings(plan_members, context.leases)?;
            }
            LaunchPortBatchState::TerminalNoEffect => {}
            LaunchPortBatchState::ProviderOwned | LaunchPortBatchState::NetavarkClaimed(_) => {
                return Err(SandboxError::OperationFailed {
                    message: "ReleaseNetwork found live or ambiguous Netavark listener authority"
                        .to_owned(),
                });
            }
        }
        Ok(())
    }

    fn require_exact_detached_evidence(
        &self,
        context: &OciAttachmentContext<'_>,
        proof: &HostManagedAttachmentDetachedProof,
        durable: &state::OciAttachmentDurableState<'_>,
        publication_evidence: &RetainedAttachmentPublicationEvidence,
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
            publication_evidence,
        })
    }

    fn require_exact_never_effected_evidence(
        &self,
        context: &OciAttachmentContext<'_>,
        proof: &HostManagedAttachmentDetachedProof,
        durable: &state::OciAttachmentDurableState<'_>,
        publication_evidence: &RetainedAttachmentPublicationEvidence,
    ) -> Result<()> {
        let attachment = durable.inspect()?;
        if attachment.as_ref().is_some_and(|attachment| {
            attachment.resource().phase() != NetworkResourcePhase::Reserved
                || attachment.resource().provider_handle().is_some()
                || attachment.association() != proof.association()
                || attachment.selected_provider_id() != proof.selected_provider_id()
        }) {
            return Err(SandboxError::OperationFailed {
                message: "ReleaseNetwork crossed retained no-effect attachment authority"
                    .to_owned(),
            });
        }
        let expected =
            proof
                .never_effected_authority()
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: "ReleaseNetwork no-effect proof omitted semantic authority state"
                        .to_owned(),
                })?;
        let snapshot = self.inspect_never_effected_authority(context, proof.association())?;
        if snapshot.authority != expected {
            return Err(SandboxError::OperationFailed {
                message: "ReleaseNetwork no-effect authority changed after retained detach"
                    .to_owned(),
            });
        }
        proof.require_current_evidence(HostManagedAttachmentDetachedEvidence {
            stable_handle_sha256: &never_effected_stable_handle_digest(
                context,
                attachment.as_ref(),
            )?,
            provider_delete_evidence_sha256: &snapshot.provider_delete_evidence_sha256,
            namespace_absence_evidence_sha256: &evidence_digest(
                "namespace_absence",
                &(
                    "explicitly_absent",
                    &context.layout.netns_root,
                    &context.layout.netns_path,
                ),
            )?,
            pep_retained_evidence_sha256: &snapshot.pep_evidence_sha256,
            listener_retained_evidence_sha256: &snapshot.listener_evidence_sha256,
            ipam_retained_evidence_sha256: &snapshot.ipam_evidence_sha256,
            segment_quarantine_evidence_sha256: &snapshot.segment_evidence_sha256,
            attachment_retained_evidence_sha256: &never_effected_attachment_authority_digest(
                context,
                attachment.as_ref(),
            )?,
            publication_evidence,
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

fn require_releaseable_port_records(
    records: &[nimbus_network::PortLeaseRecord],
    launch_claim: Option<&nimbus_network::NetworkReservationClaim>,
    proof: &HostManagedAttachmentDetachedProof,
    label: &str,
) -> Result<()> {
    let Some(authority) = proof.never_effected_authority() else {
        return require_retained_records(records, launch_claim, label);
    };
    let observed = classify_never_effected_port_records(records, &[], launch_claim, label)?;
    if observed == NeverEffectedPortAuthority::NoMembers || observed == authority.ports() {
        Ok(())
    } else {
        Err(SandboxError::OperationFailed {
            message: format!(
                "{label} authority {observed:?} crossed detached no-effect state {:?}",
                authority.ports()
            ),
        })
    }
}

const fn never_effected_ipam_authority(
    state: ContainerIpamAuthorityState,
) -> NeverEffectedIpamAuthority {
    match state {
        ContainerIpamAuthorityState::Live => NeverEffectedIpamAuthority::Live,
        ContainerIpamAuthorityState::Released => NeverEffectedIpamAuthority::Released,
        ContainerIpamAuthorityState::Absent => NeverEffectedIpamAuthority::Absent,
    }
}

const fn never_effected_segment_authority(
    state: NetworkAttachmentReservationState,
) -> Option<NeverEffectedSegmentAuthority> {
    match state {
        NetworkAttachmentReservationState::ProviderCleanupPending => {
            Some(NeverEffectedSegmentAuthority::ProviderCleanupPending)
        }
        NetworkAttachmentReservationState::ReservationCleanupPending => {
            Some(NeverEffectedSegmentAuthority::ReservationCleanupPending)
        }
        NetworkAttachmentReservationState::Absent => Some(NeverEffectedSegmentAuthority::Absent),
        NetworkAttachmentReservationState::Reserved
        | NetworkAttachmentReservationState::Adopted => None,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn forwarded_container_attachment_teardown_release_never_dispatches_netavark_classifier() {
        let host_calls = Cell::new(0);
        let machine_calls = Cell::new(0);

        ReleasePublicationComposition::MachineForwarded
            .release_listener_authority(
                || {
                    host_calls.set(host_calls.get() + 1);
                    Err(SandboxError::OperationFailed {
                        message: "Netavark classifier must not run".to_owned(),
                    })
                },
                || {
                    machine_calls.set(machine_calls.get() + 1);
                    Ok(())
                },
            )
            .expect("forwarded release should use only machine listener authority");

        assert_eq!(host_calls.get(), 0);
        assert_eq!(machine_calls.get(), 1);
    }

    #[test]
    fn forwarded_container_attachment_teardown_release_reauthenticates_absence_after_reopen() {
        let calls = Cell::new(0);
        for phase in [
            HostManagedAttachmentReleasePhase::ReleaseAuthenticated,
            HostManagedAttachmentReleasePhase::PepReleaseMayExist,
            HostManagedAttachmentReleasePhase::PepReleased,
            HostManagedAttachmentReleasePhase::ListenerReleaseMayExist,
            HostManagedAttachmentReleasePhase::ListenersReleased,
            HostManagedAttachmentReleasePhase::IpamReleaseMayExist,
        ] {
            let evidence = ReleasePublicationComposition::MachineForwarded
                .evidence_for_resume(phase, &mut || {
                    calls.set(calls.get() + 1);
                    RetainedAttachmentPublicationEvidence::machine_forwarded("a".repeat(64))
                })
                .expect("reopened forwarded release should inspect exact publication absence");
            assert!(matches!(
                evidence,
                Some(RetainedAttachmentPublicationEvidence::MachineForwarded { .. })
            ));
        }
        assert_eq!(calls.get(), 6);
    }
}
