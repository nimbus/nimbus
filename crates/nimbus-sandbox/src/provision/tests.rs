use super::*;
use nimbus_network::{
    EndpointProtocol, NetworkLeaseEpoch, NetworkPlanContentDigest, PortBindingSpec, PortLeaseFence,
    PublishedEndpointId,
};

fn provision_plan_with_binding(
    binding_spec: PortBindingSpec,
) -> Result<SandboxProvisionNetworkPlan, SandboxProvisionNetworkPlanError> {
    let tenant = TenantId::new("validation-tenant").expect("tenant should parse");
    let generation = NetworkResourceGeneration::new(3);
    let plan = NetworkPlan::new(
        NetworkPlanId::for_tenant_workload_plan(&tenant, "validation-workload"),
        generation,
        NetworkPlanContentDigest::sha256("validation-plan"),
        crate::backends::sandbox_network_plan_requirements(crate::SandboxBackendKind::Container)
            .capability_requirements()
            .clone(),
    );
    let listener_id =
        ListenerId::for_tenant_workload_listener(&tenant, "validation-workload", "api");
    let request = PortLeaseRequest::new(
        PortLeaseId::for_listener(&listener_id),
        listener_id.clone().into(),
        Some(tenant.clone()),
        PortLeaseFence::new(generation, NetworkLeaseEpoch::new(1)),
        PortLeaseAccounting::TenantPublished,
        PortPublicationIntent::host("127.0.0.1".parse().expect("address should parse")),
        binding_spec,
    )
    .with_plan_id(plan.plan_id().clone());
    let endpoint_id = PublishedEndpointId::for_workload_endpoint("validation-workload", "api");
    SandboxProvisionNetworkPlan::new(
        plan,
        tenant,
        generation,
        NetworkAttachmentId::for_workload_attachment("validation-workload", "primary"),
        [SandboxProvisionEndpointIdentity::new(
            listener_id.clone(),
            endpoint_id.clone(),
        )],
        [SandboxProvisionListener::new(
            endpoint_id,
            listener_id,
            crate::SandboxPortBinding::tcp("api", 18_080, 8_080),
            request,
        )],
        [],
    )
}

#[test]
fn duplicate_compiler_endpoint_identity_is_rejected() {
    let plan = provision_plan_with_binding(PortBindingSpec::new(
        PortProtocol::Tcp,
        PortBindRealm::Host,
        PortBindTarget::ipv4_specific(std::net::Ipv4Addr::LOCALHOST),
        PortExposure::Loopback,
        PortRequestMode::Exact(NonZeroU16::new(18_080).expect("non-zero port")),
    ))
    .expect("fixture plan should validate");
    let listener = plan.listeners()[0].clone();

    assert_eq!(
        SandboxProvisionNetworkPlan::new(
            plan.network_plan().clone(),
            plan.tenant_id().clone(),
            plan.generation(),
            plan.attachment_id().clone(),
            plan.endpoint_identities().iter().cloned(),
            [listener.clone(), listener],
            [],
        ),
        Err(SandboxProvisionNetworkPlanError::DuplicateEndpoint)
    );
}

#[test]
fn crossed_compiler_endpoint_identity_is_rejected_by_construction_and_decode() {
    let plan = provision_plan_with_binding(PortBindingSpec::new(
        PortProtocol::Tcp,
        PortBindRealm::Host,
        PortBindTarget::ipv4_specific(std::net::Ipv4Addr::LOCALHOST),
        PortExposure::Loopback,
        PortRequestMode::Exact(NonZeroU16::new(18_080).expect("non-zero port")),
    ))
    .expect("fixture plan should validate");
    let mut crossed_listener = plan.listeners()[0].clone();
    crossed_listener.endpoint_id =
        PublishedEndpointId::for_workload_endpoint("other-workload", "api");

    assert_eq!(
        SandboxProvisionNetworkPlan::new(
            plan.network_plan().clone(),
            plan.tenant_id().clone(),
            plan.generation(),
            plan.attachment_id().clone(),
            plan.endpoint_identities().iter().cloned(),
            [crossed_listener],
            plan.dependency_listeners().iter().cloned(),
        ),
        Err(SandboxProvisionNetworkPlanError::EndpointIdentityMismatch)
    );

    let mut wire = serde_json::to_value(&plan).expect("fixture plan should serialize");
    wire["listeners"][0]["endpoint_id"] = serde_json::to_value(
        PublishedEndpointId::for_workload_endpoint("other-workload", "api"),
    )
    .expect("crossed endpoint identity should serialize");
    let error = serde_json::from_value::<SandboxProvisionNetworkPlan>(wire)
        .expect_err("crossed persisted endpoint identity must fail decoding");
    assert_eq!(
        error.to_string(),
        SandboxProvisionNetworkPlanError::EndpointIdentityMismatch.to_string()
    );
}

#[test]
fn portable_status_uses_plan_identity_and_rejects_crossed_observation() {
    let plan = provision_plan_with_binding(PortBindingSpec::new(
        PortProtocol::Tcp,
        PortBindRealm::Host,
        PortBindTarget::ipv4_specific(std::net::Ipv4Addr::LOCALHOST),
        PortExposure::Loopback,
        PortRequestMode::Exact(NonZeroU16::new(18_080).expect("non-zero port")),
    ))
    .expect("fixture plan should validate");
    let first = PublishedEndpoint::new(
        "api",
        EndpointProtocol::Tcp,
        "127.0.0.1:18080".parse().expect("fixture address"),
    )
    .with_guest_port(8_080);
    let moved = PublishedEndpoint::new(
        "api",
        EndpointProtocol::Tcp,
        "127.0.0.2:28080".parse().expect("fixture address"),
    )
    .with_guest_port(8_080);
    let first_status = plan
        .project_portable_status(Some(plan.attachment_id()), &[first])
        .expect("exact status should project");
    let moved_status = plan
        .project_portable_status(Some(plan.attachment_id()), &[moved])
        .expect("moved status should project");

    assert_eq!(first_status.attachment(), moved_status.attachment());
    assert_eq!(
        first_status.published_endpoints()[0].endpoint_id(),
        moved_status.published_endpoints()[0].endpoint_id()
    );
    assert_ne!(
        first_status.published_endpoints()[0].endpoint().address,
        moved_status.published_endpoints()[0].endpoint().address
    );
    assert!(matches!(
        plan.project_portable_status(
            Some(plan.attachment_id()),
            &[PublishedEndpoint::new(
                "unknown",
                EndpointProtocol::Tcp,
                "127.0.0.1:18080".parse().expect("fixture address"),
            )
            .with_guest_port(8_080)],
        ),
        Err(SandboxProvisionNetworkPlanError::ListenerSetMismatch)
    ));
    assert!(matches!(
        plan.project_portable_status(
            Some(plan.attachment_id()),
            &[PublishedEndpoint::new(
                "api",
                EndpointProtocol::Http,
                "127.0.0.1:18080".parse().expect("fixture address"),
            )
            .with_guest_port(8_080)],
        ),
        Err(SandboxProvisionNetworkPlanError::EndpointObservationMismatch)
    ));
    assert!(matches!(
        plan.project_portable_status(
            Some(plan.attachment_id()),
            &[PublishedEndpoint::new(
                "api",
                EndpointProtocol::Tcp,
                "127.0.0.1:18080".parse().expect("fixture address"),
            )
            .with_guest_port(9_999)],
        ),
        Err(SandboxProvisionNetworkPlanError::EndpointObservationMismatch)
    ));
}

#[test]
fn crossed_bind_realm_target_and_exposure_fail_at_plan_construction() {
    let port = NonZeroU16::new(18_080).expect("port should be non-zero");
    let cases = [
        (
            PortBindingSpec::new(
                PortProtocol::Tcp,
                PortBindRealm::Unknown,
                PortBindTarget::ipv4_specific(std::net::Ipv4Addr::LOCALHOST),
                PortExposure::Loopback,
                PortRequestMode::Exact(port),
            ),
            SandboxProvisionNetworkPlanError::BindRealmMismatch,
        ),
        (
            PortBindingSpec::new(
                PortProtocol::Tcp,
                PortBindRealm::Host,
                PortBindTarget::ipv4_wildcard(),
                PortExposure::Loopback,
                PortRequestMode::Exact(port),
            ),
            SandboxProvisionNetworkPlanError::BindTargetMismatch,
        ),
        (
            PortBindingSpec::new(
                PortProtocol::Tcp,
                PortBindRealm::Host,
                PortBindTarget::ipv4_specific(std::net::Ipv4Addr::LOCALHOST),
                PortExposure::Public,
                PortRequestMode::Exact(port),
            ),
            SandboxProvisionNetworkPlanError::ExposureMismatch,
        ),
    ];
    for (binding, expected) in cases {
        assert_eq!(
            provision_plan_with_binding(binding).expect_err("crossed binding must fail"),
            expected
        );
    }
}

#[test]
fn every_current_application_protocol_maps_to_an_exact_tcp_host_lease() {
    let port = NonZeroU16::new(18_080).expect("port should be non-zero");
    for protocol in [
        nimbus_network::EndpointProtocol::Tcp,
        nimbus_network::EndpointProtocol::Http,
        nimbus_network::EndpointProtocol::Https,
    ] {
        let mut plan = provision_plan_with_binding(PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(std::net::Ipv4Addr::LOCALHOST),
            PortExposure::Loopback,
            PortRequestMode::Exact(port),
        ))
        .expect("TCP transport should validate");
        plan.listeners[0].binding.protocol = protocol;
        SandboxProvisionNetworkPlan::new(
            plan.network_plan.clone(),
            plan.tenant_id.clone(),
            plan.generation,
            plan.attachment_id.clone(),
            plan.endpoint_identities,
            plan.listeners,
            plan.dependency_listeners,
        )
        .expect("TCP, HTTP, and HTTPS application protocols all use TCP lease authority");
    }
}
