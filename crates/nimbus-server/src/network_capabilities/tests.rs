use std::collections::BTreeSet;

use nimbus_network::{
    NetworkForwardingFeature, NetworkIngressFeature, NetworkLifecycleFeature, NetworkTlsBehavior,
};

use super::*;

#[test]
fn workload_ingress_report_is_transparent_tcp_truth_independent_of_main_tls() {
    let registration = nimbus_owned_workload_ingress_registration();

    assert_eq!(
        registration.ingress().tls_behaviors(),
        &BTreeSet::from([
            NetworkTlsBehavior::Disabled,
            NetworkTlsBehavior::Passthrough,
        ])
    );
    assert!(
        !registration
            .ingress()
            .tls_behaviors()
            .contains(&NetworkTlsBehavior::TerminateAtIngress)
    );
    assert_eq!(
        registration.ingress().features(),
        &BTreeSet::<NetworkIngressFeature>::new(),
        "the transparent proxy owns no HTTP path, WebSocket, or streaming semantics"
    );
    assert_eq!(
        registration.forwarding().features(),
        &BTreeSet::from([NetworkForwardingFeature::PortForwarding])
    );
    assert_eq!(
        registration.lifecycle().features(),
        &BTreeSet::from([
            NetworkLifecycleFeature::DurableInspect,
            NetworkLifecycleFeature::Reconcile,
        ])
    );
}
