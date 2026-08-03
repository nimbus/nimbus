//! Closed capability registration for Nimbus-owned local ingress.
//!
//! This module reports facts about the existing server composition. It does
//! not validate readiness, inspect certificates, bind sockets, or provide an
//! effect interface.

use std::collections::BTreeSet;

use nimbus_network::{
    NetworkAddressFamily, NetworkBindRealmKind, NetworkControlPlaneLocality,
    NetworkEndpointCapabilitySet, NetworkExposure, NetworkForwardingCapabilitySet,
    NetworkIngressCapabilitySet, NetworkIngressFeature, NetworkIngressProviderRegistration,
    NetworkLifecycleCapabilitySet, NetworkLifecycleFeature, NetworkPortAssignmentMode,
    NetworkProviderId, NetworkSovereigntyCapabilities, NetworkTlsBehavior, PortProtocol,
};

use crate::listener_lease::SERVER_LISTENER_PROVIDER_KEY;

/// Report the closed capability facts of Nimbus-owned local ingress.
///
/// This source-owned report is effect-free: it does not construct listener
/// authority, inspect certificates, bind sockets, or validate readiness.
/// Callers that must freeze provider selection before constructing
/// [`crate::ServeOptions`] may use this function directly.
pub fn nimbus_owned_local_ingress_registration(
    tls_configured: bool,
) -> NetworkIngressProviderRegistration {
    let ingress_features = BTreeSet::from([
        NetworkIngressFeature::PathRouting,
        NetworkIngressFeature::WebSocket,
        NetworkIngressFeature::Streaming,
    ]);
    // The registration describes the aggregate Nimbus-owned ingress surface:
    // TLS terminates on the main HTTP listener while sibling wire listeners
    // remain plain TCP.
    let tls_behaviors = if tls_configured {
        BTreeSet::from([
            NetworkTlsBehavior::Disabled,
            NetworkTlsBehavior::TerminateAtIngress,
        ])
    } else {
        BTreeSet::from([NetworkTlsBehavior::Disabled])
    };

    NetworkIngressProviderRegistration::new(
        NetworkProviderId::for_registration_key(SERVER_LISTENER_PROVIDER_KEY),
        NetworkEndpointCapabilitySet::new(
            [NetworkAddressFamily::Ipv4, NetworkAddressFamily::Ipv6],
            [NetworkBindRealmKind::Host],
            [
                NetworkExposure::Loopback,
                NetworkExposure::Private,
                NetworkExposure::Public,
            ],
            [PortProtocol::Tcp],
            [
                NetworkPortAssignmentMode::Exact,
                NetworkPortAssignmentMode::ProviderAssigned,
            ],
        ),
        NetworkIngressCapabilitySet::new(ingress_features).with_tls_behaviors(tls_behaviors),
        NetworkForwardingCapabilitySet::new([]),
        NetworkLifecycleCapabilitySet::new([
            NetworkLifecycleFeature::DurableInspect,
            NetworkLifecycleFeature::Reconcile,
            NetworkLifecycleFeature::Delete,
        ]),
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    )
}

#[cfg(test)]
#[path = "network_capabilities/tests.rs"]
mod tests;
