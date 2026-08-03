use nimbus_network::NetworkTlsBehavior;

use super::*;

#[test]
fn production_ingress_capabilities_report_selected_tls_behavior() {
    let cleartext = nimbus_owned_local_ingress_registration(false);
    let terminating = nimbus_owned_local_ingress_registration(true);

    assert_eq!(
        cleartext.ingress().tls_behaviors(),
        &std::collections::BTreeSet::from([NetworkTlsBehavior::Disabled])
    );
    assert_eq!(
        terminating.ingress().tls_behaviors(),
        &std::collections::BTreeSet::from([
            NetworkTlsBehavior::Disabled,
            NetworkTlsBehavior::TerminateAtIngress,
        ])
    );
}
