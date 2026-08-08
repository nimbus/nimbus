use std::collections::BTreeSet;

use nimbus_network::{
    NetworkAddressFamily, NetworkAttachmentCapabilitySet, NetworkAttachmentMode,
    NetworkAttachmentProviderRegistration, NetworkCapabilityBundle, NetworkCapabilityDimension,
    NetworkCapabilityRegistry, NetworkCapabilityRequirements, NetworkCapabilityRole,
    NetworkControlPlaneLocality, NetworkEndpointCapabilitySet, NetworkForwardingCapabilitySet,
    NetworkIngressCapabilitySet, NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet,
    NetworkLifecycleRequirements, NetworkManagementMode, NetworkPlan, NetworkPlanContentDigest,
    NetworkPlanId, NetworkProviderId, NetworkResourceGeneration, NetworkSovereigntyCapabilities,
    NetworkSovereigntyRequirements,
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
        NetworkLifecycleRequirements::new(
            NetworkLifecycleCapabilitySet::new(BTreeSet::new()),
            NetworkLifecycleCapabilitySet::new(BTreeSet::new()),
        ),
        NetworkSovereigntyRequirements::new(
            NetworkControlPlaneLocality::ThirdParty,
            BTreeSet::new(),
            false,
        ),
    )
}

fn attachment(
    key: &str,
    modes: impl IntoIterator<Item = NetworkAttachmentMode>,
) -> NetworkAttachmentProviderRegistration {
    NetworkAttachmentProviderRegistration::new(
        NetworkProviderId::for_registration_key(key),
        NetworkAttachmentCapabilitySet::new(
            NetworkManagementMode::NimbusHostManaged,
            modes,
            BTreeSet::new(),
        ),
        BTreeSet::<NetworkAddressFamily>::new(),
        NetworkLifecycleCapabilitySet::new(BTreeSet::new()),
        NetworkSovereigntyCapabilities::new(
            NetworkControlPlaneLocality::LocalOnly,
            BTreeSet::new(),
            true,
        ),
    )
}

fn ingress(key: &str) -> NetworkIngressProviderRegistration {
    NetworkIngressProviderRegistration::new(
        NetworkProviderId::for_registration_key(key),
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

fn bundle(
    attachment_key: &str,
    modes: impl IntoIterator<Item = NetworkAttachmentMode>,
    ingress_key: &str,
) -> NetworkCapabilityBundle {
    NetworkCapabilityBundle::new(attachment(attachment_key, modes), ingress(ingress_key))
}

#[test]
fn explicitly_named_composition_satisfies_explicit_empty_feature_sets() {
    let bundle = bundle("host-attachment", [], "host-ingress");
    let selection = bundle.selection();
    let registry =
        NetworkCapabilityRegistry::new([bundle]).expect("complete composition should register");

    let selected = registry
        .select_exact(&selection, &empty_requirements())
        .expect("matching explicit capability facts should satisfy");

    assert_eq!(selected.selection(), selection);
}

#[test]
fn missing_attachment_mode_fails_closed_without_selecting_an_alternative() {
    let requested_bundle = bundle("host-attachment", [], "host-ingress");
    let requested = requested_bundle.selection();
    let alternative_bundle = bundle(
        "isolated-attachment",
        [NetworkAttachmentMode::IsolatedNamespace],
        "isolated-ingress",
    );
    let safe_alternative = alternative_bundle.selection();
    let registry = NetworkCapabilityRegistry::new([alternative_bundle, requested_bundle])
        .expect("complete compositions should register");
    let mut requirements = empty_requirements();
    requirements = NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(
            NetworkManagementMode::NimbusHostManaged,
            [NetworkAttachmentMode::IsolatedNamespace],
            BTreeSet::new(),
        ),
        requirements.endpoint().clone(),
        requirements.ingress().clone(),
        requirements.forwarding().clone(),
        requirements.lifecycle().clone(),
        requirements.sovereignty().clone(),
    );

    let error = registry
        .select_exact(&requested, &requirements)
        .expect_err("missing attachment support must fail closed");

    assert_eq!(error.requested_selection(), &requested);
    assert_eq!(error.safe_alternatives(), &[safe_alternative]);
    assert_eq!(error.provider_failures().len(), 1);
    assert_eq!(
        error.provider_failures()[0].role(),
        NetworkCapabilityRole::Attachment
    );
    assert_eq!(
        error.provider_failures()[0].mismatches()[0].dimension(),
        NetworkCapabilityDimension::AttachmentMode
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
