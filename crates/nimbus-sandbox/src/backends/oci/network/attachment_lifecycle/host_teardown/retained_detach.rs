//! Exact retained detach choreography.

use nimbus_network::{
    NetworkAttachmentReservationState, NetworkAttachmentSegmentAssociation, NetworkResourcePhase,
    NetworkSegmentQuarantineOutcome, PortLeasePhase,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::super::*;
use super::progress::{
    HostManagedAttachmentDetachPhase, HostManagedAttachmentDetachedProof,
    HostManagedAttachmentDetachedProofInput, HostManagedAttachmentEffectDisposition,
    NeverEffectedAttachmentAuthority, NeverEffectedIpamAuthority, NeverEffectedPortAuthority,
    NeverEffectedSegmentAuthority, RetainedAttachmentPublicationEvidence,
};
use crate::SandboxNetworkTeardownCommand;
use crate::backends::capabilities::host_managed_attachment_provider_id;
use crate::backends::oci::network::ipam::{
    ContainerIpamAuthorityState, inspect_container_ipam_authority,
    inspect_netavark_provider_operation,
};
use crate::backends::oci::network::netns::ExactRegularArtifactObservation;

const EVIDENCE_DIGEST_DOMAIN: &[u8] = b"nimbus.sandbox.host-attachment-evidence.v1\0";

struct RetainedDetachActions<RetainPublication, StopAuxiliary> {
    retain_publication: RetainPublication,
    stop_auxiliary: StopAuxiliary,
}

pub(super) struct NeverEffectedAuthoritySnapshot {
    pub(super) authority: NeverEffectedAttachmentAuthority,
    pub(super) provider_delete_evidence_sha256: String,
    pub(super) pep_evidence_sha256: String,
    pub(super) listener_evidence_sha256: String,
    pub(super) ipam_evidence_sha256: String,
    pub(super) segment_evidence_sha256: String,
}

impl OciAttachmentAdapter<'_> {
    /// Detach provider effects while every reusable authority stays retained.
    pub(crate) fn detach_host_managed_retained(
        &self,
        lifecycle: &OciAttachmentLifecycle<'_>,
        command: &SandboxNetworkTeardownCommand,
        current_phase: HostManagedAttachmentDetachPhase,
        record_phase: impl FnMut(HostManagedAttachmentDetachPhase) -> Result<()>,
        stop_auxiliary: impl FnOnce(AttachmentAuxiliaryDisposition) -> Result<()>,
    ) -> Result<HostManagedAttachmentDetachedProof> {
        require_publication_mode(
            &self.context,
            self.context.publication.owns_netavark_bindings(),
            "host-managed",
        )?;
        lifecycle.detach_retained(
            &self.context,
            command,
            &RealAttachmentHostEffects,
            current_phase,
            record_phase,
            RetainedDetachActions {
                retain_publication: || Ok(RetainedAttachmentPublicationEvidence::HostManaged),
                stop_auxiliary,
            },
        )
    }

    /// Detach a private attachment whose separately owned publication has
    /// already reached exact terminal port-lease state.
    pub(crate) fn detach_deferred_retained(
        &self,
        lifecycle: &OciAttachmentLifecycle<'_>,
        command: &SandboxNetworkTeardownCommand,
        current_phase: HostManagedAttachmentDetachPhase,
        record_phase: impl FnMut(HostManagedAttachmentDetachPhase) -> Result<()>,
        stop_auxiliary: impl FnOnce(AttachmentAuxiliaryDisposition) -> Result<()>,
    ) -> Result<HostManagedAttachmentDetachedProof> {
        require_publication_mode(
            &self.context,
            self.context.publication.is_deferred(),
            "deferred",
        )?;
        lifecycle.detach_retained(
            &self.context,
            command,
            &RealAttachmentHostEffects,
            current_phase,
            record_phase,
            RetainedDetachActions {
                retain_publication: || {
                    Err(SandboxError::OperationFailed {
                        message: "deferred publication must use durable terminal lease evidence"
                            .to_owned(),
                    })
                },
                stop_auxiliary,
            },
        )
    }

    /// Confirm no private attachment effect while separately owned publication
    /// is already terminal under its own durable lease authority.
    pub(crate) fn detach_deferred_never_effected_retained(
        &self,
        lifecycle: &OciAttachmentLifecycle<'_>,
        command: &SandboxNetworkTeardownCommand,
        absent_association: Option<&NetworkAttachmentSegmentAssociation>,
    ) -> Result<HostManagedAttachmentDetachedProof> {
        require_publication_mode(
            &self.context,
            self.context.publication.is_deferred(),
            "deferred",
        )?;
        lifecycle.detach_never_effected_retained(&self.context, command, absent_association)
    }

    /// Detach a machine-forwarded private attachment after its publication
    /// owner proves exact durable absence and retained listener authority.
    pub(crate) fn detach_machine_forwarded_retained(
        &self,
        lifecycle: &OciAttachmentLifecycle<'_>,
        command: &SandboxNetworkTeardownCommand,
        current_phase: HostManagedAttachmentDetachPhase,
        record_phase: impl FnMut(HostManagedAttachmentDetachPhase) -> Result<()>,
        retain_publication: impl FnOnce() -> Result<RetainedAttachmentPublicationEvidence>,
        stop_auxiliary: impl FnOnce(AttachmentAuxiliaryDisposition) -> Result<()>,
    ) -> Result<HostManagedAttachmentDetachedProof> {
        require_publication_mode(
            &self.context,
            self.context.publication.is_machine_forwarded(),
            "machine-forwarded",
        )?;
        lifecycle.detach_retained(
            &self.context,
            command,
            &RealAttachmentHostEffects,
            current_phase,
            record_phase,
            RetainedDetachActions {
                retain_publication,
                stop_auxiliary,
            },
        )
    }
}

impl OciAttachmentLifecycle<'_> {
    fn detach_never_effected_retained(
        &self,
        context: &OciAttachmentContext<'_>,
        command: &SandboxNetworkTeardownCommand,
        absent_association: Option<&NetworkAttachmentSegmentAssociation>,
    ) -> Result<HostManagedAttachmentDetachedProof> {
        authenticate_exact_command_context(context, command)?;
        let association = authority::authenticate_detach_association_with_fallback(
            self.attachments,
            self.allocator,
            context,
            AttachmentTeardownMode::Final,
            absent_association,
        )?;
        let durable = state::OciAttachmentDurableState::compile(
            self.attachments,
            context,
            association.clone(),
        )?;
        let attachment = durable.inspect()?;
        if attachment.as_ref().is_some_and(|attachment| {
            attachment.resource().phase() != NetworkResourcePhase::Reserved
                || attachment.resource().provider_handle().is_some()
        }) {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "{} attachment {} cannot claim no-effect detach from portable phase {:?}",
                    context.provider_label,
                    context.sandbox_id,
                    attachment
                        .as_ref()
                        .map(|attachment| attachment.resource().phase())
                ),
            });
        }
        let segment = self.allocator.inspect_attachment_reservation(
            context.tenant_id,
            &context.config.attachment_id,
            &context.config.reservation_claim,
        )?;
        match segment.state() {
            NetworkAttachmentReservationState::Adopted => {
                match quarantine_network_segment_hold(
                    self.allocator,
                    context.tenant_id,
                    &context.config.attachment_id,
                    &context.config.reservation_claim,
                )? {
                    NetworkSegmentQuarantineOutcome::CleanupPending => {}
                    NetworkSegmentQuarantineOutcome::AlreadyReleased => {
                        return Err(SandboxError::OperationFailed {
                            message: format!(
                                "{} attachment {} lost its segment hold before no-effect detach",
                                context.provider_label, context.sandbox_id
                            ),
                        });
                    }
                }
            }
            NetworkAttachmentReservationState::ProviderCleanupPending
            | NetworkAttachmentReservationState::ReservationCleanupPending
            | NetworkAttachmentReservationState::Absent => {}
            state => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "{} attachment {} cannot publish no-effect detach from segment state {state:?}",
                        context.provider_label, context.sandbox_id
                    ),
                });
            }
        }
        let publication_evidence = self.retained_publication_evidence(context, || {
            Err(SandboxError::OperationFailed {
                message: "machine-forwarded no-effect detach requires explicit publication absence"
                    .to_owned(),
            })
        })?;
        let snapshot = self.inspect_never_effected_authority(context, &association)?;

        HostManagedAttachmentDetachedProof::new(HostManagedAttachmentDetachedProofInput {
            command: command.clone(),
            association,
            selected_provider_id: attachment
                .as_ref()
                .map(|attachment| attachment.selected_provider_id().clone())
                .unwrap_or_else(|| command.provider_id()),
            effect_disposition: HostManagedAttachmentEffectDisposition::ConfirmedNoProviderEffect,
            never_effected_authority: Some(snapshot.authority),
            stable_handle_sha256: never_effected_stable_handle_digest(
                context,
                attachment.as_ref(),
            )?,
            provider_delete_evidence_sha256: snapshot.provider_delete_evidence_sha256,
            namespace_absence_evidence_sha256: evidence_digest(
                "namespace_absence",
                &(
                    "explicitly_absent",
                    &context.layout.netns_root,
                    &context.layout.netns_path,
                ),
            )?,
            pep_retained_evidence_sha256: snapshot.pep_evidence_sha256,
            listener_retained_evidence_sha256: snapshot.listener_evidence_sha256,
            ipam_retained_evidence_sha256: snapshot.ipam_evidence_sha256,
            segment_quarantine_evidence_sha256: snapshot.segment_evidence_sha256,
            attachment_retained_evidence_sha256: never_effected_attachment_authority_digest(
                context,
                attachment.as_ref(),
            )?,
            publication_evidence,
        })
    }

    fn detach_retained(
        &self,
        context: &OciAttachmentContext<'_>,
        command: &SandboxNetworkTeardownCommand,
        host: &impl AttachmentHostEffects,
        current_phase: HostManagedAttachmentDetachPhase,
        mut record_phase: impl FnMut(HostManagedAttachmentDetachPhase) -> Result<()>,
        actions: RetainedDetachActions<
            impl FnOnce() -> Result<RetainedAttachmentPublicationEvidence>,
            impl FnOnce(AttachmentAuxiliaryDisposition) -> Result<()>,
        >,
    ) -> Result<HostManagedAttachmentDetachedProof> {
        let RetainedDetachActions {
            retain_publication,
            stop_auxiliary,
        } = actions;
        let host_managed_publication = context.publication.owns_netavark_bindings();
        authenticate_exact_command_context(context, command)?;
        authenticate_container_network_generation_for_cleanup(
            self.ipam,
            context.layout,
            context.config,
            context.sandbox_id,
        )?;
        let association = authority::authenticate_detach_association(
            self.attachments,
            self.allocator,
            context,
            AttachmentTeardownMode::Final,
        )?;
        let durable =
            state::OciAttachmentDurableState::compile(self.attachments, context, association)?;
        let durable_record = durable
            .inspect()?
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "{} attachment {} has no durable portable authority for retained detach",
                    context.provider_label, context.sandbox_id
                ),
            })?;
        if durable_record.resource().phase().is_terminal() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "{} attachment {} cannot detach retained authority from terminal phase {:?}",
                    context.provider_label,
                    context.sandbox_id,
                    durable_record.resource().phase()
                ),
            });
        }

        let provider_observation = host.inspect_provider(self.ipam, context);
        let detach =
            recovery::prepare_retained_detach(&durable, durable_record, provider_observation)?;
        let durable_record = detach.record;
        let plan_members = complete_port_plan_members(context);
        if current_phase < HostManagedAttachmentDetachPhase::AttachmentDeleting {
            record_phase(HostManagedAttachmentDetachPhase::AttachmentDeleting)?;
        }

        if current_phase < HostManagedAttachmentDetachPhase::SegmentQuarantined {
            match quarantine_network_segment_hold(
                self.allocator,
                context.tenant_id,
                &context.config.attachment_id,
                &context.config.reservation_claim,
            )? {
                NetworkSegmentQuarantineOutcome::CleanupPending => {}
                NetworkSegmentQuarantineOutcome::AlreadyReleased => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "{} attachment {} lost its segment hold before retained detach",
                            context.provider_label, context.sandbox_id
                        ),
                    });
                }
            }
            record_phase(HostManagedAttachmentDetachPhase::SegmentQuarantined)?;
        }

        if current_phase < HostManagedAttachmentDetachPhase::PepRetained {
            if current_phase < HostManagedAttachmentDetachPhase::PepStopMayExist {
                record_phase(HostManagedAttachmentDetachPhase::PepStopMayExist)?;
            }
            stop_auxiliary(if context.auxiliary_listener.is_some() {
                AttachmentAuxiliaryDisposition::ProviderOwned
            } else {
                AttachmentAuxiliaryDisposition::NoEffect
            })?;
            record_phase(HostManagedAttachmentDetachPhase::PepRetained)?;
        }

        let published_batch_state = (host_managed_publication
            && current_phase < HostManagedAttachmentDetachPhase::ListenersRetained)
            .then(|| {
                self.ports.classify_planned_netavark_cleanup_batch(
                    &plan_members,
                    context.tenant_id,
                    context.bindings,
                    context.leases,
                    context.launch_claim,
                )
            })
            .transpose()?;
        let mut cleanup = None;
        let mut recoveries = None;
        if current_phase < HostManagedAttachmentDetachPhase::ListenerStopMayExist {
            record_phase(HostManagedAttachmentDetachPhase::ListenerStopMayExist)?;
        }
        match published_batch_state.as_ref() {
            None => {}
            Some(LaunchPortBatchState::ProviderOwned) => {
                cleanup = self.ports.begin_planned_netavark_cleanup(
                    self.lifetimes,
                    &plan_members,
                    context.tenant_id,
                    context.sandbox_id,
                    context.bindings,
                    context.leases,
                )?;
            }
            Some(LaunchPortBatchState::NetavarkClaimed(claims)) => {
                recoveries = Some(
                    self.ports
                        .recover_planned_netavark_claims_after_owner_death(
                            &plan_members,
                            context.tenant_id,
                            context.bindings,
                            context.leases,
                            claims,
                        )?,
                );
            }
            Some(
                LaunchPortBatchState::NeverBound
                | LaunchPortBatchState::RestartRetained
                | LaunchPortBatchState::TerminalNoEffect,
            ) => {}
        }
        let publication_evidence =
            self.retained_publication_evidence(context, retain_publication)?;

        if current_phase < HostManagedAttachmentDetachPhase::ProviderAbsent {
            let prepared_teardown = if detach.provider_absent {
                None
            } else {
                Some(host.prepare_provider_teardown(self.ipam, context)?)
            };
            if current_phase < HostManagedAttachmentDetachPhase::ProviderDeleteMayExist {
                record_phase(HostManagedAttachmentDetachPhase::ProviderDeleteMayExist)?;
            }
            if let Some(prepared_teardown) = prepared_teardown
                && let Err(effect_error) =
                    host.teardown_provider(self.ipam, context, prepared_teardown)
                && !matches!(
                    host.inspect_provider(self.ipam, context),
                    recovery::AttachmentProviderObservation::Absent
                        | recovery::AttachmentProviderObservation::DetachedNamespacePending
                )
            {
                if host_managed_publication {
                    self.ports.retain_ambiguous_netavark_cleanup(
                        self.lifetimes,
                        context.tenant_id,
                        context.sandbox_id,
                        cleanup.take(),
                    )?;
                }
                return Err(effect_error);
            }
            let after_provider = host.inspect_provider(self.ipam, context);
            if !matches!(
                after_provider,
                recovery::AttachmentProviderObservation::Absent
                    | recovery::AttachmentProviderObservation::DetachedNamespacePending
            ) {
                if host_managed_publication {
                    self.ports.retain_ambiguous_netavark_cleanup(
                        self.lifetimes,
                        context.tenant_id,
                        context.sandbox_id,
                        cleanup.take(),
                    )?;
                }
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "{} attachment {} lacks exact provider absence after detach: {after_provider:?}",
                        context.provider_label, context.sandbox_id
                    ),
                });
            }
            record_phase(HostManagedAttachmentDetachPhase::ProviderAbsent)?;
        }

        if current_phase < HostManagedAttachmentDetachPhase::NamespaceAbsent {
            if current_phase < HostManagedAttachmentDetachPhase::NamespaceRemoveMayExist {
                record_phase(HostManagedAttachmentDetachPhase::NamespaceRemoveMayExist)?;
            }
            let before_namespace = host.inspect_provider(self.ipam, context);
            if matches!(
                before_namespace,
                recovery::AttachmentProviderObservation::DetachedNamespacePending
            ) && let Err(error) = host.remove_namespace(context)
            {
                if host_managed_publication {
                    self.ports.retain_ambiguous_netavark_cleanup(
                        self.lifetimes,
                        context.tenant_id,
                        context.sandbox_id,
                        cleanup.take(),
                    )?;
                }
                return Err(error);
            }
            let after_namespace = host.inspect_provider(self.ipam, context);
            if after_namespace != recovery::AttachmentProviderObservation::Absent {
                if host_managed_publication {
                    self.ports.retain_ambiguous_netavark_cleanup(
                        self.lifetimes,
                        context.tenant_id,
                        context.sandbox_id,
                        cleanup.take(),
                    )?;
                }
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "{} attachment {} lacks explicit provider and namespace absence: {after_namespace:?}",
                        context.provider_label, context.sandbox_id
                    ),
                });
            }
            record_phase(HostManagedAttachmentDetachPhase::NamespaceAbsent)?;
        }

        match published_batch_state {
            Some(LaunchPortBatchState::ProviderOwned) => {
                self.ports.complete_planned_netavark_detach(
                    &plan_members,
                    context.leases,
                    cleanup.as_ref(),
                )?;
            }
            Some(LaunchPortBatchState::NetavarkClaimed(_)) => {
                self.ports
                    .prepare_recovered_planned_netavark_claims_for_rebind(
                        &plan_members,
                        context.leases,
                        recoveries
                            .as_deref()
                            .ok_or_else(|| SandboxError::OperationFailed {
                                message:
                                    "recovered Netavark cleanup lost its exact recovery guards"
                                        .to_owned(),
                            })?,
                    )?;
            }
            Some(
                LaunchPortBatchState::NeverBound
                | LaunchPortBatchState::RestartRetained
                | LaunchPortBatchState::TerminalNoEffect,
            )
            | None => {}
        }
        if current_phase < HostManagedAttachmentDetachPhase::ListenersRetained {
            record_phase(HostManagedAttachmentDetachPhase::ListenersRetained)?;
        }

        let proof = self.compile_detached_proof(
            context,
            command,
            &durable,
            &durable_record,
            publication_evidence,
        )?;
        proof.validate_detach_command(command)?;
        Ok(proof)
    }

    fn compile_detached_proof(
        &self,
        context: &OciAttachmentContext<'_>,
        command: &SandboxNetworkTeardownCommand,
        durable: &state::OciAttachmentDurableState<'_>,
        expected_record: &nimbus_network::DurableNetworkAttachmentState,
        publication_evidence: RetainedAttachmentPublicationEvidence,
    ) -> Result<HostManagedAttachmentDetachedProof> {
        let attachment = durable
            .inspect()?
            .ok_or_else(|| SandboxError::OperationFailed {
                message: "retained detach lost portable attachment authority".to_owned(),
            })?;
        if attachment != *expected_record
            || attachment.resource().phase() != NetworkResourcePhase::Deleting
        {
            return Err(SandboxError::OperationFailed {
                message: "retained detach crossed the exact portable Deleting authority".to_owned(),
            });
        }
        durable.authenticate_stable_handle(&attachment)?;
        let handle = attachment.resource().provider_handle().ok_or_else(|| {
            SandboxError::OperationFailed {
                message: "retained detach attachment omitted its stable provider handle".to_owned(),
            }
        })?;
        let segment = self.allocator.inspect_attachment_reservation(
            context.tenant_id,
            &context.config.attachment_id,
            &context.config.reservation_claim,
        )?;
        if segment.state() != NetworkAttachmentReservationState::ProviderCleanupPending
            || segment.association() != Some(attachment.association())
        {
            return Err(SandboxError::OperationFailed {
                message: "retained detach segment authority is not exact and quarantined"
                    .to_owned(),
            });
        }
        if inspect_container_ipam_authority(
            self.ipam,
            context.layout,
            context.config,
            context.sandbox_id,
        )? != ContainerIpamAuthorityState::Live
        {
            return Err(SandboxError::OperationFailed {
                message: "retained detach requires live exact IPAM authority".to_owned(),
            });
        }
        let provider_operation = inspect_netavark_provider_operation(
            self.ipam,
            context.layout,
            context.config,
            context.sandbox_id,
        )?;
        if provider_operation.label() != "detached" {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "retained detach requires exact detached provider evidence, got {}",
                    provider_operation.label()
                ),
            });
        }
        if recovery::inspect_namespace(context).map_err(|reason| SandboxError::OperationFailed {
            message: format!("retained detach cannot prove exact namespace absence: {reason}"),
        })? != ExactRegularArtifactObservation::ExplicitlyAbsent
        {
            return Err(SandboxError::OperationFailed {
                message: "retained detach requires explicit exact namespace absence".to_owned(),
            });
        }
        let (listeners, pep) = self.retained_port_plan_snapshot(context)?;
        match context.publication {
            AttachmentPublicationMode::Deferred => {
                let current = self.separate_owner_publication_evidence(context)?;
                if current != publication_evidence {
                    return Err(SandboxError::OperationFailed {
                        message:
                            "deferred publication terminal evidence changed during retained detach"
                                .to_owned(),
                    });
                }
            }
            AttachmentPublicationMode::HostManaged
            | AttachmentPublicationMode::MachineForwarded(_) => {
                require_retained_records(&listeners, context.launch_claim, "published listener")?;
            }
        }
        require_retained_records(&pep, context.launch_claim, "PEP listener")?;

        HostManagedAttachmentDetachedProof::new(HostManagedAttachmentDetachedProofInput {
            command: command.clone(),
            association: attachment.association().clone(),
            selected_provider_id: attachment.selected_provider_id().clone(),
            effect_disposition:
                HostManagedAttachmentEffectDisposition::ProviderEffectMayHaveExisted,
            never_effected_authority: None,
            stable_handle_sha256: evidence_digest("stable_handle", handle)?,
            provider_delete_evidence_sha256: evidence_digest(
                "provider_delete",
                &provider_operation,
            )?,
            namespace_absence_evidence_sha256: evidence_digest(
                "namespace_absence",
                &(
                    "explicitly_absent",
                    &context.layout.netns_root,
                    &context.layout.netns_path,
                ),
            )?,
            pep_retained_evidence_sha256: evidence_digest("pep_retained", &pep)?,
            listener_retained_evidence_sha256: evidence_digest("listeners_retained", &listeners)?,
            ipam_retained_evidence_sha256: evidence_digest(
                "ipam_retained",
                &(
                    &context.config.attachment_id,
                    &context.config.reservation_claim,
                    &context.config.segment_id,
                    provider_operation.label(),
                ),
            )?,
            segment_quarantine_evidence_sha256: evidence_digest(
                "segment_quarantined",
                &(segment.state() as u8, segment.association()),
            )?,
            attachment_retained_evidence_sha256: retained_attachment_authority_digest(&attachment)?,
            publication_evidence,
        })
    }

    fn retained_publication_evidence(
        &self,
        context: &OciAttachmentContext<'_>,
        machine_forwarded_absence: impl FnOnce() -> Result<RetainedAttachmentPublicationEvidence>,
    ) -> Result<RetainedAttachmentPublicationEvidence> {
        match context.publication {
            AttachmentPublicationMode::HostManaged => {
                Ok(RetainedAttachmentPublicationEvidence::HostManaged)
            }
            AttachmentPublicationMode::Deferred => {
                self.separate_owner_publication_evidence(context)
            }
            AttachmentPublicationMode::MachineForwarded(_) => {
                let evidence = machine_forwarded_absence()?;
                if !matches!(
                    evidence,
                    RetainedAttachmentPublicationEvidence::MachineForwarded { .. }
                ) {
                    return Err(SandboxError::OperationFailed {
                        message: "machine-forwarded retained detach received crossed publication evidence"
                            .to_owned(),
                    });
                }
                Ok(evidence)
            }
        }
    }

    pub(super) fn separate_owner_publication_evidence(
        &self,
        context: &OciAttachmentContext<'_>,
    ) -> Result<RetainedAttachmentPublicationEvidence> {
        let records = self
            .ports
            .authenticate_separate_owner_publication_terminal(
                &complete_port_plan_members(context),
                context.tenant_id,
                context.bindings,
                context.leases,
            )?;
        RetainedAttachmentPublicationEvidence::deferred(evidence_digest(
            "deferred_publication_terminal",
            &records,
        )?)
    }

    pub(super) fn inspect_never_effected_authority(
        &self,
        context: &OciAttachmentContext<'_>,
        association: &nimbus_network::NetworkAttachmentSegmentAssociation,
    ) -> Result<NeverEffectedAuthoritySnapshot> {
        let provider = recovery::inspect_provider(self.ipam, context);
        if provider != recovery::AttachmentProviderObservation::Absent {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "{} attachment {} cannot prove no-effect cleanup from provider state {provider:?}",
                    context.provider_label, context.sandbox_id
                ),
            });
        }
        let ipam = inspect_container_ipam_authority(
            self.ipam,
            context.layout,
            context.config,
            context.sandbox_id,
        )?;
        let ipam_authority = match ipam {
            ContainerIpamAuthorityState::Live => NeverEffectedIpamAuthority::Live,
            ContainerIpamAuthorityState::Released => NeverEffectedIpamAuthority::Released,
            ContainerIpamAuthorityState::Absent => NeverEffectedIpamAuthority::Absent,
        };
        let provider_delete_evidence_sha256 = match ipam {
            ContainerIpamAuthorityState::Live => {
                let operation = inspect_netavark_provider_operation(
                    self.ipam,
                    context.layout,
                    context.config,
                    context.sandbox_id,
                )?;
                if operation.label() != "reserved" {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "{} attachment {} has live IPAM with provider operation {} and cannot prove no effect",
                            context.provider_label,
                            context.sandbox_id,
                            operation.label()
                        ),
                    });
                }
                evidence_digest("provider_delete", &operation)?
            }
            ContainerIpamAuthorityState::Released | ContainerIpamAuthorityState::Absent => {
                evidence_digest(
                    "provider_delete",
                    &(
                        "confirmed_no_effect",
                        ipam_authority,
                        &context.config.attachment_id,
                        &context.config.reservation_claim,
                        &context.config.segment_id,
                    ),
                )?
            }
        };

        let (listeners, pep) = self.retained_port_plan_snapshot(context)?;
        let port_authority = match context.publication {
            AttachmentPublicationMode::Deferred => {
                self.separate_owner_publication_evidence(context)?;
                classify_never_effected_port_records(
                    &[],
                    &pep,
                    context.launch_claim,
                    "no-effect attachment-owned PEP",
                )?
            }
            AttachmentPublicationMode::HostManaged => {
                let plan_members = complete_port_plan_members(context);
                let published_batch = self.ports.classify_planned_netavark_cleanup_batch(
                    &plan_members,
                    context.tenant_id,
                    context.bindings,
                    context.leases,
                    context.launch_claim,
                )?;
                let port_authority = classify_never_effected_port_records(
                    &listeners,
                    &pep,
                    context.launch_claim,
                    "no-effect attachment",
                )?;
                let published_matches = match port_authority {
                    NeverEffectedPortAuthority::NoMembers => {
                        published_batch == LaunchPortBatchState::TerminalNoEffect
                    }
                    NeverEffectedPortAuthority::Retained => {
                        published_batch == LaunchPortBatchState::NeverBound
                            || (context.leases.is_empty()
                                && published_batch == LaunchPortBatchState::TerminalNoEffect)
                    }
                    NeverEffectedPortAuthority::Released => {
                        published_batch == LaunchPortBatchState::TerminalNoEffect
                    }
                };
                if !published_matches {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "{} attachment {} crossed its exact no-effect port batch ({port_authority:?}, {published_batch:?})",
                            context.provider_label, context.sandbox_id
                        ),
                    });
                }
                port_authority
            }
            AttachmentPublicationMode::MachineForwarded(_) => {
                return Err(SandboxError::OperationFailed {
                    message: "machine-forwarded no-effect detach requires its publication owner"
                        .to_owned(),
                });
            }
        };

        let segment = self.allocator.inspect_attachment_reservation(
            context.tenant_id,
            &context.config.attachment_id,
            &context.config.reservation_claim,
        )?;
        let segment_authority = match segment.state() {
            NetworkAttachmentReservationState::ProviderCleanupPending
                if segment.association() == Some(association) =>
            {
                NeverEffectedSegmentAuthority::ProviderCleanupPending
            }
            NetworkAttachmentReservationState::ReservationCleanupPending
                if segment.association() == Some(association) =>
            {
                NeverEffectedSegmentAuthority::ReservationCleanupPending
            }
            NetworkAttachmentReservationState::Absent if segment.association().is_none() => {
                NeverEffectedSegmentAuthority::Absent
            }
            state => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "{} attachment {} has crossed no-effect segment state {state:?}",
                        context.provider_label, context.sandbox_id
                    ),
                });
            }
        };
        let authority = NeverEffectedAttachmentAuthority::new(
            port_authority,
            ipam_authority,
            segment_authority,
        )?;
        Ok(NeverEffectedAuthoritySnapshot {
            authority,
            provider_delete_evidence_sha256,
            pep_evidence_sha256: evidence_digest("pep_retained", &pep)?,
            listener_evidence_sha256: evidence_digest("listeners_retained", &listeners)?,
            ipam_evidence_sha256: evidence_digest(
                "ipam_retained",
                &(
                    "confirmed_no_effect",
                    ipam_authority,
                    &context.config.attachment_id,
                    &context.config.reservation_claim,
                    &context.config.segment_id,
                ),
            )?,
            segment_evidence_sha256: evidence_digest(
                "segment_quarantined",
                &(segment.state() as u8, segment.association()),
            )?,
        })
    }

    /// Read published-listener and PEP authority from one host-global lease snapshot.
    pub(super) fn retained_port_plan_snapshot(
        &self,
        context: &OciAttachmentContext<'_>,
    ) -> Result<(
        Vec<nimbus_network::PortLeaseRecord>,
        Vec<nimbus_network::PortLeaseRecord>,
    )> {
        let plan_members = complete_port_plan_members(context);
        let records = self.ports.port_lease_plan_member_records_snapshot(
            &plan_members,
            &plan_members,
            "retained listener and PEP",
        )?;
        let listener_count = context.leases.len();
        if records.len() != plan_members.len() || listener_count > records.len() {
            return Err(SandboxError::OperationFailed {
                message: "retained listener snapshot crossed complete plan membership".to_owned(),
            });
        }
        let (listeners, pep) = records.split_at(listener_count);
        Ok((listeners.to_vec(), pep.to_vec()))
    }
}

pub(super) fn classify_never_effected_port_records(
    listeners: &[nimbus_network::PortLeaseRecord],
    pep: &[nimbus_network::PortLeaseRecord],
    launch_claim: Option<&nimbus_network::NetworkReservationClaim>,
    label: &str,
) -> Result<NeverEffectedPortAuthority> {
    let records = listeners.iter().chain(pep).collect::<Vec<_>>();
    if records.is_empty() {
        return Ok(NeverEffectedPortAuthority::NoMembers);
    }
    let exact_claim = |record: &nimbus_network::PortLeaseRecord| {
        record.reservation_claim().is_none() || record.reservation_claim() == launch_claim
    };
    let common_no_effect = |record: &nimbus_network::PortLeaseRecord| {
        exact_claim(record)
            && record.bind_claim().is_none()
            && record.adoption_claim().is_none()
            && record.binding().is_none()
            && record.confirmed_stopped_binding().is_none()
            && record.failure().is_none()
            && record.active_lifetime().is_none()
    };
    if records
        .iter()
        .all(|record| record.phase() == PortLeasePhase::Reserved && common_no_effect(record))
    {
        return Ok(NeverEffectedPortAuthority::Retained);
    }
    if records
        .iter()
        .all(|record| record.phase() == PortLeasePhase::Released && common_no_effect(record))
    {
        return Ok(NeverEffectedPortAuthority::Released);
    }
    Err(SandboxError::OperationFailed {
        message: format!(
            "{label} port authority is neither uniformly retained nor exact terminal no-effect"
        ),
    })
}

/// Digest the immutable portable attachment authority retained across detach.
///
/// Startup reconciliation may conservatively move an exact detached record
/// from `Deleting` to `CleanupPending`. That quarantine changes only the
/// lifecycle phase. It must not invalidate the compound detached proof when
/// every identity, generation, provider, association, and handle fence is
/// unchanged.
pub(super) fn retained_attachment_authority_digest(
    attachment: &nimbus_network::DurableNetworkAttachmentState,
) -> Result<String> {
    evidence_digest(
        "attachment_retained",
        &(
            attachment.tenant_id(),
            attachment.selected_provider_id(),
            attachment.association(),
            attachment.resource().version(),
            attachment.resource().provider_handle(),
        ),
    )
}

pub(super) fn never_effected_stable_handle_digest(
    context: &OciAttachmentContext<'_>,
    attachment: Option<&nimbus_network::DurableNetworkAttachmentState>,
) -> Result<String> {
    match attachment {
        Some(attachment) => evidence_digest(
            "stable_handle",
            &("not_issued", attachment.resource().version()),
        ),
        None => evidence_digest(
            "stable_handle",
            &(
                "not_issued",
                context.tenant_id,
                &context.config.attachment_id,
                &context.config.network_plan,
            ),
        ),
    }
}

pub(super) fn never_effected_attachment_authority_digest(
    context: &OciAttachmentContext<'_>,
    attachment: Option<&nimbus_network::DurableNetworkAttachmentState>,
) -> Result<String> {
    match attachment {
        Some(attachment) => retained_attachment_authority_digest(attachment),
        None => evidence_digest(
            "attachment_retained",
            &(
                "confirmed_absent",
                context.tenant_id,
                &context.config.attachment_id,
                &context.config.network_plan,
            ),
        ),
    }
}

pub(super) fn complete_port_plan_members(
    context: &OciAttachmentContext<'_>,
) -> Vec<nimbus_network::PortLeaseRequest> {
    let mut members = context.leases.to_vec();
    if let Some(auxiliary) = context.auxiliary_listener {
        members.push(auxiliary.request().clone());
    }
    members
}

pub(super) fn authenticate_exact_command_context(
    context: &OciAttachmentContext<'_>,
    command: &SandboxNetworkTeardownCommand,
) -> Result<()> {
    context.validate_backend_publication()?;
    let expected_provider = host_managed_attachment_provider_id(
        plan::oci_attachment_registration_kind(context.backend),
    );
    if command.tenant_id() != context.tenant_id
        || command.sandbox_id() != context.sandbox_id
        || command.attachment_id() != &context.config.attachment_id
        || context.config.network_plan.as_ref() != Some(command.network_plan())
        || command.provider_id() != expected_provider
        || command.provider_id()
            != host_managed_attachment_provider_id(plan::oci_attachment_registration_kind(
                context.backend,
            ))
    {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "{} retained detach command crossed tenant, sandbox, attachment, plan, or provider identity",
                context.provider_label
            ),
        });
    }
    Ok(())
}

fn require_publication_mode(
    context: &OciAttachmentContext<'_>,
    matches: bool,
    expected: &str,
) -> Result<()> {
    if matches {
        Ok(())
    } else {
        Err(SandboxError::OperationFailed {
            message: format!(
                "{} attachment {} requires {expected} publication composition",
                context.provider_label, context.sandbox_id
            ),
        })
    }
}

pub(super) fn require_retained_records(
    records: &[nimbus_network::PortLeaseRecord],
    launch_claim: Option<&nimbus_network::NetworkReservationClaim>,
    label: &str,
) -> Result<()> {
    for record in records {
        let clean_retained = record.phase() == PortLeasePhase::Reserved
            && record.bind_claim().is_none()
            && record.binding().is_none()
            && record.active_lifetime().is_none()
            && (record.reservation_claim().is_none() || record.reservation_claim() == launch_claim);
        if !clean_retained {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "{label} {} is not exact retained, non-bindable authority",
                    record.request().lease_id()
                ),
            });
        }
    }
    Ok(())
}

pub(super) fn evidence_digest(label: &str, evidence: &impl Serialize) -> Result<String> {
    let bytes = serde_json::to_vec(evidence).map_err(|error| SandboxError::OperationFailed {
        message: format!("failed to serialize {label} evidence: {error}"),
    })?;
    let mut digest = Sha256::new();
    digest.update(EVIDENCE_DIGEST_DOMAIN);
    digest.update(label.as_bytes());
    digest.update([0]);
    digest.update(bytes);
    Ok(format!("{:x}", digest.finalize()))
}
