use std::net::Ipv4Addr;
use std::num::NonZeroU16;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use nimbus_core::TenantId;
use nimbus_network::{
    NetworkAddressFamily, NetworkAttachmentProviderRegistration, NetworkBindRealmKind,
    NetworkCapabilityBundle, NetworkCapabilityRegistry, NetworkCapabilitySelection,
    NetworkControlPlaneLocality, NetworkEndpointCapabilitySet, NetworkExposure,
    NetworkForwardingCapabilitySet, NetworkForwardingFeature, NetworkIngressCapabilitySet,
    NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet, NetworkLifecycleFeature,
    NetworkPortAssignmentMode, NetworkProviderId, NetworkSovereigntyCapabilities,
    NetworkSovereigntyRequirements, NetworkTlsBehavior, PortBindTarget,
};
use nimbus_sandbox::{
    SandboxBackendKind, SandboxExecutionAttemptId, SandboxOwnerSpec, SandboxPortBinding,
    SandboxProcessSpec, SandboxRootSpec, SandboxSpec, sandbox_network_plan_requirements,
};
use nimbus_tenant::{
    TenantIsolationContext, TenantIsolationPolicyInput, TenantServiceGrantPolicyDecision,
    WorkloadAttributes, WorkloadLocation,
};
use nimbus_workloads::{
    NodeIdentity, WorkloadActivationIntent, WorkloadExecutionProviderId,
    WorkloadNetworkForwardingBehavior, WorkloadProvisionInspectionResult,
    WorkloadProvisionSourceGeneration, WorkloadProvisionSourceResourceVersion,
    WorkloadPublicationIntent, WorkloadSagaRecord,
};

use super::*;
use crate::workload_network_plan::WorkloadNetworkEndpointSemanticsInput;
use crate::workload_provision_composition::{
    WorkloadProvisionCompositionInput, WorkloadProvisionSourceSnapshot, compose_workload_provision,
};
use crate::workload_saga::{
    ConfirmedWorkloadProvisionCommand, IngressProvisionCapabilities, IngressPublicationCapability,
    IngressPublicationInspectionCapability, NetworkAttachmentCapability,
    NetworkAttachmentProvisionCapabilities, NetworkReservationCapability,
    WorkloadActivationCapability, WorkloadActivationPrerequisiteCapability,
    WorkloadExecutionProvisionCapabilities, WorkloadPreparationCapability,
    WorkloadProvisionCapabilityFuture, WorkloadProvisionCapabilityRegistryError,
    WorkloadReadinessCapability,
};

const TENANT: &str = "tenant-projection";
const WORKLOAD: &str = "sandbox-projection";
const PROFILE: &str = "projection";
const GENERATION: u64 = 17;

struct RecordingProvider {
    execution: Mutex<WorkloadProviderObservation<SandboxInspection>>,
    ingress: Mutex<WorkloadProviderObservation<Vec<WorkloadObservedIngressEndpoint>>>,
    execution_calls: AtomicUsize,
    ingress_calls: AtomicUsize,
    effect_calls: AtomicUsize,
}

impl RecordingProvider {
    fn new(
        execution: WorkloadProviderObservation<SandboxInspection>,
        ingress: WorkloadProviderObservation<Vec<WorkloadObservedIngressEndpoint>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            execution: Mutex::new(execution),
            ingress: Mutex::new(ingress),
            execution_calls: AtomicUsize::new(0),
            ingress_calls: AtomicUsize::new(0),
            effect_calls: AtomicUsize::new(0),
        })
    }

    fn set_execution(&self, observation: WorkloadProviderObservation<SandboxInspection>) {
        *self
            .execution
            .lock()
            .expect("execution observation lock should remain healthy") = observation;
    }

    fn set_ingress(
        &self,
        observation: WorkloadProviderObservation<Vec<WorkloadObservedIngressEndpoint>>,
    ) {
        *self
            .ingress
            .lock()
            .expect("ingress observation lock should remain healthy") = observation;
    }

    fn forbidden_effect(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionInspectionResult {
        self.effect_calls.fetch_add(1, Ordering::AcqRel);
        WorkloadProvisionInspectionResult::Ambiguous {
            attempt_id: command.attempt_id().clone(),
            dispatch_epoch: command.dispatch_epoch(),
            provider_target: command.provider_target().clone(),
        }
    }
}

macro_rules! effect_capability {
    ($trait_name:ident) => {
        impl $trait_name for RecordingProvider {
            fn execute<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.forbidden_effect(command) })
            }

            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.forbidden_effect(command) })
            }
        }
    };
}

macro_rules! inspection_capability {
    ($trait_name:ident) => {
        impl $trait_name for RecordingProvider {
            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.forbidden_effect(command) })
            }
        }
    };
}

effect_capability!(NetworkReservationCapability);
effect_capability!(WorkloadPreparationCapability);
effect_capability!(NetworkAttachmentCapability);
inspection_capability!(WorkloadActivationPrerequisiteCapability);
effect_capability!(WorkloadActivationCapability);
inspection_capability!(WorkloadReadinessCapability);
effect_capability!(IngressPublicationCapability);
inspection_capability!(IngressPublicationInspectionCapability);

impl WorkloadExecutionObservationCapability for RecordingProvider {
    fn observe<'a>(
        &'a self,
        _request: &'a WorkloadExecutionObservationRequest,
    ) -> WorkloadExecutionObservationFuture<'a> {
        Box::pin(async move {
            self.execution_calls.fetch_add(1, Ordering::AcqRel);
            self.execution
                .lock()
                .expect("execution observation lock should remain healthy")
                .clone()
        })
    }
}

impl WorkloadIngressObservationCapability for RecordingProvider {
    fn observe<'a>(
        &'a self,
        _request: &'a WorkloadIngressObservationRequest,
    ) -> WorkloadIngressObservationFuture<'a> {
        Box::pin(async move {
            self.ingress_calls.fetch_add(1, Ordering::AcqRel);
            self.ingress
                .lock()
                .expect("ingress observation lock should remain healthy")
                .clone()
        })
    }
}

struct RecordingSink {
    projections: Mutex<Vec<WorkloadObservedProjection>>,
    outcome: Mutex<Result<(), WorkloadProjectionSinkError>>,
    calls: AtomicUsize,
}

impl Default for RecordingSink {
    fn default() -> Self {
        Self {
            projections: Mutex::new(Vec::new()),
            outcome: Mutex::new(Ok(())),
            calls: AtomicUsize::new(0),
        }
    }
}

impl RecordingSink {
    fn projections(&self) -> Vec<WorkloadObservedProjection> {
        self.projections
            .lock()
            .expect("projection sink lock should remain healthy")
            .clone()
    }

    fn set_outcome(&self, outcome: Result<(), WorkloadProjectionSinkError>) {
        *self
            .outcome
            .lock()
            .expect("projection outcome lock should remain healthy") = outcome;
    }
}

impl WorkloadProjectionSink for RecordingSink {
    fn project<'a>(
        &'a self,
        projection: &'a WorkloadObservedProjection,
    ) -> WorkloadProjectionSinkFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::AcqRel);
            let outcome = self
                .outcome
                .lock()
                .expect("projection outcome lock should remain healthy")
                .clone();
            if outcome.is_ok() {
                self.projections
                    .lock()
                    .expect("projection sink lock should remain healthy")
                    .push(projection.clone());
            }
            outcome
        })
    }
}

struct Fixture {
    initial: WorkloadSagaRecord,
    observed: WorkloadSagaRecord,
    execution_provider: WorkloadExecutionProviderId,
    ingress_provider: NetworkProviderId,
}

fn fixture(publication: WorkloadPublicationIntent) -> Fixture {
    let endpoints = (publication == WorkloadPublicationIntent::PublishWhenReady)
        .then_some(vec![("api", 8_080)])
        .unwrap_or_default();
    fixture_with_endpoints(publication, &endpoints)
}

fn fixture_with_endpoints(
    publication: WorkloadPublicationIntent,
    endpoints: &[(&str, u16)],
) -> Fixture {
    let tenant = TenantId::new(TENANT).expect("fixture tenant should validate");
    let mut spec = SandboxSpec::new(
        tenant.clone(),
        SandboxOwnerSpec::standalone_named(PROFILE),
        SandboxBackendKind::Krun,
        SandboxRootSpec::rootfs("/fixture/rootfs"),
        SandboxProcessSpec::new(["/bin/true"]),
    );
    for (name, guest_port) in endpoints {
        spec = spec.with_port_binding(SandboxPortBinding::tcp(*name, 0, *guest_port));
    }
    let context = TenantIsolationContext::system(tenant.clone(), "workload-projection-test")
        .with_deployment_generation(GENERATION)
        .with_workload_location(WorkloadLocation::new().with_node_id("node-projection"));
    let decision = context
        .admit_decision(
            TenantIsolationPolicyInput::new(
                WorkloadAttributes::sandbox(PROFILE)
                    .with_sandbox_id(WORKLOAD)
                    .with_sandbox_backend(SandboxBackendKind::Krun),
            )
            .with_services(TenantServiceGrantPolicyDecision::new(std::iter::empty::<
                String,
            >())),
        )
        .expect("fixture decision should admit");
    let requirements = sandbox_network_plan_requirements(SandboxBackendKind::Krun);
    let ingress_provider = NetworkProviderId::for_registration_key("projection-ingress");
    let lifecycle = NetworkLifecycleCapabilitySet::new([
        NetworkLifecycleFeature::DurableInspect,
        NetworkLifecycleFeature::Reconcile,
        NetworkLifecycleFeature::Delete,
    ]);
    let attachment = NetworkAttachmentProviderRegistration::new(
        requirements.required_attachment_provider_id().clone(),
        requirements.capability_requirements().attachment().clone(),
        [NetworkAddressFamily::Ipv4],
        lifecycle.clone(),
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let ingress = NetworkIngressProviderRegistration::new(
        ingress_provider.clone(),
        NetworkEndpointCapabilitySet::new(
            [NetworkAddressFamily::Ipv4],
            [NetworkBindRealmKind::Host],
            [NetworkExposure::Loopback],
            [PortProtocol::Tcp],
            [
                NetworkPortAssignmentMode::Exact,
                NetworkPortAssignmentMode::ProviderAssigned,
            ],
        ),
        NetworkIngressCapabilitySet::new([]).with_tls_behaviors([NetworkTlsBehavior::Disabled]),
        NetworkForwardingCapabilitySet::new([NetworkForwardingFeature::PortForwarding]),
        lifecycle,
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let registry =
        NetworkCapabilityRegistry::new([NetworkCapabilityBundle::new(attachment, ingress)])
            .expect("fixture provider realm should validate");
    let selection = NetworkCapabilitySelection::new(
        requirements.required_attachment_provider_id().clone(),
        ingress_provider.clone(),
    );
    let execution_provider =
        WorkloadExecutionProviderId::for_registration_key("projection-execution");
    let source_version = WorkloadProvisionSourceResourceVersion::new("projection-source-v1")
        .expect("source version should validate");
    let endpoint_semantics = endpoints
        .iter()
        .map(|(name, _)| {
            WorkloadNetworkEndpointSemanticsInput::new(
                name,
                WorkloadNetworkForwardingBehavior::PortForwarded,
                NetworkTlsBehavior::Disabled,
            )
        })
        .collect::<Vec<_>>();
    let composed = compose_workload_provision(WorkloadProvisionCompositionInput {
        decision: &decision,
        local_node: &NodeIdentity::new("node-projection").expect("node should validate"),
        source: WorkloadProvisionSourceSnapshot::StandaloneSandbox {
            stable_resource_id: WORKLOAD,
            profile: PROFILE,
            source_generation: WorkloadProvisionSourceGeneration::new(5),
            resource_version: &source_version,
            sandbox_spec: &spec,
        },
        execution_provider_id: &execution_provider,
        capability_selection: &selection,
        capability_registry: &registry,
        sovereignty: NetworkSovereigntyRequirements::new(
            NetworkControlPlaneLocality::LocalOnly,
            [],
            true,
        ),
        endpoint_semantics: &endpoint_semantics,
        activation: WorkloadActivationIntent::ActivateWhenAttached,
        publication,
    })
    .expect("fixture provision should compose");
    let (key, intent) = composed.into_parts();
    let initial = WorkloadSagaRecord::new(key, intent).expect("fixture record should validate");
    let mut observed = initial.clone();
    for _ in 0..10 {
        if observed.phase() == WorkloadSagaPhase::Observed {
            break;
        }
        observed = crate::workload_saga::test_support::confirmed_provision(&observed);
    }
    assert_eq!(observed.phase(), WorkloadSagaPhase::Observed);
    Fixture {
        initial,
        observed,
        execution_provider,
        ingress_provider,
    }
}

fn exact_inspection(record: &WorkloadSagaRecord) -> SandboxInspection {
    let intent = record.active_intent();
    let execution = record.current_execution_reference();
    let spec = decode_sandbox_spec(intent.executable()).expect("fixture executable should decode");
    SandboxInspection::provider_authenticated_running(
        SandboxHandle::new(
            record.key().tenant_id().clone(),
            SandboxId::new(execution.execution_id().as_str()),
            spec.display_name(),
            spec.backend,
            SandboxStatus::Ready,
            Vec::new(),
        ),
        SandboxExecutionAttemptId::new(execution.attempt_id().to_string())
            .expect("fixture attempt ID should be valid"),
        b"workload-projection-fixture",
    )
}

fn lifetime(generation: u64) -> PortLeaseLifetime {
    serde_json::from_value(serde_json::json!({
        "generation": generation,
        "effect_scope": "process_bound"
    }))
    .expect("fixture lifetime should validate")
}

fn exact_ingress(record: &WorkloadSagaRecord) -> WorkloadObservedIngressEndpoint {
    exact_ingress_named(record, "api", 49_152)
}

fn exact_ingress_named(
    record: &WorkloadSagaRecord,
    endpoint_name: &str,
    published_port: u16,
) -> WorkloadObservedIngressEndpoint {
    let plan = record.active_intent().network().compiled_plan();
    let listener = plan
        .content()
        .listeners()
        .iter()
        .find(|listener| listener.name() == endpoint_name)
        .expect("published fixture should retain the named listener");
    let active_lifetime = lifetime(3);
    WorkloadObservedIngressEndpoint::new(
        listener.endpoint_id().clone(),
        SocketAddr::from((Ipv4Addr::LOCALHOST, published_port)),
        WorkloadIngressBindingWitness::new(
            plan.plan().plan_id().clone(),
            plan.plan().digest(),
            plan.content().identity().generation(),
            listener.listener_id().clone(),
            listener.port_lease_id().clone(),
            active_lifetime,
            active_lifetime,
            PortBoundEndpoint::new(
                PortProtocol::Tcp,
                PortBindRealm::Host,
                PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
                NonZeroU16::new(published_port).expect("bound port should be nonzero"),
            )
            .expect("bound endpoint should validate"),
            PortBindingProvenance::ProviderAssigned,
        ),
    )
}

fn capabilities(
    fixture: &Fixture,
    provider: Arc<RecordingProvider>,
) -> Arc<WorkloadProvisionCapabilityRegistry> {
    let attachment_provider = fixture
        .observed
        .active_intent()
        .source()
        .attachment_provider_id()
        .clone();
    Arc::new(
        WorkloadProvisionCapabilityRegistry::new(
            [NetworkAttachmentProvisionCapabilities::new(
                attachment_provider,
                provider.clone(),
            )],
            [WorkloadExecutionProvisionCapabilities::new(
                fixture.execution_provider.clone(),
                provider.clone(),
            )],
            [IngressProvisionCapabilities::new(
                fixture.ingress_provider.clone(),
                provider,
            )],
        )
        .expect("fixture capability registry should validate"),
    )
}

fn orchestrator(
    fixture: &Fixture,
    provider: Arc<RecordingProvider>,
    sink: Arc<RecordingSink>,
) -> WorkloadProjectionOrchestrator {
    WorkloadProjectionOrchestrator::new(capabilities(fixture, provider), sink)
}

#[tokio::test]
async fn registry_rejects_duplicate_or_missing_observers_without_fallback() {
    let fixture = fixture(WorkloadPublicationIntent::Withheld);
    let exact = RecordingProvider::new(
        WorkloadProviderObservation::Present(exact_inspection(&fixture.observed)),
        WorkloadProviderObservation::Ambiguous,
    );
    let duplicate = WorkloadProvisionCapabilityRegistry::new(
        [],
        [
            WorkloadExecutionProvisionCapabilities::new(
                fixture.execution_provider.clone(),
                exact.clone(),
            ),
            WorkloadExecutionProvisionCapabilities::new(
                fixture.execution_provider.clone(),
                exact.clone(),
            ),
        ],
        [],
    );
    assert!(matches!(
        duplicate,
        Err(WorkloadProvisionCapabilityRegistryError::DuplicateExecution { .. })
    ));

    let alternative = RecordingProvider::new(
        WorkloadProviderObservation::Present(exact_inspection(&fixture.observed)),
        WorkloadProviderObservation::Ambiguous,
    );
    let registry = Arc::new(
        WorkloadProvisionCapabilityRegistry::new(
            [],
            [WorkloadExecutionProvisionCapabilities::new(
                WorkloadExecutionProviderId::for_registration_key("alternative-execution"),
                alternative.clone(),
            )],
            [],
        )
        .expect("alternative-only registry should validate"),
    );
    let sink = Arc::new(RecordingSink::default());
    let state = WorkloadProjectionOrchestrator::new(registry, sink.clone())
        .project_record(&fixture.observed, WorkloadProvisionRunDisposition::Observed)
        .await;
    assert_eq!(
        state,
        WorkloadProjectionState::Rejected(
            WorkloadProjectionRejectedReason::MissingExecutionObservationCapability
        )
    );
    assert_eq!(alternative.execution_calls.load(Ordering::Acquire), 0);
    assert!(sink.projections().is_empty());
}

#[tokio::test]
async fn missing_exact_ingress_observer_never_falls_back_to_another_provider() {
    let fixture = fixture(WorkloadPublicationIntent::PublishWhenReady);
    let exact_execution = RecordingProvider::new(
        WorkloadProviderObservation::Present(exact_inspection(&fixture.observed)),
        WorkloadProviderObservation::Ambiguous,
    );
    let alternative_ingress = RecordingProvider::new(
        WorkloadProviderObservation::Ambiguous,
        WorkloadProviderObservation::Present(vec![exact_ingress(&fixture.observed)]),
    );
    let registry = Arc::new(
        WorkloadProvisionCapabilityRegistry::new(
            [],
            [WorkloadExecutionProvisionCapabilities::new(
                fixture.execution_provider.clone(),
                exact_execution.clone(),
            )],
            [IngressProvisionCapabilities::new(
                NetworkProviderId::for_registration_key("alternative-ingress"),
                alternative_ingress.clone(),
            )],
        )
        .expect("alternative-only ingress registry should validate"),
    );
    let sink = Arc::new(RecordingSink::default());

    assert_eq!(
        WorkloadProjectionOrchestrator::new(registry, sink.clone())
            .project_record(&fixture.observed, WorkloadProvisionRunDisposition::Observed)
            .await,
        WorkloadProjectionState::Rejected(
            WorkloadProjectionRejectedReason::MissingIngressObservationCapability
        )
    );
    assert_eq!(exact_execution.execution_calls.load(Ordering::Acquire), 1);
    assert_eq!(alternative_ingress.ingress_calls.load(Ordering::Acquire), 0);
    assert_eq!(alternative_ingress.effect_calls.load(Ordering::Acquire), 0);
    assert_eq!(sink.calls.load(Ordering::Acquire), 0);
    assert!(sink.projections().is_empty());
}

#[tokio::test]
async fn non_observed_dispositions_invoke_zero_observation_or_sink_calls() {
    let fixture = fixture(WorkloadPublicationIntent::Withheld);
    let provider = RecordingProvider::new(
        WorkloadProviderObservation::Present(exact_inspection(&fixture.observed)),
        WorkloadProviderObservation::Ambiguous,
    );
    let sink = Arc::new(RecordingSink::default());
    let orchestrator = orchestrator(&fixture, provider.clone(), sink.clone());

    assert_eq!(
        orchestrator
            .project_record(&fixture.initial, WorkloadProvisionRunDisposition::Waiting)
            .await,
        WorkloadProjectionState::Pending(WorkloadProjectionPendingReason::ProvisionWaiting)
    );
    assert_eq!(
        orchestrator
            .project_record(
                &fixture.initial,
                WorkloadProvisionRunDisposition::DefiniteFailure,
            )
            .await,
        WorkloadProjectionState::Rejected(
            WorkloadProjectionRejectedReason::ProvisionDefiniteFailure
        )
    );
    assert_eq!(provider.execution_calls.load(Ordering::Acquire), 0);
    assert_eq!(provider.ingress_calls.load(Ordering::Acquire), 0);
    assert_eq!(provider.effect_calls.load(Ordering::Acquire), 0);
    assert!(sink.projections().is_empty());
}

#[tokio::test]
async fn observed_withheld_reads_execution_once_and_projects_exact_identity() {
    let fixture = fixture(WorkloadPublicationIntent::Withheld);
    let provider = RecordingProvider::new(
        WorkloadProviderObservation::Present(exact_inspection(&fixture.observed)),
        WorkloadProviderObservation::Ambiguous,
    );
    let sink = Arc::new(RecordingSink::default());
    let state = orchestrator(&fixture, provider.clone(), sink.clone())
        .project_record(&fixture.observed, WorkloadProvisionRunDisposition::Observed)
        .await;

    assert_eq!(state, WorkloadProjectionState::Projected);
    assert_eq!(provider.execution_calls.load(Ordering::Acquire), 1);
    assert_eq!(provider.ingress_calls.load(Ordering::Acquire), 0);
    assert_eq!(provider.effect_calls.load(Ordering::Acquire), 0);
    let projections = sink.projections();
    assert_eq!(projections.len(), 1);
    assert_eq!(
        projections[0].handle().id.as_str(),
        fixture
            .observed
            .current_execution_reference()
            .execution_id()
            .as_str()
    );
    assert!(projections[0].handle().published_endpoints.is_empty());
}

#[tokio::test]
async fn crossed_execution_handle_is_rejected_before_ingress_or_sink_access() {
    let fixture = fixture(WorkloadPublicationIntent::PublishWhenReady);
    let mut crossed = exact_inspection(&fixture.observed);
    crossed.handle.id = SandboxId::new("crossed-execution");
    let provider = RecordingProvider::new(
        WorkloadProviderObservation::Present(crossed),
        WorkloadProviderObservation::Present(vec![exact_ingress(&fixture.observed)]),
    );
    let sink = Arc::new(RecordingSink::default());

    assert_eq!(
        orchestrator(&fixture, provider.clone(), sink.clone())
            .project_record(&fixture.observed, WorkloadProvisionRunDisposition::Observed)
            .await,
        WorkloadProjectionState::Rejected(
            WorkloadProjectionRejectedReason::InvalidExecutionEvidence
        )
    );
    assert_eq!(provider.execution_calls.load(Ordering::Acquire), 1);
    assert_eq!(provider.ingress_calls.load(Ordering::Acquire), 0);
    assert_eq!(provider.effect_calls.load(Ordering::Acquire), 0);
    assert_eq!(sink.calls.load(Ordering::Acquire), 0);
    assert!(sink.projections().is_empty());
}

#[tokio::test]
async fn crossed_execution_attempt_is_rejected_before_ingress_or_sink_access() {
    let fixture = fixture(WorkloadPublicationIntent::PublishWhenReady);
    let exact = exact_inspection(&fixture.observed);
    let crossed = SandboxInspection::provider_authenticated_running(
        exact.handle,
        SandboxExecutionAttemptId::new("wea_crossed").expect("crossed attempt ID should be valid"),
        b"crossed-attempt-fixture",
    );
    let provider = RecordingProvider::new(
        WorkloadProviderObservation::Present(crossed),
        WorkloadProviderObservation::Present(vec![exact_ingress(&fixture.observed)]),
    );
    let sink = Arc::new(RecordingSink::default());

    assert_eq!(
        orchestrator(&fixture, provider.clone(), sink.clone())
            .project_record(&fixture.observed, WorkloadProvisionRunDisposition::Observed)
            .await,
        WorkloadProjectionState::Rejected(
            WorkloadProjectionRejectedReason::InvalidExecutionEvidence
        )
    );
    assert_eq!(provider.execution_calls.load(Ordering::Acquire), 1);
    assert_eq!(provider.ingress_calls.load(Ordering::Acquire), 0);
    assert_eq!(provider.effect_calls.load(Ordering::Acquire), 0);
    assert_eq!(sink.calls.load(Ordering::Acquire), 0);
    assert!(sink.projections().is_empty());
}

#[tokio::test]
async fn sink_unavailability_is_pending_while_semantic_rejection_is_terminal() {
    let fixture = fixture(WorkloadPublicationIntent::Withheld);
    let provider = RecordingProvider::new(
        WorkloadProviderObservation::Present(exact_inspection(&fixture.observed)),
        WorkloadProviderObservation::Ambiguous,
    );
    let sink = Arc::new(RecordingSink::default());
    let projection_orchestrator = orchestrator(&fixture, provider.clone(), sink.clone());

    sink.set_outcome(Err(WorkloadProjectionSinkError::unavailable(
        "temporary services outage",
    )));
    assert_eq!(
        projection_orchestrator
            .project_record(&fixture.observed, WorkloadProvisionRunDisposition::Observed)
            .await,
        WorkloadProjectionState::Pending(
            WorkloadProjectionPendingReason::ProjectionSinkUnavailable
        )
    );
    sink.set_outcome(Err(WorkloadProjectionSinkError::rejected(
        "crossed source resource version",
    )));
    assert_eq!(
        projection_orchestrator
            .project_record(&fixture.observed, WorkloadProvisionRunDisposition::Observed)
            .await,
        WorkloadProjectionState::Rejected(WorkloadProjectionRejectedReason::ProjectionSinkRejected)
    );
    assert_eq!(sink.calls.load(Ordering::Acquire), 2);
    assert!(sink.projections().is_empty());
    assert_eq!(provider.effect_calls.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn publish_when_ready_requires_exact_provider_assigned_endpoint_evidence() {
    let fixture = fixture(WorkloadPublicationIntent::PublishWhenReady);
    let provider = RecordingProvider::new(
        WorkloadProviderObservation::Present(exact_inspection(&fixture.observed)),
        WorkloadProviderObservation::Present(vec![exact_ingress(&fixture.observed)]),
    );
    let sink = Arc::new(RecordingSink::default());
    let state = orchestrator(&fixture, provider.clone(), sink.clone())
        .project_record(&fixture.observed, WorkloadProvisionRunDisposition::Observed)
        .await;

    assert_eq!(state, WorkloadProjectionState::Projected);
    assert_eq!(provider.execution_calls.load(Ordering::Acquire), 1);
    assert_eq!(provider.ingress_calls.load(Ordering::Acquire), 1);
    assert_eq!(provider.effect_calls.load(Ordering::Acquire), 0);
    let projections = sink.projections();
    assert_eq!(projections.len(), 1);
    assert_eq!(projections[0].handle().published_endpoints.len(), 1);
    assert_eq!(
        projections[0].handle().published_endpoints[0].address,
        "127.0.0.1:49152"
            .parse::<SocketAddr>()
            .expect("expected endpoint should parse")
    );
    assert_ne!(
        projections[0].handle().published_endpoints[0]
            .address
            .port(),
        0
    );
    let endpoint_handle = &projections[0].published_endpoint_handles()[0];
    assert_eq!(
        endpoint_handle.endpoint_id(),
        fixture
            .observed
            .active_intent()
            .network()
            .compiled_plan()
            .content()
            .listeners()[0]
            .endpoint_id()
    );
    assert_eq!(
        endpoint_handle.generation(),
        NetworkResourceGeneration::new(GENERATION)
    );
    assert_eq!(
        endpoint_handle.endpoint(),
        &projections[0].handle().published_endpoints[0]
    );
}

#[tokio::test]
async fn ingress_observation_order_is_irrelevant_and_projection_is_canonical() {
    let fixture = fixture_with_endpoints(
        WorkloadPublicationIntent::PublishWhenReady,
        &[("api", 8_080), ("admin", 9_090)],
    );
    let provider = RecordingProvider::new(
        WorkloadProviderObservation::Present(exact_inspection(&fixture.observed)),
        WorkloadProviderObservation::Present(vec![
            exact_ingress_named(&fixture.observed, "api", 49_152),
            exact_ingress_named(&fixture.observed, "admin", 49_153),
        ]),
    );
    let sink = Arc::new(RecordingSink::default());

    assert_eq!(
        orchestrator(&fixture, provider.clone(), sink.clone())
            .project_record(&fixture.observed, WorkloadProvisionRunDisposition::Observed)
            .await,
        WorkloadProjectionState::Projected
    );
    let projections = sink.projections();
    let endpoints = &projections[0].handle().published_endpoints;
    let endpoint_handles = projections[0].published_endpoint_handles();
    assert_eq!(
        endpoints
            .iter()
            .map(|endpoint| endpoint.name.as_str())
            .collect::<Vec<_>>(),
        ["admin", "api"]
    );
    assert_eq!(endpoints[0].address.port(), 49_153);
    assert_eq!(endpoints[1].address.port(), 49_152);
    assert_eq!(
        endpoint_handles
            .iter()
            .map(|endpoint| endpoint.endpoint().name.as_str())
            .collect::<Vec<_>>(),
        ["admin", "api"]
    );
    assert_eq!(provider.effect_calls.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn closed_provider_states_and_withheld_endpoints_fail_before_sink_mutation() {
    let published = fixture(WorkloadPublicationIntent::PublishWhenReady);
    let provider = RecordingProvider::new(
        WorkloadProviderObservation::Absent,
        WorkloadProviderObservation::Present(vec![exact_ingress(&published.observed)]),
    );
    let sink = Arc::new(RecordingSink::default());
    let projection_orchestrator = orchestrator(&published, provider.clone(), sink.clone());
    for (observation, expected) in [
        (
            WorkloadProviderObservation::Absent,
            WorkloadProjectionPendingReason::ExecutionAbsent,
        ),
        (
            WorkloadProviderObservation::InProgress,
            WorkloadProjectionPendingReason::ExecutionInProgress,
        ),
        (
            WorkloadProviderObservation::Ambiguous,
            WorkloadProjectionPendingReason::ExecutionAmbiguous,
        ),
    ] {
        provider.set_execution(observation);
        assert_eq!(
            projection_orchestrator
                .project_record(
                    &published.observed,
                    WorkloadProvisionRunDisposition::Observed,
                )
                .await,
            WorkloadProjectionState::Pending(expected)
        );
    }
    provider.set_execution(WorkloadProviderObservation::Present(exact_inspection(
        &published.observed,
    )));
    for (observation, expected) in [
        (
            WorkloadProviderObservation::Absent,
            WorkloadProjectionPendingReason::IngressAbsent,
        ),
        (
            WorkloadProviderObservation::InProgress,
            WorkloadProjectionPendingReason::IngressInProgress,
        ),
        (
            WorkloadProviderObservation::Ambiguous,
            WorkloadProjectionPendingReason::IngressAmbiguous,
        ),
    ] {
        provider.set_ingress(observation);
        assert_eq!(
            projection_orchestrator
                .project_record(
                    &published.observed,
                    WorkloadProvisionRunDisposition::Observed,
                )
                .await,
            WorkloadProjectionState::Pending(expected)
        );
    }
    assert!(sink.projections().is_empty());

    let withheld = fixture(WorkloadPublicationIntent::Withheld);
    let mut inspection = exact_inspection(&withheld.observed);
    inspection
        .handle
        .published_endpoints
        .push(PublishedEndpoint::new(
            "crossed",
            nimbus_network::EndpointProtocol::Tcp,
            "127.0.0.1:49152"
                .parse()
                .expect("crossed endpoint should parse"),
        ));
    let provider = RecordingProvider::new(
        WorkloadProviderObservation::Present(inspection),
        WorkloadProviderObservation::Present(Vec::new()),
    );
    let sink = Arc::new(RecordingSink::default());
    assert_eq!(
        orchestrator(&withheld, provider.clone(), sink.clone())
            .project_record(
                &withheld.observed,
                WorkloadProvisionRunDisposition::Observed,
            )
            .await,
        WorkloadProjectionState::Rejected(
            WorkloadProjectionRejectedReason::WithheldPublicationCarriedEndpoints
        )
    );
    assert_eq!(provider.ingress_calls.load(Ordering::Acquire), 0);
    assert!(sink.projections().is_empty());
}

#[tokio::test]
async fn crossed_ingress_witness_matrix_and_missing_ready_endpoint_mutate_nothing() {
    let fixture = fixture(WorkloadPublicationIntent::PublishWhenReady);
    let exact = exact_ingress(&fixture.observed);
    let mut cases = Vec::new();

    let mut wrong_plan_id = exact.clone();
    wrong_plan_id.binding.plan_id = NetworkPlanId::for_tenant_workload_plan(
        &TenantId::new(TENANT).expect("fixture tenant should validate"),
        "crossed-workload",
    );
    cases.push(vec![wrong_plan_id]);
    let mut wrong_plan = exact.clone();
    wrong_plan.binding.plan_digest = NetworkPlanDigest::from_bytes([9; 32]);
    cases.push(vec![wrong_plan]);
    let mut wrong_generation = exact.clone();
    wrong_generation.binding.generation = NetworkResourceGeneration::new(GENERATION + 1);
    cases.push(vec![wrong_generation]);
    let mut wrong_listener = exact.clone();
    wrong_listener.binding.listener_id = ListenerId::for_tenant_workload_listener(
        &TenantId::new(TENANT).expect("fixture tenant should validate"),
        "crossed-workload",
        "api",
    );
    cases.push(vec![wrong_listener]);
    let mut wrong_lease = exact.clone();
    wrong_lease.binding.port_lease_id =
        PortLeaseId::for_listener(&ListenerId::for_tenant_workload_listener(
            &TenantId::new(TENANT).expect("fixture tenant should validate"),
            "crossed-workload",
            "api",
        ));
    cases.push(vec![wrong_lease]);
    let mut wrong_lifetime = exact.clone();
    wrong_lifetime.binding.binding_lifetime = lifetime(4);
    cases.push(vec![wrong_lifetime]);
    let mut wrong_protocol = exact.clone();
    wrong_protocol.binding.bound_endpoint = PortBoundEndpoint::new(
        PortProtocol::Udp,
        PortBindRealm::Host,
        PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
        NonZeroU16::new(49_152).expect("bound port should be nonzero"),
    )
    .expect("crossed protocol endpoint should validate structurally");
    cases.push(vec![wrong_protocol]);
    let mut wrong_host = exact.clone();
    wrong_host.binding.bound_endpoint = PortBoundEndpoint::new(
        PortProtocol::Tcp,
        PortBindRealm::Host,
        PortBindTarget::ipv4_specific(Ipv4Addr::new(127, 0, 0, 2)),
        NonZeroU16::new(49_152).expect("bound port should be nonzero"),
    )
    .expect("crossed host endpoint should validate structurally");
    cases.push(vec![wrong_host]);
    cases.push(Vec::new());
    cases.push(vec![exact.clone(), exact]);

    for observations in cases {
        let provider = RecordingProvider::new(
            WorkloadProviderObservation::Present(exact_inspection(&fixture.observed)),
            WorkloadProviderObservation::Present(observations),
        );
        let sink = Arc::new(RecordingSink::default());
        let state = orchestrator(&fixture, provider.clone(), sink.clone())
            .project_record(&fixture.observed, WorkloadProvisionRunDisposition::Observed)
            .await;
        assert_eq!(
            state,
            WorkloadProjectionState::Rejected(
                WorkloadProjectionRejectedReason::InvalidIngressEvidence
            )
        );
        assert_eq!(provider.effect_calls.load(Ordering::Acquire), 0);
        assert!(sink.projections().is_empty());
    }
}

#[tokio::test]
async fn exact_replay_is_idempotent_and_ephemeral_evidence_is_not_portable_serde() {
    let fixture = fixture(WorkloadPublicationIntent::PublishWhenReady);
    let provider = RecordingProvider::new(
        WorkloadProviderObservation::Present(exact_inspection(&fixture.observed)),
        WorkloadProviderObservation::Present(vec![exact_ingress(&fixture.observed)]),
    );
    let sink = Arc::new(RecordingSink::default());
    let orchestrator = orchestrator(&fixture, provider.clone(), sink.clone());
    for _ in 0..2 {
        assert_eq!(
            orchestrator
                .project_record(&fixture.observed, WorkloadProvisionRunDisposition::Observed,)
                .await,
            WorkloadProjectionState::Projected
        );
    }
    let projections = sink.projections();
    assert_eq!(projections.len(), 2);
    assert_eq!(projections[0], projections[1]);
    assert_eq!(provider.effect_calls.load(Ordering::Acquire), 0);

    let portable = serde_json::to_string(&fixture.observed)
        .expect("portable workload record should serialize");
    for forbidden in [
        "SandboxInspection",
        "published_endpoints",
        "provider_handle",
        "binding_lifetime",
        "49152",
    ] {
        assert!(
            !portable.contains(forbidden),
            "portable workload record must exclude ephemeral projection evidence `{forbidden}`"
        );
    }
}
