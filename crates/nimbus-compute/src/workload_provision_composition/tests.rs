use nimbus_core::TenantId;
use nimbus_network::{
    NetworkAddressFamily, NetworkAttachmentProviderRegistration, NetworkBindRealmKind,
    NetworkCapabilityBundle, NetworkControlPlaneLocality, NetworkEndpointCapabilitySet,
    NetworkExposure, NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet, NetworkLifecycleFeature,
    NetworkPortAssignmentMode, NetworkProviderId, NetworkSovereigntyCapabilities,
    NetworkSovereigntyRequirements, PortProtocol,
};
use nimbus_sandbox::{
    SandboxBackendKind, SandboxOwnerSpec, SandboxProcessSpec, SandboxRootSpec, SandboxSpec,
    sandbox_network_plan_requirements,
};
use nimbus_tenant::{
    TenantIsolationContext, TenantIsolationDecision, TenantIsolationPolicyInput,
    TenantServiceGrantPolicyDecision, WorkloadAttributes, WorkloadLocation,
};

use super::*;

const GENERATION: u64 = 17;

fn decision(node: Option<&str>) -> TenantIsolationDecision {
    let mut context = TenantIsolationContext::system(
        TenantId::new("tenant-composition").expect("tenant should validate"),
        "workload-provision-composition-test",
    )
    .with_deployment_generation(GENERATION);
    if let Some(node) = node {
        context = context.with_workload_location(WorkloadLocation::new().with_node_id(node));
    }
    context
        .admit_decision(
            TenantIsolationPolicyInput::new(
                WorkloadAttributes::sandbox("python")
                    .with_sandbox_id("sandbox-a")
                    .with_sandbox_backend(SandboxBackendKind::Krun),
            )
            .with_services(TenantServiceGrantPolicyDecision::new(std::iter::empty::<
                String,
            >())),
        )
        .expect("fixture decision should admit")
}

fn sandbox_spec() -> SandboxSpec {
    SandboxSpec::new(
        TenantId::new("tenant-composition").expect("tenant should validate"),
        SandboxOwnerSpec::standalone_named("python"),
        SandboxBackendKind::Krun,
        SandboxRootSpec::rootfs("/fixture/rootfs"),
        SandboxProcessSpec::new(["/bin/true"]),
    )
}

fn lifecycle() -> NetworkLifecycleCapabilitySet {
    NetworkLifecycleCapabilitySet::new([
        NetworkLifecycleFeature::DurableInspect,
        NetworkLifecycleFeature::Reconcile,
        NetworkLifecycleFeature::Delete,
    ])
}

fn registry() -> (NetworkCapabilityRegistry, NetworkCapabilitySelection) {
    let source = sandbox_network_plan_requirements(SandboxBackendKind::Krun);
    let ingress_provider = NetworkProviderId::for_registration_key("composition-ingress");
    let attachment = NetworkAttachmentProviderRegistration::new(
        source.required_attachment_provider_id().clone(),
        source.capability_requirements().attachment().clone(),
        [NetworkAddressFamily::Ipv4, NetworkAddressFamily::Ipv6],
        lifecycle(),
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let ingress = NetworkIngressProviderRegistration::new(
        ingress_provider.clone(),
        NetworkEndpointCapabilitySet::new(
            [NetworkAddressFamily::Ipv4, NetworkAddressFamily::Ipv6],
            [NetworkBindRealmKind::Host],
            [NetworkExposure::Loopback, NetworkExposure::Private],
            [PortProtocol::Tcp],
            [
                NetworkPortAssignmentMode::Exact,
                NetworkPortAssignmentMode::ProviderAssigned,
            ],
        ),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        lifecycle(),
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let selection = NetworkCapabilitySelection::new(
        source.required_attachment_provider_id().clone(),
        ingress_provider,
    );
    (
        NetworkCapabilityRegistry::new([NetworkCapabilityBundle::new(attachment, ingress)])
            .expect("registry should validate"),
        selection,
    )
}

fn version() -> WorkloadProvisionSourceResourceVersion {
    WorkloadProvisionSourceResourceVersion::new("source-etag-4")
        .expect("source version should validate")
}

fn execution_provider() -> WorkloadExecutionProviderId {
    WorkloadExecutionProviderId::for_registration_key("composition-execution")
}

#[test]
fn pure_composition_binds_node_source_and_selected_reports() {
    let decision = decision(Some("node-a"));
    let spec = sandbox_spec();
    let (registry, selection) = registry();
    let local_node = NodeIdentity::new("node-a").expect("node should validate");
    let source_version = version();

    let composed = compose_workload_provision(WorkloadProvisionCompositionInput {
        decision: &decision,
        local_node: &local_node,
        execution_provider_id: &execution_provider(),
        source: WorkloadProvisionSourceSnapshot::StandaloneSandbox {
            stable_resource_id: "sandbox-a",
            profile: "python",
            source_generation: WorkloadProvisionSourceGeneration::new(91),
            resource_version: &source_version,
            sandbox_spec: &spec,
        },
        capability_selection: &selection,
        capability_registry: &registry,
        sovereignty: NetworkSovereigntyRequirements::new(
            NetworkControlPlaneLocality::LocalOnly,
            [],
            true,
        ),
        endpoint_semantics: &[],
        activation: WorkloadActivationIntent::ActivateWhenAttached,
        publication: WorkloadPublicationIntent::Withheld,
    })
    .expect("exact composition should succeed");

    assert_eq!(composed.key().tenant_id(), decision.tenant_id());
    assert_eq!(composed.key().workload_id().as_str(), "sandbox-a");
    assert_eq!(composed.intent().admission().assigned_node(), &local_node);
    assert_eq!(
        composed.intent().source().source_generation().as_u64(),
        91,
        "source generation remains independent of deployment generation"
    );
    assert_eq!(composed.intent().generation().as_u64(), GENERATION);
    assert_eq!(
        composed
            .intent()
            .network()
            .compiled_plan()
            .content()
            .capability_selection_evidence()
            .expect("connected plan requires source-report evidence")
            .selection(),
        &selection
    );
}

#[test]
fn crossed_local_node_rejects_before_submission() {
    let spec = sandbox_spec();
    let (registry, selection) = registry();
    let source_version = version();
    let admitted = decision(Some("node-a"));
    let crossed = NodeIdentity::new("node-b").expect("node should validate");
    assert!(matches!(
        compose_workload_provision(WorkloadProvisionCompositionInput {
            decision: &admitted,
            local_node: &crossed,
            execution_provider_id: &execution_provider(),
            source: WorkloadProvisionSourceSnapshot::StandaloneSandbox {
                stable_resource_id: "sandbox-a",
                profile: "python",
                source_generation: WorkloadProvisionSourceGeneration::new(1),
                resource_version: &source_version,
                sandbox_spec: &spec,
            },
            capability_selection: &selection,
            capability_registry: &registry,
            sovereignty: NetworkSovereigntyRequirements::new(
                NetworkControlPlaneLocality::LocalOnly,
                [],
                true,
            ),
            endpoint_semantics: &[],
            activation: WorkloadActivationIntent::PrepareOnly,
            publication: WorkloadPublicationIntent::Withheld,
        }),
        Err(WorkloadProvisionCompositionError::NodeMismatch { .. })
    ));

    let absent = decision(None);
    let local = NodeIdentity::new("node-a").expect("node should validate");
    assert!(matches!(
        compose_workload_provision(WorkloadProvisionCompositionInput {
            decision: &absent,
            local_node: &local,
            execution_provider_id: &execution_provider(),
            source: WorkloadProvisionSourceSnapshot::StandaloneSandbox {
                stable_resource_id: "sandbox-a",
                profile: "python",
                source_generation: WorkloadProvisionSourceGeneration::new(1),
                resource_version: &source_version,
                sandbox_spec: &spec,
            },
            capability_selection: &selection,
            capability_registry: &registry,
            sovereignty: NetworkSovereigntyRequirements::new(
                NetworkControlPlaneLocality::LocalOnly,
                [],
                true,
            ),
            endpoint_semantics: &[],
            activation: WorkloadActivationIntent::PrepareOnly,
            publication: WorkloadPublicationIntent::Withheld,
        }),
        Err(WorkloadProvisionCompositionError::MissingNodeAssignment)
    ));
}

#[test]
fn crossed_source_snapshot_rejects_before_submission() {
    let decision = decision(Some("node-a"));
    let spec = sandbox_spec();
    let (registry, selection) = registry();
    let local_node = NodeIdentity::new("node-a").expect("node should validate");
    let source_version = version();

    assert!(matches!(
        compose_workload_provision(WorkloadProvisionCompositionInput {
            decision: &decision,
            local_node: &local_node,
            execution_provider_id: &execution_provider(),
            source: WorkloadProvisionSourceSnapshot::StandaloneSandbox {
                stable_resource_id: "sandbox-crossed",
                profile: "python",
                source_generation: WorkloadProvisionSourceGeneration::new(1),
                resource_version: &source_version,
                sandbox_spec: &spec,
            },
            capability_selection: &selection,
            capability_registry: &registry,
            sovereignty: NetworkSovereigntyRequirements::new(
                NetworkControlPlaneLocality::LocalOnly,
                [],
                true,
            ),
            endpoint_semantics: &[],
            activation: WorkloadActivationIntent::PrepareOnly,
            publication: WorkloadPublicationIntent::Withheld,
        }),
        Err(WorkloadProvisionCompositionError::Network(
            WorkloadNetworkPlanCompileError::SandboxResourceIdMismatch { .. }
        ))
    ));
}

#[test]
fn crossed_publication_rejects_before_submission() {
    let decision = decision(Some("node-a"));
    let spec = sandbox_spec();
    let (registry, selection) = registry();
    let local_node = NodeIdentity::new("node-a").expect("node should validate");
    let source_version = version();

    assert!(matches!(
        compose_workload_provision(WorkloadProvisionCompositionInput {
            decision: &decision,
            local_node: &local_node,
            execution_provider_id: &execution_provider(),
            source: WorkloadProvisionSourceSnapshot::StandaloneSandbox {
                stable_resource_id: "sandbox-a",
                profile: "python",
                source_generation: WorkloadProvisionSourceGeneration::new(1),
                resource_version: &source_version,
                sandbox_spec: &spec,
            },
            capability_selection: &selection,
            capability_registry: &registry,
            sovereignty: NetworkSovereigntyRequirements::new(
                NetworkControlPlaneLocality::LocalOnly,
                [],
                true,
            ),
            endpoint_semantics: &[],
            activation: WorkloadActivationIntent::ActivateWhenAttached,
            publication: WorkloadPublicationIntent::PublishWhenReady,
        }),
        Err(WorkloadProvisionCompositionError::Network(
            WorkloadNetworkPlanCompileError::PublicationRequiresListener
        ))
    ));
}

#[test]
fn source_generation_changes_source_and_desired_digests_without_changing_deployment_generation() {
    let decision = decision(Some("node-a"));
    let spec = sandbox_spec();
    let (registry, selection) = registry();
    let local_node = NodeIdentity::new("node-a").expect("node should validate");
    let source_version = version();
    let compose = |source_generation| {
        compose_workload_provision(WorkloadProvisionCompositionInput {
            decision: &decision,
            local_node: &local_node,
            execution_provider_id: &execution_provider(),
            source: WorkloadProvisionSourceSnapshot::StandaloneSandbox {
                stable_resource_id: "sandbox-a",
                profile: "python",
                source_generation: WorkloadProvisionSourceGeneration::new(source_generation),
                resource_version: &source_version,
                sandbox_spec: &spec,
            },
            capability_selection: &selection,
            capability_registry: &registry,
            sovereignty: NetworkSovereigntyRequirements::new(
                NetworkControlPlaneLocality::LocalOnly,
                [],
                true,
            ),
            endpoint_semantics: &[],
            activation: WorkloadActivationIntent::PrepareOnly,
            publication: WorkloadPublicationIntent::Withheld,
        })
        .expect("composition should validate")
    };
    let first = compose(1);
    let second = compose(2);

    assert_eq!(first.intent().generation(), second.intent().generation());
    assert_ne!(
        first.intent().source().source_digest(),
        second.intent().source().source_digest()
    );
    assert_ne!(
        first.intent().desired_digest(),
        second.intent().desired_digest()
    );
}
