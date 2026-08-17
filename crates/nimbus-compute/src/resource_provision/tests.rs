use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_core::{TenantId, WorkloadId};
use nimbus_network::{
    EndpointProtocol, NetworkAddressFamily, NetworkAttachmentProviderRegistration,
    NetworkBindRealmKind, NetworkCapabilityBundle, NetworkCapabilityRegistry,
    NetworkCapabilitySelection, NetworkControlPlaneLocality, NetworkEndpointCapabilitySet,
    NetworkExposure, NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet, NetworkLifecycleFeature,
    NetworkPortAssignmentMode, NetworkProviderId, NetworkSovereigntyCapabilities,
    NetworkSovereigntyRequirements, NetworkTlsBehavior, PortProtocol,
};
use nimbus_sandbox::{
    SandboxBackendKind, SandboxOwnerSpec, SandboxPortBinding, SandboxProcessSpec, SandboxRootSpec,
    SandboxSpec,
};
use nimbus_services::{EmptyServiceDefinitionCatalog, ServiceBackend, ServiceManager};
use nimbus_tenant::{
    TenantIsolationContext, TenantIsolationPolicyInput, WorkloadAttributes, WorkloadLocation,
};
use nimbus_workloads::{
    WorkloadActivationIntent, WorkloadExecutionReference, WorkloadFailureEvidence,
    WorkloadNetworkForwardingBehavior, WorkloadOwnerEvidenceDigest,
    WorkloadProvisionInspectionResult, WorkloadProvisionSourceGeneration,
    WorkloadProvisionSourceResourceVersion, WorkloadPublicationIntent, WorkloadSagaCommit,
    WorkloadSagaExpected, WorkloadSagaFuture, WorkloadSagaKey, WorkloadSagaPage,
    WorkloadSagaPageRequest, WorkloadSagaPhase, WorkloadSagaRecord, WorkloadSagaStore,
    WorkloadSagaStoreError, WorkloadSagaTenantPage, WorkloadSagaTenantPageRequest,
    WorkloadTeardownCause, WorkloadTeardownCommandMode, WorkloadTeardownStep,
};
use tokio::sync::Semaphore;

use super::*;
use crate::embedded_local_node_identity;
use crate::workload_projection::ServiceManagerWorkloadProjectionSink;
use crate::workload_provision_source::ServiceManagerWorkloadProvisionSourceAuthority;
use crate::workload_provisioner::WorkloadProvisionCompensationState;
use crate::workload_saga::{
    ConfirmedWorkloadProvisionCommand, ConfirmedWorkloadTeardownCommand,
    FinalIngressWithdrawalCapability, IngressProvisionCapabilities, IngressPublicationCapability,
    IngressPublicationInspectionCapability, IngressTeardownCapabilities,
    NetworkAttachmentCapability, NetworkAttachmentProvisionCapabilities,
    NetworkAttachmentTeardownCapabilities, NetworkDetachmentCapability, NetworkReleaseCapability,
    NetworkReservationCapability, WorkloadActivationCapability,
    WorkloadActivationPrerequisiteCapability, WorkloadExecutionDrainCapability,
    WorkloadExecutionProvisionCapabilities, WorkloadExecutionStopCapability,
    WorkloadPreparationCapability, WorkloadProvisionCapabilityFuture,
    WorkloadProvisionCapabilityRegistry, WorkloadProvisionSourceAuthority,
    WorkloadReadinessCapability, WorkloadSagaCoordinator, WorkloadTeardownCapabilityFuture,
    WorkloadTeardownCapabilityRegistry, WorkloadTeardownExecuteOutcome,
    WorkloadTeardownInspectOutcome, WorkloadTeardownProviderObservation,
    WorkloadTeardownProviderOutcome, WorkloadTeardownRuntime,
};

fn tenant() -> TenantId {
    TenantId::new("tenant-a").expect("fixture tenant should validate")
}

fn spec(bindings: impl IntoIterator<Item = SandboxPortBinding>) -> SandboxSpec {
    SandboxSpec::new(
        tenant(),
        SandboxOwnerSpec::standalone_named("worker"),
        SandboxBackendKind::Krun,
        SandboxRootSpec::rootfs("/fixture/rootfs"),
        SandboxProcessSpec::new(["/bin/true"]),
    )
    .with_port_bindings(bindings)
}

fn decision() -> nimbus_tenant::TenantIsolationDecision {
    TenantIsolationContext::system(tenant(), "resource-provision-test")
        .with_deployment_generation(1)
        .with_workload_location(
            WorkloadLocation::new().with_node_id(embedded_local_node_identity().as_str()),
        )
        .admit_decision(TenantIsolationPolicyInput::new(
            WorkloadAttributes::sandbox("worker")
                .with_sandbox_id("stable-worker")
                .with_sandbox_backend(SandboxBackendKind::Krun),
        ))
        .expect("fixture workload should admit")
}

fn source(spec: SandboxSpec) -> WorkloadProvisionSource {
    WorkloadProvisionSource::StandaloneSandbox {
        stable_resource_id: "stable-worker".to_owned(),
        profile: "worker".to_owned(),
        source_generation: WorkloadProvisionSourceGeneration::new(1),
        resource_version: WorkloadProvisionSourceResourceVersion::new("source-v1")
            .expect("fixture source version should validate"),
        sandbox_spec: spec,
    }
}

struct NativeSagaStore {
    records: Mutex<BTreeMap<WorkloadSagaKey, WorkloadSagaRecord>>,
    pause_after_first_submission: AtomicBool,
    fail_failed_provision_once: AtomicBool,
    first_submission_applied: Semaphore,
    release_first_submission: Semaphore,
    recorded_applied: Semaphore,
    release_recorded: Semaphore,
}

impl Default for NativeSagaStore {
    fn default() -> Self {
        Self {
            records: Mutex::new(BTreeMap::new()),
            pause_after_first_submission: AtomicBool::new(false),
            fail_failed_provision_once: AtomicBool::new(false),
            first_submission_applied: Semaphore::new(0),
            release_first_submission: Semaphore::new(0),
            recorded_applied: Semaphore::new(0),
            release_recorded: Semaphore::new(0),
        }
    }
}

impl NativeSagaStore {
    fn pausing_after_first_submission() -> Arc<Self> {
        Arc::new(Self {
            pause_after_first_submission: AtomicBool::new(true),
            ..Self::default()
        })
    }

    fn failing_failed_provision_once() -> Arc<Self> {
        Arc::new(Self {
            fail_failed_provision_once: AtomicBool::new(true),
            ..Self::default()
        })
    }

    fn record_count(&self) -> usize {
        self.records
            .lock()
            .expect("native saga store lock should remain healthy")
            .len()
    }

    fn record(&self, key: &WorkloadSagaKey) -> WorkloadSagaRecord {
        self.records
            .lock()
            .expect("native saga store lock should remain healthy")
            .get(key)
            .cloned()
            .expect("native saga record should exist")
    }
}

impl WorkloadSagaStore for NativeSagaStore {
    fn load<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            Ok(self
                .records
                .lock()
                .expect("native saga store lock should remain healthy")
                .get(key)
                .cloned())
        })
    }

    fn compare_and_swap<'a>(
        &'a self,
        expected: WorkloadSagaExpected,
        next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            if next.teardown_disposition().is_some_and(|disposition| {
                matches!(
                    disposition.cause(),
                    WorkloadTeardownCause::FailedProvision { .. }
                )
            }) && self
                .fail_failed_provision_once
                .swap(false, Ordering::AcqRel)
            {
                return Err(WorkloadSagaStoreError::Unavailable);
            }
            let key = next.key().clone();
            let recorded = next.phase() == WorkloadSagaPhase::Recorded;
            let first_submission = {
                let mut records = self
                    .records
                    .lock()
                    .expect("native saga store lock should remain healthy");
                if records.get(&key) == Some(&next) {
                    return Ok(WorkloadSagaCommit::Unchanged);
                }
                let observed = records.get(&key);
                let matches = match (expected, observed) {
                    (WorkloadSagaExpected::Missing, None) => true,
                    (WorkloadSagaExpected::Revision(expected), Some(record)) => {
                        record.revision() == expected
                    }
                    _ => false,
                };
                if !matches {
                    return Err(WorkloadSagaStoreError::Conflict {
                        expected,
                        observed: observed.map(WorkloadSagaRecord::revision),
                    });
                }
                let first_submission = records.is_empty();
                records.insert(key, next);
                first_submission
            };
            if first_submission && self.pause_after_first_submission.load(Ordering::Acquire) {
                self.first_submission_applied.add_permits(1);
                self.release_first_submission
                    .acquire()
                    .await
                    .expect("native submission release semaphore should remain open")
                    .forget();
            }
            if recorded && self.pause_after_first_submission.load(Ordering::Acquire) {
                self.recorded_applied.add_permits(1);
                self.release_recorded
                    .acquire()
                    .await
                    .expect("native recorded release semaphore should remain open")
                    .forget();
            }
            Ok(WorkloadSagaCommit::Applied)
        })
    }

    fn list_recoverable<'a>(
        &'a self,
        request: WorkloadSagaPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaPage> {
        Box::pin(async move { WorkloadSagaPage::new(&request, Vec::new(), false) })
    }

    fn list_restart_candidates<'a>(
        &'a self,
        request: nimbus_workloads::WorkloadRestartCandidatePageRequest,
    ) -> nimbus_workloads::WorkloadSagaFuture<'a, nimbus_workloads::WorkloadRestartCandidatePage>
    {
        Box::pin(async move {
            nimbus_workloads::WorkloadRestartCandidatePage::new(&request, Vec::new(), false)
        })
    }

    fn list_for_tenant<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage> {
        Box::pin(async move { WorkloadSagaTenantPage::new(tenant_id, &request, Vec::new(), false) })
    }
}

struct NativeProvisionProvider {
    calls: Mutex<Vec<(WorkloadSagaKey, nimbus_workloads::WorkloadProvisionStep)>>,
    provision_behavior: NativeProvisionBehavior,
    provision_in_progress_calls: AtomicUsize,
    provision_in_progress: Semaphore,
    teardown_calls: Mutex<Vec<WorkloadTeardownStep>>,
    teardown_modes: Mutex<Vec<WorkloadTeardownCommandMode>>,
    failure_step: Option<nimbus_workloads::WorkloadProvisionStep>,
    teardown_behavior: NativeTeardownBehavior,
    teardown_one_shot_observed: AtomicBool,
    execution_observations: AtomicUsize,
}

#[derive(Debug, Clone, Copy, Default)]
enum NativeProvisionBehavior {
    #[default]
    Succeed,
    InProgressThenSucceededAt(nimbus_workloads::WorkloadProvisionStep),
    AlwaysInProgressAt(nimbus_workloads::WorkloadProvisionStep),
}

#[derive(Debug, Clone, Copy, Default)]
enum NativeTeardownBehavior {
    #[default]
    Succeed,
    InProgressOnceAt(WorkloadTeardownStep),
    DefiniteFailureAt(WorkloadTeardownStep),
}

impl Default for NativeProvisionProvider {
    fn default() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            provision_behavior: NativeProvisionBehavior::Succeed,
            provision_in_progress_calls: AtomicUsize::new(0),
            provision_in_progress: Semaphore::new(0),
            teardown_calls: Mutex::new(Vec::new()),
            teardown_modes: Mutex::new(Vec::new()),
            failure_step: None,
            teardown_behavior: NativeTeardownBehavior::Succeed,
            teardown_one_shot_observed: AtomicBool::new(false),
            execution_observations: AtomicUsize::new(0),
        }
    }
}

impl NativeProvisionProvider {
    fn outcome(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionInspectionResult {
        self.calls
            .lock()
            .expect("native provision provider lock should remain healthy")
            .push((command.claim().attempt().key().clone(), command.step()));
        let in_progress = match self.provision_behavior {
            NativeProvisionBehavior::InProgressThenSucceededAt(step) if step == command.step() => {
                self.provision_in_progress_calls
                    .fetch_add(1, Ordering::AcqRel)
                    < 2
            }
            NativeProvisionBehavior::AlwaysInProgressAt(step) if step == command.step() => {
                self.provision_in_progress_calls
                    .fetch_add(1, Ordering::AcqRel);
                true
            }
            _ => false,
        };
        if in_progress {
            self.provision_in_progress.add_permits(1);
            return WorkloadProvisionInspectionResult::InProgress {
                attempt_id: command.attempt_id().clone(),
                dispatch_epoch: command.dispatch_epoch(),
                provider_target: command.provider_target().clone(),
                evidence: WorkloadOwnerEvidenceDigest::sha256("native fixture in progress"),
            };
        }
        if self.failure_step == Some(command.step()) {
            return WorkloadProvisionInspectionResult::DefiniteFailure {
                attempt_id: command.attempt_id().clone(),
                dispatch_epoch: command.dispatch_epoch(),
                provider_target: command.provider_target().clone(),
                failure: WorkloadFailureEvidence::new(
                    "native_fixture_failure",
                    WorkloadOwnerEvidenceDigest::sha256("native fixture failure"),
                )
                .expect("native fixture failure should validate"),
            };
        }
        WorkloadProvisionInspectionResult::Succeeded {
            attempt_id: command.attempt_id().clone(),
            dispatch_epoch: command.dispatch_epoch(),
            provider_target: command.provider_target().clone(),
            evidence: crate::workload_saga::test_support::success_for(command.claim().attempt()),
        }
    }

    fn calls_for(&self, key: &WorkloadSagaKey) -> usize {
        self.calls
            .lock()
            .expect("native provision provider lock should remain healthy")
            .iter()
            .filter(|(candidate, _)| candidate == key)
            .count()
    }

    fn failing_at(step: nimbus_workloads::WorkloadProvisionStep) -> Arc<Self> {
        Arc::new(Self {
            failure_step: Some(step),
            ..Self::default()
        })
    }

    fn in_progress_then_succeeded_at(step: nimbus_workloads::WorkloadProvisionStep) -> Arc<Self> {
        Arc::new(Self {
            provision_behavior: NativeProvisionBehavior::InProgressThenSucceededAt(step),
            ..Self::default()
        })
    }

    fn always_in_progress_at(step: nimbus_workloads::WorkloadProvisionStep) -> Arc<Self> {
        Arc::new(Self {
            provision_behavior: NativeProvisionBehavior::AlwaysInProgressAt(step),
            ..Self::default()
        })
    }

    fn failing_at_with_teardown(
        step: nimbus_workloads::WorkloadProvisionStep,
        teardown_behavior: NativeTeardownBehavior,
    ) -> Arc<Self> {
        Arc::new(Self {
            failure_step: Some(step),
            teardown_behavior,
            ..Self::default()
        })
    }

    fn teardown_calls(&self) -> Vec<WorkloadTeardownStep> {
        self.teardown_calls
            .lock()
            .expect("native teardown provider lock should remain healthy")
            .clone()
    }

    fn teardown_modes(&self) -> Vec<WorkloadTeardownCommandMode> {
        self.teardown_modes
            .lock()
            .expect("native teardown provider lock should remain healthy")
            .clone()
    }

    fn teardown_outcome(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownProviderObservation {
        self.teardown_calls
            .lock()
            .expect("native teardown provider lock should remain healthy")
            .push(command.step());
        self.teardown_modes
            .lock()
            .expect("native teardown provider lock should remain healthy")
            .push(command.mode());
        let success = || {
            Box::new(crate::workload_saga::test_support::teardown_success_for(
                command.step(),
                command.subjects(),
            ))
        };
        let outcome = match (self.teardown_behavior, command.mode()) {
            (NativeTeardownBehavior::DefiniteFailureAt(step), mode) if step == command.step() => {
                let failure = WorkloadFailureEvidence::new(
                    "native_fixture_teardown_failure",
                    WorkloadOwnerEvidenceDigest::sha256("native fixture teardown failure"),
                )
                .expect("native teardown failure should validate");
                match mode {
                    WorkloadTeardownCommandMode::Execute => {
                        WorkloadTeardownProviderOutcome::Execute(
                            WorkloadTeardownExecuteOutcome::DefiniteFailure(failure),
                        )
                    }
                    WorkloadTeardownCommandMode::Inspect => {
                        WorkloadTeardownProviderOutcome::Inspect(
                            WorkloadTeardownInspectOutcome::DefiniteFailure(failure),
                        )
                    }
                }
            }
            (
                NativeTeardownBehavior::InProgressOnceAt(step),
                WorkloadTeardownCommandMode::Execute,
            ) if step == command.step() => {
                WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Ambiguous)
            }
            (
                NativeTeardownBehavior::InProgressOnceAt(step),
                WorkloadTeardownCommandMode::Inspect,
            ) if step == command.step()
                && !self.teardown_one_shot_observed.swap(true, Ordering::AcqRel) =>
            {
                WorkloadTeardownProviderOutcome::Inspect(
                    WorkloadTeardownInspectOutcome::InProgress(
                        WorkloadOwnerEvidenceDigest::sha256("native teardown still in progress"),
                    ),
                )
            }
            (_, WorkloadTeardownCommandMode::Execute) => WorkloadTeardownProviderOutcome::Execute(
                WorkloadTeardownExecuteOutcome::Succeeded(success()),
            ),
            (_, WorkloadTeardownCommandMode::Inspect) => WorkloadTeardownProviderOutcome::Inspect(
                WorkloadTeardownInspectOutcome::Satisfied(success()),
            ),
        };
        WorkloadTeardownProviderObservation::for_command(command, outcome)
    }
}

macro_rules! native_effect_capability {
    ($trait_name:ident) => {
        impl $trait_name for NativeProvisionProvider {
            fn execute<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.outcome(command) })
            }

            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.outcome(command) })
            }
        }
    };
}

macro_rules! native_inspection_capability {
    ($trait_name:ident) => {
        impl $trait_name for NativeProvisionProvider {
            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.outcome(command) })
            }
        }
    };
}

native_effect_capability!(NetworkReservationCapability);
native_effect_capability!(WorkloadPreparationCapability);
native_effect_capability!(NetworkAttachmentCapability);
native_inspection_capability!(WorkloadActivationPrerequisiteCapability);
native_effect_capability!(WorkloadActivationCapability);
native_inspection_capability!(WorkloadReadinessCapability);
native_effect_capability!(IngressPublicationCapability);
native_inspection_capability!(IngressPublicationInspectionCapability);

macro_rules! native_teardown_capability {
    ($trait_name:ident) => {
        impl $trait_name for NativeProvisionProvider {
            fn execute<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadTeardownCommand,
            ) -> WorkloadTeardownCapabilityFuture<'a> {
                Box::pin(async move { self.teardown_outcome(command) })
            }

            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadTeardownCommand,
            ) -> WorkloadTeardownCapabilityFuture<'a> {
                Box::pin(async move { self.teardown_outcome(command) })
            }
        }
    };
}

native_teardown_capability!(FinalIngressWithdrawalCapability);
native_teardown_capability!(WorkloadExecutionDrainCapability);
native_teardown_capability!(WorkloadExecutionStopCapability);
native_teardown_capability!(NetworkDetachmentCapability);
native_teardown_capability!(NetworkReleaseCapability);

impl crate::workload_projection::WorkloadExecutionObservationCapability
    for NativeProvisionProvider
{
    fn observe<'a>(
        &'a self,
        request: &'a crate::workload_projection::WorkloadExecutionObservationRequest,
    ) -> crate::workload_projection::WorkloadExecutionObservationFuture<'a> {
        Box::pin(async move {
            self.execution_observations.fetch_add(1, Ordering::AcqRel);
            crate::workload_projection::WorkloadProviderObservation::Present(
                crate::workload_projection::test_support::exact_execution_inspection(
                    request,
                    b"native-provision-provider",
                ),
            )
        })
    }
}

impl crate::workload_projection::WorkloadIngressObservationCapability for NativeProvisionProvider {
    fn observe<'a>(
        &'a self,
        _request: &'a crate::workload_projection::WorkloadIngressObservationRequest,
    ) -> crate::workload_projection::WorkloadIngressObservationFuture<'a> {
        Box::pin(async { crate::workload_projection::WorkloadProviderObservation::Ambiguous })
    }
}

fn native_provider_realm() -> (NetworkCapabilityRegistry, NetworkCapabilitySelection) {
    let requirements = nimbus_sandbox::sandbox_network_plan_requirements(SandboxBackendKind::Krun);
    let ingress_provider = NetworkProviderId::for_registration_key("native-fixture-ingress");
    let lifecycle = NetworkLifecycleCapabilitySet::new([
        NetworkLifecycleFeature::DurableInspect,
        NetworkLifecycleFeature::Reconcile,
        NetworkLifecycleFeature::Delete,
    ]);
    let attachment = NetworkAttachmentProviderRegistration::new(
        requirements.required_attachment_provider_id().clone(),
        requirements.capability_requirements().attachment().clone(),
        [NetworkAddressFamily::Ipv4, NetworkAddressFamily::Ipv6],
        lifecycle.clone(),
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
        lifecycle,
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let selection = NetworkCapabilitySelection::new(
        requirements.required_attachment_provider_id().clone(),
        ingress_provider,
    );
    (
        NetworkCapabilityRegistry::new([NetworkCapabilityBundle::new(attachment, ingress)])
            .expect("native fixture provider reports should validate"),
        selection,
    )
}

fn native_provisioner(
    manager: Arc<ServiceManager>,
    store: Arc<NativeSagaStore>,
    provider: Arc<NativeProvisionProvider>,
) -> Arc<WorkloadProvisioner> {
    let (provider_reports, selection) = native_provider_realm();
    let execution_provider = sandbox_execution_provider_id(SandboxBackendKind::Krun);
    let attachment_provider =
        nimbus_sandbox::sandbox_network_plan_requirements(SandboxBackendKind::Krun)
            .required_attachment_provider_id()
            .clone();
    let capabilities = WorkloadProvisionCapabilityRegistry::new(
        [NetworkAttachmentProvisionCapabilities::new(
            attachment_provider.clone(),
            provider.clone(),
        )],
        [WorkloadExecutionProvisionCapabilities::new(
            execution_provider,
            provider.clone(),
        )],
        [IngressProvisionCapabilities::new(
            selection.ingress_provider_id().clone(),
            provider.clone(),
        )],
    )
    .expect("native fixture capabilities should validate");
    let store: Arc<dyn WorkloadSagaStore> = store;
    let source_authority: Arc<dyn WorkloadProvisionSourceAuthority> = Arc::new(
        ServiceManagerWorkloadProvisionSourceAuthority::new(manager.clone()),
    );
    let coordinator = Arc::new(WorkloadSagaCoordinator::new(store));
    let teardown_capabilities = WorkloadTeardownCapabilityRegistry::new(
        [NetworkAttachmentTeardownCapabilities::new(
            attachment_provider,
            provider.clone(),
            provider.clone(),
        )],
        [
            crate::workload_saga::WorkloadExecutionTeardownCapabilities::new(
                sandbox_execution_provider_id(SandboxBackendKind::Krun),
                provider.clone(),
                provider.clone(),
            ),
        ],
        [IngressTeardownCapabilities::new(
            selection.ingress_provider_id().clone(),
            provider,
        )],
    )
    .expect("native fixture teardown capabilities should validate");
    let teardown_runtime = Arc::new(WorkloadTeardownRuntime::new(
        Arc::clone(&coordinator),
        Arc::clone(&source_authority),
        provider_reports.clone(),
        Arc::new(teardown_capabilities),
    ));
    Arc::new(
        WorkloadProvisioner::new(
            embedded_local_node_identity(),
            provider_reports,
            selection,
            NetworkSovereigntyRequirements::new(
                NetworkControlPlaneLocality::LocalOnly,
                BTreeSet::new(),
                true,
            ),
            Arc::clone(&coordinator),
            teardown_runtime,
            source_authority,
            capabilities,
            Arc::new(ServiceManagerWorkloadProjectionSink::new(manager)),
        )
        .expect("native fixture provider realm should be coherent"),
    )
}

#[test]
fn canonical_request_preserves_named_port_forwarding_and_exact_tls_semantics() {
    let spec = spec([
        SandboxPortBinding::new("tcp", EndpointProtocol::Tcp, 14001, 4001),
        SandboxPortBinding::new("http", EndpointProtocol::Http, 14002, 4002),
        SandboxPortBinding::new("https", EndpointProtocol::Https, 14003, 4003),
    ]);

    let request = provision_request(decision(), source(spec.clone()), &spec);

    assert_eq!(
        request.execution_provider_id,
        sandbox_execution_provider_id(SandboxBackendKind::Krun)
    );
    assert_eq!(
        request.activation,
        WorkloadActivationIntent::ActivateWhenAttached
    );
    assert_eq!(
        request.publication,
        WorkloadPublicationIntent::PublishWhenReady
    );
    assert_eq!(request.endpoint_semantics.len(), 3);
    assert_eq!(request.endpoint_semantics[0].listener_name(), "tcp");
    assert_eq!(request.endpoint_semantics[1].listener_name(), "http");
    assert_eq!(request.endpoint_semantics[2].listener_name(), "https");
    assert!(request.endpoint_semantics.iter().all(|endpoint| {
        endpoint.forwarding() == WorkloadNetworkForwardingBehavior::PortForwarded
    }));
    assert_eq!(
        request.endpoint_semantics[0].tls(),
        NetworkTlsBehavior::Disabled
    );
    assert_eq!(
        request.endpoint_semantics[1].tls(),
        NetworkTlsBehavior::Disabled
    );
    assert_eq!(
        request.endpoint_semantics[2].tls(),
        NetworkTlsBehavior::Passthrough
    );
}

#[test]
fn canonical_request_withholds_publication_when_no_bindings_exist() {
    let spec = spec([]);

    let request = provision_request(decision(), source(spec.clone()), &spec);

    assert!(request.endpoint_semantics.is_empty());
    assert_eq!(request.publication, WorkloadPublicationIntent::Withheld);
    assert_eq!(
        request.activation,
        WorkloadActivationIntent::ActivateWhenAttached
    );
}

#[tokio::test]
async fn native_service_and_sandbox_callers_use_compute_dispatch() {
    let tenant_id = tenant();
    let manager = Arc::new(ServiceManager::new(
        Arc::new(EmptyServiceDefinitionCatalog),
        SandboxBackendKind::Krun,
    ));
    let service_spec = SandboxSpec::new(
        tenant_id.clone(),
        SandboxOwnerSpec::service("service-worker"),
        SandboxBackendKind::Krun,
        SandboxRootSpec::rootfs("/fixture/service-rootfs"),
        SandboxProcessSpec::new(["/bin/service"]),
    );
    manager
        .create_service_definition(
            &tenant_id,
            "service-worker",
            ServiceBackend::sandbox(service_spec),
            BTreeMap::new(),
        )
        .expect("native sandbox-backed service source should be declared");

    let store = Arc::new(NativeSagaStore::default());
    let provider = Arc::new(NativeProvisionProvider::default());
    let facade = ComputeResourceProvisioner::new(
        manager.clone(),
        native_provisioner(manager, store.clone(), provider.clone()),
    );
    let context = TenantIsolationContext::system(tenant_id.clone(), "native-resource-provision");
    let cancellation = WorkloadProvisionCancellation::default();

    let standalone = facade
        .provision_standalone_sandbox(
            &context,
            "standalone-worker",
            "worker",
            SandboxSpec::new(
                tenant_id.clone(),
                SandboxOwnerSpec::standalone_named("standalone-worker"),
                SandboxBackendKind::Krun,
                SandboxRootSpec::rootfs("/fixture/standalone-rootfs"),
                SandboxProcessSpec::new(["/bin/worker"]),
            ),
            BTreeMap::new(),
            &cancellation,
        )
        .await
        .expect("standalone native caller should complete through compute dispatch");
    let service = facade
        .provision_sandbox_service(&context, "service-worker", &cancellation)
        .await
        .expect("service native caller should complete through compute dispatch");

    let standalone_observation = standalone
        .observation
        .as_ref()
        .expect("standalone dispatch should project exact provider evidence");
    assert_eq!(
        standalone_observation.source_generation,
        standalone.source.generation
    );
    assert_eq!(
        standalone_observation.handle.id.as_str(),
        WorkloadExecutionReference::for_intent(
            store
                .record(&WorkloadSagaKey::new(
                    tenant_id.clone(),
                    WorkloadId::new("standalone-worker").expect("standalone ID should validate"),
                ))
                .active_intent(),
        )
        .execution_id()
        .as_str()
    );
    let service_observation = service
        .observation
        .as_ref()
        .expect("service dispatch should project exact provider evidence");
    assert_eq!(
        service_observation.source_generation,
        service.definition.generation
    );

    let standalone_key = WorkloadSagaKey::new(
        tenant_id.clone(),
        WorkloadId::new("standalone-worker").expect("standalone ID should validate"),
    );
    let service_key = WorkloadSagaKey::new(
        tenant_id,
        WorkloadId::new("service-worker").expect("service ID should validate"),
    );
    assert_eq!(store.record_count(), 2);
    assert_eq!(provider.calls_for(&standalone_key), 6);
    assert_eq!(provider.calls_for(&service_key), 6);
    assert_eq!(provider.execution_observations.load(Ordering::Acquire), 2);

    let replayed_standalone = facade
        .provision_standalone_sandbox(
            &context,
            "standalone-worker",
            "worker",
            standalone.source.spec.clone(),
            BTreeMap::new(),
            &WorkloadProvisionCancellation::default(),
        )
        .await
        .expect("exact standalone replay should reuse compute dispatch truth");
    let replayed_service = facade
        .provision_sandbox_service(
            &context,
            "service-worker",
            &WorkloadProvisionCancellation::default(),
        )
        .await
        .expect("exact service replay should reuse compute dispatch truth");
    assert_eq!(replayed_standalone, standalone);
    assert_eq!(replayed_service, service);
    assert_eq!(store.record_count(), 2);
    assert_eq!(provider.calls_for(&standalone_key), 6);
    assert_eq!(provider.calls_for(&service_key), 6);
    assert_eq!(
        provider.execution_observations.load(Ordering::Acquire),
        4,
        "exact replay may refresh read-only observed state but must not repeat provider effects"
    );
}

fn native_service_facade(
    service_name: &str,
    provider: Arc<NativeProvisionProvider>,
) -> (
    Arc<NativeSagaStore>,
    Arc<WorkloadProvisioner>,
    ComputeResourceProvisioner,
) {
    let manager = Arc::new(ServiceManager::new(
        Arc::new(EmptyServiceDefinitionCatalog),
        SandboxBackendKind::Krun,
    ));
    manager
        .create_service_definition(
            &tenant(),
            service_name,
            ServiceBackend::sandbox(SandboxSpec::new(
                tenant(),
                SandboxOwnerSpec::service(service_name),
                SandboxBackendKind::Krun,
                SandboxRootSpec::rootfs(format!("/fixture/{service_name}")),
                SandboxProcessSpec::new(["/bin/service"]),
            )),
            BTreeMap::new(),
        )
        .expect("native service source should be declared");
    let store = Arc::new(NativeSagaStore::default());
    let provisioner = native_provisioner(Arc::clone(&manager), Arc::clone(&store), provider);
    let facade = ComputeResourceProvisioner::new(Arc::clone(&manager), Arc::clone(&provisioner));
    (store, provisioner, facade)
}

#[tokio::test]
async fn foreground_service_provision_resumes_durable_waiting_until_exact_projection() {
    let service_name = "foreground-convergence";
    let provider = NativeProvisionProvider::in_progress_then_succeeded_at(
        nimbus_workloads::WorkloadProvisionStep::AttachNetwork,
    );
    let (store, _provisioner, facade) = native_service_facade(service_name, Arc::clone(&provider));
    let context = TenantIsolationContext::system(tenant(), "foreground-convergence");

    let snapshot = facade
        .provision_sandbox_service_until_observed(
            &context,
            service_name,
            &WorkloadProvisionCancellation::default(),
        )
        .await
        .expect("foreground compute owner should resume exact durable truth until projection");

    assert!(snapshot.observation.is_some());
    let key = WorkloadSagaKey::new(
        tenant(),
        WorkloadId::new(service_name).expect("fixture service ID should validate"),
    );
    assert_eq!(store.record(&key).phase(), WorkloadSagaPhase::Observed);
    assert_eq!(provider.calls_for(&key), 8);
    assert_eq!(
        provider.provision_in_progress_calls.load(Ordering::Acquire),
        3,
        "one Execute and one bounded Inspect return in progress before exact resume observes success"
    );
}

#[tokio::test]
async fn foreground_service_provision_cancels_without_a_busy_retry_owner() {
    let service_name = "foreground-cancellation";
    let provider = NativeProvisionProvider::always_in_progress_at(
        nimbus_workloads::WorkloadProvisionStep::AttachNetwork,
    );
    let (_store, provisioner, facade) = native_service_facade(service_name, Arc::clone(&provider));
    let key = WorkloadSagaKey::new(
        tenant(),
        WorkloadId::new(service_name).expect("fixture service ID should validate"),
    );
    let cancellation = WorkloadProvisionCancellation::default();
    let waiter_cancellation = cancellation.clone();
    let waiter = tokio::spawn(async move {
        facade
            .provision_sandbox_service_until_observed(
                &TenantIsolationContext::system(tenant(), "foreground-cancellation"),
                service_name,
                &waiter_cancellation,
            )
            .await
    });

    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        provider.provision_in_progress.acquire_many(3),
    )
    .await
    .expect("foreground provision should enter its first exact resume")
    .expect("provider progress signal should remain open")
    .forget();
    cancellation.cancel();
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
        .await
        .expect("cancelled foreground waiter should return")
        .expect("cancelled foreground task should join");
    assert!(matches!(
        result,
        Err(ComputeResourceProvisionError::Provision(error))
            if matches!(error.as_ref(), WorkloadProvisionError::WaiterCancelled)
    ));

    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while provisioner.has_running_tracked_task(&key) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the exact tracked resume task should settle after its waiter cancels");
    assert!(
        !provisioner.has_running_tracked_task(&key),
        "cancellation must leave no running owner that can retry provider inspection"
    );
}

fn failed_native_facade(
    failure_step: nimbus_workloads::WorkloadProvisionStep,
) -> (
    Arc<ServiceManager>,
    Arc<NativeSagaStore>,
    Arc<NativeProvisionProvider>,
    ComputeResourceProvisioner,
) {
    let manager = Arc::new(ServiceManager::new(
        Arc::new(EmptyServiceDefinitionCatalog),
        SandboxBackendKind::Krun,
    ));
    let store = Arc::new(NativeSagaStore::default());
    let provider = NativeProvisionProvider::failing_at(failure_step);
    let facade = ComputeResourceProvisioner::new(
        Arc::clone(&manager),
        native_provisioner(
            Arc::clone(&manager),
            Arc::clone(&store),
            Arc::clone(&provider),
        ),
    );
    (manager, store, provider, facade)
}

fn assert_failed_start_was_compensated(
    store: &NativeSagaStore,
    provider: &NativeProvisionProvider,
    stable_name: &str,
) {
    let record = store.record(&WorkloadSagaKey::new(
        tenant(),
        WorkloadId::new(stable_name).expect("fixture workload ID should validate"),
    ));
    assert_eq!(record.phase(), WorkloadSagaPhase::Recorded);
    assert_eq!(
        provider.teardown_calls(),
        [
            WorkloadTeardownStep::StopExecution,
            WorkloadTeardownStep::DetachNetwork,
            WorkloadTeardownStep::ReleaseNetwork,
        ]
    );
}

#[tokio::test]
async fn failed_service_start_enters_durable_compensation_without_caller_stop() {
    let (manager, store, provider, facade) =
        failed_native_facade(nimbus_workloads::WorkloadProvisionStep::ActivateWorkload);
    manager
        .create_service_definition(
            &tenant(),
            "failed-service",
            ServiceBackend::sandbox(SandboxSpec::new(
                tenant(),
                SandboxOwnerSpec::service("failed-service"),
                SandboxBackendKind::Krun,
                SandboxRootSpec::rootfs("/fixture/failed-service"),
                SandboxProcessSpec::new(["/bin/false"]),
            )),
            BTreeMap::new(),
        )
        .expect("failed service source should be declared");
    let context = TenantIsolationContext::system(tenant(), "failed-service-compensation");

    let result = facade
        .provision_sandbox_service(
            &context,
            "failed-service",
            &WorkloadProvisionCancellation::default(),
        )
        .await;

    assert!(matches!(
        result,
        Err(ComputeResourceProvisionError::Rejected {
            reason: "provision_definite_failure"
        })
    ));
    assert_failed_start_was_compensated(&store, &provider, "failed-service");
}

#[tokio::test]
async fn failed_sandbox_start_enters_durable_compensation_without_caller_stop() {
    let (_manager, store, provider, facade) =
        failed_native_facade(nimbus_workloads::WorkloadProvisionStep::ActivateWorkload);
    let context = TenantIsolationContext::system(tenant(), "failed-sandbox-compensation");

    let result = facade
        .provision_standalone_sandbox(
            &context,
            "failed-sandbox",
            "worker",
            SandboxSpec::new(
                tenant(),
                SandboxOwnerSpec::standalone_named("failed-sandbox"),
                SandboxBackendKind::Krun,
                SandboxRootSpec::rootfs("/fixture/failed-sandbox"),
                SandboxProcessSpec::new(["/bin/false"]),
            ),
            BTreeMap::new(),
            &WorkloadProvisionCancellation::default(),
        )
        .await;

    assert!(matches!(
        result,
        Err(ComputeResourceProvisionError::Rejected {
            reason: "provision_definite_failure"
        })
    ));
    assert_failed_start_was_compensated(&store, &provider, "failed-sandbox");
}

#[tokio::test]
async fn concurrent_failed_provision_callers_share_one_cause_and_teardown_sequence() {
    let (_manager, store, provider, facade) =
        failed_native_facade(nimbus_workloads::WorkloadProvisionStep::ActivateWorkload);
    let context = TenantIsolationContext::system(tenant(), "concurrent-failed-provision");
    let left_cancellation = WorkloadProvisionCancellation::default();
    let right_cancellation = WorkloadProvisionCancellation::default();
    let left = facade.provision_standalone_sandbox(
        &context,
        "concurrent-failed-sandbox",
        "worker",
        SandboxSpec::new(
            tenant(),
            SandboxOwnerSpec::standalone_named("concurrent-failed-sandbox"),
            SandboxBackendKind::Krun,
            SandboxRootSpec::rootfs("/fixture/concurrent-failed-sandbox"),
            SandboxProcessSpec::new(["/bin/false"]),
        ),
        BTreeMap::new(),
        &left_cancellation,
    );
    let right = facade.provision_standalone_sandbox(
        &context,
        "concurrent-failed-sandbox",
        "worker",
        SandboxSpec::new(
            tenant(),
            SandboxOwnerSpec::standalone_named("concurrent-failed-sandbox"),
            SandboxBackendKind::Krun,
            SandboxRootSpec::rootfs("/fixture/concurrent-failed-sandbox"),
            SandboxProcessSpec::new(["/bin/false"]),
        ),
        BTreeMap::new(),
        &right_cancellation,
    );

    let (left, right) = tokio::join!(left, right);
    for result in [left, right] {
        assert!(matches!(
            result,
            Err(ComputeResourceProvisionError::Rejected {
                reason: "provision_definite_failure"
            })
        ));
    }
    assert_failed_start_was_compensated(&store, &provider, "concurrent-failed-sandbox");
    assert_eq!(
        provider.calls_for(&WorkloadSagaKey::new(
            tenant(),
            WorkloadId::new("concurrent-failed-sandbox")
                .expect("fixture workload ID should validate"),
        )),
        5,
        "same-key contenders must share one provision attempt through its definite failure"
    );
}

#[tokio::test]
async fn failed_provision_compensation_survives_waiter_cancellation() {
    let manager = Arc::new(ServiceManager::new(
        Arc::new(EmptyServiceDefinitionCatalog),
        SandboxBackendKind::Krun,
    ));
    let store = NativeSagaStore::pausing_after_first_submission();
    let provider = NativeProvisionProvider::failing_at(
        nimbus_workloads::WorkloadProvisionStep::ActivateWorkload,
    );
    let provisioner = native_provisioner(
        Arc::clone(&manager),
        Arc::clone(&store),
        Arc::clone(&provider),
    );
    let facade = ComputeResourceProvisioner::new(manager, Arc::clone(&provisioner));
    let cancellation = WorkloadProvisionCancellation::default();
    let waiter_cancellation = cancellation.clone();
    let waiter = tokio::spawn(async move {
        facade
            .provision_standalone_sandbox(
                &TenantIsolationContext::system(tenant(), "cancelled-failed-provision"),
                "cancelled-failed-provision",
                "worker",
                SandboxSpec::new(
                    tenant(),
                    SandboxOwnerSpec::standalone_named("cancelled-failed-provision"),
                    SandboxBackendKind::Krun,
                    SandboxRootSpec::rootfs("/fixture/cancelled-failed-provision"),
                    SandboxProcessSpec::new(["/bin/false"]),
                ),
                BTreeMap::new(),
                &waiter_cancellation,
            )
            .await
    });

    store
        .first_submission_applied
        .acquire()
        .await
        .expect("first submission signal should remain open")
        .forget();
    let key = WorkloadSagaKey::new(
        tenant(),
        WorkloadId::new("cancelled-failed-provision")
            .expect("cancelled fixture workload ID should validate"),
    );
    assert!(provisioner.has_tracked_submission(&key));
    cancellation.cancel();
    let cancelled = waiter.await.expect("cancelled waiter task should join");
    assert!(matches!(
        cancelled,
        Err(ComputeResourceProvisionError::Provision(error))
            if matches!(error.as_ref(), WorkloadProvisionError::WaiterCancelled)
    ));
    assert!(provider.teardown_calls().is_empty());

    store.release_first_submission.add_permits(1);
    store
        .recorded_applied
        .acquire()
        .await
        .expect("recorded compensation signal should remain open")
        .forget();
    assert_eq!(store.record(&key).phase(), WorkloadSagaPhase::Recorded);
    assert_eq!(
        provider.teardown_calls(),
        [
            WorkloadTeardownStep::StopExecution,
            WorkloadTeardownStep::DetachNetwork,
            WorkloadTeardownStep::ReleaseNetwork,
        ]
    );
    assert!(
        provisioner.has_tracked_submission(&key),
        "the retained task must not be removed before Recorded is durable"
    );
    store.release_recorded.add_permits(1);
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while provisioner.has_tracked_submission(&key) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the retained task should leave the keyed supervisor after completion");
}

#[tokio::test]
async fn failed_provision_waiting_compensation_retains_owner_until_exact_inspection_completes() {
    let manager = Arc::new(ServiceManager::new(
        Arc::new(EmptyServiceDefinitionCatalog),
        SandboxBackendKind::Krun,
    ));
    let store = Arc::new(NativeSagaStore::default());
    let provider = NativeProvisionProvider::failing_at_with_teardown(
        nimbus_workloads::WorkloadProvisionStep::ActivateWorkload,
        NativeTeardownBehavior::InProgressOnceAt(WorkloadTeardownStep::StopExecution),
    );
    let provisioner = native_provisioner(
        Arc::clone(&manager),
        Arc::clone(&store),
        Arc::clone(&provider),
    );
    let facade = ComputeResourceProvisioner::new(manager, Arc::clone(&provisioner));
    let stable_name = "waiting-failed-provision";
    let key = WorkloadSagaKey::new(
        tenant(),
        WorkloadId::new(stable_name).expect("waiting fixture workload ID should validate"),
    );

    let first = facade
        .provision_standalone_sandbox(
            &TenantIsolationContext::system(tenant(), "waiting-failed-provision"),
            stable_name,
            "worker",
            SandboxSpec::new(
                tenant(),
                SandboxOwnerSpec::standalone_named(stable_name),
                SandboxBackendKind::Krun,
                SandboxRootSpec::rootfs("/fixture/waiting-failed-provision"),
                SandboxProcessSpec::new(["/bin/false"]),
            ),
            BTreeMap::new(),
            &WorkloadProvisionCancellation::default(),
        )
        .await;

    assert!(matches!(
        first,
        Err(ComputeResourceProvisionError::Rejected {
            reason: "provision_definite_failure"
        })
    ));
    assert!(provisioner.has_tracked_submission(&key));
    assert_eq!(
        provider.teardown_modes(),
        [
            WorkloadTeardownCommandMode::Execute,
            WorkloadTeardownCommandMode::Inspect,
        ]
    );

    let completed = provisioner
        .resume(key.clone(), &WorkloadProvisionCancellation::default())
        .await
        .expect("exact retained resume should inspect and converge");
    assert_eq!(
        completed.compensation(),
        WorkloadProvisionCompensationState::Completed
    );
    assert_eq!(store.record(&key).phase(), WorkloadSagaPhase::Recorded);
    assert_eq!(
        provider.teardown_modes(),
        [
            WorkloadTeardownCommandMode::Execute,
            WorkloadTeardownCommandMode::Inspect,
            WorkloadTeardownCommandMode::Inspect,
            WorkloadTeardownCommandMode::Execute,
            WorkloadTeardownCommandMode::Execute,
        ],
        "the issued Stop claim is inspected to success; only later Detach and Release execute"
    );
    assert!(!provisioner.has_tracked_submission(&key));
}

#[tokio::test]
async fn failed_provision_cleanup_pending_retains_key_and_blocks_reuse() {
    let manager = Arc::new(ServiceManager::new(
        Arc::new(EmptyServiceDefinitionCatalog),
        SandboxBackendKind::Krun,
    ));
    let store = Arc::new(NativeSagaStore::default());
    let provider = NativeProvisionProvider::failing_at_with_teardown(
        nimbus_workloads::WorkloadProvisionStep::ActivateWorkload,
        NativeTeardownBehavior::DefiniteFailureAt(WorkloadTeardownStep::StopExecution),
    );
    let provisioner = native_provisioner(
        Arc::clone(&manager),
        Arc::clone(&store),
        Arc::clone(&provider),
    );
    let facade = ComputeResourceProvisioner::new(manager, Arc::clone(&provisioner));
    let stable_name = "cleanup-pending-failed-provision";
    let key = WorkloadSagaKey::new(
        tenant(),
        WorkloadId::new(stable_name).expect("cleanup fixture workload ID should validate"),
    );

    let first = facade
        .provision_standalone_sandbox(
            &TenantIsolationContext::system(tenant(), "cleanup-pending-failed-provision"),
            stable_name,
            "worker",
            SandboxSpec::new(
                tenant(),
                SandboxOwnerSpec::standalone_named(stable_name),
                SandboxBackendKind::Krun,
                SandboxRootSpec::rootfs("/fixture/cleanup-pending-failed-provision"),
                SandboxProcessSpec::new(["/bin/false"]),
            ),
            BTreeMap::new(),
            &WorkloadProvisionCancellation::default(),
        )
        .await;
    assert!(matches!(
        first,
        Err(ComputeResourceProvisionError::Rejected {
            reason: "provision_definite_failure"
        })
    ));
    assert!(provisioner.has_tracked_submission(&key));
    let before_calls = provider.teardown_calls();
    let before_record = store.record(&key);

    let replay = provisioner
        .resume(key.clone(), &WorkloadProvisionCancellation::default())
        .await
        .expect("cleanup-pending replay should return the retained fenced outcome");
    assert_eq!(
        replay.compensation(),
        WorkloadProvisionCompensationState::CleanupPending
    );
    assert_eq!(store.record(&key), before_record);
    assert_eq!(provider.teardown_calls(), before_calls);
    assert!(provisioner.has_tracked_submission(&key));
}

#[tokio::test]
async fn failed_provision_compensation_error_retries_exact_run_without_provision_effects() {
    let manager = Arc::new(ServiceManager::new(
        Arc::new(EmptyServiceDefinitionCatalog),
        SandboxBackendKind::Krun,
    ));
    let store = NativeSagaStore::failing_failed_provision_once();
    let provider = NativeProvisionProvider::failing_at_with_teardown(
        nimbus_workloads::WorkloadProvisionStep::ActivateWorkload,
        NativeTeardownBehavior::Succeed,
    );
    let provisioner = native_provisioner(
        Arc::clone(&manager),
        Arc::clone(&store),
        Arc::clone(&provider),
    );
    let facade = ComputeResourceProvisioner::new(manager, Arc::clone(&provisioner));
    let stable_name = "compensation-error-failed-provision";
    let key = WorkloadSagaKey::new(
        tenant(),
        WorkloadId::new(stable_name).expect("compensation error fixture ID should validate"),
    );

    let first = facade
        .provision_standalone_sandbox(
            &TenantIsolationContext::system(tenant(), "compensation-error-failed-provision"),
            stable_name,
            "worker",
            SandboxSpec::new(
                tenant(),
                SandboxOwnerSpec::standalone_named(stable_name),
                SandboxBackendKind::Krun,
                SandboxRootSpec::rootfs("/fixture/compensation-error-failed-provision"),
                SandboxProcessSpec::new(["/bin/false"]),
            ),
            BTreeMap::new(),
            &WorkloadProvisionCancellation::default(),
        )
        .await;

    assert!(matches!(
        first,
        Err(ComputeResourceProvisionError::Provision(error))
            if matches!(error.as_ref(), WorkloadProvisionError::Compensation { .. })
    ));
    assert!(provisioner.has_tracked_submission(&key));
    assert!(provider.teardown_calls().is_empty());
    let provision_calls = provider.calls_for(&key);

    let completed = provisioner
        .resume(key.clone(), &WorkloadProvisionCancellation::default())
        .await
        .expect("same-process retry should resume the exact failed run and compensate it");

    assert_eq!(
        completed.compensation(),
        WorkloadProvisionCompensationState::Completed
    );
    assert_eq!(store.record(&key).phase(), WorkloadSagaPhase::Recorded);
    assert_eq!(provider.calls_for(&key), provision_calls);
    assert_eq!(
        provider.teardown_calls(),
        [
            WorkloadTeardownStep::StopExecution,
            WorkloadTeardownStep::DetachNetwork,
            WorkloadTeardownStep::ReleaseNetwork,
        ]
    );
    assert!(!provisioner.has_tracked_submission(&key));
}
