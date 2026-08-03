use super::*;

fn requirements() -> NetworkCapabilityRequirements {
    test_requirements()
}

fn provider() -> NetworkProviderCapabilities {
    NetworkProviderCapabilities::new(
        NetworkProviderId::for_registration_key("tested-provider"),
        NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []),
        NetworkEndpointCapabilitySet::new([], [], [], [], []),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        NetworkLifecycleCapabilitySet::new([]),
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    )
}

#[test]
fn empty_ingress_capability_set_has_no_implicit_tls_behavior() {
    assert!(
        NetworkIngressCapabilitySet::new([])
            .tls_behaviors()
            .is_empty(),
        "absence of TLS evidence must not fabricate cleartext support"
    );
}

#[test]
fn empty_tls_evidence_does_not_satisfy_disabled_requirement() {
    let mut requirements = requirements();
    requirements.ingress =
        NetworkIngressCapabilitySet::new([]).with_tls_behaviors([NetworkTlsBehavior::Disabled]);

    assert_rejected(
        &requirements,
        &provider(),
        NetworkCapabilityMismatch::TlsBehavior {
            required: NetworkTlsBehavior::Disabled,
        },
    );
}

fn assert_rejected(
    requirements: &NetworkCapabilityRequirements,
    provider: &NetworkProviderCapabilities,
    expected: NetworkCapabilityMismatch,
) -> NetworkCapabilitySatisfactionError {
    let error = provider
        .ensure_satisfied(requirements, [])
        .expect_err("unsupported requirement must fail closed");

    assert_eq!(error.provider_id(), provider.provider_id());
    assert_eq!(error.mismatches(), std::slice::from_ref(&expected));
    assert_eq!(error.mismatches()[0].dimension(), expected.dimension());
    assert!(error.safe_alternatives().is_empty());
    assert!(
        error.to_string().contains(&expected.to_string()),
        "diagnostic must name the exact mismatch"
    );
    error
}

#[test]
fn management_mode_has_positive_and_named_negative_proof() {
    let mut requirements = requirements();
    requirements.attachment.management_mode = NetworkManagementMode::ProviderManaged;
    let mut supported = provider();
    supported.attachment.management_mode = NetworkManagementMode::ProviderManaged;
    supported
        .ensure_satisfied(&requirements, [])
        .expect("matching management ownership should satisfy");

    let rejected = provider();
    let error = assert_rejected(
        &requirements,
        &rejected,
        NetworkCapabilityMismatch::ManagementMode {
            required: NetworkManagementMode::ProviderManaged,
            offered: NetworkManagementMode::NimbusHostManaged,
        },
    );
    assert_eq!(
        error.to_string(),
        format!(
            "network provider `{}` does not satisfy requirements: \
             management_mode(required=provider_managed, offered=nimbus_host_managed); \
             safe alternatives: none",
            rejected.provider_id()
        )
    );
}

#[test]
fn attachment_mode_has_positive_and_named_negative_proof() {
    let mut requirements = requirements();
    requirements
        .attachment
        .attachment_modes
        .insert(NetworkAttachmentMode::IsolatedNamespace);
    let mut supported = provider();
    supported
        .attachment
        .attachment_modes
        .insert(NetworkAttachmentMode::IsolatedNamespace);
    supported
        .ensure_satisfied(&requirements, [])
        .expect("required attachment shape should satisfy");

    assert_rejected(
        &requirements,
        &provider(),
        NetworkCapabilityMismatch::AttachmentMode {
            required: NetworkAttachmentMode::IsolatedNamespace,
        },
    );
}

#[test]
fn isolation_mode_has_positive_and_named_negative_proof() {
    let mut requirements = requirements();
    requirements
        .attachment
        .isolation_modes
        .insert(NetworkIsolationMode::TenantSegment);
    let mut supported = provider();
    supported
        .attachment
        .isolation_modes
        .insert(NetworkIsolationMode::TenantSegment);
    supported
        .ensure_satisfied(&requirements, [])
        .expect("required isolation proof should satisfy");

    assert_rejected(
        &requirements,
        &provider(),
        NetworkCapabilityMismatch::IsolationMode {
            required: NetworkIsolationMode::TenantSegment,
        },
    );
}

#[test]
fn address_family_has_positive_and_named_negative_proof() {
    let mut requirements = requirements();
    requirements
        .endpoint
        .address_families
        .insert(NetworkAddressFamily::Ipv6);
    let mut supported = provider();
    supported
        .endpoint
        .address_families
        .insert(NetworkAddressFamily::Ipv6);
    supported
        .ensure_satisfied(&requirements, [])
        .expect("required address family should satisfy");

    assert_rejected(
        &requirements,
        &provider(),
        NetworkCapabilityMismatch::AddressFamily {
            required: NetworkAddressFamily::Ipv6,
        },
    );
}

#[test]
fn bind_realm_has_positive_and_named_negative_proof() {
    let mut requirements = requirements();
    requirements
        .endpoint
        .bind_realms
        .insert(NetworkBindRealmKind::ProvenIsolated);
    let mut supported = provider();
    supported
        .endpoint
        .bind_realms
        .insert(NetworkBindRealmKind::ProvenIsolated);
    supported
        .ensure_satisfied(&requirements, [])
        .expect("required bind-realm proof should satisfy");

    assert_rejected(
        &requirements,
        &provider(),
        NetworkCapabilityMismatch::BindRealm {
            required: NetworkBindRealmKind::ProvenIsolated,
        },
    );
}

#[test]
fn exposure_has_positive_and_named_negative_proof() {
    let mut requirements = requirements();
    requirements
        .endpoint
        .exposures
        .insert(NetworkExposure::Private);
    let mut supported = provider();
    supported
        .endpoint
        .exposures
        .insert(NetworkExposure::Private);
    supported
        .ensure_satisfied(&requirements, [])
        .expect("required exposure should satisfy");

    assert_rejected(
        &requirements,
        &provider(),
        NetworkCapabilityMismatch::Exposure {
            required: NetworkExposure::Private,
        },
    );
}

#[test]
fn protocol_has_positive_and_named_negative_proof() {
    let mut requirements = requirements();
    requirements.endpoint.protocols.insert(PortProtocol::Udp);
    let mut supported = provider();
    supported.endpoint.protocols.insert(PortProtocol::Udp);
    supported
        .ensure_satisfied(&requirements, [])
        .expect("required transport protocol should satisfy");

    assert_rejected(
        &requirements,
        &provider(),
        NetworkCapabilityMismatch::Protocol {
            required: PortProtocol::Udp,
        },
    );
}

#[test]
fn port_assignment_has_positive_and_named_negative_proof() {
    let mut requirements = requirements();
    requirements
        .endpoint
        .port_assignment_modes
        .insert(NetworkPortAssignmentMode::NimbusAllocatedRange);
    let mut supported = provider();
    supported
        .endpoint
        .port_assignment_modes
        .insert(NetworkPortAssignmentMode::NimbusAllocatedRange);
    supported
        .ensure_satisfied(&requirements, [])
        .expect("required port assignment should satisfy");

    assert_rejected(
        &requirements,
        &provider(),
        NetworkCapabilityMismatch::PortAssignment {
            required: NetworkPortAssignmentMode::NimbusAllocatedRange,
        },
    );
}

#[test]
fn ingress_feature_has_positive_and_named_negative_proof() {
    let mut requirements = requirements();
    requirements
        .ingress
        .features
        .insert(NetworkIngressFeature::TlsTermination);
    let mut supported = provider();
    supported
        .ingress
        .features
        .insert(NetworkIngressFeature::TlsTermination);
    supported
        .ensure_satisfied(&requirements, [])
        .expect("required ingress behavior should satisfy");

    assert_rejected(
        &requirements,
        &provider(),
        NetworkCapabilityMismatch::IngressFeature {
            required: NetworkIngressFeature::TlsTermination,
        },
    );
}

#[test]
fn forwarding_feature_has_positive_and_named_negative_proof() {
    let mut requirements = requirements();
    requirements
        .forwarding
        .features
        .insert(NetworkForwardingFeature::ConnectionDrain);
    let mut supported = provider();
    supported
        .forwarding
        .features
        .insert(NetworkForwardingFeature::ConnectionDrain);
    supported
        .ensure_satisfied(&requirements, [])
        .expect("required forwarding behavior should satisfy");

    assert_rejected(
        &requirements,
        &provider(),
        NetworkCapabilityMismatch::ForwardingFeature {
            required: NetworkForwardingFeature::ConnectionDrain,
        },
    );
}

#[test]
fn lifecycle_feature_has_positive_and_named_negative_proof() {
    let mut requirements = requirements();
    requirements
        .lifecycle
        .features
        .insert(NetworkLifecycleFeature::Reconcile);
    let mut supported = provider();
    supported
        .lifecycle
        .features
        .insert(NetworkLifecycleFeature::Reconcile);
    supported
        .ensure_satisfied(&requirements, [])
        .expect("required lifecycle behavior should satisfy");

    assert_rejected(
        &requirements,
        &provider(),
        NetworkCapabilityMismatch::LifecycleFeature {
            required: NetworkLifecycleFeature::Reconcile,
        },
    );
}

#[test]
fn control_plane_locality_has_positive_and_named_negative_proof() {
    let mut requirements = requirements();
    requirements.sovereignty.maximum_control_plane_locality =
        NetworkControlPlaneLocality::OperatorLocal;
    let mut supported = provider();
    supported.sovereignty.control_plane_locality = NetworkControlPlaneLocality::OperatorLocal;
    supported
        .ensure_satisfied(&requirements, [])
        .expect("provider at the admitted locality boundary should satisfy");

    let mut rejected = provider();
    rejected.sovereignty.control_plane_locality = NetworkControlPlaneLocality::ThirdParty;
    assert_rejected(
        &requirements,
        &rejected,
        NetworkCapabilityMismatch::ControlPlaneLocality {
            maximum_allowed: NetworkControlPlaneLocality::OperatorLocal,
            offered: NetworkControlPlaneLocality::ThirdParty,
        },
    );
}

#[test]
fn external_dependency_has_positive_and_named_negative_proof() {
    let mut requirements = requirements();
    requirements
        .sovereignty
        .allowed_external_dependencies
        .insert(NetworkExternalDependency::Dns);
    let mut supported = provider();
    supported
        .sovereignty
        .required_external_dependencies
        .insert(NetworkExternalDependency::Dns);
    supported
        .ensure_satisfied(&requirements, [])
        .expect("explicitly admitted dependency should satisfy");

    assert_rejected(
        &test_requirements(),
        &supported,
        NetworkCapabilityMismatch::ExternalDependency {
            disallowed: NetworkExternalDependency::Dns,
        },
    );
}

#[test]
fn offline_restart_has_positive_and_named_negative_proof() {
    let mut requirements = requirements();
    requirements.sovereignty.offline_restart_required = true;
    provider()
        .ensure_satisfied(&requirements, [])
        .expect("offline-capable provider should satisfy");

    let mut rejected = provider();
    rejected.sovereignty.offline_restart_supported = false;
    assert_rejected(
        &requirements,
        &rejected,
        NetworkCapabilityMismatch::OfflineRestart {
            required: true,
            offered: false,
        },
    );
}

#[test]
fn full_requirement_set_satisfies_with_a_strict_provider_superset() {
    let requirements = NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(
            NetworkManagementMode::ProviderManaged,
            [
                NetworkAttachmentMode::VirtualMachineGuest,
                NetworkAttachmentMode::ProviderVirtualNetwork,
            ],
            [
                NetworkIsolationMode::WorkloadNamespace,
                NetworkIsolationMode::ProviderBoundary,
            ],
        ),
        NetworkEndpointCapabilitySet::new(
            [NetworkAddressFamily::Ipv4, NetworkAddressFamily::Ipv6],
            [
                NetworkBindRealmKind::Host,
                NetworkBindRealmKind::ProvenIsolated,
            ],
            [NetworkExposure::Loopback, NetworkExposure::Private],
            [PortProtocol::Tcp, PortProtocol::Udp],
            [
                NetworkPortAssignmentMode::Exact,
                NetworkPortAssignmentMode::ProviderAssigned,
            ],
        ),
        NetworkIngressCapabilitySet::new([
            NetworkIngressFeature::HostRouting,
            NetworkIngressFeature::TlsTermination,
            NetworkIngressFeature::WebSocket,
        ]),
        NetworkForwardingCapabilitySet::new([
            NetworkForwardingFeature::PortForwarding,
            NetworkForwardingFeature::ConnectionDrain,
        ]),
        NetworkLifecycleCapabilitySet::new([
            NetworkLifecycleFeature::DurableInspect,
            NetworkLifecycleFeature::Reconcile,
            NetworkLifecycleFeature::Delete,
        ]),
        NetworkSovereigntyRequirements::new(
            NetworkControlPlaneLocality::OperatorLocal,
            [
                NetworkExternalDependency::Dns,
                NetworkExternalDependency::HostedCertificate,
            ],
            true,
        ),
    );
    let provider = NetworkProviderCapabilities::new(
        NetworkProviderId::for_registration_key("full-provider"),
        NetworkAttachmentCapabilitySet::new(
            NetworkManagementMode::ProviderManaged,
            [
                NetworkAttachmentMode::HostNetwork,
                NetworkAttachmentMode::VirtualMachineGuest,
                NetworkAttachmentMode::ProviderVirtualNetwork,
            ],
            [
                NetworkIsolationMode::WorkloadNamespace,
                NetworkIsolationMode::TenantSegment,
                NetworkIsolationMode::ProviderBoundary,
            ],
        ),
        NetworkEndpointCapabilitySet::new(
            [NetworkAddressFamily::Ipv4, NetworkAddressFamily::Ipv6],
            [
                NetworkBindRealmKind::Host,
                NetworkBindRealmKind::ProvenIsolated,
            ],
            [
                NetworkExposure::Loopback,
                NetworkExposure::Private,
                NetworkExposure::Public,
            ],
            [PortProtocol::Tcp, PortProtocol::Udp],
            [
                NetworkPortAssignmentMode::Exact,
                NetworkPortAssignmentMode::NimbusAllocatedRange,
                NetworkPortAssignmentMode::ProviderAssigned,
            ],
        ),
        NetworkIngressCapabilitySet::new([
            NetworkIngressFeature::HostRouting,
            NetworkIngressFeature::PathRouting,
            NetworkIngressFeature::TlsTermination,
            NetworkIngressFeature::WebSocket,
            NetworkIngressFeature::Streaming,
        ]),
        NetworkForwardingCapabilitySet::new([
            NetworkForwardingFeature::PortForwarding,
            NetworkForwardingFeature::ConnectionDrain,
        ]),
        NetworkLifecycleCapabilitySet::new([
            NetworkLifecycleFeature::DurableInspect,
            NetworkLifecycleFeature::Reconcile,
            NetworkLifecycleFeature::Delete,
        ]),
        NetworkSovereigntyCapabilities::new(
            NetworkControlPlaneLocality::LocalOnly,
            [NetworkExternalDependency::Dns],
            true,
        ),
    );

    provider
        .ensure_satisfied(&requirements, [])
        .expect("strict provider superset should satisfy all requirements");
}

#[test]
fn complete_mismatch_vector_has_fixed_dimension_order() {
    let requirements = NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(
            NetworkManagementMode::ProviderManaged,
            [NetworkAttachmentMode::IsolatedNamespace],
            [NetworkIsolationMode::TenantSegment],
        ),
        NetworkEndpointCapabilitySet::new(
            [NetworkAddressFamily::Ipv6],
            [NetworkBindRealmKind::ProvenIsolated],
            [NetworkExposure::Private],
            [PortProtocol::Udp],
            [NetworkPortAssignmentMode::NimbusAllocatedRange],
        ),
        NetworkIngressCapabilitySet::new([NetworkIngressFeature::TlsTermination]),
        NetworkForwardingCapabilitySet::new([NetworkForwardingFeature::ConnectionDrain]),
        NetworkLifecycleCapabilitySet::new([NetworkLifecycleFeature::Reconcile]),
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let mut unsupported = provider();
    unsupported.sovereignty.control_plane_locality = NetworkControlPlaneLocality::ThirdParty;
    unsupported
        .sovereignty
        .required_external_dependencies
        .insert(NetworkExternalDependency::Dns);
    unsupported.sovereignty.offline_restart_supported = false;

    let error = unsupported
        .ensure_satisfied(&requirements, [])
        .expect_err("every dimension should be rejected");
    let dimensions: Vec<_> = error
        .mismatches()
        .iter()
        .map(NetworkCapabilityMismatch::dimension)
        .collect();

    assert_eq!(
        dimensions,
        [
            NetworkCapabilityDimension::ManagementMode,
            NetworkCapabilityDimension::AttachmentMode,
            NetworkCapabilityDimension::IsolationMode,
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
fn reordered_duplicate_facts_and_alternatives_are_byte_stable() {
    let first_requirements = NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(
            NetworkManagementMode::NimbusHostManaged,
            [
                NetworkAttachmentMode::IsolatedNamespace,
                NetworkAttachmentMode::HostNetwork,
                NetworkAttachmentMode::IsolatedNamespace,
            ],
            [],
        ),
        NetworkEndpointCapabilitySet::new([], [], [], [], []),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        NetworkLifecycleCapabilitySet::new([]),
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::ThirdParty, [], false),
    );
    let second_requirements = NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(
            NetworkManagementMode::NimbusHostManaged,
            [
                NetworkAttachmentMode::HostNetwork,
                NetworkAttachmentMode::IsolatedNamespace,
            ],
            [],
        ),
        NetworkEndpointCapabilitySet::new([], [], [], [], []),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        NetworkLifecycleCapabilitySet::new([]),
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::ThirdParty, [], false),
    );
    assert_eq!(first_requirements, second_requirements);

    let first_alternative = NetworkProviderId::for_registration_key("alternative-a");
    let second_alternative = NetworkProviderId::for_registration_key("alternative-b");
    let mut first_provider = provider();
    first_provider.attachment.attachment_modes = [
        NetworkAttachmentMode::VirtualMachineGuest,
        NetworkAttachmentMode::HostNetwork,
        NetworkAttachmentMode::VirtualMachineGuest,
    ]
    .into_iter()
    .collect();
    let mut second_provider = provider();
    second_provider.attachment.attachment_modes = [
        NetworkAttachmentMode::HostNetwork,
        NetworkAttachmentMode::VirtualMachineGuest,
    ]
    .into_iter()
    .collect();
    assert_eq!(first_provider, second_provider);

    let first = first_provider
        .ensure_satisfied(
            &first_requirements,
            [
                second_alternative.clone(),
                first_alternative.clone(),
                second_alternative.clone(),
            ],
        )
        .expect_err("provider lacks both attachment modes");
    let second = second_provider
        .ensure_satisfied(
            &second_requirements,
            [first_alternative.clone(), second_alternative.clone()],
        )
        .expect_err("provider lacks both attachment modes");

    assert_eq!(first, second);
    assert_eq!(
        first.safe_alternatives(),
        BTreeSet::from([first_alternative, second_alternative])
            .into_iter()
            .collect::<Vec<_>>()
    );
    assert_eq!(first.to_string(), second.to_string());
    assert_eq!(
        serde_json::to_vec(&first).expect("error should serialize"),
        serde_json::to_vec(&second).expect("error should serialize")
    );
}

#[test]
fn unknown_runtime_evidence_and_unknown_wire_fields_fail_closed() {
    assert_eq!(
        NetworkExposure::try_from(PortExposure::Unknown),
        Err(NetworkCapabilityFactError::UnknownExposure)
    );
    assert_eq!(
        NetworkBindRealmKind::try_from(&PortBindRealm::Unknown),
        Err(NetworkCapabilityFactError::UnknownBindRealm)
    );

    let mut wire = serde_json::to_value(requirements()).expect("requirements should serialize");
    wire.as_object_mut()
        .expect("requirements wire should be an object")
        .insert("invented".to_owned(), serde_json::Value::Bool(true));
    assert!(serde_json::from_value::<NetworkCapabilityRequirements>(wire).is_err());
    assert!(
        serde_json::from_str::<NetworkExposure>(r#""unknown""#).is_err(),
        "unknown exposure has no capability wire value"
    );
    assert!(
        serde_json::from_str::<NetworkBindRealmKind>(r#""unknown""#).is_err(),
        "unknown bind realm has no capability wire value"
    );
}

#[test]
fn every_external_dependency_requires_explicit_admission() {
    for dependency in [
        NetworkExternalDependency::PublicNetwork,
        NetworkExternalDependency::Dns,
        NetworkExternalDependency::HostedCertificate,
        NetworkExternalDependency::Relay,
        NetworkExternalDependency::ExternalControlPlane,
    ] {
        let mut provider = provider();
        provider
            .sovereignty
            .required_external_dependencies
            .insert(dependency);
        assert_rejected(
            &requirements(),
            &provider,
            NetworkCapabilityMismatch::ExternalDependency {
                disallowed: dependency,
            },
        );
    }
}

#[test]
fn local_tls_does_not_imply_a_hosted_certificate_dependency() {
    let mut requirements = requirements();
    requirements
        .ingress
        .features
        .insert(NetworkIngressFeature::TlsTermination);
    let mut provider = provider();
    provider
        .ingress
        .features
        .insert(NetworkIngressFeature::TlsTermination);

    provider
        .ensure_satisfied(&requirements, [])
        .expect("local TLS support must not imply an external certificate service");
}

#[test]
fn port_assignment_modes_do_not_substitute_for_one_another() {
    let modes = [
        NetworkPortAssignmentMode::Exact,
        NetworkPortAssignmentMode::NimbusAllocatedRange,
        NetworkPortAssignmentMode::ProviderAssigned,
    ];
    for (index, required) in modes.into_iter().enumerate() {
        let mut requirements = requirements();
        requirements.endpoint.port_assignment_modes.insert(required);
        let mut provider = provider();
        provider
            .endpoint
            .port_assignment_modes
            .insert(modes[(index + 1) % modes.len()]);
        assert_rejected(
            &requirements,
            &provider,
            NetworkCapabilityMismatch::PortAssignment { required },
        );
    }
}
