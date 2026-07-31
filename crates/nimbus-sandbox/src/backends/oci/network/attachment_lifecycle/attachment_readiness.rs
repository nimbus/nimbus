//! Read-only complete readiness composition for host-managed OCI attachments.
//!
//! This module owns one decision over desired, durable, and observed evidence.
//! It has no provider-effect, lifecycle-mutation, cleanup, release, or capacity
//! authority.

use std::net::Ipv4Addr;

use nimbus_network::{
    NetworkCondition, NetworkConditionKind, NetworkConditionState, NetworkObservation,
    NetworkResourcePhase,
};

use super::super::{OciEgressPinObservation, OciEgressPinProvider};
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
    pin_provider: &dyn OciEgressPinProvider,
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

    let association = match authority::authenticate_attach_association(lifecycle.allocator, context)
    {
        Ok(association) => association,
        Err(error) => {
            return not_ready(OciAttachmentReadinessFailure::DurableAuthorityRejected(
                error.to_string(),
            ));
        }
    };
    let durable =
        match OciAttachmentDurableState::compile(lifecycle.attachments, context, association) {
            Ok(durable) => durable,
            Err(error) => {
                return not_ready(OciAttachmentReadinessFailure::DurableAuthorityRejected(
                    error.to_string(),
                ));
            }
        };
    let record = match durable.inspect() {
        Ok(Some(record)) => record,
        Ok(None) => {
            return not_ready(OciAttachmentReadinessFailure::MissingDurableAuthority);
        }
        Err(error) => {
            return not_ready(OciAttachmentReadinessFailure::DurableAuthorityRejected(
                error.to_string(),
            ));
        }
    };
    if record.resource().phase() != NetworkResourcePhase::Active {
        return not_ready(OciAttachmentReadinessFailure::DurablePhase(
            record.resource().phase(),
        ));
    }
    if let Err(error) = durable.authenticate_stable_handle(&record) {
        return not_ready(OciAttachmentReadinessFailure::DurableAuthorityRejected(
            error.to_string(),
        ));
    }

    let assigned_ips = match recovery::inspect_provider(lifecycle.ipam, context) {
        recovery::AttachmentProviderObservation::Present { assigned_ips } => assigned_ips,
        observation => {
            return not_ready(OciAttachmentReadinessFailure::ProviderNotReady(format!(
                "{observation:?}"
            )));
        }
    };

    let Some(proxy) = proxy else {
        return not_ready(OciAttachmentReadinessFailure::MissingEgressProxyAssignment);
    };
    match pin_provider.inspect(context.layout, proxy) {
        OciEgressPinObservation::Ready => {}
        OciEgressPinObservation::NotReady { reason } => {
            return not_ready(OciAttachmentReadinessFailure::EgressPinNotReady(reason));
        }
        OciEgressPinObservation::Unknown { reason } => {
            return not_ready(OciAttachmentReadinessFailure::EgressPinUnknown(reason));
        }
    }

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

    if let EgressReadinessState::NotReady(reason) = pep {
        return not_ready(OciAttachmentReadinessFailure::PepNotReady(reason));
    }

    let observation = match NetworkObservation::new(
        record.resource().version().clone(),
        NetworkResourcePhase::Active,
        Some(record.selected_provider_id().clone()),
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
        assigned_ips,
    })
}

fn not_ready(reason: OciAttachmentReadinessFailure) -> OciAttachmentReadinessState {
    OciAttachmentReadinessState::NotReady(reason)
}
