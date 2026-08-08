//! Exact cross-authority authentication for OCI attachment generations.
//!
//! The segment allocator owns the reservation association; the portable
//! attachment authority owns desired lifecycle state. This module is the one
//! composition seam that requires those records to agree before sandbox-owned
//! provider effects.

use std::collections::BTreeMap;
use std::num::NonZeroU16;

use nimbus_network::{
    LocalNetworkAttachmentAuthority, NetworkAttachmentReservationObservation,
    NetworkAttachmentReservationState, NetworkAttachmentSegmentAssociation, NetworkResourcePhase,
    NetworkSegmentId, PortBindRealm, PortLeaseAccounting, PortLeaseEffectScope, PortLeasePhase,
    PortProtocol, PortPublicationIntent, PortRequestMode,
};

use super::{AttachmentTeardownMode, OciAttachmentAdapter, OciAttachmentContext};
use crate::backends::oci::network::{
    OciSegmentAllocator, authenticate_container_network_generation,
};
use crate::error::{Result, SandboxError};

use super::{AttachmentAttachAuthority, OciAttachmentLifecycle, OciPortProvider};
use crate::backends::oci::port_lease::{provider_binding, published_scope, target_for_ip};
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
    fn authenticate_active_deferred_pep_recovery(
        &self,
        context: &OciAttachmentContext<'_>,
    ) -> Result<()> {
        context.validate_backend_publication()?;
        if !context.publication.is_deferred() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "{} attachment {} is not an ingress-deferred recovery",
                    context.provider_label, context.sandbox_id
                ),
            });
        }
        authenticate_container_network_generation(
            self.ipam,
            context.layout,
            context.config,
            context.sandbox_id,
        )?;
        let association = authenticate_attach_association(self.allocator, context)?;
        let durable = super::state::OciAttachmentDurableState::compile(
            self.attachments,
            context,
            association,
        )?;
        let record = durable
            .inspect()?
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "{} attachment {} lacks durable Active recovery authority",
                    context.provider_label, context.sandbox_id
                ),
            })?;
        if record.resource().phase() != NetworkResourcePhase::Active {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "{} attachment {} cannot recover its PEP from durable phase {:?}",
                    context.provider_label,
                    context.sandbox_id,
                    record.resource().phase()
                ),
            });
        }
        durable.authenticate_stable_handle(&record)?;
        if !matches!(
            super::recovery::inspect_provider(self.ipam, context),
            super::recovery::AttachmentProviderObservation::Present { .. }
        ) {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "{} attachment {} cannot recover its PEP without exact provider-present evidence",
                    context.provider_label, context.sandbox_id
                ),
            });
        }

        let auxiliary =
            context
                .auxiliary_listener
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "{} attachment {} lacks its planned egress PEP authority",
                        context.provider_label, context.sandbox_id
                    ),
                })?;
        let mut plan_members = context.leases.to_vec();
        plan_members.push(auxiliary.request().clone());
        let authority = self.ports.authority()?;
        let mut recovery_members = Vec::new();
        for (binding, request) in context.bindings.iter().zip(context.leases) {
            let member = authority
                .inspect_plan_member(&plan_members, request)
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "{} attachment {} published listener {} failed complete-plan recovery authentication: {error}",
                        context.provider_label,
                        context.sandbox_id,
                        request.lease_id()
                    ),
                })?;
            match member.phase() {
                PortLeasePhase::Reserved
                    if member.reservation_claim() == context.launch_claim
                        && member.bind_claim().is_none()
                        && member.binding().is_none()
                        && member.active_lifetime().is_none() => {}
                PortLeasePhase::Active
                    if member.reservation_claim().is_none()
                        && member.active_lifetime().is_some_and(|lifetime| {
                            lifetime.effect_scope() == PortLeaseEffectScope::ProviderManaged
                        }) =>
                {
                    self.ports.require_active_planned_machine_bindings(
                        context.tenant_id,
                        std::slice::from_ref(binding),
                        std::slice::from_ref(request),
                        &plan_members,
                    )?;
                    recovery_members.push(request.clone());
                }
                _ => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "{} attachment {} published listener {} is neither pristine deferred ingress authority nor an exact dead planned machine binding (phase={:?}, reservation_claim_matches={}, bind_claim={}, binding={}, lifetime={:?})",
                            context.provider_label,
                            context.sandbox_id,
                            request.lease_id(),
                            member.phase(),
                            member.reservation_claim() == context.launch_claim,
                            member.bind_claim().is_some(),
                            member.binding().is_some(),
                            member.active_lifetime()
                        ),
                    });
                }
            }
        }
        let auxiliary_request = auxiliary.request();
        let auxiliary_record = authority
            .inspect_plan_member(&plan_members, auxiliary_request)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "{} attachment {} PEP failed complete-plan recovery authentication: {error}",
                    context.provider_label, context.sandbox_id
                ),
            })?;
        let auxiliary_addr = auxiliary.bind_addr()?;
        let expected_binding = provider_binding(
            auxiliary_request,
            auxiliary_addr,
            OciPortProvider::EgressPep,
        )?;
        if auxiliary_record.phase() != PortLeasePhase::Active
            || auxiliary_record.reservation_claim().is_some()
            || auxiliary_record.binding() != Some(&expected_binding)
            || auxiliary_record.active_lifetime().is_none_or(|lifetime| {
                lifetime.effect_scope() != PortLeaseEffectScope::ProcessBound
            })
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "{} attachment {} PEP is not exact Active process-bound authority",
                    context.provider_label, context.sandbox_id
                ),
            });
        }
        recovery_members.push(auxiliary_request.clone());
        let recoveries = authority
            .recover_dead_plan_members(&plan_members, &recovery_members)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "{} attachment {} could not atomically authenticate dead planned listener ownership: {error}",
                    context.provider_label, context.sandbox_id
                ),
            })?;
        drop(recoveries);
        Ok(())
    }

    /// Authenticate compiler-issued listener authority for an attachment-only
    /// phase without deriving replacement IDs or fences from `SandboxId`.
    ///
    /// Deferred attach deliberately leaves every published lease reserved and
    /// performs no bind. The exact durable request and reservation coordinator
    /// are therefore the complete pre-effect authority at this boundary.
    pub(super) fn authenticate_deferred_listener_authority(
        &self,
        context: &OciAttachmentContext<'_>,
    ) -> Result<()> {
        let plan =
            context
                .config
                .network_plan
                .as_ref()
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "{} deferred attachment {} lacks its exact compiled network plan",
                        context.provider_label, context.sandbox_id
                    ),
                })?;
        let reservation_claim =
            context
                .launch_claim
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "{} deferred attachment {} lacks its retained launch reservation",
                        context.provider_label, context.sandbox_id
                    ),
                })?;
        if context.bindings.len() != context.leases.len() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "{} deferred attachment {} has {} listener bindings but {} exact leases",
                    context.provider_label,
                    context.sandbox_id,
                    context.bindings.len(),
                    context.leases.len()
                ),
            });
        }

        let records = self
            .ports
            .authority()?
            .list()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to inspect {} deferred attachment port authority for {}: {error}",
                    context.provider_label, context.sandbox_id
                ),
            })?
            .into_iter()
            .map(|record| (record.request().lease_id().clone(), record))
            .collect::<BTreeMap<_, _>>();

        for (binding, request) in context.bindings.iter().zip(context.leases) {
            let (target, exposure) = published_scope(binding.host_address)?;
            let expected_port = match NonZeroU16::new(binding.host_port) {
                Some(port) => PortRequestMode::Exact(port),
                None => PortRequestMode::ProviderAssigned,
            };
            if request.plan_id() != Some(plan.plan_id())
                || request.tenant_id() != Some(context.tenant_id)
                || request.generation() != plan.generation()
                || request.accounting() != PortLeaseAccounting::TenantPublished
                || request.publication() != &PortPublicationIntent::host(binding.host_address)
                || request.binding().protocol() != PortProtocol::Tcp
                || request.binding().realm() != &PortBindRealm::Host
                || request.binding().target() != &target
                || request.binding().exposure() != exposure
                || request.binding().port() != &expected_port
            {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "{} deferred attachment {} listener {:?} crossed its exact plan, tenant, generation, accounting, publication, or binding intent",
                        context.provider_label, context.sandbox_id, binding.name
                    ),
                });
            }
            let record =
                records
                    .get(request.lease_id())
                    .ok_or_else(|| SandboxError::OperationFailed {
                        message: format!(
                            "{} deferred attachment {} port lease {} does not exist",
                            context.provider_label,
                            context.sandbox_id,
                            request.lease_id()
                        ),
                    })?;
            if record.request() != request
                || record.phase() != PortLeasePhase::Reserved
                || record.reservation_claim() != Some(reservation_claim)
                || record.bind_claim().is_some()
                || record.adoption_claim().is_some()
                || record.binding().is_some()
                || record.confirmed_stopped_binding().is_some()
                || record.failure().is_some()
            {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "{} deferred attachment {} port lease {} crossed its exact durable request, reservation coordinator, or never-bound state",
                        context.provider_label,
                        context.sandbox_id,
                        request.lease_id()
                    ),
                });
            }
        }
        if let Some(auxiliary) = context.auxiliary_listener {
            let bind_addr = auxiliary.bind_addr()?;
            let request = auxiliary.request();
            let requested_port_matches = match request.binding().port() {
                PortRequestMode::Exact(port) => port.get() == bind_addr.port(),
                PortRequestMode::Range(range) => {
                    range.start().get() <= bind_addr.port() && bind_addr.port() <= range.end().get()
                }
                PortRequestMode::ProviderAssigned => false,
            };
            if request.plan_id() != Some(plan.plan_id())
                || request.tenant_id() != Some(context.tenant_id)
                || request.generation() != plan.generation()
                || request.accounting() != PortLeaseAccounting::HostInternal
                || request.publication() != &PortPublicationIntent::Unpublished
                || request.binding().protocol() != PortProtocol::Tcp
                || request.binding().realm() != &PortBindRealm::Host
                || request.binding().target() != &target_for_ip(bind_addr.ip())?
                || request.binding().exposure() != nimbus_network::PortExposure::Private
                || !requested_port_matches
            {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "{} deferred attachment {} crossed its exact auxiliary listener authority",
                        context.provider_label, context.sandbox_id
                    ),
                });
            }
            let record =
                records
                    .get(request.lease_id())
                    .ok_or_else(|| SandboxError::OperationFailed {
                        message: format!(
                            "{} deferred attachment {} auxiliary port lease {} does not exist",
                            context.provider_label,
                            context.sandbox_id,
                            request.lease_id()
                        ),
                    })?;
            if record.request() != request
                || record.reserved_port().map(NonZeroU16::get) != Some(bind_addr.port())
                || record.phase() != PortLeasePhase::Reserved
                || record.reservation_claim() != Some(reservation_claim)
                || record.bind_claim().is_some()
                || record.adoption_claim().is_some()
                || record.binding().is_some()
                || record.confirmed_stopped_binding().is_some()
                || record.failure().is_some()
            {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "{} deferred attachment {} auxiliary port lease {} crossed its exact durable request, assignment, reservation coordinator, or never-bound state",
                        context.provider_label,
                        context.sandbox_id,
                        request.lease_id()
                    ),
                });
            }
        }
        Ok(())
    }

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
                let mut launch_batch = context.leases.to_vec();
                if let Some(auxiliary) = context.auxiliary_listener {
                    launch_batch.push(auxiliary.request().clone());
                }
                self.ports
                    .require_never_bound_launch_batch(&launch_batch, claim)?;
                if !context.publication.is_deferred()
                    && let Some(auxiliary) = context.auxiliary_listener
                {
                    self.ports.require_internal_listener_authority(
                        context.tenant_id,
                        context.sandbox_id,
                        auxiliary.bind_addr()?,
                        auxiliary.request(),
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

impl OciAttachmentAdapter<'_> {
    /// Prove an exact Active private attachment plus a dead planned PEP owner
    /// without acquiring durable recovery authority or performing effects.
    pub(crate) fn authenticate_active_deferred_pep_recovery(
        &self,
        lifecycle: &OciAttachmentLifecycle<'_>,
    ) -> Result<()> {
        lifecycle.authenticate_active_deferred_pep_recovery(&self.context)
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
            let attachment_id = context.config.attachment_id.clone();
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
            &context.config.attachment_id,
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
