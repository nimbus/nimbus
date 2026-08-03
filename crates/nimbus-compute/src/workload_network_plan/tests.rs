use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use nimbus_core::TenantId;
use nimbus_network::{
    EndpointProtocol, NetworkAddressFamily, NetworkAttachmentProviderRegistration,
    NetworkBindRealmKind, NetworkCapabilityBundle, NetworkCapabilityDimension,
    NetworkCapabilityRegistry, NetworkCapabilitySelection, NetworkControlPlaneLocality,
    NetworkEndpointCapabilitySet, NetworkExposure, NetworkExternalDependency,
    NetworkForwardingCapabilitySet, NetworkForwardingFeature, NetworkIngressCapabilitySet,
    NetworkIngressFeature, NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet,
    NetworkLifecycleFeature, NetworkPlanUpdate, NetworkPortAssignmentMode, NetworkProviderId,
    NetworkResourceId, NetworkSovereigntyCapabilities, NetworkSovereigntyRequirements,
    NetworkTlsBehavior, PortProtocol,
};
use nimbus_sandbox::{
    SandboxBackendKind, SandboxOwnerSpec, SandboxPortBinding, SandboxProcessSpec, SandboxRootSpec,
    SandboxSpec, sandbox_network_plan_requirements,
};
use nimbus_tenant::{
    TenantIsolationContext, TenantIsolationDecision, TenantIsolationPolicyInput,
    TenantNetworkEndpointDecision, TenantNetworkPolicyDecision, TenantServiceGrantPolicyDecision,
    WorkloadAttributes, WorkloadKind, WorkloadLocation,
};
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, WorkloadActivationIntent, WorkloadNetworkForwardingBehavior,
    WorkloadPublicationIntent,
};

use super::{
    AdmittedWorkloadNetworkSource, EGRESS_PEP_LISTENER_NAME, WorkloadNetworkEndpointSemanticsInput,
    WorkloadNetworkPlanCompileError, WorkloadNetworkPlanCompiler, require_sovereignty_refinement,
};

const TENANT: &str = "tenant-a";
const GENERATION: u64 = 7;

fn sovereignty() -> NetworkSovereigntyRequirements {
    NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true)
}

fn sandbox_spec(
    tenant: &str,
    owner: SandboxOwnerSpec,
    backend: SandboxBackendKind,
    bindings: impl IntoIterator<Item = SandboxPortBinding>,
) -> SandboxSpec {
    SandboxSpec::new(
        TenantId::new(tenant).expect("tenant should parse"),
        owner,
        backend,
        SandboxRootSpec::rootfs("/fixture/rootfs"),
        SandboxProcessSpec::new(["/bin/true"]),
    )
    .with_port_bindings(bindings)
}

fn endpoint_semantics(spec: &SandboxSpec) -> Vec<WorkloadNetworkEndpointSemanticsInput<'_>> {
    spec.port_bindings
        .iter()
        .map(|binding| {
            WorkloadNetworkEndpointSemanticsInput::new(
                &binding.name,
                WorkloadNetworkForwardingBehavior::PortForwarded,
                match binding.protocol {
                    EndpointProtocol::Https => NetworkTlsBehavior::TerminateAtIngress,
                    EndpointProtocol::Tcp | EndpointProtocol::Http => NetworkTlsBehavior::Disabled,
                },
            )
        })
        .collect()
}

fn admitted_decision(
    tenant: &str,
    workload: WorkloadAttributes,
    generation: Option<u64>,
    node: Option<&str>,
    endpoints: impl IntoIterator<Item = TenantNetworkEndpointDecision>,
) -> TenantIsolationDecision {
    let mut context = TenantIsolationContext::system(
        TenantId::new(tenant).expect("tenant should parse"),
        "network-plan-compiler-test",
    );
    if let Some(generation) = generation {
        context = context.with_deployment_generation(generation);
    }
    if let Some(node) = node {
        context = context.with_workload_location(WorkloadLocation::new().with_node_id(node));
    }
    context
        .admit_decision(
            TenantIsolationPolicyInput::new(workload)
                .with_services(TenantServiceGrantPolicyDecision::new(["upstream"]))
                .with_network(TenantNetworkPolicyDecision::new(endpoints)),
        )
        .expect("fixture decision should admit")
}

fn standalone_decision(
    tenant: &str,
    profile: &str,
    sandbox_id: &str,
    backend: SandboxBackendKind,
    generation: Option<u64>,
    node: Option<&str>,
) -> TenantIsolationDecision {
    admitted_decision(
        tenant,
        WorkloadAttributes::sandbox(profile)
            .with_sandbox_id(sandbox_id)
            .with_sandbox_backend(backend),
        generation,
        node,
        [],
    )
}

fn ingress_registration(
    ingress_provider_id: NetworkProviderId,
    tls: bool,
    forwarding: bool,
    public_exposure: bool,
) -> NetworkIngressProviderRegistration {
    let mut features = vec![NetworkIngressFeature::Streaming];
    if tls {
        features.push(NetworkIngressFeature::TlsTermination);
    }
    let mut exposures = vec![NetworkExposure::Loopback, NetworkExposure::Private];
    if public_exposure {
        exposures.push(NetworkExposure::Public);
    }
    NetworkIngressProviderRegistration::new(
        ingress_provider_id,
        NetworkEndpointCapabilitySet::new(
            [NetworkAddressFamily::Ipv4, NetworkAddressFamily::Ipv6],
            [NetworkBindRealmKind::Host],
            exposures,
            [PortProtocol::Tcp],
            [
                NetworkPortAssignmentMode::Exact,
                NetworkPortAssignmentMode::ProviderAssigned,
            ],
        ),
        NetworkIngressCapabilitySet::new(features).with_tls_behaviors(
            std::iter::once(NetworkTlsBehavior::Disabled)
                .chain(tls.then_some(NetworkTlsBehavior::TerminateAtIngress)),
        ),
        NetworkForwardingCapabilitySet::new(
            forwarding.then_some(NetworkForwardingFeature::PortForwarding),
        ),
        complete_lifecycle(),
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    )
}

fn registry_for(
    backend: SandboxBackendKind,
    tls: bool,
) -> (NetworkCapabilityRegistry, NetworkCapabilitySelection) {
    registry_for_capabilities(backend, tls, true, true)
}

fn registry_for_capabilities(
    backend: SandboxBackendKind,
    tls: bool,
    forwarding: bool,
    public_exposure: bool,
) -> (NetworkCapabilityRegistry, NetworkCapabilitySelection) {
    let source = sandbox_network_plan_requirements(backend);
    let ingress_provider_id = NetworkProviderId::for_registration_key("test.local-ingress");
    let attachment = NetworkAttachmentProviderRegistration::new(
        source.required_attachment_provider_id().clone(),
        source.capability_requirements().attachment().clone(),
        [NetworkAddressFamily::Ipv4, NetworkAddressFamily::Ipv6],
        complete_lifecycle(),
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let ingress = ingress_registration(
        ingress_provider_id.clone(),
        tls,
        forwarding,
        public_exposure,
    );
    let selection = NetworkCapabilitySelection::new(
        source.required_attachment_provider_id().clone(),
        ingress_provider_id,
    );
    (
        NetworkCapabilityRegistry::new([NetworkCapabilityBundle::new(attachment, ingress)])
            .expect("fixture registry should be valid"),
        selection,
    )
}

fn complete_lifecycle() -> NetworkLifecycleCapabilitySet {
    NetworkLifecycleCapabilitySet::new([
        NetworkLifecycleFeature::DurableInspect,
        NetworkLifecycleFeature::Reconcile,
        NetworkLifecycleFeature::Delete,
    ])
}

fn compile_standalone(
    decision: &TenantIsolationDecision,
    spec: &SandboxSpec,
    selection: &NetworkCapabilitySelection,
    registry: &NetworkCapabilityRegistry,
) -> Result<CompiledWorkloadNetworkPlan, WorkloadNetworkPlanCompileError> {
    let publication = if spec.port_bindings.is_empty() {
        WorkloadPublicationIntent::Withheld
    } else {
        WorkloadPublicationIntent::PublishWhenReady
    };
    WorkloadNetworkPlanCompiler.compile(
        decision,
        AdmittedWorkloadNetworkSource::Sandbox {
            stable_resource_id: decision
                .workload()
                .sandbox_id()
                .expect("standalone fixture should have an ID"),
            profile: decision.workload().name(),
            generation: GENERATION,
            sandbox_spec: spec,
        },
        Some(selection),
        registry,
        sovereignty(),
        &endpoint_semantics(spec),
        WorkloadActivationIntent::ActivateWhenAttached,
        publication,
    )
}

#[test]
fn explicit_empty_plan_is_deterministic_and_resource_free() {
    let decision = admitted_decision(
        TENANT,
        WorkloadAttributes::new(WorkloadKind::SystemTask, "maintenance"),
        Some(GENERATION),
        Some("node-a"),
        [],
    );
    let registry = NetworkCapabilityRegistry::new([]).expect("empty registry should be valid");
    let compile = || {
        WorkloadNetworkPlanCompiler
            .compile(
                &decision,
                AdmittedWorkloadNetworkSource::Empty,
                None,
                &registry,
                sovereignty(),
                &[],
                WorkloadActivationIntent::PrepareOnly,
                WorkloadPublicationIntent::Withheld,
            )
            .expect("empty admitted source should compile")
    };

    let first = compile();
    let replay = compile();
    assert_eq!(first, replay);
    assert_eq!(first.plan().generation().as_u64(), GENERATION);
    assert!(first.content().attachment().is_none());
    assert!(first.content().routes().is_empty());
    assert!(first.content().listeners().is_empty());
    assert!(first.plan().readiness_requirements().is_empty());
    assert!(first.content().capability_selection().is_none());

    let unexpected_selection = NetworkCapabilitySelection::new(
        NetworkProviderId::for_registration_key("unused-attachment"),
        NetworkProviderId::for_registration_key("unused-ingress"),
    );
    assert!(matches!(
        WorkloadNetworkPlanCompiler.compile(
            &decision,
            AdmittedWorkloadNetworkSource::Empty,
            Some(&unexpected_selection),
            &registry,
            sovereignty(),
            &[],
            WorkloadActivationIntent::PrepareOnly,
            WorkloadPublicationIntent::Withheld,
        ),
        Err(WorkloadNetworkPlanCompileError::EmptySourceHasCapabilitySelection)
    ));

    let routed_decision = admitted_decision(
        TENANT,
        WorkloadAttributes::new(WorkloadKind::SystemTask, "maintenance"),
        Some(GENERATION),
        Some("node-a"),
        [TenantNetworkEndpointDecision::new(
            "upstream",
            "primary",
            EndpointProtocol::Tcp,
            "db.internal.example",
            5432,
        )],
    );
    assert!(matches!(
        WorkloadNetworkPlanCompiler.compile(
            &routed_decision,
            AdmittedWorkloadNetworkSource::Empty,
            None,
            &registry,
            sovereignty(),
            &[],
            WorkloadActivationIntent::PrepareOnly,
            WorkloadPublicationIntent::Withheld,
        ),
        Err(WorkloadNetworkPlanCompileError::EmptySourceHasRoutes)
    ));
}

#[test]
fn sandbox_plan_retains_attachment_listeners_and_exact_readiness() {
    let decision = standalone_decision(
        TENANT,
        "python",
        "sandbox-a",
        SandboxBackendKind::Krun,
        Some(GENERATION),
        Some("node-a"),
    );
    let spec = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::standalone_named("python"),
        SandboxBackendKind::Krun,
        [
            SandboxPortBinding::new("https", EndpointProtocol::Https, 18443, 8443)
                .with_host_address(IpAddr::V6(Ipv6Addr::LOCALHOST)),
            SandboxPortBinding::new("http", EndpointProtocol::Http, 0, 8080)
                .with_host_address(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        ],
    );
    let (registry, selection) = registry_for(SandboxBackendKind::Krun, true);

    let compiled = compile_standalone(&decision, &spec, &selection, &registry)
        .expect("sandbox plan should compile");
    let content = compiled.content();
    assert_eq!(content.attachment().expect("attachment").name(), "default");
    assert_eq!(
        content
            .listeners()
            .iter()
            .map(|listener| listener.name())
            .collect::<Vec<_>>(),
        ["http", "https"]
    );
    assert_eq!(compiled.plan().readiness_requirements().len(), 4);
    assert!(
        compiled
            .plan()
            .readiness_requirements()
            .iter()
            .any(|requirement| {
                matches!(requirement.resource_id(), NetworkResourceId::Attachment(_))
                    && requirement.provider_id() == selection.attachment_provider_id()
            })
    );
    assert_eq!(
        compiled
            .plan()
            .readiness_requirements()
            .iter()
            .filter(|requirement| matches!(
                requirement.resource_id(),
                NetworkResourceId::Listener(_)
            ))
            .count(),
        3,
        "two published listeners plus one internal PEP listener must become ready"
    );
    assert!(
        compiled
            .plan()
            .requirements()
            .endpoint()
            .address_families()
            .contains(&NetworkAddressFamily::Ipv6)
    );
    assert!(
        compiled
            .plan()
            .requirements()
            .ingress()
            .tls_behaviors()
            .contains(&NetworkTlsBehavior::TerminateAtIngress)
    );
}

#[test]
fn endpoint_semantics_reject_missing_extra_duplicate_and_crossed_names() {
    let decision = standalone_decision(
        TENANT,
        "python",
        "sandbox-semantics",
        SandboxBackendKind::Container,
        Some(GENERATION),
        Some("node-a"),
    );
    let spec = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::standalone_named("python"),
        SandboxBackendKind::Container,
        [SandboxPortBinding::new(
            "http",
            EndpointProtocol::Http,
            18080,
            8080,
        )],
    );
    let (registry, selection) = registry_for(SandboxBackendKind::Container, false);
    let valid = WorkloadNetworkEndpointSemanticsInput::new(
        "http",
        WorkloadNetworkForwardingBehavior::PortForwarded,
        NetworkTlsBehavior::Disabled,
    );
    let extra = WorkloadNetworkEndpointSemanticsInput::new(
        "other",
        WorkloadNetworkForwardingBehavior::PortForwarded,
        NetworkTlsBehavior::Disabled,
    );
    let compile = |semantics: &[WorkloadNetworkEndpointSemanticsInput<'_>]| {
        WorkloadNetworkPlanCompiler.compile(
            &decision,
            AdmittedWorkloadNetworkSource::Sandbox {
                stable_resource_id: "sandbox-semantics",
                profile: "python",
                generation: GENERATION,
                sandbox_spec: &spec,
            },
            Some(&selection),
            &registry,
            sovereignty(),
            semantics,
            WorkloadActivationIntent::PrepareOnly,
            WorkloadPublicationIntent::Withheld,
        )
    };

    assert!(matches!(
        compile(&[]),
        Err(WorkloadNetworkPlanCompileError::MissingEndpointSemantics { .. })
    ));
    assert!(matches!(
        compile(&[valid, valid]),
        Err(WorkloadNetworkPlanCompileError::DuplicateEndpointSemantics { .. })
    ));
    assert!(matches!(
        compile(&[valid, extra]),
        Err(WorkloadNetworkPlanCompileError::UnexpectedEndpointSemantics { .. })
    ));
    assert!(matches!(
        compile(&[extra]),
        Err(WorkloadNetworkPlanCompileError::MissingEndpointSemantics { .. })
    ));
}

#[test]
fn crossed_forwarding_semantics_rejects_before_submission() {
    let decision = standalone_decision(
        TENANT,
        "python",
        "sandbox-forwarding",
        SandboxBackendKind::Container,
        Some(GENERATION),
        Some("node-a"),
    );
    let spec = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::standalone_named("python"),
        SandboxBackendKind::Container,
        [SandboxPortBinding::new(
            "http",
            EndpointProtocol::Http,
            18080,
            8080,
        )],
    );
    let (registry, selection) = registry_for(SandboxBackendKind::Container, false);
    assert!(matches!(
        WorkloadNetworkPlanCompiler.compile(
            &decision,
            AdmittedWorkloadNetworkSource::Sandbox {
                stable_resource_id: "sandbox-forwarding",
                profile: "python",
                generation: GENERATION,
                sandbox_spec: &spec,
            },
            Some(&selection),
            &registry,
            sovereignty(),
            &[WorkloadNetworkEndpointSemanticsInput::new(
                "http",
                WorkloadNetworkForwardingBehavior::None,
                NetworkTlsBehavior::Disabled,
            )],
            WorkloadActivationIntent::PrepareOnly,
            WorkloadPublicationIntent::Withheld,
        ),
        Err(WorkloadNetworkPlanCompileError::ForwardingBehaviorMismatch)
    ));
}

#[test]
fn crossed_tls_semantics_rejects_before_submission() {
    let decision = standalone_decision(
        TENANT,
        "python",
        "sandbox-tls",
        SandboxBackendKind::Container,
        Some(GENERATION),
        Some("node-a"),
    );
    let spec = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::standalone_named("python"),
        SandboxBackendKind::Container,
        [SandboxPortBinding::new(
            "http",
            EndpointProtocol::Http,
            18080,
            8080,
        )],
    );
    let (registry, selection) = registry_for(SandboxBackendKind::Container, true);
    assert!(matches!(
        WorkloadNetworkPlanCompiler.compile(
            &decision,
            AdmittedWorkloadNetworkSource::Sandbox {
                stable_resource_id: "sandbox-tls",
                profile: "python",
                generation: GENERATION,
                sandbox_spec: &spec,
            },
            Some(&selection),
            &registry,
            sovereignty(),
            &[WorkloadNetworkEndpointSemanticsInput::new(
                "http",
                WorkloadNetworkForwardingBehavior::PortForwarded,
                NetworkTlsBehavior::TerminateAtIngress,
            )],
            WorkloadActivationIntent::PrepareOnly,
            WorkloadPublicationIntent::Withheld,
        ),
        Err(WorkloadNetworkPlanCompileError::TlsBehaviorMismatch)
    ));
}

#[test]
fn listener_order_is_canonical_and_address_or_port_does_not_change_identity() {
    let decision = standalone_decision(
        TENANT,
        "python",
        "sandbox-a",
        SandboxBackendKind::Container,
        Some(GENERATION),
        Some("node-a"),
    );
    let first_spec = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::standalone(),
        SandboxBackendKind::Container,
        [
            SandboxPortBinding::new("zeta", EndpointProtocol::Tcp, 19000, 9000),
            SandboxPortBinding::new("alpha", EndpointProtocol::Tcp, 18000, 8000),
        ],
    );
    let reversed_spec = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::standalone(),
        SandboxBackendKind::Container,
        [
            SandboxPortBinding::new("alpha", EndpointProtocol::Tcp, 18000, 8000),
            SandboxPortBinding::new("zeta", EndpointProtocol::Tcp, 19000, 9000),
        ],
    );
    let changed_spec = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::standalone(),
        SandboxBackendKind::Container,
        [
            SandboxPortBinding::new("alpha", EndpointProtocol::Tcp, 28000, 8000)
                .with_host_address("10.0.0.2".parse().expect("IP")),
            SandboxPortBinding::new("zeta", EndpointProtocol::Tcp, 19000, 9000),
        ],
    );
    let (registry, selection) = registry_for(SandboxBackendKind::Container, false);

    let first = compile_standalone(&decision, &first_spec, &selection, &registry)
        .expect("first order should compile");
    let reversed = compile_standalone(&decision, &reversed_spec, &selection, &registry)
        .expect("reverse order should compile");
    assert_eq!(
        first, reversed,
        "input order must not change compiled bytes"
    );

    let changed = compile_standalone(&decision, &changed_spec, &selection, &registry)
        .expect("changed listener content should compile");
    let first_alpha = &first.content().listeners()[0];
    let changed_alpha = &changed.content().listeners()[0];
    assert_eq!(first_alpha.listener_id(), changed_alpha.listener_id());
    assert_eq!(first_alpha.endpoint_id(), changed_alpha.endpoint_id());
    assert_eq!(first_alpha.port_lease_id(), changed_alpha.port_lease_id());
    assert_ne!(
        first.plan().content_digest(),
        changed.plan().content_digest()
    );
    assert!(matches!(
        first.plan().classify_update(changed.plan()),
        Err(nimbus_network::NetworkPlanUpdateError::EqualGenerationContentConflict { .. })
    ));

    let other_tenant_decision = standalone_decision(
        "tenant-b",
        "python",
        "sandbox-a",
        SandboxBackendKind::Container,
        Some(GENERATION),
        Some("node-a"),
    );
    let other_tenant_spec = sandbox_spec(
        "tenant-b",
        SandboxOwnerSpec::standalone(),
        SandboxBackendKind::Container,
        [
            SandboxPortBinding::new("zeta", EndpointProtocol::Tcp, 19000, 9000),
            SandboxPortBinding::new("alpha", EndpointProtocol::Tcp, 18000, 8000),
        ],
    );
    let other_tenant = compile_standalone(
        &other_tenant_decision,
        &other_tenant_spec,
        &selection,
        &registry,
    )
    .expect("same local names in another tenant should compile independently");
    assert_ne!(first.plan().plan_id(), other_tenant.plan().plan_id());
    assert_ne!(
        first.content().listeners()[0].listener_id(),
        other_tenant.content().listeners()[0].listener_id()
    );
    assert_ne!(
        first.content().listeners()[0].endpoint_id(),
        other_tenant.content().listeners()[0].endpoint_id()
    );

    let sibling_decision = standalone_decision(
        TENANT,
        "python",
        "sandbox-b",
        SandboxBackendKind::Container,
        Some(GENERATION),
        Some("node-a"),
    );
    let sibling = compile_standalone(&sibling_decision, &first_spec, &selection, &registry)
        .expect("same-profile standalone sandboxes must compile independently");
    assert_ne!(first.plan().plan_id(), sibling.plan().plan_id());
    assert_ne!(
        first.content().listeners()[0].listener_id(),
        sibling.content().listeners()[0].listener_id()
    );
}

#[test]
fn service_routes_remain_separate_from_published_listeners() {
    let endpoint = TenantNetworkEndpointDecision::new(
        "upstream",
        "primary",
        EndpointProtocol::Tcp,
        "db.internal.example",
        5432,
    );
    let decision = admitted_decision(
        TENANT,
        WorkloadAttributes::service("api").with_sandbox_backend(SandboxBackendKind::Container),
        Some(GENERATION),
        Some("node-a"),
        [endpoint],
    );
    let spec = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::service("api"),
        SandboxBackendKind::Container,
        [SandboxPortBinding::new(
            "http",
            EndpointProtocol::Http,
            18080,
            8080,
        )],
    );
    let (registry, selection) = registry_for(SandboxBackendKind::Container, false);
    let compiled = WorkloadNetworkPlanCompiler
        .compile(
            &decision,
            AdmittedWorkloadNetworkSource::SandboxBackedService {
                service_name: "api",
                service_generation: GENERATION,
                sandbox_spec: &spec,
            },
            Some(&selection),
            &registry,
            sovereignty(),
            &endpoint_semantics(&spec),
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::PublishWhenReady,
        )
        .expect("sandbox-backed service should compile");

    assert_eq!(compiled.content().routes().len(), 1);
    assert_eq!(compiled.content().routes()[0].service_name(), "upstream");
    assert_eq!(compiled.content().listeners().len(), 1);
    assert_eq!(compiled.content().listeners()[0].name(), "http");
    assert_eq!(
        compiled.plan().readiness_requirements().len(),
        3,
        "routes add no readiness requirement"
    );
}

#[test]
fn admitted_route_permutation_compiles_to_identical_payload() {
    let primary = TenantNetworkEndpointDecision::new(
        "upstream",
        "primary",
        EndpointProtocol::Tcp,
        "db-primary.internal.example",
        5432,
    );
    let replica = TenantNetworkEndpointDecision::new(
        "upstream",
        "replica",
        EndpointProtocol::Tcp,
        "db-replica.internal.example",
        5432,
    );
    let workload =
        || WorkloadAttributes::service("api").with_sandbox_backend(SandboxBackendKind::Container);
    let first_decision = admitted_decision(
        TENANT,
        workload(),
        Some(GENERATION),
        Some("node-a"),
        [primary.clone(), replica.clone()],
    );
    let reversed_decision = admitted_decision(
        TENANT,
        workload(),
        Some(GENERATION),
        Some("node-a"),
        [replica, primary],
    );
    let spec = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::service("api"),
        SandboxBackendKind::Container,
        [],
    );
    let (registry, selection) = registry_for(SandboxBackendKind::Container, false);
    let compile = |decision: &TenantIsolationDecision| {
        WorkloadNetworkPlanCompiler
            .compile(
                decision,
                AdmittedWorkloadNetworkSource::SandboxBackedService {
                    service_name: "api",
                    service_generation: GENERATION,
                    sandbox_spec: &spec,
                },
                Some(&selection),
                &registry,
                sovereignty(),
                &endpoint_semantics(&spec),
                WorkloadActivationIntent::ActivateWhenAttached,
                WorkloadPublicationIntent::Withheld,
            )
            .expect("equivalent admitted route order should compile")
    };

    let first = compile(&first_decision);
    let reversed = compile(&reversed_decision);
    assert_eq!(first_decision.id(), reversed_decision.id());
    assert_eq!(first, reversed);
}

#[test]
fn admitted_route_address_and_port_changes_preserve_logical_resource_identity() {
    let endpoint = |host: &str, host_port: u16, guest_port: u16| {
        TenantNetworkEndpointDecision::new(
            "upstream",
            "primary",
            EndpointProtocol::Tcp,
            host,
            host_port,
        )
        .with_guest_port(guest_port)
    };
    let workload =
        || WorkloadAttributes::service("api").with_sandbox_backend(SandboxBackendKind::Container);
    let first_decision = admitted_decision(
        TENANT,
        workload(),
        Some(GENERATION),
        Some("node-a"),
        [endpoint("db-primary.internal.example", 5432, 15432)],
    );
    let changed_decision = admitted_decision(
        TENANT,
        workload(),
        Some(GENERATION),
        Some("node-a"),
        [endpoint("db-replacement.internal.example", 6432, 16432)],
    );
    assert_ne!(
        first_decision.id(),
        changed_decision.id(),
        "admission decisions must still bind changed route content"
    );

    let spec = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::service("api"),
        SandboxBackendKind::Container,
        [],
    );
    let (registry, selection) = registry_for(SandboxBackendKind::Container, false);
    let compile = |decision: &TenantIsolationDecision| {
        WorkloadNetworkPlanCompiler
            .compile(
                decision,
                AdmittedWorkloadNetworkSource::SandboxBackedService {
                    service_name: "api",
                    service_generation: GENERATION,
                    sandbox_spec: &spec,
                },
                Some(&selection),
                &registry,
                sovereignty(),
                &endpoint_semantics(&spec),
                WorkloadActivationIntent::ActivateWhenAttached,
                WorkloadPublicationIntent::Withheld,
            )
            .expect("valid admitted route should compile")
    };

    let first = compile(&first_decision);
    let changed = compile(&changed_decision);
    assert_eq!(first.plan().plan_id(), changed.plan().plan_id());
    assert_eq!(
        first
            .content()
            .attachment()
            .map(|value| value.attachment_id()),
        changed
            .content()
            .attachment()
            .map(|value| value.attachment_id())
    );
    assert_eq!(
        first.content().routes()[0].route_id(),
        changed.content().routes()[0].route_id()
    );
    assert_ne!(
        first.plan().content_digest(),
        changed.plan().content_digest(),
        "address and port remain desired content even though identity is stable"
    );
}

#[test]
fn source_correlation_fails_closed_before_capability_selection() {
    let decision = standalone_decision(
        TENANT,
        "python",
        "sandbox-a",
        SandboxBackendKind::Container,
        Some(GENERATION),
        Some("node-a"),
    );
    let crossed_tenant = sandbox_spec(
        "tenant-b",
        SandboxOwnerSpec::standalone(),
        SandboxBackendKind::Container,
        [],
    );
    let empty_registry =
        NetworkCapabilityRegistry::new([]).expect("empty registry should be valid");
    let bogus_selection = NetworkCapabilitySelection::new(
        NetworkProviderId::for_registration_key("bogus-attachment"),
        NetworkProviderId::for_registration_key("bogus-ingress"),
    );

    let error = WorkloadNetworkPlanCompiler
        .compile(
            &decision,
            AdmittedWorkloadNetworkSource::Sandbox {
                stable_resource_id: "sandbox-a",
                profile: "python",
                generation: GENERATION,
                sandbox_spec: &crossed_tenant,
            },
            Some(&bogus_selection),
            &empty_registry,
            sovereignty(),
            &endpoint_semantics(&crossed_tenant),
            WorkloadActivationIntent::PrepareOnly,
            WorkloadPublicationIntent::Withheld,
        )
        .expect_err("cross-tenant source must fail before provider lookup");
    assert!(matches!(
        error,
        WorkloadNetworkPlanCompileError::TenantMismatch { .. }
    ));
}

#[test]
fn missing_generation_node_and_crossed_source_fields_are_typed() {
    let spec = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::standalone(),
        SandboxBackendKind::Container,
        [],
    );
    let (registry, selection) = registry_for(SandboxBackendKind::Container, false);

    let no_generation = standalone_decision(
        TENANT,
        "python",
        "sandbox-a",
        SandboxBackendKind::Container,
        None,
        Some("node-a"),
    );
    assert!(matches!(
        WorkloadNetworkPlanCompiler.compile(
            &no_generation,
            AdmittedWorkloadNetworkSource::Sandbox {
                stable_resource_id: "sandbox-a",
                profile: "python",
                generation: GENERATION,
                sandbox_spec: &spec,
            },
            Some(&selection),
            &registry,
            sovereignty(),
            &endpoint_semantics(&spec),
            WorkloadActivationIntent::PrepareOnly,
            WorkloadPublicationIntent::Withheld,
        ),
        Err(WorkloadNetworkPlanCompileError::MissingDeploymentGeneration)
    ));

    let no_node = standalone_decision(
        TENANT,
        "python",
        "sandbox-a",
        SandboxBackendKind::Container,
        Some(GENERATION),
        None,
    );
    assert!(matches!(
        WorkloadNetworkPlanCompiler.compile(
            &no_node,
            AdmittedWorkloadNetworkSource::Sandbox {
                stable_resource_id: "sandbox-a",
                profile: "python",
                generation: GENERATION,
                sandbox_spec: &spec,
            },
            Some(&selection),
            &registry,
            sovereignty(),
            &endpoint_semantics(&spec),
            WorkloadActivationIntent::PrepareOnly,
            WorkloadPublicationIntent::Withheld,
        ),
        Err(WorkloadNetworkPlanCompileError::MissingNodeAssignment)
    ));

    let decision = standalone_decision(
        TENANT,
        "python",
        "sandbox-a",
        SandboxBackendKind::Container,
        Some(GENERATION),
        Some("node-a"),
    );
    for (profile, resource_id, generation, expected) in [
        ("other", "sandbox-a", GENERATION, "name"),
        ("python", "sandbox-b", GENERATION, "id"),
        ("python", "sandbox-a", GENERATION + 1, "generation"),
    ] {
        let error = WorkloadNetworkPlanCompiler
            .compile(
                &decision,
                AdmittedWorkloadNetworkSource::Sandbox {
                    stable_resource_id: resource_id,
                    profile,
                    generation,
                    sandbox_spec: &spec,
                },
                Some(&selection),
                &registry,
                sovereignty(),
                &endpoint_semantics(&spec),
                WorkloadActivationIntent::PrepareOnly,
                WorkloadPublicationIntent::Withheld,
            )
            .expect_err("crossed source field must fail");
        assert!(
            match expected {
                "name" => matches!(
                    error,
                    WorkloadNetworkPlanCompileError::WorkloadNameMismatch { .. }
                ),
                "id" => matches!(
                    error,
                    WorkloadNetworkPlanCompileError::SandboxResourceIdMismatch { .. }
                ),
                "generation" => matches!(
                    error,
                    WorkloadNetworkPlanCompileError::GenerationMismatch { .. }
                ),
                _ => false,
            },
            "wrong typed error for {expected}: {error}"
        );
    }

    let service_owned = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::service("python"),
        SandboxBackendKind::Container,
        [],
    );
    assert!(matches!(
        WorkloadNetworkPlanCompiler.compile(
            &decision,
            AdmittedWorkloadNetworkSource::Sandbox {
                stable_resource_id: "sandbox-a",
                profile: "python",
                generation: GENERATION,
                sandbox_spec: &service_owned,
            },
            Some(&selection),
            &registry,
            sovereignty(),
            &endpoint_semantics(&service_owned),
            WorkloadActivationIntent::PrepareOnly,
            WorkloadPublicationIntent::Withheld,
        ),
        Err(WorkloadNetworkPlanCompileError::StandaloneSandboxOwnedByService { .. })
    ));

    let krun_spec = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::standalone(),
        SandboxBackendKind::Krun,
        [],
    );
    assert!(matches!(
        WorkloadNetworkPlanCompiler.compile(
            &decision,
            AdmittedWorkloadNetworkSource::Sandbox {
                stable_resource_id: "sandbox-a",
                profile: "python",
                generation: GENERATION,
                sandbox_spec: &krun_spec,
            },
            Some(&selection),
            &registry,
            sovereignty(),
            &endpoint_semantics(&krun_spec),
            WorkloadActivationIntent::PrepareOnly,
            WorkloadPublicationIntent::Withheld,
        ),
        Err(WorkloadNetworkPlanCompileError::SandboxBackendMismatch { .. })
    ));

    let empty_workload = admitted_decision(
        TENANT,
        WorkloadAttributes::new(WorkloadKind::SystemTask, "maintenance"),
        Some(GENERATION),
        Some("node-a"),
        [],
    );
    assert!(matches!(
        WorkloadNetworkPlanCompiler.compile(
            &empty_workload,
            AdmittedWorkloadNetworkSource::Sandbox {
                stable_resource_id: "sandbox-a",
                profile: "python",
                generation: GENERATION,
                sandbox_spec: &spec,
            },
            Some(&selection),
            &registry,
            sovereignty(),
            &endpoint_semantics(&spec),
            WorkloadActivationIntent::PrepareOnly,
            WorkloadPublicationIntent::Withheld,
        ),
        Err(WorkloadNetworkPlanCompileError::WorkloadKindMismatch { .. })
    ));
}

#[test]
fn crossed_workload_generation_rejects_before_submission() {
    let decision = standalone_decision(
        TENANT,
        "python",
        "sandbox-generation",
        SandboxBackendKind::Container,
        Some(GENERATION),
        Some("node-a"),
    );
    let spec = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::standalone_named("python"),
        SandboxBackendKind::Container,
        [],
    );
    let (registry, selection) = registry_for(SandboxBackendKind::Container, false);

    assert!(matches!(
        WorkloadNetworkPlanCompiler.compile(
            &decision,
            AdmittedWorkloadNetworkSource::Sandbox {
                stable_resource_id: "sandbox-generation",
                profile: "python",
                generation: GENERATION + 1,
                sandbox_spec: &spec,
            },
            Some(&selection),
            &registry,
            sovereignty(),
            &[],
            WorkloadActivationIntent::PrepareOnly,
            WorkloadPublicationIntent::Withheld,
        ),
        Err(WorkloadNetworkPlanCompileError::GenerationMismatch { .. })
    ));
}

#[test]
fn crossed_address_semantics_rejects_before_submission() {
    let decision = standalone_decision(
        TENANT,
        "python",
        "sandbox-address",
        SandboxBackendKind::Container,
        Some(GENERATION),
        Some("node-a"),
    );
    let spec = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::standalone_named("python"),
        SandboxBackendKind::Container,
        [
            SandboxPortBinding::new("public", EndpointProtocol::Tcp, 18080, 8080)
                .with_host_address(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 10))),
        ],
    );
    let (registry, selection) =
        registry_for_capabilities(SandboxBackendKind::Container, false, true, false);

    assert!(matches!(
        compile_standalone(&decision, &spec, &selection, &registry),
        Err(WorkloadNetworkPlanCompileError::CapabilitySelection(_))
    ));
}

#[test]
fn crossed_provider_selection_rejects_before_submission() {
    let decision = standalone_decision(
        TENANT,
        "python",
        "sandbox-a",
        SandboxBackendKind::Container,
        Some(GENERATION),
        Some("node-a"),
    );
    let https_spec = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::standalone(),
        SandboxBackendKind::Container,
        [SandboxPortBinding::new(
            "https",
            EndpointProtocol::Https,
            18443,
            8443,
        )],
    );
    let (no_tls_registry, selection) = registry_for(SandboxBackendKind::Container, false);
    let unsatisfied = compile_standalone(&decision, &https_spec, &selection, &no_tls_registry)
        .expect_err("TLS need must fail against a non-TLS registration");
    assert!(matches!(
        unsatisfied,
        WorkloadNetworkPlanCompileError::CapabilitySelection(_)
    ));

    let tcp_spec = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::standalone(),
        SandboxBackendKind::Container,
        [SandboxPortBinding::new(
            "postgres",
            EndpointProtocol::Tcp,
            15432,
            5432,
        )],
    );
    let (no_forwarding_registry, no_forwarding_selection) =
        registry_for_capabilities(SandboxBackendKind::Container, false, false, true);
    assert!(matches!(
        compile_standalone(
            &decision,
            &tcp_spec,
            &no_forwarding_selection,
            &no_forwarding_registry,
        ),
        Err(WorkloadNetworkPlanCompileError::CapabilitySelection(_))
    ));

    let public_spec = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::standalone(),
        SandboxBackendKind::Container,
        [
            SandboxPortBinding::new("public", EndpointProtocol::Tcp, 18080, 8080)
                .with_host_address(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 10))),
        ],
    );
    let (private_registry, private_selection) =
        registry_for_capabilities(SandboxBackendKind::Container, false, true, false);
    assert!(matches!(
        compile_standalone(
            &decision,
            &public_spec,
            &private_selection,
            &private_registry,
        ),
        Err(WorkloadNetworkPlanCompileError::CapabilitySelection(_))
    ));

    let (registry, _) = registry_for(SandboxBackendKind::Container, true);
    let provider_managed = NetworkCapabilitySelection::new(
        NetworkProviderId::for_registration_key("machine.provider-managed"),
        selection.ingress_provider_id().clone(),
    );
    let substituted = WorkloadNetworkPlanCompiler
        .compile(
            &decision,
            AdmittedWorkloadNetworkSource::Sandbox {
                stable_resource_id: "sandbox-a",
                profile: "python",
                generation: GENERATION,
                sandbox_spec: &https_spec,
            },
            Some(&provider_managed),
            &registry,
            sovereignty(),
            &endpoint_semantics(&https_spec),
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::PublishWhenReady,
        )
        .expect_err("host-managed source must reject a substituted attachment provider");
    assert!(matches!(
        substituted,
        WorkloadNetworkPlanCompileError::AttachmentProviderMismatch { .. }
    ));
}

#[test]
fn every_compile_failure_precedes_store_lease_provider_manager_and_sandbox_effects() {
    #[derive(Debug, Default, PartialEq, Eq)]
    struct RecordingUpperBoundaryCounters {
        store: u64,
        lease: u64,
        provider: u64,
        network_manager_mutation: u64,
        sandbox_start: u64,
    }

    impl RecordingUpperBoundaryCounters {
        fn record_post_compile_effects(&mut self) {
            self.store += 1;
            self.lease += 1;
            self.provider += 1;
            self.network_manager_mutation += 1;
            self.sandbox_start += 1;
        }
    }

    fn cross_compile_boundary(
        result: Result<CompiledWorkloadNetworkPlan, WorkloadNetworkPlanCompileError>,
        counters: &mut RecordingUpperBoundaryCounters,
    ) -> Result<CompiledWorkloadNetworkPlan, WorkloadNetworkPlanCompileError> {
        let compiled = result?;
        counters.record_post_compile_effects();
        Ok(compiled)
    }

    let decision = standalone_decision(
        TENANT,
        "python",
        "sandbox-a",
        SandboxBackendKind::Container,
        Some(GENERATION),
        Some("node-a"),
    );
    let spec = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::standalone(),
        SandboxBackendKind::Container,
        [],
    );
    let (registry, selection) = registry_for(SandboxBackendKind::Container, false);

    let mut success_counters = RecordingUpperBoundaryCounters::default();
    cross_compile_boundary(
        compile_standalone(&decision, &spec, &selection, &registry),
        &mut success_counters,
    )
    .expect("the recording boundary must prove it can observe a successful compile");
    assert_eq!(
        success_counters,
        RecordingUpperBoundaryCounters {
            store: 1,
            lease: 1,
            provider: 1,
            network_manager_mutation: 1,
            sandbox_start: 1,
        }
    );

    let missing_selection = WorkloadNetworkPlanCompiler.compile(
        &decision,
        AdmittedWorkloadNetworkSource::Sandbox {
            stable_resource_id: "sandbox-a",
            profile: "python",
            generation: GENERATION,
            sandbox_spec: &spec,
        },
        None,
        &registry,
        sovereignty(),
        &endpoint_semantics(&spec),
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
    );
    let crossed_tenant_spec = sandbox_spec(
        "tenant-b",
        SandboxOwnerSpec::standalone(),
        SandboxBackendKind::Container,
        [],
    );
    let crossed_source = WorkloadNetworkPlanCompiler.compile(
        &decision,
        AdmittedWorkloadNetworkSource::Sandbox {
            stable_resource_id: "sandbox-a",
            profile: "python",
            generation: GENERATION,
            sandbox_spec: &crossed_tenant_spec,
        },
        Some(&selection),
        &registry,
        sovereignty(),
        &endpoint_semantics(&crossed_tenant_spec),
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
    );
    let duplicate_spec = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::standalone(),
        SandboxBackendKind::Container,
        [
            SandboxPortBinding::new("api", EndpointProtocol::Tcp, 18080, 8080),
            SandboxPortBinding::new("api", EndpointProtocol::Tcp, 18081, 8081),
        ],
    );
    let invalid_portable_content =
        compile_standalone(&decision, &duplicate_spec, &selection, &registry);
    let sovereignty_relaxation = WorkloadNetworkPlanCompiler.compile(
        &decision,
        AdmittedWorkloadNetworkSource::Sandbox {
            stable_resource_id: "sandbox-a",
            profile: "python",
            generation: GENERATION,
            sandbox_spec: &spec,
        },
        Some(&selection),
        &registry,
        NetworkSovereigntyRequirements::new(
            NetworkControlPlaneLocality::ThirdParty,
            [NetworkExternalDependency::ExternalControlPlane],
            false,
        ),
        &endpoint_semantics(&spec),
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
    );

    for error in [
        missing_selection,
        crossed_source,
        invalid_portable_content,
        sovereignty_relaxation,
    ] {
        let mut counters = RecordingUpperBoundaryCounters::default();
        cross_compile_boundary(error, &mut counters)
            .expect_err("every compile error must short-circuit the effect boundary");
        assert_eq!(counters, RecordingUpperBoundaryCounters::default());
    }
}

#[test]
fn crossed_sovereignty_rejects_before_submission() {
    let source = NetworkSovereigntyRequirements::new(
        NetworkControlPlaneLocality::OperatorLocal,
        [
            NetworkExternalDependency::Dns,
            NetworkExternalDependency::Relay,
        ],
        false,
    );
    let stricter = NetworkSovereigntyRequirements::new(
        NetworkControlPlaneLocality::LocalOnly,
        [NetworkExternalDependency::Dns],
        true,
    );
    require_sovereignty_refinement(&source, &stricter)
        .expect("narrower locality/dependencies and stronger offline restart must refine source");

    let strict_source = NetworkSovereigntyRequirements::new(
        NetworkControlPlaneLocality::LocalOnly,
        [NetworkExternalDependency::Dns],
        true,
    );
    let relaxed = NetworkSovereigntyRequirements::new(
        NetworkControlPlaneLocality::ThirdParty,
        [
            NetworkExternalDependency::Dns,
            NetworkExternalDependency::ExternalControlPlane,
        ],
        false,
    );
    assert!(matches!(
        require_sovereignty_refinement(&strict_source, &relaxed),
        Err(WorkloadNetworkPlanCompileError::SourceSovereigntyRelaxation { dimensions })
            if dimensions == [
                NetworkCapabilityDimension::ControlPlaneLocality,
                NetworkCapabilityDimension::ExternalDependency,
                NetworkCapabilityDimension::OfflineRestart,
            ]
    ));
}

#[test]
fn sovereignty_is_digest_bound_and_exact_selection_fails_closed() {
    let decision = standalone_decision(
        TENANT,
        "python",
        "sandbox-a",
        SandboxBackendKind::Container,
        Some(GENERATION),
        Some("node-a"),
    );
    let spec = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::standalone(),
        SandboxBackendKind::Container,
        [],
    );
    let (local_registry, local_selection) = registry_for(SandboxBackendKind::Container, false);
    let compile = |requirements| {
        WorkloadNetworkPlanCompiler.compile(
            &decision,
            AdmittedWorkloadNetworkSource::Sandbox {
                stable_resource_id: "sandbox-a",
                profile: "python",
                generation: GENERATION,
                sandbox_spec: &spec,
            },
            Some(&local_selection),
            &local_registry,
            requirements,
            &endpoint_semantics(&spec),
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::Withheld,
        )
    };
    let local = compile(sovereignty()).expect("local provider should satisfy local requirements");
    assert_eq!(
        local.plan().readiness_requirements().len(),
        2,
        "attachment-only sandbox still requires attachment and PEP readiness"
    );
    for (broadened, dimension) in [
        (
            NetworkSovereigntyRequirements::new(
                NetworkControlPlaneLocality::OperatorLocal,
                [],
                true,
            ),
            NetworkCapabilityDimension::ControlPlaneLocality,
        ),
        (
            NetworkSovereigntyRequirements::new(
                NetworkControlPlaneLocality::LocalOnly,
                [NetworkExternalDependency::ExternalControlPlane],
                true,
            ),
            NetworkCapabilityDimension::ExternalDependency,
        ),
        (
            NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], false),
            NetworkCapabilityDimension::OfflineRestart,
        ),
    ] {
        assert!(matches!(
            compile(broadened),
            Err(WorkloadNetworkPlanCompileError::SourceSovereigntyRelaxation {
                dimensions,
            }) if dimensions == [dimension]
        ));
    }

    let empty_decision = admitted_decision(
        TENANT,
        WorkloadAttributes::new(WorkloadKind::SystemTask, "maintenance"),
        Some(GENERATION),
        Some("node-a"),
        [],
    );
    let empty_registry =
        NetworkCapabilityRegistry::new([]).expect("empty capability registry should validate");
    let compile_empty = |requirements| {
        WorkloadNetworkPlanCompiler
            .compile(
                &empty_decision,
                AdmittedWorkloadNetworkSource::Empty,
                None,
                &empty_registry,
                requirements,
                &[],
                WorkloadActivationIntent::PrepareOnly,
                WorkloadPublicationIntent::Withheld,
            )
            .expect("source-free plan should retain its admitted sovereignty")
    };
    let empty_local = compile_empty(sovereignty());
    let operator_local = compile_empty(NetworkSovereigntyRequirements::new(
        NetworkControlPlaneLocality::OperatorLocal,
        [],
        true,
    ));
    let external = compile_empty(NetworkSovereigntyRequirements::new(
        NetworkControlPlaneLocality::LocalOnly,
        [NetworkExternalDependency::ExternalControlPlane],
        true,
    ));
    let online_only = compile_empty(NetworkSovereigntyRequirements::new(
        NetworkControlPlaneLocality::LocalOnly,
        [],
        false,
    ));
    for candidate in [&operator_local, &external, &online_only] {
        assert_eq!(
            candidate.plan().requirements().sovereignty(),
            candidate.content().sovereignty_requirements()
        );
        assert_ne!(
            empty_local.plan().content_digest(),
            candidate.plan().content_digest()
        );
        assert_ne!(empty_local.plan().digest(), candidate.plan().digest());
    }
    assert_ne!(operator_local.plan().digest(), external.plan().digest());
    assert_ne!(external.plan().digest(), online_only.plan().digest());
}

#[test]
fn reserved_pep_name_and_duplicate_listeners_fail_closed() {
    let decision = standalone_decision(
        TENANT,
        "python",
        "sandbox-a",
        SandboxBackendKind::Container,
        Some(GENERATION),
        Some("node-a"),
    );
    let (registry, selection) = registry_for(SandboxBackendKind::Container, false);
    let reserved = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::standalone(),
        SandboxBackendKind::Container,
        [SandboxPortBinding::new(
            EGRESS_PEP_LISTENER_NAME,
            EndpointProtocol::Tcp,
            18080,
            8080,
        )],
    );
    assert!(matches!(
        compile_standalone(&decision, &reserved, &selection, &registry),
        Err(WorkloadNetworkPlanCompileError::ReservedListenerName { .. })
    ));

    let duplicate = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::standalone(),
        SandboxBackendKind::Container,
        [
            SandboxPortBinding::new("http", EndpointProtocol::Http, 18080, 8080),
            SandboxPortBinding::new("http", EndpointProtocol::Http, 18081, 8081),
        ],
    );
    assert!(matches!(
        compile_standalone(&decision, &duplicate, &selection, &registry),
        Err(WorkloadNetworkPlanCompileError::DuplicateEndpointSemantics { .. })
    ));
}

#[test]
fn newer_generation_creates_a_new_workload_incarnation_and_exact_fence() {
    let first_decision = standalone_decision(
        TENANT,
        "python",
        "sandbox-a",
        SandboxBackendKind::Container,
        Some(GENERATION),
        Some("node-a"),
    );
    let next_decision = standalone_decision(
        TENANT,
        "python",
        "sandbox-a",
        SandboxBackendKind::Container,
        Some(GENERATION + 1),
        Some("node-a"),
    );
    let spec = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::standalone(),
        SandboxBackendKind::Container,
        [],
    );
    let (registry, selection) = registry_for(SandboxBackendKind::Container, false);
    let first = compile_standalone(&first_decision, &spec, &selection, &registry)
        .expect("first generation should compile");
    let next = WorkloadNetworkPlanCompiler
        .compile(
            &next_decision,
            AdmittedWorkloadNetworkSource::Sandbox {
                stable_resource_id: "sandbox-a",
                profile: "python",
                generation: GENERATION + 1,
                sandbox_spec: &spec,
            },
            Some(&selection),
            &registry,
            sovereignty(),
            &endpoint_semantics(&spec),
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::Withheld,
        )
        .expect("next generation should compile");

    assert_ne!(
        first.plan().plan_id(),
        next.plan().plan_id(),
        "the admitted workload subject is generation-scoped, so replacement generations must not inherit network authority"
    );
    assert_eq!(next.plan().generation().as_u64(), GENERATION + 1);
    assert!(matches!(
        first.plan().classify_update(first.plan()),
        Ok(NetworkPlanUpdate::Idempotent)
    ));
}

#[path = "tests/child_process.rs"]
mod child_process;
