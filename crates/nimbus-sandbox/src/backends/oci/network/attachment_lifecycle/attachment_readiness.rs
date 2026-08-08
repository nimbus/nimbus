//! Read-only complete readiness composition for host-managed OCI attachments.
//!
//! This module owns one decision over desired, durable, and observed evidence.
//! It has no provider-effect, lifecycle-mutation, cleanup, release, or capacity
//! authority.

use std::net::Ipv4Addr;

use nimbus_network::{
    NetworkCondition, NetworkConditionKind, NetworkConditionState, NetworkObservation,
    NetworkProviderId, NetworkResourcePhase, NetworkResourceVersion,
};

use super::super::MachineForwardedPublicationReadiness;
use super::super::{OciEgressPinObservation, OciEgressPinObserver};
use super::state::OciAttachmentDurableState;
use super::{OciAttachmentContext, OciAttachmentLifecycle, authority, recovery};
use crate::backends::oci::egress::{
    EgressProxyAssignment, EgressReadinessFailure, EgressReadinessState,
};

/// Exact portable evidence emitted only after every sandbox-private facet is
/// authenticated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OciAttachmentReadinessEvidence {
    observation: NetworkObservation,
    assigned_ips: Vec<Ipv4Addr>,
}

/// Common attachment/IPAM/Netavark/pin/PEP evidence before one publication
/// provider proves its own current listener effects.
///
/// This cannot be converted into a portable Ready observation without the
/// host-managed or machine-forwarded completion owned by the selected mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OciAttachmentBaseReadinessEvidence {
    tenant_id: nimbus_core::TenantId,
    sandbox_id: crate::instance::SandboxId,
    version: NetworkResourceVersion,
    selected_provider_id: NetworkProviderId,
    assigned_ips: Vec<Ipv4Addr>,
}

impl OciAttachmentBaseReadinessEvidence {
    pub(crate) fn assigned_ips(&self) -> &[Ipv4Addr] {
        &self.assigned_ips
    }
}

impl OciAttachmentReadinessEvidence {
    pub(crate) fn observation(&self) -> &NetworkObservation {
        &self.observation
    }

    pub(crate) fn assigned_ips(&self) -> &[Ipv4Addr] {
        &self.assigned_ips
    }
}

/// Closed fail-closed reason that complete attachment readiness cannot be
/// emitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OciAttachmentReadinessFailure {
    UnsupportedPublicationMode,
    InvalidContext(String),
    MissingDurableAuthority,
    DurableAuthorityRejected(String),
    DurablePhase(NetworkResourcePhase),
    ProviderNotReady(String),
    MissingEgressProxyAssignment,
    EgressPinNotReady(String),
    EgressPinUnknown(String),
    ListenerPublicationRejected(String),
    MachinePublicationRejected(String),
    PepNotReady(EgressReadinessFailure),
    ObservationRejected(String),
}

/// One exact complete observation or one named missing/conflicting/unknown
/// facet. There is deliberately no partial-ready state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OciAttachmentReadinessState {
    Ready(OciAttachmentReadinessEvidence),
    NotReady(OciAttachmentReadinessFailure),
}

/// Common evidence is intentionally not a complete readiness state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OciAttachmentBaseReadinessState {
    Ready(OciAttachmentBaseReadinessEvidence),
    NotReady(OciAttachmentReadinessFailure),
}

impl OciAttachmentReadinessState {
    pub(crate) fn is_ready(&self) -> bool {
        match self {
            Self::Ready(evidence) => {
                debug_assert_eq!(
                    evidence.observation().observed_phase(),
                    NetworkResourcePhase::Active
                );
                debug_assert!(!evidence.assigned_ips().is_empty());
                true
            }
            Self::NotReady(_) => false,
        }
    }
}

pub(super) fn inspect_host_managed_readiness(
    lifecycle: &OciAttachmentLifecycle<'_>,
    context: &OciAttachmentContext<'_>,
    pin_provider: &dyn OciEgressPinObserver,
    proxy: Option<&EgressProxyAssignment>,
    pep: EgressReadinessState,
) -> OciAttachmentReadinessState {
    if let Err(error) = context.validate_backend_publication() {
        return not_ready(OciAttachmentReadinessFailure::InvalidContext(
            error.to_string(),
        ));
    }
    if !context.publication.owns_netavark_bindings() {
        return not_ready(OciAttachmentReadinessFailure::UnsupportedPublicationMode);
    }

    let base = match inspect_common_base(lifecycle, context, pin_provider, proxy, pep) {
        OciAttachmentBaseReadinessState::Ready(base) => base,
        OciAttachmentBaseReadinessState::NotReady(reason) => return not_ready(reason),
    };

    if let Err(error) = lifecycle
        .ports
        .inspect_active_netavark_bindings_with_lifetimes(
            lifecycle.lifetimes,
            context.tenant_id,
            context.sandbox_id,
            context.bindings,
            context.leases,
        )
    {
        return not_ready(OciAttachmentReadinessFailure::ListenerPublicationRejected(
            error.to_string(),
        ));
    }

    complete(base)
}

pub(super) fn inspect_machine_forwarded_base_readiness(
    lifecycle: &OciAttachmentLifecycle<'_>,
    context: &OciAttachmentContext<'_>,
    pin_provider: &dyn OciEgressPinObserver,
    proxy: Option<&EgressProxyAssignment>,
    pep: EgressReadinessState,
) -> OciAttachmentBaseReadinessState {
    if let Err(error) = context.validate_backend_publication() {
        return base_not_ready(OciAttachmentReadinessFailure::InvalidContext(
            error.to_string(),
        ));
    }
    if context.publication.owns_netavark_bindings() {
        return base_not_ready(OciAttachmentReadinessFailure::UnsupportedPublicationMode);
    }
    inspect_common_base(lifecycle, context, pin_provider, proxy, pep)
}

/// Inspect attachment and egress prerequisites while proving that ingress is
/// still deliberately deferred.
pub(super) fn inspect_non_routable_readiness(
    lifecycle: &OciAttachmentLifecycle<'_>,
    context: &OciAttachmentContext<'_>,
    pin_provider: &dyn OciEgressPinObserver,
    proxy: Option<&EgressProxyAssignment>,
    pep: EgressReadinessState,
) -> OciAttachmentBaseReadinessState {
    if let Err(error) = context.validate_backend_publication() {
        return base_not_ready(OciAttachmentReadinessFailure::InvalidContext(
            error.to_string(),
        ));
    }
    if !context.publication.is_deferred() {
        return base_not_ready(OciAttachmentReadinessFailure::UnsupportedPublicationMode);
    }
    inspect_common_base(lifecycle, context, pin_provider, proxy, pep)
}

pub(super) fn complete_machine_forwarded_readiness(
    context: &OciAttachmentContext<'_>,
    base: OciAttachmentBaseReadinessEvidence,
    publication: std::result::Result<MachineForwardedPublicationReadiness, String>,
) -> OciAttachmentReadinessState {
    let Some(forwarder) = context.publication.machine_forwarder() else {
        return not_ready(OciAttachmentReadinessFailure::UnsupportedPublicationMode);
    };
    let publication = match publication {
        Ok(publication) => publication,
        Err(reason) => {
            return not_ready(OciAttachmentReadinessFailure::MachinePublicationRejected(
                reason,
            ));
        }
    };
    if base.tenant_id != *context.tenant_id
        || base.sandbox_id != *context.sandbox_id
        || publication.tenant_id() != context.tenant_id
        || publication.sandbox_id() != context.sandbox_id
        || publication.provider_instance() != forwarder.provider_instance()
        || publication.provider_generation() != forwarder.provider_generation()
    {
        return not_ready(OciAttachmentReadinessFailure::MachinePublicationRejected(
            "machine publication proof does not authenticate the attachment context and exact \
             provider generation"
                .to_owned(),
        ));
    }
    complete(base)
}

fn inspect_common_base(
    lifecycle: &OciAttachmentLifecycle<'_>,
    context: &OciAttachmentContext<'_>,
    pin_provider: &dyn OciEgressPinObserver,
    proxy: Option<&EgressProxyAssignment>,
    pep: EgressReadinessState,
) -> OciAttachmentBaseReadinessState {
    let association = match authority::authenticate_attach_association(lifecycle.allocator, context)
    {
        Ok(association) => association,
        Err(error) => {
            return base_not_ready(OciAttachmentReadinessFailure::DurableAuthorityRejected(
                error.to_string(),
            ));
        }
    };
    let durable =
        match OciAttachmentDurableState::compile(lifecycle.attachments, context, association) {
            Ok(durable) => durable,
            Err(error) => {
                return base_not_ready(OciAttachmentReadinessFailure::DurableAuthorityRejected(
                    error.to_string(),
                ));
            }
        };
    let record = match durable.inspect() {
        Ok(Some(record)) => record,
        Ok(None) => {
            return base_not_ready(OciAttachmentReadinessFailure::MissingDurableAuthority);
        }
        Err(error) => {
            return base_not_ready(OciAttachmentReadinessFailure::DurableAuthorityRejected(
                error.to_string(),
            ));
        }
    };
    if record.resource().phase() != NetworkResourcePhase::Active {
        return base_not_ready(OciAttachmentReadinessFailure::DurablePhase(
            record.resource().phase(),
        ));
    }
    if let Err(error) = durable.authenticate_stable_handle(&record) {
        return base_not_ready(OciAttachmentReadinessFailure::DurableAuthorityRejected(
            error.to_string(),
        ));
    }

    let assigned_ips = match recovery::inspect_provider(lifecycle.ipam, context) {
        recovery::AttachmentProviderObservation::Present { assigned_ips } => assigned_ips,
        observation => {
            return base_not_ready(OciAttachmentReadinessFailure::ProviderNotReady(format!(
                "{observation:?}"
            )));
        }
    };

    let Some(proxy) = proxy else {
        return base_not_ready(OciAttachmentReadinessFailure::MissingEgressProxyAssignment);
    };
    match pin_provider.inspect(context.layout, proxy) {
        OciEgressPinObservation::Ready => {}
        OciEgressPinObservation::NotReady { reason } => {
            return base_not_ready(OciAttachmentReadinessFailure::EgressPinNotReady(reason));
        }
        OciEgressPinObservation::Unknown { reason } => {
            return base_not_ready(OciAttachmentReadinessFailure::EgressPinUnknown(reason));
        }
    }

    if let EgressReadinessState::NotReady(reason) = pep {
        return base_not_ready(OciAttachmentReadinessFailure::PepNotReady(reason));
    }

    OciAttachmentBaseReadinessState::Ready(OciAttachmentBaseReadinessEvidence {
        tenant_id: context.tenant_id.clone(),
        sandbox_id: context.sandbox_id.clone(),
        version: record.resource().version().clone(),
        selected_provider_id: record.selected_provider_id().clone(),
        assigned_ips,
    })
}

fn complete(base: OciAttachmentBaseReadinessEvidence) -> OciAttachmentReadinessState {
    let observation = match NetworkObservation::new(
        base.version,
        NetworkResourcePhase::Active,
        Some(base.selected_provider_id),
        vec![NetworkCondition::new(
            NetworkConditionKind::Ready,
            NetworkConditionState::True,
        )],
    ) {
        Ok(observation) => observation,
        Err(error) => {
            return not_ready(OciAttachmentReadinessFailure::ObservationRejected(
                error.to_string(),
            ));
        }
    };
    OciAttachmentReadinessState::Ready(OciAttachmentReadinessEvidence {
        observation,
        assigned_ips: base.assigned_ips,
    })
}

fn not_ready(reason: OciAttachmentReadinessFailure) -> OciAttachmentReadinessState {
    OciAttachmentReadinessState::NotReady(reason)
}

fn base_not_ready(reason: OciAttachmentReadinessFailure) -> OciAttachmentBaseReadinessState {
    OciAttachmentBaseReadinessState::NotReady(reason)
}
