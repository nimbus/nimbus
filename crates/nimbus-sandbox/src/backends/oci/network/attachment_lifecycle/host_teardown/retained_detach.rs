//! Exact retained detach choreography.

use nimbus_network::{
    NetworkAttachmentReservationState, NetworkResourcePhase, NetworkSegmentQuarantineOutcome,
    PortLeasePhase,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::super::*;
use super::progress::{
    HostManagedAttachmentDetachPhase, HostManagedAttachmentDetachedProof,
    HostManagedAttachmentDetachedProofInput, RetainedAttachmentPublicationEvidence,
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
        lifecycle.detach_host_managed_retained(
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
        lifecycle.detach_host_managed_retained(
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
    fn detach_host_managed_retained(
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
        let machine_forwarded = !context.publication.owns_netavark_bindings();
        authenticate_exact_command_context(context, command, machine_forwarded)?;
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

        let published_batch_state = (!machine_forwarded
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
        let publication_evidence = if machine_forwarded {
            let evidence = retain_publication()?;
            if !matches!(
                evidence,
                RetainedAttachmentPublicationEvidence::MachineForwarded { .. }
            ) {
                return Err(SandboxError::OperationFailed {
                    message: "machine-forwarded retained detach received host-managed publication evidence"
                        .to_owned(),
                });
            }
            evidence
        } else {
            RetainedAttachmentPublicationEvidence::HostManaged
        };

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
                if !machine_forwarded {
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
                if !machine_forwarded {
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
                if !machine_forwarded {
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
                if !machine_forwarded {
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
        require_retained_records(&listeners, context.launch_claim, "published listener")?;
        require_retained_records(&pep, context.launch_claim, "PEP listener")?;

        HostManagedAttachmentDetachedProof::new(HostManagedAttachmentDetachedProofInput {
            command: command.clone(),
            association: attachment.association().clone(),
            selected_provider_id: attachment.selected_provider_id().clone(),
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
    machine_forwarded: bool,
) -> Result<()> {
    context.validate_backend_publication()?;
    if machine_forwarded == context.publication.owns_netavark_bindings() {
        return Err(SandboxError::OperationFailed {
            message: if machine_forwarded {
                "retained machine-forwarded detach requires machine-forwarded publication"
                    .to_owned()
            } else {
                "retained host detach requires host-managed publication".to_owned()
            },
        });
    }
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
