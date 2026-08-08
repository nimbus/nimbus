use super::*;
use nimbus_network::{
    NetworkLeaseEpoch, NetworkPlanContentDigest, PortBindingSpec, PortLeaseFence,
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
    SandboxProvisionNetworkPlan::new(
        plan,
        tenant,
        generation,
        NetworkAttachmentId::for_workload_attachment("validation-workload", "primary"),
        [SandboxProvisionListener::new(
            listener_id,
            crate::SandboxPortBinding::tcp("api", 18_080, 8_080),
            request,
        )],
        [],
    )
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
            plan.listeners,
            plan.dependency_listeners,
        )
        .expect("TCP, HTTP, and HTTPS application protocols all use TCP lease authority");
    }
}
