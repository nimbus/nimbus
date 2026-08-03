use std::net::{IpAddr, Ipv4Addr};
use std::num::NonZeroU16;

use nimbus_core::TenantId;
use nimbus_network::{
    EndpointProtocol, IngressRouteId, ListenerId, NetworkAttachmentCapabilitySet,
    NetworkAttachmentProviderRegistration, NetworkCapabilityBundle, NetworkCapabilityRequirements,
    NetworkCapabilitySelection, NetworkCapabilitySelectionEvidence, NetworkConditionKind,
    NetworkControlPlaneLocality, NetworkEndpointCapabilitySet, NetworkExternalDependency,
    NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet, NetworkLifecycleFeature,
    NetworkManagementMode, NetworkPlan, NetworkPlanContentDigest, NetworkPlanId, NetworkProviderId,
    NetworkReadinessRequirement, NetworkResourceGeneration, NetworkSovereigntyCapabilities,
    NetworkSovereigntyRequirements, NetworkTlsBehavior, PortLeaseId, PublishedEndpointId,
};

use super::{
    CompiledWorkloadNetworkPlan, WORKLOAD_NETWORK_PLAN_FORMAT_VERSION,
    WorkloadNetworkAttachmentBlueprint, WorkloadNetworkDependencyListenerBlueprint,
    WorkloadNetworkEndpointSemantics, WorkloadNetworkForwardingBehavior,
    WorkloadNetworkListenerBlueprint, WorkloadNetworkPlanContent, WorkloadNetworkPlanError,
    WorkloadNetworkPlanIdentity, WorkloadNetworkPortRequestMode, WorkloadNetworkRouteBlueprint,
};
use crate::{WorkloadActivationIntent, WorkloadPublicationIntent};

const INCARNATION: &str = "tenant-a/workload-a";

fn identity() -> WorkloadNetworkPlanIdentity {
    WorkloadNetworkPlanIdentity::new(
        TenantId::new("tenant-a").expect("tenant should validate"),
        INCARNATION,
        NetworkResourceGeneration::new(1),
    )
    .expect("identity should validate")
}

fn selection() -> NetworkCapabilitySelection {
    NetworkCapabilitySelection::new(
        NetworkProviderId::for_registration_key("attachment"),
        NetworkProviderId::for_registration_key("ingress"),
    )
}

fn selection_evidence() -> NetworkCapabilitySelectionEvidence {
    NetworkCapabilityBundle::new(
        NetworkAttachmentProviderRegistration::new(
            selection().attachment_provider_id().clone(),
            NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []),
            [],
            NetworkLifecycleCapabilitySet::new([]),
            NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
        ),
        NetworkIngressProviderRegistration::new(
            selection().ingress_provider_id().clone(),
            NetworkEndpointCapabilitySet::new([], [], [], [], []),
            NetworkIngressCapabilitySet::new([]),
            NetworkForwardingCapabilitySet::new([]),
            NetworkLifecycleCapabilitySet::new([]),
            NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
        ),
    )
    .selection_evidence()
}

fn forwarded_http() -> WorkloadNetworkEndpointSemantics {
    WorkloadNetworkEndpointSemantics::new(
        WorkloadNetworkForwardingBehavior::PortForwarded,
        NetworkTlsBehavior::Disabled,
    )
}

fn direct_cleartext() -> WorkloadNetworkEndpointSemantics {
    WorkloadNetworkEndpointSemantics::new(
        WorkloadNetworkForwardingBehavior::None,
        NetworkTlsBehavior::Disabled,
    )
}

fn sovereignty() -> NetworkSovereigntyRequirements {
    NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true)
}

fn requirements(sovereignty: NetworkSovereigntyRequirements) -> NetworkCapabilityRequirements {
    NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []),
        NetworkEndpointCapabilitySet::new([], [], [], [], []),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        NetworkLifecycleCapabilitySet::new([]),
        sovereignty,
    )
}

fn empty_content() -> WorkloadNetworkPlanContent {
    WorkloadNetworkPlanContent::new(
        identity(),
        requirements(sovereignty()),
        None,
        None,
        None,
        [],
        [],
        [],
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
    )
    .expect("empty admitted content should validate")
}

fn plan_for(content: &WorkloadNetworkPlanContent) -> NetworkPlan {
    CompiledWorkloadNetworkPlan::from_content(content.clone())
        .expect("fixture content should derive an exact plan")
        .plan()
        .clone()
}

fn route(service_name: &str, route_name: &str) -> WorkloadNetworkRouteBlueprint {
    WorkloadNetworkRouteBlueprint::new(
        &identity(),
        service_name,
        route_name,
        EndpointProtocol::Http,
        "api.internal",
        8080,
        Some(3000),
    )
    .expect("route fixture should validate")
}

fn listener(name: &str) -> WorkloadNetworkListenerBlueprint {
    listener_with(
        name,
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        WorkloadNetworkPortRequestMode::exact(
            NonZeroU16::new(8080).expect("fixture port should be non-zero"),
        ),
    )
}

fn listener_with(
    name: &str,
    desired_host_address: IpAddr,
    port_request: WorkloadNetworkPortRequestMode,
) -> WorkloadNetworkListenerBlueprint {
    WorkloadNetworkListenerBlueprint::new(
        &identity(),
        name,
        EndpointProtocol::Http,
        desired_host_address,
        port_request,
        forwarded_http(),
        Some(3000),
    )
    .expect("listener fixture should validate")
}

fn populated_content(
    routes: impl IntoIterator<Item = WorkloadNetworkRouteBlueprint>,
    listeners: impl IntoIterator<Item = WorkloadNetworkListenerBlueprint>,
) -> WorkloadNetworkPlanContent {
    let identity = identity();
    WorkloadNetworkPlanContent::new(
        identity.clone(),
        requirements(sovereignty()),
        Some(selection()),
        Some(selection_evidence()),
        Some(
            WorkloadNetworkAttachmentBlueprint::new(&identity, "default")
                .expect("attachment fixture should validate"),
        ),
        routes,
        listeners,
        [],
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    )
    .expect("populated admitted content should validate")
}

fn dependency_listener(name: &str) -> WorkloadNetworkDependencyListenerBlueprint {
    WorkloadNetworkDependencyListenerBlueprint::new(
        &identity(),
        name,
        NetworkProviderId::for_registration_key("pep"),
    )
    .expect("dependency listener should validate")
}

fn plan_with(
    content: &WorkloadNetworkPlanContent,
    plan_id: NetworkPlanId,
    generation: NetworkResourceGeneration,
    requirements: NetworkCapabilityRequirements,
    readiness: impl IntoIterator<Item = NetworkReadinessRequirement>,
) -> NetworkPlan {
    NetworkPlan::new(
        plan_id,
        generation,
        NetworkPlanContentDigest::sha256(content.canonical_bytes()),
        requirements,
    )
    .with_readiness_requirements(readiness)
    .expect("fixture readiness should be unique")
}

fn assert_wire_plan_rejected(
    content: &WorkloadNetworkPlanContent,
    candidate: &NetworkPlan,
    message: &str,
) {
    let exact = CompiledWorkloadNetworkPlan::from_content(content.clone())
        .expect("fixture content should derive an exact plan");
    let mut wire = serde_json::to_value(exact).expect("compiled fixture should serialize");
    wire["plan"] = serde_json::to_value(candidate).expect("candidate plan should serialize");
    let error = serde_json::from_value::<CompiledWorkloadNetworkPlan>(wire)
        .expect_err("crossed envelope must fail while decoding");
    assert!(error.to_string().contains(message), "{error}");
}

#[test]
fn resource_free_plan_has_no_selection_evidence() {
    let content = empty_content();
    let compiled = CompiledWorkloadNetworkPlan::from_content(content)
        .expect("matching content should derive its envelope");
    let wire = serde_json::to_vec(&compiled).expect("compiled plan should serialize");

    assert_eq!(
        serde_json::from_slice::<CompiledWorkloadNetworkPlan>(&wire)
            .expect("compiled plan should deserialize"),
        compiled
    );
    assert_eq!(
        compiled.content().format_version(),
        WORKLOAD_NETWORK_PLAN_FORMAT_VERSION
    );
    assert!(compiled.content().attachment().is_none());
    assert!(compiled.content().routes().is_empty());
    assert!(compiled.content().listeners().is_empty());
    assert!(compiled.content().capability_selection().is_none());
    assert!(compiled.content().capability_selection_evidence().is_none());
    assert_eq!(
        String::from_utf8(compiled.content().canonical_bytes())
            .expect("canonical JSON should be UTF-8"),
        r#"{"formatVersion":2,"identity":{"tenantId":"tenant-a","workloadIncarnationKey":"tenant-a/workload-a","generation":1},"capabilityRequirements":{"attachment":{"management_mode":"nimbus_host_managed","attachment_modes":[],"isolation_modes":[]},"endpoint":{"address_families":[],"bind_realms":[],"exposures":[],"protocols":[],"port_assignment_modes":[]},"ingress":{"features":[],"tls_behaviors":[]},"forwarding":{"features":[]},"lifecycle":{"features":[]},"sovereignty":{"maximum_control_plane_locality":"local_only","allowed_external_dependencies":[],"offline_restart_required":true}},"routes":[],"listeners":[],"dependencyListeners":[],"activation":"prepare_only","publication":"withheld"}"#,
        "the version-two empty content encoding is durable digest authority"
    );
}

#[test]
fn connected_plan_requires_selection_evidence() {
    let content = populated_content([route("orders", "api")], [listener("api")]);
    let compiled = CompiledWorkloadNetworkPlan::from_content(content)
        .expect("matching populated content should derive its envelope");
    let value = serde_json::to_value(&compiled).expect("compiled plan should serialize");
    assert!(compiled.content().capability_selection().is_some());
    assert!(compiled.content().capability_selection_evidence().is_some());
    let content_fields = value["content"]
        .as_object()
        .expect("content should be an object")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let listener_fields = value["content"]["listeners"][0]
        .as_object()
        .expect("listener should be an object")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();

    assert_eq!(
        content_fields,
        [
            "formatVersion",
            "identity",
            "capabilityRequirements",
            "capabilitySelection",
            "capabilitySelectionEvidence",
            "attachment",
            "routes",
            "listeners",
            "dependencyListeners",
            "activation",
            "publication",
        ]
    );
    assert_eq!(
        listener_fields,
        [
            "listenerId",
            "endpointId",
            "portLeaseId",
            "name",
            "protocol",
            "desiredHostAddress",
            "portRequest",
            "endpointSemantics",
            "guestPort",
        ]
    );
    assert_eq!(
        serde_json::from_value::<CompiledWorkloadNetworkPlan>(value)
            .expect("populated compiled plan should deserialize"),
        compiled
    );
}

#[test]
fn canonical_order_is_independent_of_route_and_listener_input_order() {
    let first = populated_content(
        [route("orders", "metrics"), route("orders", "api")],
        [listener("metrics"), listener("api")],
    );
    let second = populated_content(
        [route("orders", "api"), route("orders", "metrics")],
        [listener("api"), listener("metrics")],
    );

    assert_eq!(first, second);
    assert_eq!(first.canonical_bytes(), second.canonical_bytes());
    assert_eq!(first.routes()[0].route_name(), "api");
    assert_eq!(first.listeners()[0].name(), "api");
}

#[test]
fn duplicate_route_logical_and_stable_identities_fail_closed() {
    let first = route("orders", "api");
    let duplicate_name = WorkloadNetworkRouteBlueprint::from_wire(
        IngressRouteId::for_workload_route(INCARNATION, "orders", "other-id-input"),
        "orders".to_owned(),
        "api".to_owned(),
        EndpointProtocol::Https,
        "other.internal".to_owned(),
        8443,
        None,
    )
    .expect("individual route should validate");
    let error = WorkloadNetworkPlanContent::new(
        identity(),
        requirements(sovereignty()),
        None,
        None,
        None,
        [first.clone(), duplicate_name],
        [],
        [],
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
    )
    .expect_err("duplicate logical route should fail");
    assert!(matches!(
        error,
        WorkloadNetworkPlanError::DuplicateRouteName { .. }
    ));

    let duplicate_id = WorkloadNetworkRouteBlueprint::from_wire(
        first.route_id().clone(),
        "billing".to_owned(),
        "other".to_owned(),
        EndpointProtocol::Tcp,
        "billing.internal".to_owned(),
        9000,
        None,
    )
    .expect("individual route should validate");
    let error = WorkloadNetworkPlanContent::new(
        identity(),
        requirements(sovereignty()),
        None,
        None,
        None,
        [first, duplicate_id],
        [],
        [],
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
    )
    .expect_err("duplicate stable route ID should fail");
    assert!(matches!(
        error,
        WorkloadNetworkPlanError::DuplicateRouteId { .. }
    ));
}

#[test]
fn duplicate_listener_logical_and_stable_identities_fail_closed() {
    let first = listener("api");
    let tenant = TenantId::new("tenant-a").expect("tenant should validate");
    let other_listener_id =
        ListenerId::for_tenant_workload_listener(&tenant, "workload-a", "other");
    let duplicate_name = WorkloadNetworkListenerBlueprint::from_wire(
        other_listener_id.clone(),
        PublishedEndpointId::for_workload_endpoint(INCARNATION, "other"),
        PortLeaseId::for_listener(&other_listener_id),
        "api".to_owned(),
        EndpointProtocol::Tcp,
        IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        WorkloadNetworkPortRequestMode::ProviderAssigned,
        direct_cleartext(),
        None,
    )
    .expect("individual listener should validate");
    let error = WorkloadNetworkPlanContent::new(
        identity(),
        requirements(sovereignty()),
        Some(selection()),
        Some(selection_evidence()),
        None,
        [],
        [first.clone(), duplicate_name],
        [],
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
    )
    .expect_err("duplicate logical listener should fail");
    assert!(matches!(
        error,
        WorkloadNetworkPlanError::DuplicateListenerName { .. }
    ));

    let duplicate_listener_id = WorkloadNetworkListenerBlueprint::from_wire(
        first.listener_id().clone(),
        PublishedEndpointId::for_workload_endpoint(INCARNATION, "other"),
        first.port_lease_id().clone(),
        "other".to_owned(),
        EndpointProtocol::Tcp,
        IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        WorkloadNetworkPortRequestMode::ProviderAssigned,
        direct_cleartext(),
        None,
    )
    .expect("individual listener should validate");
    let error = WorkloadNetworkPlanContent::new(
        identity(),
        requirements(sovereignty()),
        Some(selection()),
        Some(selection_evidence()),
        None,
        [],
        [first.clone(), duplicate_listener_id],
        [],
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
    )
    .expect_err("duplicate stable listener ID should fail");
    assert!(matches!(
        error,
        WorkloadNetworkPlanError::DuplicateListenerId { .. }
    ));

    let duplicate_endpoint_id = WorkloadNetworkListenerBlueprint::from_wire(
        other_listener_id.clone(),
        first.endpoint_id().clone(),
        PortLeaseId::for_listener(&other_listener_id),
        "other".to_owned(),
        EndpointProtocol::Tcp,
        IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        WorkloadNetworkPortRequestMode::ProviderAssigned,
        direct_cleartext(),
        None,
    )
    .expect("individual listener should validate");
    let error = WorkloadNetworkPlanContent::new(
        identity(),
        requirements(sovereignty()),
        Some(selection()),
        Some(selection_evidence()),
        None,
        [],
        [first, duplicate_endpoint_id],
        [],
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
    )
    .expect_err("duplicate stable endpoint ID should fail");
    assert!(matches!(
        error,
        WorkloadNetworkPlanError::DuplicatePublishedEndpointId { .. }
    ));
}

#[test]
fn crossed_port_lease_identity_fails_before_content_construction() {
    let tenant = TenantId::new("tenant-a").expect("tenant should validate");
    let listener_id = ListenerId::for_tenant_workload_listener(&tenant, "workload-a", "api");
    let other_listener_id =
        ListenerId::for_tenant_workload_listener(&tenant, "workload-a", "other");
    let error = WorkloadNetworkListenerBlueprint::from_wire(
        listener_id,
        PublishedEndpointId::for_workload_endpoint(INCARNATION, "api"),
        PortLeaseId::for_listener(&other_listener_id),
        "api".to_owned(),
        EndpointProtocol::Http,
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        WorkloadNetworkPortRequestMode::ProviderAssigned,
        direct_cleartext(),
        None,
    )
    .expect_err("crossed lease identity should fail");

    assert!(matches!(
        error,
        WorkloadNetworkPlanError::PortLeaseIdentityMismatch { .. }
    ));
}

#[test]
fn strict_wire_rejects_unknown_versions_and_fields() {
    let content = populated_content([route("orders", "api")], [listener("api")]);
    for version in [0, 1, 3] {
        let mut unsupported = serde_json::to_value(&content).expect("content should serialize");
        unsupported["formatVersion"] = serde_json::json!(version);
        assert!(
            serde_json::from_value::<WorkloadNetworkPlanContent>(unsupported)
                .expect_err("unknown content version should fail")
                .to_string()
                .contains("unsupported")
        );
    }

    let mut unknown_content_field =
        serde_json::to_value(&content).expect("content should serialize");
    unknown_content_field["providerHandle"] = serde_json::json!("opaque");
    assert!(
        serde_json::from_value::<WorkloadNetworkPlanContent>(unknown_content_field)
            .expect_err("unknown content field should fail")
            .to_string()
            .contains("unknown field")
    );

    let mut unknown_listener_field =
        serde_json::to_value(&content).expect("content should serialize");
    unknown_listener_field["listeners"][0]["leaseEpoch"] = serde_json::json!(7);
    assert!(
        serde_json::from_value::<WorkloadNetworkPlanContent>(unknown_listener_field)
            .expect_err("unknown listener field should fail")
            .to_string()
            .contains("unknown field")
    );

    let compiled = CompiledWorkloadNetworkPlan::new(plan_for(&content), content)
        .expect("matching content should compile");
    let mut unknown_compiled_field =
        serde_json::to_value(compiled).expect("compiled plan should serialize");
    unknown_compiled_field["leaseEpoch"] = serde_json::json!(7);
    assert!(
        serde_json::from_value::<CompiledWorkloadNetworkPlan>(unknown_compiled_field)
            .expect_err("unknown compiled-plan field should fail")
            .to_string()
            .contains("unknown field")
    );
}

#[test]
fn exact_content_digest_is_required_on_construction_and_deserialization() {
    let content = populated_content([route("orders", "api")], [listener("api")]);
    let matching = CompiledWorkloadNetworkPlan::new(plan_for(&content), content.clone())
        .expect("matching digest should compile");
    let tenant = TenantId::new("tenant-a").expect("tenant should validate");
    let mismatched_plan = NetworkPlan::new(
        NetworkPlanId::for_tenant_workload_plan(&tenant, "workload-a"),
        NetworkResourceGeneration::new(1),
        NetworkPlanContentDigest::sha256(b"different content"),
        requirements(content.sovereignty_requirements().clone()),
    );
    assert!(matches!(
        CompiledWorkloadNetworkPlan::new(mismatched_plan, content.clone()),
        Err(WorkloadNetworkPlanError::ContentDigestMismatch { .. })
    ));

    let mut wire = serde_json::to_value(matching).expect("compiled plan should serialize");
    wire["content"]["listeners"][0]["desiredHostAddress"] = serde_json::json!("0.0.0.0");
    assert!(
        serde_json::from_value::<CompiledWorkloadNetworkPlan>(wire)
            .expect_err("tampered content should fail digest authentication")
            .to_string()
            .contains("does not match envelope digest")
    );
}

#[test]
fn complete_envelope_is_rederived_on_construction_and_deserialization() {
    let content = populated_content([route("orders", "api")], [listener("api")]);
    let exact = plan_for(&content);
    let exact_readiness = exact.readiness_requirements().to_vec();

    let other_tenant = TenantId::new("tenant-b").expect("tenant should validate");
    let crossed_identity = plan_with(
        &content,
        NetworkPlanId::for_tenant_workload_plan(&other_tenant, INCARNATION),
        exact.generation(),
        exact.requirements().clone(),
        exact_readiness.clone(),
    );
    assert!(matches!(
        CompiledWorkloadNetworkPlan::new(crossed_identity.clone(), content.clone()),
        Err(WorkloadNetworkPlanError::PlanIdentityMismatch { .. })
    ));
    assert_wire_plan_rejected(&content, &crossed_identity, "does not match derived ID");

    let crossed_generation = plan_with(
        &content,
        exact.plan_id().clone(),
        NetworkResourceGeneration::new(exact.generation().as_u64() + 1),
        exact.requirements().clone(),
        exact_readiness.clone(),
    );
    assert!(matches!(
        CompiledWorkloadNetworkPlan::new(crossed_generation.clone(), content.clone()),
        Err(WorkloadNetworkPlanError::PlanGenerationMismatch { .. })
    ));
    assert_wire_plan_rejected(&content, &crossed_generation, "retained generation");

    let changed_sovereignty = plan_with(
        &content,
        exact.plan_id().clone(),
        exact.generation(),
        requirements(NetworkSovereigntyRequirements::new(
            NetworkControlPlaneLocality::OperatorLocal,
            [NetworkExternalDependency::ExternalControlPlane],
            false,
        )),
        exact_readiness.clone(),
    );
    assert!(matches!(
        CompiledWorkloadNetworkPlan::new(changed_sovereignty.clone(), content.clone()),
        Err(WorkloadNetworkPlanError::PlanSovereigntyMismatch)
    ));
    assert_wire_plan_rejected(&content, &changed_sovereignty, "sovereignty does not match");

    let changed_capabilities = NetworkCapabilityRequirements::new(
        exact.requirements().attachment().clone(),
        exact.requirements().endpoint().clone(),
        exact.requirements().ingress().clone(),
        exact.requirements().forwarding().clone(),
        NetworkLifecycleCapabilitySet::new([NetworkLifecycleFeature::DurableInspect]),
        exact.requirements().sovereignty().clone(),
    );
    let crossed_capabilities = plan_with(
        &content,
        exact.plan_id().clone(),
        exact.generation(),
        changed_capabilities,
        exact_readiness.clone(),
    );
    assert!(matches!(
        CompiledWorkloadNetworkPlan::new(crossed_capabilities.clone(), content.clone()),
        Err(WorkloadNetworkPlanError::PlanCapabilityRequirementsMismatch)
    ));
    assert_wire_plan_rejected(&content, &crossed_capabilities, "capabilities do not match");

    let mut removed_readiness = exact_readiness.clone();
    removed_readiness.pop().expect("fixture has readiness");
    let missing_readiness = plan_with(
        &content,
        exact.plan_id().clone(),
        exact.generation(),
        exact.requirements().clone(),
        removed_readiness,
    );
    assert!(matches!(
        CompiledWorkloadNetworkPlan::new(missing_readiness.clone(), content.clone()),
        Err(WorkloadNetworkPlanError::PlanReadinessRequirementsMismatch)
    ));
    assert_wire_plan_rejected(&content, &missing_readiness, "readiness does not match");

    let mut provider_crossed = exact_readiness;
    let first = provider_crossed
        .first_mut()
        .expect("fixture has readiness requirements");
    *first = NetworkReadinessRequirement::new(
        first.resource_id().clone(),
        NetworkProviderId::for_registration_key("crossed-provider"),
        first.condition_kind(),
    );
    let crossed_readiness = plan_with(
        &content,
        exact.plan_id().clone(),
        exact.generation(),
        exact.requirements().clone(),
        provider_crossed,
    );
    assert!(matches!(
        CompiledWorkloadNetworkPlan::new(crossed_readiness.clone(), content.clone()),
        Err(WorkloadNetworkPlanError::PlanReadinessRequirementsMismatch)
    ));
    assert_wire_plan_rejected(&content, &crossed_readiness, "readiness does not match");
}

#[test]
fn tenant_qualified_resource_ids_are_rederived_during_construction_and_decode() {
    let base = populated_content([route("orders", "api")], [listener("api")]);
    let other_identity = WorkloadNetworkPlanIdentity::new(
        TenantId::new("tenant-b").expect("tenant should validate"),
        "other-incarnation",
        base.identity().generation(),
    )
    .expect("other identity should validate");

    let crossed_attachment = WorkloadNetworkAttachmentBlueprint::from_wire(
        other_identity.attachment_id("default"),
        "default".to_owned(),
    )
    .expect("raw attachment should be structurally valid");
    assert!(matches!(
        WorkloadNetworkPlanContent::new(
            base.identity().clone(),
            base.capability_requirements().clone(),
            base.capability_selection().cloned(),
            base.capability_selection_evidence().cloned(),
            Some(crossed_attachment),
            base.routes().iter().cloned(),
            base.listeners().iter().cloned(),
            base.dependency_listeners().iter().cloned(),
            base.activation(),
            base.publication(),
        ),
        Err(WorkloadNetworkPlanError::AttachmentIdentityMismatch { .. })
    ));

    let crossed_route = WorkloadNetworkRouteBlueprint::from_wire(
        other_identity.route_id("orders", "api"),
        "orders".to_owned(),
        "api".to_owned(),
        EndpointProtocol::Http,
        "api.internal".to_owned(),
        8080,
        Some(3000),
    )
    .expect("raw route should be structurally valid");
    assert!(matches!(
        WorkloadNetworkPlanContent::new(
            base.identity().clone(),
            base.capability_requirements().clone(),
            base.capability_selection().cloned(),
            base.capability_selection_evidence().cloned(),
            base.attachment().cloned(),
            [crossed_route],
            base.listeners().iter().cloned(),
            base.dependency_listeners().iter().cloned(),
            base.activation(),
            base.publication(),
        ),
        Err(WorkloadNetworkPlanError::RouteIdentityMismatch { .. })
    ));

    let crossed_listener_id = other_identity.listener_id("api");
    let crossed_listener = WorkloadNetworkListenerBlueprint::from_wire(
        crossed_listener_id.clone(),
        other_identity.endpoint_id("api"),
        PortLeaseId::for_listener(&crossed_listener_id),
        "api".to_owned(),
        EndpointProtocol::Http,
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        WorkloadNetworkPortRequestMode::ProviderAssigned,
        forwarded_http(),
        Some(3000),
    )
    .expect("matched crossed listener and lease should be structurally valid");
    assert!(matches!(
        WorkloadNetworkPlanContent::new(
            base.identity().clone(),
            base.capability_requirements().clone(),
            base.capability_selection().cloned(),
            base.capability_selection_evidence().cloned(),
            base.attachment().cloned(),
            base.routes().iter().cloned(),
            [crossed_listener],
            base.dependency_listeners().iter().cloned(),
            base.activation(),
            base.publication(),
        ),
        Err(WorkloadNetworkPlanError::ListenerIdentityMismatch { .. })
    ));

    let correct_listener_id = base.listeners()[0].listener_id().clone();
    let crossed_endpoint = WorkloadNetworkListenerBlueprint::from_wire(
        correct_listener_id.clone(),
        other_identity.endpoint_id("api"),
        PortLeaseId::for_listener(&correct_listener_id),
        "api".to_owned(),
        EndpointProtocol::Http,
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        WorkloadNetworkPortRequestMode::ProviderAssigned,
        forwarded_http(),
        Some(3000),
    )
    .expect("crossed endpoint should remain structurally valid");
    assert!(matches!(
        WorkloadNetworkPlanContent::new(
            base.identity().clone(),
            base.capability_requirements().clone(),
            base.capability_selection().cloned(),
            base.capability_selection_evidence().cloned(),
            base.attachment().cloned(),
            base.routes().iter().cloned(),
            [crossed_endpoint],
            base.dependency_listeners().iter().cloned(),
            base.activation(),
            base.publication(),
        ),
        Err(WorkloadNetworkPlanError::PublishedEndpointIdentityMismatch { .. })
    ));

    let base_wire = serde_json::to_value(&base).expect("content should serialize");
    let assert_decode_rejected = |wire, expected: &str| {
        let error = serde_json::from_value::<WorkloadNetworkPlanContent>(wire)
            .expect_err("crossed resource identity must fail decoding");
        assert!(error.to_string().contains(expected), "{error}");
    };

    let mut crossed_attachment_wire = base_wire.clone();
    crossed_attachment_wire["attachment"]["attachmentId"] =
        serde_json::to_value(other_identity.attachment_id("default"))
            .expect("attachment ID should serialize");
    assert_decode_rejected(crossed_attachment_wire, "attachment ID");

    let mut crossed_route_wire = base_wire.clone();
    crossed_route_wire["routes"][0]["routeId"] =
        serde_json::to_value(other_identity.route_id("orders", "api"))
            .expect("route ID should serialize");
    assert_decode_rejected(crossed_route_wire, "route ID");
    for field in ["serviceName", "routeName"] {
        let mut crossed_name_wire = base_wire.clone();
        crossed_name_wire["routes"][0][field] = serde_json::json!("crossed-name");
        assert_decode_rejected(crossed_name_wire, "route ID");
    }

    let mut crossed_listener_wire = base_wire.clone();
    crossed_listener_wire["listeners"][0]["listenerId"] =
        serde_json::to_value(&crossed_listener_id).expect("listener ID should serialize");
    crossed_listener_wire["listeners"][0]["portLeaseId"] =
        serde_json::to_value(PortLeaseId::for_listener(&crossed_listener_id))
            .expect("lease ID should serialize");
    assert_decode_rejected(crossed_listener_wire, "listener ID");

    let mut crossed_endpoint_wire = base_wire.clone();
    crossed_endpoint_wire["listeners"][0]["endpointId"] =
        serde_json::to_value(other_identity.endpoint_id("api"))
            .expect("endpoint ID should serialize");
    assert_decode_rejected(crossed_endpoint_wire, "endpoint ID");

    let mut crossed_listener_name_wire = base_wire;
    crossed_listener_name_wire["listeners"][0]["name"] = serde_json::json!("crossed-name");
    assert_decode_rejected(crossed_listener_name_wire, "listener ID");
}

#[test]
fn dependency_listener_is_exact_readiness_provenance() {
    let identity = identity();
    let content = WorkloadNetworkPlanContent::new(
        identity.clone(),
        requirements(sovereignty()),
        Some(selection()),
        Some(selection_evidence()),
        Some(
            WorkloadNetworkAttachmentBlueprint::new(&identity, "default")
                .expect("attachment should validate"),
        ),
        [],
        [listener("api")],
        [dependency_listener("nimbus-internal-egress-pep")],
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    )
    .expect("dependency content should validate");
    let compiled = CompiledWorkloadNetworkPlan::from_content(content.clone())
        .expect("dependency content should derive an envelope");
    assert_eq!(compiled.plan().readiness_requirements().len(), 3);
    let dependency = content
        .dependency_listeners()
        .first()
        .expect("dependency should be retained");
    assert!(
        compiled
            .plan()
            .readiness_requirements()
            .contains(&NetworkReadinessRequirement::new(
                dependency.listener_id().clone().into(),
                dependency.provider_id().clone(),
                NetworkConditionKind::Ready,
            ))
    );
}

#[test]
fn address_and_port_mutations_change_content_without_changing_ids() {
    let first_listener = listener_with(
        "api",
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        WorkloadNetworkPortRequestMode::exact(
            NonZeroU16::new(8080).expect("fixture port should be non-zero"),
        ),
    );
    let second_listener = listener_with(
        "api",
        IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        WorkloadNetworkPortRequestMode::exact(
            NonZeroU16::new(9090).expect("fixture port should be non-zero"),
        ),
    );
    assert_eq!(first_listener.listener_id(), second_listener.listener_id());
    assert_eq!(first_listener.endpoint_id(), second_listener.endpoint_id());
    assert_eq!(
        first_listener.port_lease_id(),
        second_listener.port_lease_id()
    );

    let first = populated_content([], [first_listener]);
    let second = populated_content([], [second_listener]);
    assert_ne!(first.canonical_bytes(), second.canonical_bytes());
}

#[test]
fn semantic_field_mutations_change_the_exact_canonical_bytes() {
    let base = populated_content([route("orders", "api")], [listener("api")]);
    assert_eq!(
        base.canonical_bytes(),
        serde_json::to_vec(&base).expect("content should serialize once")
    );

    fn retained_leaf_paths(value: &serde_json::Value, path: &str, output: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(fields) => {
                for (name, value) in fields {
                    retained_leaf_paths(value, &format!("{path}/{name}"), output);
                }
            }
            serde_json::Value::Array(values) if !values.is_empty() => {
                for (index, value) in values.iter().enumerate() {
                    retained_leaf_paths(value, &format!("{path}/{index}"), output);
                }
            }
            serde_json::Value::Array(_) | serde_json::Value::Null => output.push(path.to_owned()),
            serde_json::Value::Bool(_)
            | serde_json::Value::Number(_)
            | serde_json::Value::String(_) => output.push(path.to_owned()),
        }
    }

    let base_value = serde_json::to_value(&base).expect("content should serialize");
    let mut leaf_paths = Vec::new();
    retained_leaf_paths(&base_value, "", &mut leaf_paths);
    leaf_paths.sort();
    assert_eq!(
        leaf_paths,
        [
            "/activation",
            "/attachment/attachmentId",
            "/attachment/name",
            "/capabilityRequirements/attachment/attachment_modes",
            "/capabilityRequirements/attachment/isolation_modes",
            "/capabilityRequirements/attachment/management_mode",
            "/capabilityRequirements/endpoint/address_families",
            "/capabilityRequirements/endpoint/bind_realms",
            "/capabilityRequirements/endpoint/exposures",
            "/capabilityRequirements/endpoint/port_assignment_modes",
            "/capabilityRequirements/endpoint/protocols",
            "/capabilityRequirements/forwarding/features",
            "/capabilityRequirements/ingress/features",
            "/capabilityRequirements/ingress/tls_behaviors",
            "/capabilityRequirements/lifecycle/features",
            "/capabilityRequirements/sovereignty/allowed_external_dependencies",
            "/capabilityRequirements/sovereignty/maximum_control_plane_locality",
            "/capabilityRequirements/sovereignty/offline_restart_required",
            "/capabilitySelection/attachment_provider_id",
            "/capabilitySelection/ingress_provider_id",
            "/capabilitySelectionEvidence/selection/attachment_provider_id",
            "/capabilitySelectionEvidence/selection/ingress_provider_id",
            "/capabilitySelectionEvidence/source_digest",
            "/dependencyListeners",
            "/formatVersion",
            "/identity/generation",
            "/identity/tenantId",
            "/identity/workloadIncarnationKey",
            "/listeners/0/desiredHostAddress",
            "/listeners/0/endpointId",
            "/listeners/0/endpointSemantics/forwarding",
            "/listeners/0/endpointSemantics/tls",
            "/listeners/0/guestPort",
            "/listeners/0/listenerId",
            "/listeners/0/name",
            "/listeners/0/portLeaseId",
            "/listeners/0/portRequest/kind",
            "/listeners/0/portRequest/port",
            "/listeners/0/protocol",
            "/publication",
            "/routes/0/guestPort",
            "/routes/0/host",
            "/routes/0/hostPort",
            "/routes/0/protocol",
            "/routes/0/routeId",
            "/routes/0/routeName",
            "/routes/0/serviceName",
        ],
        "every retained semantic leaf must be named by the digest-completeness proof"
    );
    let base_digest = NetworkPlanContentDigest::sha256(base.canonical_bytes());
    for path in leaf_paths {
        let mut candidate = base_value.clone();
        let leaf = candidate
            .pointer_mut(&path)
            .unwrap_or_else(|| panic!("retained semantic leaf {path} must exist"));
        *leaf = match leaf {
            serde_json::Value::Bool(value) => serde_json::Value::Bool(!*value),
            serde_json::Value::Number(value) => {
                serde_json::json!(value.as_u64().expect("fixture numbers are unsigned") + 1)
            }
            serde_json::Value::String(value) => {
                serde_json::Value::String(format!("{value}-changed"))
            }
            serde_json::Value::Array(values) => {
                let mut values = values.clone();
                values.push(serde_json::json!("external_control_plane"));
                serde_json::Value::Array(values)
            }
            serde_json::Value::Null | serde_json::Value::Object(_) => {
                panic!("{path} is not a semantic leaf")
            }
        };
        let candidate_bytes =
            serde_json::to_vec(&candidate).expect("mutated retained JSON should serialize");
        assert_ne!(base.canonical_bytes(), candidate_bytes, "leaf {path}");
        assert_ne!(
            base_digest,
            NetworkPlanContentDigest::sha256(candidate_bytes),
            "leaf {path} must contribute to the exact content digest"
        );
    }

    let provider_assigned = populated_content(
        [route("orders", "api")],
        [listener_with(
            "api",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            WorkloadNetworkPortRequestMode::ProviderAssigned,
        )],
    );
    let changed_route = populated_content([route("orders", "metrics")], [listener("api")]);
    let changed_sovereignty = WorkloadNetworkPlanContent::new(
        base.identity().clone(),
        requirements(NetworkSovereigntyRequirements::new(
            NetworkControlPlaneLocality::OperatorLocal,
            [NetworkExternalDependency::ExternalControlPlane],
            false,
        )),
        base.capability_selection().cloned(),
        base.capability_selection_evidence().cloned(),
        base.attachment().cloned(),
        base.routes().iter().cloned(),
        base.listeners().iter().cloned(),
        base.dependency_listeners().iter().cloned(),
        base.activation(),
        base.publication(),
    )
    .expect("changed sovereignty should validate");
    let withheld = WorkloadNetworkPlanContent::new(
        base.identity().clone(),
        base.capability_requirements().clone(),
        base.capability_selection().cloned(),
        base.capability_selection_evidence().cloned(),
        base.attachment().cloned(),
        base.routes().iter().cloned(),
        base.listeners().iter().cloned(),
        base.dependency_listeners().iter().cloned(),
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
    )
    .expect("changed lifecycle intent should validate");

    for candidate in [
        provider_assigned,
        changed_route,
        changed_sovereignty,
        withheld,
    ] {
        assert_ne!(base.canonical_bytes(), candidate.canonical_bytes());
    }
}

#[test]
fn required_strings_and_numeric_ports_fail_closed() {
    assert!(matches!(
        WorkloadNetworkRouteBlueprint::new(
            &identity(),
            " ",
            "api",
            EndpointProtocol::Http,
            "api.internal",
            8080,
            None,
        ),
        Err(WorkloadNetworkPlanError::EmptyRequiredField { .. })
    ));
    assert!(matches!(
        WorkloadNetworkRouteBlueprint::new(
            &identity(),
            "orders",
            "api",
            EndpointProtocol::Http,
            "api.internal",
            0,
            None,
        ),
        Err(WorkloadNetworkPlanError::ZeroPort { .. })
    ));
    assert!(matches!(
        WorkloadNetworkAttachmentBlueprint::new(&identity(), ""),
        Err(WorkloadNetworkPlanError::EmptyRequiredField { .. })
    ));

    assert!(matches!(
        WorkloadNetworkRouteBlueprint::new(
            &identity(),
            "orders",
            "api",
            EndpointProtocol::Http,
            "https://api.internal/path",
            8080,
            None,
        ),
        Err(WorkloadNetworkPlanError::InvalidRouteHost { .. })
    ));

    assert!(matches!(
        WorkloadNetworkListenerBlueprint::new(
            &identity(),
            "bad name",
            EndpointProtocol::Tcp,
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            WorkloadNetworkPortRequestMode::ProviderAssigned,
            direct_cleartext(),
            None,
        ),
        Err(WorkloadNetworkPlanError::InvalidRequiredField { .. })
    ));
}
