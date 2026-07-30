//! Transport-free connectivity-resource control-plane primitives.
//!
//! This crate owns portable network intent, identity, leases, state
//! transitions, and reconciliation contracts. Provider effects such as socket
//! binding, packet forwarding, namespaces, bridges, firewalls, TLS
//! termination, and protocol parsing remain in their effect-owning crates.
//!
//! `nimbus-core` is this crate's only workspace dependency. Upper-layer crates
//! inject provider capabilities without creating reverse dependencies.

mod attachment_state;
mod capability;
mod capability_registry;
mod endpoint;
mod identity;
mod manager;
mod plan;
mod port_lease;
mod provider;
mod readiness;
mod segment;
mod state;
mod state_store;
mod status;

pub use attachment_state::{
    DurableNetworkAttachmentState, LocalNetworkAttachmentAuthority, NetworkAttachmentStateError,
};
pub use capability::{
    NetworkAddressFamily, NetworkAttachmentCapabilitySet, NetworkAttachmentMode,
    NetworkBindRealmKind, NetworkCapabilityDimension, NetworkCapabilityFactError,
    NetworkCapabilityMismatch, NetworkCapabilityRequirements, NetworkControlPlaneLocality,
    NetworkEndpointCapabilitySet, NetworkExposure, NetworkExternalDependency,
    NetworkForwardingCapabilitySet, NetworkForwardingFeature, NetworkIngressCapabilitySet,
    NetworkIngressFeature, NetworkIsolationMode, NetworkLifecycleCapabilitySet,
    NetworkLifecycleFeature, NetworkManagementMode, NetworkPortAssignmentMode,
    NetworkSovereigntyCapabilities, NetworkSovereigntyRequirements,
};
pub use capability_registry::{
    NetworkAttachmentProviderRegistration, NetworkCapabilityBundle,
    NetworkCapabilityProviderFailure, NetworkCapabilityRegistry, NetworkCapabilityRegistryError,
    NetworkCapabilityRole, NetworkCapabilitySelection, NetworkCapabilitySelectionError,
    NetworkIngressProviderRegistration,
};
pub use endpoint::{EndpointProtocol, PublishedEndpoint};
pub use identity::{
    IngressRouteId, ListenerId, NetworkAttachmentId, NetworkLeaseEpoch, NetworkPlanId,
    NetworkProviderId, NetworkResourceGeneration, NetworkResourceIdParseError,
    NetworkResourceIdParseErrorKind, NetworkSegmentId, PortLeaseId, PublishedEndpointId,
};
pub use manager::{
    LocalNetworkAuthority, LocalNetworkAuthorityRootMismatch, LocalNetworkManager,
    LocalNetworkManagerBootstrap, LocalNetworkManagerError,
};
pub use plan::{
    NetworkPlan, NetworkPlanContentDigest, NetworkPlanDigest, NetworkPlanDigestParseError,
    NetworkPlanUpdate, NetworkPlanUpdateError,
};
pub use port_lease::{
    LocalPortLeaseAuthority, NetworkReservationLifetimeAttempt, NetworkReservationLifetimeGuard,
    PortAddressFamily, PortBindAttempt, PortBindAttemptError, PortBindClaim, PortBindFailure,
    PortBindFailureKind, PortBindRealm, PortBindRealmError, PortBindRealmErrorKind, PortBindTarget,
    PortBindTargetError, PortBindingMismatch, PortBindingProvenance, PortBindingSpec,
    PortBoundEndpoint, PortBoundEndpointError, PortExposure, PortIpv6Overlap, PortIsolatedRealm,
    PortLeaseAccounting, PortLeaseBatchReservationWithLifetimes, PortLeaseBinding,
    PortLeaseEffectScope, PortLeaseError, PortLeaseFence, PortLeaseFenceMismatch,
    PortLeaseLifetime, PortLeaseLifetimeGeneration, PortLeaseLifetimeGuard,
    PortLeaseLifetimeReconciliation, PortLeaseOperation, PortLeasePhase, PortLeaseRecord,
    PortLeaseRecoveryAttempt, PortLeaseRecoveryGuard, PortLeaseRequest,
    PortLeaseReservationWithLifetime, PortProtocol, PortPublicationIntent, PortRange,
    PortRangeError, PortRequestMode, TenantPublishedPortLimit,
};
pub use provider::{NetworkProviderHandle, NetworkProviderHandleError, NetworkReservationClaim};
pub use readiness::{
    NetworkReadinessDependency, NetworkReadinessDependencyError, NetworkReadinessEvaluationError,
    NetworkReadinessEvidence, NetworkReadinessEvidenceError, NetworkReadinessRequirement,
    NetworkReadinessRequirementError,
};
pub use segment::{
    AllocatedSegment, NetworkAttachmentReservationObservation, NetworkAttachmentReservationState,
    NetworkAttachmentSegmentAssociation, NetworkSegmentAllocator, NetworkSegmentCleanup,
    NetworkSegmentFinalizeOutcome, NetworkSegmentGrowth, NetworkSegmentQuarantineOutcome,
    NetworkSegmentReleaseOutcome,
};
pub use state::{
    DurableNetworkResourceState, NetworkResourceId, NetworkResourcePhase, NetworkResourceVersion,
    NetworkStateError, NetworkStateMutation, NetworkStateTransition, NetworkTransitionEvidence,
};
pub use state_store::{
    LocalNetworkStateStore, LocalNetworkStateStoreOptions, NetworkStatePartition,
    NetworkStateStoreError, NetworkStateTransactionError,
};
pub use status::{
    NetworkCondition, NetworkConditionKind, NetworkConditionState, NetworkObservation,
    NetworkObservationError, NetworkStatus, NetworkStatusError, NetworkStatusUpdate,
};

#[cfg(feature = "test-support")]
pub use state_store::test_support;

#[cfg(test)]
mod tests {
    use nimbus_core::Cidr;

    #[test]
    fn core_network_vocabulary_is_available_at_the_dependency_boundary() {
        let cidr =
            Cidr::new("10.89.0.0".parse().expect("valid IPv4 address"), 24).expect("valid CIDR");

        assert_eq!(cidr.to_string(), "10.89.0.0/24");
    }
}
