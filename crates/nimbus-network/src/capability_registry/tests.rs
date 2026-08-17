use std::collections::BTreeSet;

use super::*;
use crate::{
    NetworkAttachmentMode, NetworkBindRealmKind, NetworkCapabilityDimension,
    NetworkControlPlaneLocality, NetworkEndpointCapabilitySet, NetworkExposure,
    NetworkExternalDependency, NetworkForwardingCapabilitySet, NetworkForwardingFeature,
    NetworkIngressCapabilitySet, NetworkIngressFeature, NetworkIsolationMode,
    NetworkLifecycleFeature, NetworkLifecycleRequirements, NetworkManagementMode,
    NetworkPlanContentDigest, NetworkPlanDigest, NetworkPortAssignmentMode,
    NetworkSovereigntyRequirements, PortProtocol,
};

fn attachment(
    key: &str,
    address_families: impl IntoIterator<Item = NetworkAddressFamily>,
    lifecycle: impl IntoIterator<Item = NetworkLifecycleFeature>,
    offline_restart: bool,
) -> NetworkAttachmentProviderRegistration {
    NetworkAttachmentProviderRegistration::new(
        NetworkProviderId::for_registration_key(key),
        NetworkAttachmentCapabilitySet::new(
            NetworkManagementMode::NimbusHostManaged,
            [NetworkAttachmentMode::IsolatedNamespace],
            [
                NetworkIsolationMode::WorkloadNamespace,
                NetworkIsolationMode::TenantSegment,
            ],
        ),
        address_families,
        NetworkLifecycleCapabilitySet::new(lifecycle),
        NetworkSovereigntyCapabilities::new(
            NetworkControlPlaneLocality::LocalOnly,
            [],
            offline_restart,
        ),
    )
}

fn ingress(
    key: &str,
    address_families: impl IntoIterator<Item = NetworkAddressFamily>,
    lifecycle: impl IntoIterator<Item = NetworkLifecycleFeature>,
    offline_restart: bool,
) -> NetworkIngressProviderRegistration {
    NetworkIngressProviderRegistration::new(
        NetworkProviderId::for_registration_key(key),
        NetworkEndpointCapabilitySet::new(
            address_families,
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
        NetworkLifecycleCapabilitySet::new(lifecycle),
        NetworkSovereigntyCapabilities::new(
            NetworkControlPlaneLocality::LocalOnly,
            [],
            offline_restart,
        ),
    )
}

fn complete_lifecycle() -> [NetworkLifecycleFeature; 3] {
    [
        NetworkLifecycleFeature::DurableInspect,
        NetworkLifecycleFeature::Reconcile,
        NetworkLifecycleFeature::Delete,
    ]
}

fn inspect_reconcile_lifecycle() -> [NetworkLifecycleFeature; 2] {
    [
        NetworkLifecycleFeature::DurableInspect,
        NetworkLifecycleFeature::Reconcile,
    ]
}

fn complete_lifecycle_requirements() -> NetworkLifecycleRequirements {
    NetworkLifecycleRequirements::new(
        NetworkLifecycleCapabilitySet::new(complete_lifecycle()),
        NetworkLifecycleCapabilitySet::new(complete_lifecycle()),
    )
}

fn requirements(
    address_families: impl IntoIterator<Item = NetworkAddressFamily>,
    lifecycle: NetworkLifecycleRequirements,
    offline_restart: bool,
) -> NetworkCapabilityRequirements {
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
            address_families,
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
        lifecycle,
        NetworkSovereigntyRequirements::new(
            NetworkControlPlaneLocality::LocalOnly,
            [],
            offline_restart,
        ),
    )
}

fn bundle(attachment_key: &str, ingress_key: &str) -> NetworkCapabilityBundle {
    NetworkCapabilityBundle::new(
        attachment(
            attachment_key,
            [NetworkAddressFamily::Ipv4],
            complete_lifecycle(),
            true,
        ),
        ingress(
            ingress_key,
            [NetworkAddressFamily::Ipv4],
            complete_lifecycle(),
            true,
        ),
    )
}

#[test]
fn identical_duplicate_bundle_is_idempotent() {
    let bundle = bundle("attachment-a", "ingress-a");
    let registry = NetworkCapabilityRegistry::new([bundle.clone(), bundle])
        .expect("identical duplicate should be idempotent");

    assert_eq!(registry.selections().len(), 1);
}

#[test]
fn same_role_and_provider_with_divergent_report_is_rejected() {
    let first = bundle("attachment-a", "ingress-a");
    let divergent = NetworkCapabilityBundle::new(
        attachment(
            "attachment-a",
            [NetworkAddressFamily::Ipv4, NetworkAddressFamily::Ipv6],
            complete_lifecycle(),
            true,
        ),
        ingress(
            "ingress-a",
            [NetworkAddressFamily::Ipv4],
            complete_lifecycle(),
            true,
        ),
    );

    let errors = [
        NetworkCapabilityRegistry::new([first.clone(), divergent.clone()])
            .expect_err("divergent duplicate composition must fail"),
        NetworkCapabilityRegistry::new([divergent, first])
            .expect_err("input order must not change the divergent-report identity"),
    ];

    assert_eq!(errors[0], errors[1]);
    assert_eq!(
        errors[0],
        NetworkCapabilityRegistryError::ProviderReportConflict {
            role: NetworkCapabilityRole::Attachment,
            provider_id: NetworkProviderId::for_registration_key("attachment-a"),
        }
    );
}

#[test]
fn one_provider_identity_cannot_cross_roles() {
    let error = NetworkCapabilityRegistry::new([bundle("same-provider", "same-provider")])
        .expect_err("one identity cannot own two capability roles");

    assert_eq!(
        error,
        NetworkCapabilityRegistryError::ProviderRoleConflict {
            provider_id: NetworkProviderId::for_registration_key("same-provider"),
        }
    );
}

#[test]
fn crossed_role_conflict_is_independent_of_bundle_input_order() {
    let first = bundle("provider-a", "provider-b");
    let crossed = bundle("provider-b", "provider-a");

    let errors = [
        NetworkCapabilityRegistry::new([first.clone(), crossed.clone()])
            .expect_err("crossed provider roles must conflict"),
        NetworkCapabilityRegistry::new([crossed, first])
            .expect_err("input order must not change the crossed-role conflict"),
    ];

    assert_eq!(errors[0], errors[1]);
    let canonical_conflict = [
        NetworkProviderId::for_registration_key("provider-a"),
        NetworkProviderId::for_registration_key("provider-b"),
    ]
    .into_iter()
    .min()
    .expect("two crossed providers have one canonical minimum");
    assert_eq!(
        errors[0],
        NetworkCapabilityRegistryError::ProviderRoleConflict {
            provider_id: canonical_conflict,
        }
    );
}

#[test]
fn known_provider_ids_do_not_create_an_implicit_pair() {
    let first = bundle("attachment-a", "ingress-a");
    let second = bundle("attachment-b", "ingress-b");
    let registry =
        NetworkCapabilityRegistry::new([first, second]).expect("explicit bundles should register");
    let requested = NetworkCapabilitySelection::new(
        NetworkProviderId::for_registration_key("attachment-a"),
        NetworkProviderId::for_registration_key("ingress-b"),
    );

    let error = registry
        .select_exact(
            &requested,
            &requirements(
                [NetworkAddressFamily::Ipv4],
                complete_lifecycle_requirements(),
                true,
            ),
        )
        .expect_err("known IDs must not imply compatibility");

    assert!(error.is_unregistered_composition());
    assert!(error.missing_roles().is_empty());
    assert_eq!(error.registered_compositions().len(), 2);
}

#[test]
fn unknown_provider_roles_are_named_in_stable_role_order() {
    let registry = NetworkCapabilityRegistry::new([bundle("attachment-a", "ingress-a")])
        .expect("bundle should register");
    let requested = NetworkCapabilitySelection::new(
        NetworkProviderId::for_registration_key("missing-attachment"),
        NetworkProviderId::for_registration_key("missing-ingress"),
    );

    let error = registry
        .select_exact(
            &requested,
            &requirements(
                [NetworkAddressFamily::Ipv4],
                complete_lifecycle_requirements(),
                true,
            ),
        )
        .expect_err("unknown role providers must fail");

    assert_eq!(
        error.missing_roles(),
        &[
            NetworkCapabilityRole::Attachment,
            NetworkCapabilityRole::Ingress,
        ]
    );
}

#[test]
fn exact_sovereignty_selection_returns_the_authenticated_bundle() {
    let offered = bundle("attachment-local", "ingress-local");
    let selection = offered.selection();
    let expected_evidence = offered.selection_evidence();
    let registry = NetworkCapabilityRegistry::new([offered]).expect("bundle should register");

    let selected = registry
        .select_exact_sovereignty(
            &selection,
            &NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
        )
        .expect("exact local providers should satisfy local sovereignty");

    assert_eq!(selected.selection_evidence(), expected_evidence);
}

#[test]
fn exact_sovereignty_selection_attributes_each_provider_failure() {
    let offered = NetworkCapabilityBundle::new(
        NetworkAttachmentProviderRegistration::new(
            NetworkProviderId::for_registration_key("attachment-external"),
            NetworkAttachmentCapabilitySet::new(
                NetworkManagementMode::NimbusHostManaged,
                [NetworkAttachmentMode::IsolatedNamespace],
                [NetworkIsolationMode::TenantSegment],
            ),
            [NetworkAddressFamily::Ipv4],
            NetworkLifecycleCapabilitySet::new(complete_lifecycle()),
            NetworkSovereigntyCapabilities::new(
                NetworkControlPlaneLocality::ThirdParty,
                [NetworkExternalDependency::ExternalControlPlane],
                false,
            ),
        ),
        NetworkIngressProviderRegistration::new(
            NetworkProviderId::for_registration_key("ingress-external"),
            NetworkEndpointCapabilitySet::new([], [], [], [], []),
            NetworkIngressCapabilitySet::new([]),
            NetworkForwardingCapabilitySet::new([]),
            NetworkLifecycleCapabilitySet::new(complete_lifecycle()),
            NetworkSovereigntyCapabilities::new(
                NetworkControlPlaneLocality::ThirdParty,
                [NetworkExternalDependency::Dns],
                false,
            ),
        ),
    );
    let selection = offered.selection();
    let registry = NetworkCapabilityRegistry::new([offered]).expect("bundle should register");

    let error = registry
        .select_exact_sovereignty(
            &selection,
            &NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
        )
        .expect_err("external providers must fail local-only sovereignty");

    assert_eq!(error.provider_failures().len(), 2);
    assert_eq!(
        error
            .provider_failures()
            .iter()
            .map(NetworkCapabilityProviderFailure::role)
            .collect::<Vec<_>>(),
        [
            NetworkCapabilityRole::Attachment,
            NetworkCapabilityRole::Ingress,
        ]
    );
    for failure in error.provider_failures() {
        assert_eq!(
            failure
                .mismatches()
                .iter()
                .map(NetworkCapabilityMismatch::dimension)
                .collect::<Vec<_>>(),
            [
                NetworkCapabilityDimension::ControlPlaneLocality,
                NetworkCapabilityDimension::ExternalDependency,
                NetworkCapabilityDimension::OfflineRestart,
            ]
        );
    }
}

#[test]
fn shared_dimensions_are_checked_independently_for_both_roles() {
    let weak = NetworkCapabilityBundle::new(
        attachment(
            "attachment-weak",
            [NetworkAddressFamily::Ipv4],
            [NetworkLifecycleFeature::DurableInspect],
            false,
        ),
        ingress(
            "ingress-weak",
            [NetworkAddressFamily::Ipv4],
            [NetworkLifecycleFeature::DurableInspect],
            false,
        ),
    );
    let selection = weak.selection();
    let registry = NetworkCapabilityRegistry::new([weak]).expect("bundle should register");

    let error = registry
        .select_exact(
            &selection,
            &requirements(
                [NetworkAddressFamily::Ipv4, NetworkAddressFamily::Ipv6],
                complete_lifecycle_requirements(),
                true,
            ),
        )
        .expect_err("both roles must carry shared evidence");

    assert_eq!(error.provider_failures().len(), 2);
    assert_eq!(
        error
            .provider_failures()
            .iter()
            .map(NetworkCapabilityProviderFailure::role)
            .collect::<Vec<_>>(),
        [
            NetworkCapabilityRole::Attachment,
            NetworkCapabilityRole::Ingress,
        ]
    );
    for failure in error.provider_failures() {
        assert_eq!(
            failure
                .mismatches()
                .iter()
                .map(NetworkCapabilityMismatch::dimension)
                .collect::<Vec<_>>(),
            [
                NetworkCapabilityDimension::AddressFamily,
                NetworkCapabilityDimension::LifecycleFeature,
                NetworkCapabilityDimension::LifecycleFeature,
                NetworkCapabilityDimension::OfflineRestart,
            ]
        );
    }
}

#[test]
fn role_specific_lifecycle_accepts_attachment_delete_without_ingress_delete() {
    let offered = NetworkCapabilityBundle::new(
        attachment(
            "attachment-complete",
            [NetworkAddressFamily::Ipv4],
            complete_lifecycle(),
            true,
        ),
        ingress(
            "ingress-observable",
            [NetworkAddressFamily::Ipv4],
            inspect_reconcile_lifecycle(),
            true,
        ),
    );
    let selection = offered.selection();
    let registry = NetworkCapabilityRegistry::new([offered]).expect("bundle should register");
    let required = requirements(
        [NetworkAddressFamily::Ipv4],
        NetworkLifecycleRequirements::new(
            NetworkLifecycleCapabilitySet::new(complete_lifecycle()),
            NetworkLifecycleCapabilitySet::new(inspect_reconcile_lifecycle()),
        ),
        true,
    );

    registry
        .select_exact(&selection, &required)
        .expect("ingress deletion is not required for NNC6.4 publication");
}

#[test]
fn missing_attachment_delete_fails_only_attachment_role() {
    let offered = NetworkCapabilityBundle::new(
        attachment(
            "attachment-without-delete",
            [NetworkAddressFamily::Ipv4],
            inspect_reconcile_lifecycle(),
            true,
        ),
        ingress(
            "ingress-complete",
            [NetworkAddressFamily::Ipv4],
            inspect_reconcile_lifecycle(),
            true,
        ),
    );
    let selection = offered.selection();
    let registry = NetworkCapabilityRegistry::new([offered]).expect("bundle should register");
    let error = registry
        .select_exact(
            &selection,
            &requirements(
                [NetworkAddressFamily::Ipv4],
                NetworkLifecycleRequirements::new(
                    NetworkLifecycleCapabilitySet::new(complete_lifecycle()),
                    NetworkLifecycleCapabilitySet::new(inspect_reconcile_lifecycle()),
                ),
                true,
            ),
        )
        .expect_err("attachment deletion must fail closed");

    assert!(error.safe_alternatives().is_empty());
    assert_eq!(error.provider_failures().len(), 1);
    assert_eq!(
        error.provider_failures()[0].role(),
        NetworkCapabilityRole::Attachment
    );
    assert_eq!(
        error.provider_failures()[0].mismatches(),
        &[NetworkCapabilityMismatch::LifecycleFeature {
            required: NetworkLifecycleFeature::Delete,
        }]
    );
}

#[test]
fn missing_ingress_reconcile_fails_only_ingress_role() {
    let offered = NetworkCapabilityBundle::new(
        attachment(
            "attachment-complete",
            [NetworkAddressFamily::Ipv4],
            complete_lifecycle(),
            true,
        ),
        ingress(
            "ingress-without-reconcile",
            [NetworkAddressFamily::Ipv4],
            [NetworkLifecycleFeature::DurableInspect],
            true,
        ),
    );
    let selection = offered.selection();
    let registry = NetworkCapabilityRegistry::new([offered]).expect("bundle should register");
    let error = registry
        .select_exact(
            &selection,
            &requirements(
                [NetworkAddressFamily::Ipv4],
                NetworkLifecycleRequirements::new(
                    NetworkLifecycleCapabilitySet::new(complete_lifecycle()),
                    NetworkLifecycleCapabilitySet::new(inspect_reconcile_lifecycle()),
                ),
                true,
            ),
        )
        .expect_err("ingress reconciliation must fail closed");

    assert!(error.safe_alternatives().is_empty());
    assert_eq!(error.provider_failures().len(), 1);
    assert_eq!(
        error.provider_failures()[0].role(),
        NetworkCapabilityRole::Ingress
    );
    assert_eq!(
        error.provider_failures()[0].mismatches(),
        &[NetworkCapabilityMismatch::LifecycleFeature {
            required: NetworkLifecycleFeature::Reconcile,
        }]
    );
}

#[test]
fn role_specific_lifecycle_wire_rejects_uniform_legacy_shape_and_binds_roles() {
    let required = requirements(
        [NetworkAddressFamily::Ipv4],
        NetworkLifecycleRequirements::new(
            NetworkLifecycleCapabilitySet::new([NetworkLifecycleFeature::Delete]),
            NetworkLifecycleCapabilitySet::new([NetworkLifecycleFeature::Reconcile]),
        ),
        true,
    );
    let encoded = serde_json::to_value(&required).expect("requirements should serialize");
    assert_eq!(
        encoded.get("lifecycle"),
        Some(&serde_json::json!({
            "attachment": { "features": ["delete"] },
            "ingress": { "features": ["reconcile"] },
        }))
    );

    let mut legacy = encoded;
    legacy["lifecycle"] = serde_json::json!({ "features": ["delete"] });
    assert!(
        serde_json::from_value::<NetworkCapabilityRequirements>(legacy).is_err(),
        "the uniform lifecycle wire shape must fail closed"
    );

    let swapped = requirements(
        [NetworkAddressFamily::Ipv4],
        NetworkLifecycleRequirements::new(
            NetworkLifecycleCapabilitySet::new([NetworkLifecycleFeature::Reconcile]),
            NetworkLifecycleCapabilitySet::new([NetworkLifecycleFeature::Delete]),
        ),
        true,
    );
    assert_ne!(
        serde_json::to_vec(&required).expect("requirements should serialize"),
        serde_json::to_vec(&swapped).expect("requirements should serialize")
    );
    assert_ne!(
        NetworkPlanDigest::for_content(
            NetworkPlanContentDigest::sha256(b"role-specific-lifecycle"),
            &required,
            &[],
        ),
        NetworkPlanDigest::for_content(
            NetworkPlanContentDigest::sha256(b"role-specific-lifecycle"),
            &swapped,
            &[],
        ),
    );
}

#[test]
fn complete_mismatch_diagnostic_has_fixed_role_and_dimension_order() {
    let offered = NetworkCapabilityBundle::new(
        NetworkAttachmentProviderRegistration::new(
            NetworkProviderId::for_registration_key("attachment-weak"),
            NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []),
            [],
            NetworkLifecycleCapabilitySet::new([]),
            NetworkSovereigntyCapabilities::new(
                NetworkControlPlaneLocality::ThirdParty,
                [NetworkExternalDependency::Dns],
                false,
            ),
        ),
        NetworkIngressProviderRegistration::new(
            NetworkProviderId::for_registration_key("ingress-weak"),
            NetworkEndpointCapabilitySet::new([], [], [], [], []),
            NetworkIngressCapabilitySet::new([]),
            NetworkForwardingCapabilitySet::new([]),
            NetworkLifecycleCapabilitySet::new([]),
            NetworkSovereigntyCapabilities::new(
                NetworkControlPlaneLocality::ThirdParty,
                [NetworkExternalDependency::Dns],
                false,
            ),
        ),
    );
    let selection = offered.selection();
    let registry = NetworkCapabilityRegistry::new([offered]).expect("bundle should register");
    let requirements = NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(
            NetworkManagementMode::ProviderManaged,
            [NetworkAttachmentMode::IsolatedNamespace],
            [NetworkIsolationMode::TenantSegment],
        ),
        NetworkEndpointCapabilitySet::new(
            [NetworkAddressFamily::Ipv6],
            [NetworkBindRealmKind::ProvenIsolated],
            [NetworkExposure::Public],
            [PortProtocol::Udp],
            [NetworkPortAssignmentMode::NimbusAllocatedRange],
        ),
        NetworkIngressCapabilitySet::new([NetworkIngressFeature::TlsTermination]),
        NetworkForwardingCapabilitySet::new([NetworkForwardingFeature::ConnectionDrain]),
        NetworkLifecycleRequirements::new(
            NetworkLifecycleCapabilitySet::new([NetworkLifecycleFeature::Delete]),
            NetworkLifecycleCapabilitySet::new([NetworkLifecycleFeature::Delete]),
        ),
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );

    let error = registry
        .select_exact(&selection, &requirements)
        .expect_err("weak bundle must fail every required dimension");

    assert_eq!(
        error
            .provider_failures()
            .iter()
            .map(NetworkCapabilityProviderFailure::role)
            .collect::<Vec<_>>(),
        [
            NetworkCapabilityRole::Attachment,
            NetworkCapabilityRole::Ingress,
        ]
    );
    assert_eq!(
        error.provider_failures()[0]
            .mismatches()
            .iter()
            .map(NetworkCapabilityMismatch::dimension)
            .collect::<Vec<_>>(),
        [
            NetworkCapabilityDimension::ManagementMode,
            NetworkCapabilityDimension::AttachmentMode,
            NetworkCapabilityDimension::IsolationMode,
            NetworkCapabilityDimension::AddressFamily,
            NetworkCapabilityDimension::LifecycleFeature,
            NetworkCapabilityDimension::ControlPlaneLocality,
            NetworkCapabilityDimension::ExternalDependency,
            NetworkCapabilityDimension::OfflineRestart,
        ]
    );
    assert_eq!(
        error.provider_failures()[1]
            .mismatches()
            .iter()
            .map(NetworkCapabilityMismatch::dimension)
            .collect::<Vec<_>>(),
        [
            NetworkCapabilityDimension::AddressFamily,
            NetworkCapabilityDimension::BindRealm,
            NetworkCapabilityDimension::Exposure,
            NetworkCapabilityDimension::Protocol,
            NetworkCapabilityDimension::PortAssignment,
            NetworkCapabilityDimension::IngressFeature,
            NetworkCapabilityDimension::ForwardingFeature,
            NetworkCapabilityDimension::LifecycleFeature,
            NetworkCapabilityDimension::ControlPlaneLocality,
            NetworkCapabilityDimension::ExternalDependency,
            NetworkCapabilityDimension::OfflineRestart,
        ]
    );
}

#[test]
fn satisfying_alternative_is_reported_but_never_selected() {
    let weak = NetworkCapabilityBundle::new(
        attachment(
            "attachment-weak",
            [NetworkAddressFamily::Ipv4],
            complete_lifecycle(),
            true,
        ),
        ingress("ingress-weak", [NetworkAddressFamily::Ipv4], [], false),
    );
    let requested = weak.selection();
    let safe_z = bundle("attachment-z", "ingress-z");
    let safe_a = bundle("attachment-a", "ingress-a");
    let registry = NetworkCapabilityRegistry::new([safe_z, weak, safe_a])
        .expect("all complete bundles should register");

    let error = registry
        .select_exact(
            &requested,
            &requirements(
                [NetworkAddressFamily::Ipv4],
                complete_lifecycle_requirements(),
                true,
            ),
        )
        .expect_err("requested weak bundle must not be replaced");

    assert_eq!(error.requested_selection(), &requested);
    assert_eq!(error.safe_alternatives().len(), 2);
    assert!(
        error.safe_alternatives()[0] < error.safe_alternatives()[1],
        "BTreeMap identity order must determine diagnostics"
    );
}

#[test]
fn incomplete_bundle_cannot_appear_as_a_safe_alternative() {
    let requested = NetworkCapabilityBundle::new(
        attachment(
            "attachment-requested",
            [NetworkAddressFamily::Ipv4],
            [],
            false,
        ),
        ingress("ingress-requested", [NetworkAddressFamily::Ipv4], [], false),
    );
    let incomplete = NetworkCapabilityBundle::new(
        attachment(
            "attachment-incomplete",
            [NetworkAddressFamily::Ipv4],
            complete_lifecycle(),
            true,
        ),
        ingress(
            "ingress-incomplete",
            [NetworkAddressFamily::Ipv4],
            [],
            false,
        ),
    );
    let selection = requested.selection();
    let registry = NetworkCapabilityRegistry::new([requested, incomplete])
        .expect("complete report values should register");

    let error = registry
        .select_exact(
            &selection,
            &requirements(
                [NetworkAddressFamily::Ipv4],
                complete_lifecycle_requirements(),
                true,
            ),
        )
        .expect_err("requested bundle should fail");

    assert!(error.safe_alternatives().is_empty());
}

#[test]
fn selection_and_bundle_wire_shapes_reject_missing_and_unknown_fields() {
    let selection = bundle("attachment-a", "ingress-a").selection();
    assert_eq!(
        serde_json::to_string(&selection).expect("selection should serialize"),
        format!(
            r#"{{"attachment_provider_id":"{}","ingress_provider_id":"{}"}}"#,
            selection.attachment_provider_id(),
            selection.ingress_provider_id()
        )
    );
    let mut selection_value = serde_json::to_value(&selection).expect("selection should serialize");
    selection_value
        .as_object_mut()
        .expect("selection should be an object")
        .remove("ingress_provider_id");
    assert!(
        serde_json::from_value::<NetworkCapabilitySelection>(selection_value).is_err(),
        "missing role must fail closed"
    );
    assert!(
        serde_json::from_value::<NetworkCapabilitySelection>(serde_json::json!({
            "attachment_provider_id": "not-a-provider-id",
            "ingress_provider_id": selection.ingress_provider_id(),
        }))
        .is_err(),
        "malformed provider identity must fail closed"
    );

    let mut bundle_value =
        serde_json::to_value(bundle("attachment-a", "ingress-a")).expect("bundle should serialize");
    bundle_value
        .as_object_mut()
        .expect("bundle should be an object")
        .insert("unexpected".to_owned(), serde_json::Value::Bool(true));
    assert!(
        serde_json::from_value::<NetworkCapabilityBundle>(bundle_value).is_err(),
        "unknown bundle fields must fail closed"
    );
}

#[test]
fn reordered_bundle_input_has_identical_selection_and_error_wire_output() {
    let bundle_a = bundle("attachment-a", "ingress-a");
    let bundle_z = bundle("attachment-z", "ingress-z");
    let registries = [
        NetworkCapabilityRegistry::new([bundle_z.clone(), bundle_a.clone()])
            .expect("registry should build"),
        NetworkCapabilityRegistry::new([bundle_a, bundle_z]).expect("registry should build"),
    ];
    let requested = NetworkCapabilitySelection::new(
        NetworkProviderId::for_registration_key("missing-attachment"),
        NetworkProviderId::for_registration_key("missing-ingress"),
    );
    let errors = registries.map(|registry| {
        registry
            .select_exact(
                &requested,
                &requirements(
                    [NetworkAddressFamily::Ipv4],
                    complete_lifecycle_requirements(),
                    true,
                ),
            )
            .expect_err("missing exact pair should fail")
    });

    assert_eq!(errors[0], errors[1]);
    assert_eq!(
        serde_json::to_vec(&errors[0]).expect("error should serialize"),
        serde_json::to_vec(&errors[1]).expect("error should serialize")
    );
}

#[test]
fn canonical_registration_collections_deduplicate_and_sort() {
    let registration = attachment(
        "attachment-a",
        [
            NetworkAddressFamily::Ipv6,
            NetworkAddressFamily::Ipv4,
            NetworkAddressFamily::Ipv6,
        ],
        complete_lifecycle(),
        true,
    );

    assert_eq!(
        registration.address_families(),
        &BTreeSet::from([NetworkAddressFamily::Ipv4, NetworkAddressFamily::Ipv6,])
    );
}

#[test]
fn provider_report_digest_binds_complete_selected_reports() {
    let baseline = bundle("attachment-a", "ingress-a");
    let identical = bundle("attachment-a", "ingress-a");
    let changed_attachment = NetworkCapabilityBundle::new(
        attachment(
            "attachment-a",
            [NetworkAddressFamily::Ipv4, NetworkAddressFamily::Ipv6],
            complete_lifecycle(),
            true,
        ),
        ingress(
            "ingress-a",
            [NetworkAddressFamily::Ipv4],
            complete_lifecycle(),
            true,
        ),
    );
    let changed_ingress = NetworkCapabilityBundle::new(
        attachment(
            "attachment-a",
            [NetworkAddressFamily::Ipv4],
            complete_lifecycle(),
            true,
        ),
        ingress(
            "ingress-a",
            [NetworkAddressFamily::Ipv4, NetworkAddressFamily::Ipv6],
            complete_lifecycle(),
            true,
        ),
    );

    let evidence = baseline.selection_evidence();
    assert_eq!(evidence, identical.selection_evidence());
    assert_eq!(evidence.selection(), &baseline.selection());
    assert_ne!(
        evidence.source_digest(),
        changed_attachment.selection_evidence().source_digest(),
        "attachment source evidence must participate in the digest"
    );
    assert_ne!(
        evidence.source_digest(),
        changed_ingress.selection_evidence().source_digest(),
        "ingress source evidence must participate in the digest"
    );

    let encoded = serde_json::to_value(&evidence).expect("selection evidence should serialize");
    assert_eq!(
        encoded
            .get("source_digest")
            .and_then(serde_json::Value::as_str)
            .expect("digest should use canonical string wire form")
            .len(),
        64
    );
    assert_eq!(
        serde_json::from_value::<NetworkCapabilitySelectionEvidence>(encoded)
            .expect("selection evidence should round trip"),
        evidence
    );
}
