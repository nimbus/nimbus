//! Transport-free connectivity-resource control-plane primitives.
//!
//! This crate owns portable network intent, identity, leases, state
//! transitions, and reconciliation contracts. Provider effects such as socket
//! binding, packet forwarding, namespaces, bridges, firewalls, TLS
//! termination, and protocol parsing remain in their effect-owning crates.
//!
//! `nimbus-core` is this crate's only workspace dependency. Upper-layer crates
//! inject provider capabilities without creating reverse dependencies.

mod endpoint;
mod identity;
mod plan;
mod provider;
mod segment;
mod state;
mod state_store;
mod status;

pub use endpoint::{EndpointProtocol, PublishedEndpoint};
pub use identity::{
    IngressRouteId, ListenerId, NetworkAttachmentId, NetworkLeaseEpoch, NetworkPlanId,
    NetworkProviderId, NetworkResourceGeneration, NetworkResourceIdParseError,
    NetworkResourceIdParseErrorKind, NetworkSegmentId, PortLeaseId, PublishedEndpointId,
};
pub use plan::{
    NetworkPlan, NetworkPlanDigest, NetworkPlanDigestParseError, NetworkPlanUpdate,
    NetworkPlanUpdateError,
};
pub use provider::{NetworkProviderHandle, NetworkProviderHandleError};
pub use segment::AllocatedSegment;
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
