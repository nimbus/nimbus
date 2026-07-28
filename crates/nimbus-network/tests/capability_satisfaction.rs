use std::collections::BTreeSet;

use nimbus_network::{
    NetworkAttachmentCapabilitySet, NetworkAttachmentMode, NetworkCapabilityRequirements,
    NetworkControlPlaneLocality, NetworkEndpointCapabilitySet, NetworkForwardingCapabilitySet,
    NetworkIngressCapabilitySet, NetworkLifecycleCapabilitySet, NetworkManagementMode, NetworkPlan,
    NetworkPlanContentDigest, NetworkPlanId, NetworkProviderCapabilities, NetworkProviderId,
    NetworkResourceGeneration, NetworkSovereigntyCapabilities, NetworkSovereigntyRequirements,
};

fn empty_requirements() -> NetworkCapabilityRequirements {
    NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(
            NetworkManagementMode::NimbusHostManaged,
            BTreeSet::new(),
            BTreeSet::new(),
        ),
        NetworkEndpointCapabilitySet::new(
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
        ),
        NetworkIngressCapabilitySet::new(BTreeSet::new()),
        NetworkForwardingCapabilitySet::new(BTreeSet::new()),
        NetworkLifecycleCapabilitySet::new(BTreeSet::new()),
        NetworkSovereigntyRequirements::new(
            NetworkControlPlaneLocality::ThirdParty,
            BTreeSet::new(),
            false,
        ),
    )
}

fn host_provider() -> NetworkProviderCapabilities {
    NetworkProviderCapabilities::new(
        NetworkProviderId::for_registration_key("host"),
        NetworkAttachmentCapabilitySet::new(
            NetworkManagementMode::NimbusHostManaged,
            BTreeSet::new(),
            BTreeSet::new(),
        ),
        NetworkEndpointCapabilitySet::new(
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
        ),
        NetworkIngressCapabilitySet::new(BTreeSet::new()),
        NetworkForwardingCapabilitySet::new(BTreeSet::new()),
        NetworkLifecycleCapabilitySet::new(BTreeSet::new()),
        NetworkSovereigntyCapabilities::new(
            NetworkControlPlaneLocality::LocalOnly,
            BTreeSet::new(),
            true,
        ),
    )
}

#[test]
fn explicitly_named_provider_satisfies_explicit_empty_feature_sets() {
    host_provider()
        .ensure_satisfied(&empty_requirements(), [])
        .expect("matching explicit capability facts should satisfy");
}

#[test]
fn missing_attachment_mode_fails_closed_without_selecting_an_alternative() {
    let requirements = NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(
            NetworkManagementMode::NimbusHostManaged,
            BTreeSet::from([NetworkAttachmentMode::IsolatedNamespace]),
            BTreeSet::new(),
        ),
        NetworkEndpointCapabilitySet::new(
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
        ),
        NetworkIngressCapabilitySet::new(BTreeSet::new()),
        NetworkForwardingCapabilitySet::new(BTreeSet::new()),
        NetworkLifecycleCapabilitySet::new(BTreeSet::new()),
        NetworkSovereigntyRequirements::new(
            NetworkControlPlaneLocality::ThirdParty,
            BTreeSet::new(),
            false,
        ),
    );
    let safe_alternative = NetworkProviderId::for_registration_key("isolated");

    let error = host_provider()
        .ensure_satisfied(
            &requirements,
            [safe_alternative.clone(), safe_alternative.clone()],
        )
        .expect_err("missing attachment support must fail closed");

    assert_eq!(error.provider_id(), host_provider().provider_id());
    assert_eq!(error.safe_alternatives(), &[safe_alternative]);
    assert_eq!(error.mismatches().len(), 1);
    assert_eq!(
        error.mismatches()[0].dimension().to_string(),
        "attachment_mode"
    );
}

#[test]
fn network_plan_requires_capability_requirements_as_desired_state() {
    let requirements = empty_requirements();
    let plan = NetworkPlan::new(
        "netplan_01ARZ3NDEKTSV4RRFFQ69G5FAV"
            .parse::<NetworkPlanId>()
            .expect("fixture plan ID should parse"),
        NetworkResourceGeneration::new(1),
        NetworkPlanContentDigest::sha256(b"desired"),
        requirements.clone(),
    );

    assert_eq!(plan.requirements(), &requirements);
}
