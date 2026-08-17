use nimbus_network::{
    NetworkAddressFamily, NetworkAttachmentCapabilitySet, NetworkAttachmentMode,
    NetworkAttachmentProviderRegistration, NetworkBindRealmKind, NetworkCapabilityBundle,
    NetworkCapabilityRegistry, NetworkCapabilityRequirements, NetworkCapabilitySelection,
    NetworkControlPlaneLocality, NetworkEndpointCapabilitySet, NetworkExposure,
    NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet, NetworkIngressFeature,
    NetworkIngressProviderRegistration, NetworkIsolationMode, NetworkLifecycleCapabilitySet,
    NetworkLifecycleFeature, NetworkLifecycleRequirements, NetworkManagementMode, NetworkPlan,
    NetworkPlanContentDigest, NetworkPlanId, NetworkPortAssignmentMode, NetworkProviderId,
    NetworkResourceGeneration, NetworkSovereigntyCapabilities, NetworkSovereigntyRequirements,
    PortProtocol,
};

fn attachment(
    key: &str,
    modes: impl IntoIterator<Item = NetworkAttachmentMode>,
) -> NetworkAttachmentProviderRegistration {
    NetworkAttachmentProviderRegistration::new(
        NetworkProviderId::for_registration_key(key),
        NetworkAttachmentCapabilitySet::new(
            NetworkManagementMode::NimbusHostManaged,
            modes,
            [
                NetworkIsolationMode::WorkloadNamespace,
                NetworkIsolationMode::TenantSegment,
            ],
        ),
        [NetworkAddressFamily::Ipv4],
        NetworkLifecycleCapabilitySet::new([
            NetworkLifecycleFeature::DurableInspect,
            NetworkLifecycleFeature::Reconcile,
            NetworkLifecycleFeature::Delete,
        ]),
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    )
}

fn ingress(key: &str) -> NetworkIngressProviderRegistration {
    NetworkIngressProviderRegistration::new(
        NetworkProviderId::for_registration_key(key),
        NetworkEndpointCapabilitySet::new(
            [NetworkAddressFamily::Ipv4],
            [NetworkBindRealmKind::Host],
            [NetworkExposure::Loopback, NetworkExposure::Private],
            [PortProtocol::Tcp],
            [
                NetworkPortAssignmentMode::Exact,
                NetworkPortAssignmentMode::ProviderAssigned,
            ],
        ),
        NetworkIngressCapabilitySet::new([
            NetworkIngressFeature::PathRouting,
            NetworkIngressFeature::WebSocket,
            NetworkIngressFeature::Streaming,
        ]),
        NetworkForwardingCapabilitySet::new([]),
        NetworkLifecycleCapabilitySet::new([
            NetworkLifecycleFeature::DurableInspect,
            NetworkLifecycleFeature::Reconcile,
            NetworkLifecycleFeature::Delete,
        ]),
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    )
}

fn local_requirements() -> NetworkCapabilityRequirements {
    NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(
            NetworkManagementMode::NimbusHostManaged,
            [NetworkAttachmentMode::IsolatedNamespace],
            [
                NetworkIsolationMode::WorkloadNamespace,
                NetworkIsolationMode::TenantSegment,
            ],
        ),
        NetworkEndpointCapabilitySet::new(
            [NetworkAddressFamily::Ipv4],
            [NetworkBindRealmKind::Host],
            [NetworkExposure::Loopback],
            [PortProtocol::Tcp],
            [NetworkPortAssignmentMode::ProviderAssigned],
        ),
        NetworkIngressCapabilitySet::new([
            NetworkIngressFeature::PathRouting,
            NetworkIngressFeature::WebSocket,
            NetworkIngressFeature::Streaming,
        ]),
        NetworkForwardingCapabilitySet::new([]),
        NetworkLifecycleRequirements::new(
            NetworkLifecycleCapabilitySet::new([
                NetworkLifecycleFeature::DurableInspect,
                NetworkLifecycleFeature::Reconcile,
                NetworkLifecycleFeature::Delete,
            ]),
            NetworkLifecycleCapabilitySet::new([
                NetworkLifecycleFeature::DurableInspect,
                NetworkLifecycleFeature::Reconcile,
                NetworkLifecycleFeature::Delete,
            ]),
        ),
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    )
}

#[test]
fn exact_registered_composition_is_selected_without_fallback() {
    let attachment = attachment(
        "nimbus-sandbox.container.host-managed-attachment",
        [NetworkAttachmentMode::IsolatedNamespace],
    );
    let ingress = ingress("nimbus-server.tcp-listener");
    let selection = NetworkCapabilitySelection::new(
        attachment.provider_id().clone(),
        ingress.provider_id().clone(),
    );
    let registry =
        NetworkCapabilityRegistry::new([NetworkCapabilityBundle::new(attachment, ingress)])
            .expect("one complete bundle should register");

    let selected = registry
        .select_exact(&selection, &local_requirements())
        .expect("the exact complete local composition should satisfy");

    assert_eq!(selected.selection(), selection);
}

#[test]
fn known_but_unregistered_pair_is_not_synthesized() {
    let container = attachment(
        "nimbus-sandbox.container.host-managed-attachment",
        [NetworkAttachmentMode::IsolatedNamespace],
    );
    let krun = attachment(
        "nimbus-sandbox.krun.host-managed-attachment",
        [
            NetworkAttachmentMode::IsolatedNamespace,
            NetworkAttachmentMode::VirtualMachineGuest,
        ],
    );
    let local = ingress("nimbus-server.tcp-listener");
    let alternate = ingress("fixture.alternate-ingress");
    let registry = NetworkCapabilityRegistry::new([
        NetworkCapabilityBundle::new(container.clone(), local.clone()),
        NetworkCapabilityBundle::new(krun, alternate.clone()),
    ])
    .expect("two explicit bundles should register");
    let unregistered = NetworkCapabilitySelection::new(
        container.provider_id().clone(),
        alternate.provider_id().clone(),
    );

    let error = registry
        .select_exact(&unregistered, &local_requirements())
        .expect_err("known providers must not imply an unregistered pair");

    assert!(error.is_unregistered_composition());
    assert_eq!(error.requested_selection(), &unregistered);
}

#[test]
fn exact_selection_and_registry_order_do_not_change_provider_neutral_plan_digest() {
    let requirements = local_requirements();
    let plan = NetworkPlan::new(
        "netplan_01ARZ3NDEKTSV4RRFFQ69G5FAV"
            .parse::<NetworkPlanId>()
            .expect("fixture plan ID should parse"),
        NetworkResourceGeneration::new(1),
        NetworkPlanContentDigest::sha256(b"local-plan"),
        requirements.clone(),
    );
    let expected = plan.digest();
    let container = attachment(
        "nimbus-sandbox.container.host-managed-attachment",
        [NetworkAttachmentMode::IsolatedNamespace],
    );
    let krun = attachment(
        "nimbus-sandbox.krun.host-managed-attachment",
        [
            NetworkAttachmentMode::IsolatedNamespace,
            NetworkAttachmentMode::VirtualMachineGuest,
        ],
    );
    let local_ingress = ingress("nimbus-server.tcp-listener");
    let container_bundle = NetworkCapabilityBundle::new(container, local_ingress.clone());
    let krun_bundle = NetworkCapabilityBundle::new(krun, local_ingress);
    let container_selection = container_bundle.selection();
    let krun_selection = krun_bundle.selection();

    let forward = NetworkCapabilityRegistry::new([container_bundle.clone(), krun_bundle.clone()])
        .expect("forward bundle order should register");
    forward
        .select_exact(&container_selection, &requirements)
        .expect("container selection should satisfy");
    assert_eq!(plan.digest(), expected);

    let reverse = NetworkCapabilityRegistry::new([krun_bundle.clone(), container_bundle])
        .expect("reverse bundle order should register");
    reverse
        .select_exact(&krun_selection, &requirements)
        .expect("krun selection should satisfy");
    assert_eq!(plan.digest(), expected);

    let different_membership = NetworkCapabilityRegistry::new([krun_bundle])
        .expect("single-member registry should register");
    different_membership
        .select_exact(&krun_selection, &requirements)
        .expect("the retained exact selection should satisfy");
    assert_eq!(plan.digest(), expected);
}
