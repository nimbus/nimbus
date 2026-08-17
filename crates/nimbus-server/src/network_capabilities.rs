//! Closed capability registration for Nimbus-owned local ingress.
//!
//! This module reports facts about the existing server composition. It does
//! not validate readiness, inspect certificates, bind sockets, or provide an
//! effect interface.

use nimbus_network::{
    NetworkAddressFamily, NetworkBindRealmKind, NetworkControlPlaneLocality,
    NetworkEndpointCapabilitySet, NetworkExposure, NetworkForwardingCapabilitySet,
    NetworkForwardingFeature, NetworkIngressCapabilitySet, NetworkIngressProviderRegistration,
    NetworkLifecycleCapabilitySet, NetworkLifecycleFeature, NetworkPortAssignmentMode,
    NetworkProviderId, NetworkSovereigntyCapabilities, NetworkTlsBehavior, PortProtocol,
};

use crate::listener_lease::SERVER_LISTENER_PROVIDER_KEY;

/// Report the closed capability facts of Nimbus-owned workload ingress.
///
/// The concrete adapter owns a transparent TCP proxy over server-held host
/// listeners. It neither terminates TLS nor interprets HTTP paths or
/// WebSockets. Main-server HTTP TLS configuration is therefore deliberately
/// absent from this report. Constructing it performs no provider or socket
/// effect.
pub fn nimbus_owned_workload_ingress_registration() -> NetworkIngressProviderRegistration {
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
        NetworkIngressCapabilitySet::new([]).with_tls_behaviors([
            NetworkTlsBehavior::Disabled,
            NetworkTlsBehavior::Passthrough,
        ]),
        NetworkForwardingCapabilitySet::new([NetworkForwardingFeature::PortForwarding]),
        NetworkLifecycleCapabilitySet::new([
            NetworkLifecycleFeature::DurableInspect,
            NetworkLifecycleFeature::Reconcile,
        ]),
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    )
}

/// Stable provider identity shared by capability reporting and publication.
pub fn nimbus_owned_local_ingress_provider_id() -> NetworkProviderId {
    NetworkProviderId::for_registration_key(SERVER_LISTENER_PROVIDER_KEY)
}

#[cfg(test)]
#[path = "network_capabilities/tests.rs"]
mod tests;
