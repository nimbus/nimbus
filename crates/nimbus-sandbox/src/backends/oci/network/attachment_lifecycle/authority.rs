//! Exact cross-authority authentication for OCI attachment generations.
//!
//! The segment allocator owns the reservation association; the portable
//! attachment authority owns desired lifecycle state. This module is the one
//! composition seam that requires those records to agree before sandbox-owned
//! provider effects.

use nimbus_network::{
    LocalNetworkAttachmentAuthority, NetworkAttachmentReservationObservation,
    NetworkAttachmentReservationState, NetworkAttachmentSegmentAssociation, NetworkResourcePhase,
    NetworkSegmentId,
};

use super::{AttachmentTeardownMode, OciAttachmentContext};
use crate::backends::oci::network::{OciSegmentAllocator, default_network_attachment_id};
use crate::error::{Result, SandboxError};

use super::{AttachmentAttachAuthority, OciAttachmentLifecycle, OciPortProvider};
use crate::backends::oci::port_lifecycle::LaunchPortBatchState;

pub(super) fn authenticate_attach_association(
    allocator: &OciSegmentAllocator,
    context: &OciAttachmentContext<'_>,
) -> Result<NetworkAttachmentSegmentAssociation> {
    let observation = inspect_allocator(allocator, context)?;
    if observation.state() != NetworkAttachmentReservationState::Adopted {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "{} attachment {} requires an adopted exact reservation before effects, got {:?}",
                context.provider_label,
                context.sandbox_id,
                observation.state()
            ),
        });
    }
    require_exact_config_association(context, &observation)
}

impl OciAttachmentLifecycle<'_> {
    pub(super) fn authenticate_attach_port_authority(
        &self,
        context: &OciAttachmentContext<'_>,
        attach_authority: AttachmentAttachAuthority<'_>,
    ) -> Result<()> {
        let port_authority: Result<()> = match attach_authority {
            AttachmentAttachAuthority::FreshLaunch(claim) => {
                let Some(context_claim) = context.launch_claim else {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "{} attachment {} is missing its fresh launch reservation claim",
                            context.provider_label, context.sandbox_id
                        ),
                    });
                };
                if context_claim != claim || &context.config.reservation_claim != claim {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "{} attachment {} launch reservation claim does not match its exact \
                             attachment generation",
                            context.provider_label, context.sandbox_id
                        ),
                    });
                }
                self.ports
                    .require_never_bound_launch_batch(context.leases, claim)?;
                if let Some(auxiliary) = context.auxiliary_listener {
                    self.ports.require_internal_listener_authority(
                        context.tenant_id,
                        context.sandbox_id,
                        auxiliary.bind_addr()?,
                        auxiliary.request(),
                    )?;
                    self.ports.require_never_bound_launch_batch(
                        std::slice::from_ref(auxiliary.request()),
                        claim,
                    )?;
                }
                Ok(())
            }
            AttachmentAttachAuthority::RestartRetained => {
                if context.launch_claim.is_some() {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "{} attachment {} restart still carries a launch reservation claim",
                            context.provider_label, context.sandbox_id
                        ),
                    });
                }
                let publication_state = if context.publication.owns_netavark_bindings() {
                    self.ports.classify_netavark_cleanup_batch(
                        context.tenant_id,
                        context.sandbox_id,
                        context.bindings,
                        context.leases,
                        None,
                    )?
                } else {
                    self.ports.classify_machine_cleanup_batch(
                        context.tenant_id,
                        context.sandbox_id,
                        context.bindings,
                        context.leases,
                    )?
                };
                if publication_state != LaunchPortBatchState::RestartRetained
                    && !(context.leases.is_empty()
                        && publication_state == LaunchPortBatchState::TerminalNoEffect)
                {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "{} attachment {} restart requires confirmed-stop publication \
                             authority before effects, got {publication_state:?}",
                            context.provider_label, context.sandbox_id
                        ),
                    });
                }
                if let Some(auxiliary) = context.auxiliary_listener {
                    self.ports.require_restart_retained_internal_listener(
                        context.tenant_id,
                        context.sandbox_id,
                        auxiliary.bind_addr()?,
                        auxiliary.request(),
                        OciPortProvider::EgressPep,
                    )?;
                }
                Ok(())
            }
        };
        port_authority?;
        Ok(())
    }
}

pub(super) fn authenticate_detach_association(
    attachments: Option<&LocalNetworkAttachmentAuthority>,
    allocator: &OciSegmentAllocator,
    context: &OciAttachmentContext<'_>,
    mode: AttachmentTeardownMode,
) -> Result<NetworkAttachmentSegmentAssociation> {
    let observation = inspect_allocator(allocator, context)?;
    match observation.state() {
        NetworkAttachmentReservationState::Reserved
        | NetworkAttachmentReservationState::Adopted
        | NetworkAttachmentReservationState::ProviderCleanupPending => {
            require_exact_config_association(context, &observation)
        }
        NetworkAttachmentReservationState::Absent => {
            let attachments = attachments.ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "{} attachment {} has no valid manager-derived durable attachment authority",
                    context.provider_label, context.sandbox_id
                ),
            })?;
            let attachment_id = default_network_attachment_id(context.sandbox_id);
            let record = attachments
                .get(context.tenant_id, &attachment_id)
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "durable attachment authority rejected cleanup inspection for \
                         {attachment_id}: {error}"
                    ),
                })?
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "{} attachment {} has neither allocator association nor durable terminal \
                         authority",
                        context.provider_label, context.sandbox_id
                    ),
                })?;
            match record.resource().phase() {
                phase if phase.is_terminal() => {}
                NetworkResourcePhase::Reserved
                | NetworkResourcePhase::Deleting
                | NetworkResourcePhase::CleanupPending
                    if mode == AttachmentTeardownMode::Final =>
                {
                    // Final detach releases allocator capacity immediately
                    // before publishing the portable terminal phase. Exact
                    // attachment and IPAM tombstones must reopen that bounded
                    // cross-authority crash interval, including a retry after
                    // a fallible workload/provider callback retained the
                    // portable CleanupPending fence, without recreating or
                    // replaying provider effects. Restart may never use this
                    // interval because it must retain allocation authority.
                }
                phase => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "{} attachment {} lost allocator association while durable phase \
                             {phase:?} cannot complete {mode:?} detach",
                            context.provider_label, context.sandbox_id
                        ),
                    });
                }
            }
            authenticate_config_values(context, record.association())?;
            Ok(record.association().clone())
        }
        state => Err(SandboxError::OperationFailed {
            message: format!(
                "{} attachment {} cannot detach from allocator reservation state {state:?}",
                context.provider_label, context.sandbox_id
            ),
        }),
    }
}

fn inspect_allocator(
    allocator: &OciSegmentAllocator,
    context: &OciAttachmentContext<'_>,
) -> Result<NetworkAttachmentReservationObservation> {
    allocator
        .inspect_attachment_reservation(
            context.tenant_id,
            &default_network_attachment_id(context.sandbox_id),
            &context.config.reservation_claim,
        )
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "{} attachment {} could not authenticate its exact reservation claim before \
                 effects: {error}",
                context.provider_label, context.sandbox_id
            ),
        })
}

fn require_exact_config_association(
    context: &OciAttachmentContext<'_>,
    observation: &NetworkAttachmentReservationObservation,
) -> Result<NetworkAttachmentSegmentAssociation> {
    let association = observation
        .association()
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "{} attachment {} allocator state {:?} omitted its exact segment association",
                context.provider_label,
                context.sandbox_id,
                observation.state()
            ),
        })?;
    authenticate_config_values(context, association)?;
    Ok(association.clone())
}

fn authenticate_config_values(
    context: &OciAttachmentContext<'_>,
    association: &NetworkAttachmentSegmentAssociation,
) -> Result<()> {
    let configured_segment = context
        .config
        .segment_id
        .parse::<NetworkSegmentId>()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "{} attachment {} carries invalid segment identity {:?}: {error}",
                context.provider_label, context.sandbox_id, context.config.segment_id
            ),
        })?;
    if association.reservation_claim() != &context.config.reservation_claim
        || association.segment_id() != &configured_segment
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "{} attachment {} durable association does not match reservation claim and \
                 segment {} in its network configuration",
                context.provider_label, context.sandbox_id, configured_segment
            ),
        });
    }
    Ok(())
}
