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
    NetworkProviderId, NetworkSovereigntyCapabilities, PortProtocol,
};

use crate::listener_lease::SERVER_LISTENER_PROVIDER_KEY;

pub(crate) fn nimbus_owned_local_ingress_registration(
    tls_configured: bool,
) -> NetworkIngressProviderRegistration {
    let mut ingress_features = BTreeSet::from([
        NetworkIngressFeature::PathRouting,
        NetworkIngressFeature::WebSocket,
        NetworkIngressFeature::Streaming,
    ]);
    if tls_configured {
        ingress_features.insert(NetworkIngressFeature::TlsTermination);
    }

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
        NetworkIngressCapabilitySet::new(ingress_features),
        NetworkForwardingCapabilitySet::new([]),
        NetworkLifecycleCapabilitySet::new([
            NetworkLifecycleFeature::DurableInspect,
            NetworkLifecycleFeature::Reconcile,
            NetworkLifecycleFeature::Delete,
        ]),
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    )
}
