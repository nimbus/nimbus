//! Definition-retirement contract tests and their private injected authorities.
//!
//! This test-only module is an explicit deep-module exception: its barriers,
//! saga store, capability providers, fixture construction, and ten behavioral
//! proofs jointly own one lifecycle contract and do not serve sibling tests.

use std::collections::BTreeSet;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_compute::config::control_plane::ControlPlaneConfig;
use nimbus_compute::config::deployment::DeploymentConfig;
use nimbus_compute::config::node_services::NodeServicesConfig;
use nimbus_compute::config::runtime::RuntimeGovernorConfig;
use nimbus_compute::state::{ComputeState, ComputeStateConfig, ComputeWorkloadComposition};
use nimbus_compute::workload_saga::provision_provider::{
    ProviderProvisionEffectObservation, ProviderProvisionPhaseAdapter,
};
use nimbus_compute::workload_saga::{
    ConfirmedWorkloadProvisionCommand, ConfirmedWorkloadTeardownCommand,
    ExactWorkloadTeardownCapabilityRealm, FinalIngressWithdrawalCapability,
    IngressProvisionCapabilities, IngressPublicationCapability,
    IngressPublicationInspectionCapability, IngressTeardownCapabilities,
    NetworkAttachmentCapability, NetworkAttachmentProvisionCapabilities,
    NetworkAttachmentTeardownCapabilities, NetworkDetachmentCapability, NetworkReleaseCapability,
    NetworkReservationCapability, WorkloadActivationCapability,
    WorkloadActivationPrerequisiteCapability, WorkloadExecutionDrainCapability,
    WorkloadExecutionProvisionCapabilities, WorkloadExecutionStopCapability,
    WorkloadExecutionTeardownCapabilities, WorkloadPreparationCapability,
    WorkloadProvisionCapabilityFuture, WorkloadProvisionCapabilityRegistry,
    WorkloadProvisionDecision, WorkloadProvisionSourceAuthority, WorkloadReadinessCapability,
    WorkloadRestartCapabilityRegistry, WorkloadTeardownCapabilityFuture,
    WorkloadTeardownCapabilityRegistry, WorkloadTeardownExecuteOutcome,
    WorkloadTeardownInspectOutcome, WorkloadTeardownProviderObservation,
    WorkloadTeardownProviderOutcome,
};
use nimbus_compute::{
    ServiceManagerWorkloadProjectionSink, ServiceManagerWorkloadProvisionSourceAuthority,
    WorkloadExecutionObservationCapability, WorkloadExecutionObservationFuture,
    WorkloadExecutionObservationRequest, WorkloadIngressObservationCapability,
    WorkloadIngressObservationFuture, WorkloadIngressObservationRequest, WorkloadProjectionSink,
    WorkloadProviderObservation,
};
use nimbus_network::{
    LocalNetworkManager, NetworkAddressFamily, NetworkAttachmentProviderRegistration,
    NetworkCapabilityBundle, NetworkCapabilityRegistry, NetworkCapabilityRequirements,
    NetworkControlPlaneLocality, NetworkLifecycleCapabilitySet, NetworkProviderId,
    NetworkResourceGeneration, NetworkSovereigntyCapabilities, NetworkSovereigntyRequirements,
    NetworkTlsBehavior,
};
use nimbus_sandbox::{
    ProviderCommandAttemptJournal, SandboxExecutionAttemptId, SandboxInspection, SandboxPortBinding,
};
use nimbus_services::{ServiceDefinitionSource, SessionLifecycleState, SessionTarget};
use nimbus_tenant::{TenantIsolationContext, WorkloadLocation};
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity,
    WorkloadActivationIntent, WorkloadAdmissionEvidence, WorkloadExecutionProviderId,
    WorkloadFailureEvidence, WorkloadGeneration, WorkloadNetworkAttachmentBlueprint,
    WorkloadNetworkEndpointSemantics, WorkloadNetworkForwardingBehavior, WorkloadNetworkIntent,
    WorkloadNetworkListenerBlueprint, WorkloadNetworkPlanContent, WorkloadNetworkPlanIdentity,
    WorkloadNetworkPortRequestMode, WorkloadOwnerEvidenceDigest, WorkloadProvisionDisposition,
    WorkloadProvisionEffectResult, WorkloadProvisionSourceIdentity, WorkloadProvisionStep,
    WorkloadProvisionSubjects, WorkloadProvisionSuccessEvidence, WorkloadPublicationIntent,
    WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaFuture, WorkloadSagaIntent,
    WorkloadSagaKey, WorkloadSagaPage, WorkloadSagaPageRequest, WorkloadSagaPhase,
    WorkloadSagaRecord, WorkloadSagaStore, WorkloadSagaStoreError, WorkloadSagaTenantPage,
    WorkloadSagaTenantPageRequest, WorkloadTeardownCommandMode, WorkloadTeardownStep,
    WorkloadTeardownSubjects, WorkloadTeardownSuccessEvidence,
};
use tokio::sync::Semaphore;

use super::*;

const SERVICE_NAME: &str = "retirement-worker";
const ORDERED_TEARDOWN_STEPS: [WorkloadTeardownStep; 5] = [
    WorkloadTeardownStep::WithdrawPublication,
    WorkloadTeardownStep::DrainExecution,
    WorkloadTeardownStep::StopExecution,
    WorkloadTeardownStep::DetachNetwork,
    WorkloadTeardownStep::ReleaseNetwork,
];

struct SemanticBarrier {
    entered: Semaphore,
    release: Semaphore,
}

impl Default for SemanticBarrier {
    fn default() -> Self {
        Self {
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
        }
    }
}

impl SemanticBarrier {
    async fn enter_and_wait(&self) {
        self.entered.add_permits(1);
        self.release
            .acquire()
            .await
            .expect("semantic barrier release should remain open")
            .forget();
    }

    async fn wait_until_entered(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(2), self.entered.acquire())
            .await
            .expect("semantic barrier should be entered before timeout")
            .expect("semantic barrier entry should remain open")
            .forget();
    }

    fn release(&self) {
        self.release.add_permits(1);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StoreFault {
    None,
    AmbiguousStoppedSuccessorBeforeApply,
}

struct ControlledSagaStore {
    record: Mutex<Option<WorkloadSagaRecord>>,
    load_calls: AtomicUsize,
    compare_and_swap_calls: AtomicUsize,
    fault: Mutex<StoreFault>,
    recorded_barrier: Mutex<Option<Arc<SemanticBarrier>>>,
    recorded_barrier_used: AtomicBool,
    stale_load_barrier: Mutex<Option<Arc<SemanticBarrier>>>,
    stale_load_used: AtomicBool,
}

impl ControlledSagaStore {
    fn new(record: Option<WorkloadSagaRecord>) -> Arc<Self> {
        Arc::new(Self {
            record: Mutex::new(record),
            load_calls: AtomicUsize::new(0),
            compare_and_swap_calls: AtomicUsize::new(0),
            fault: Mutex::new(StoreFault::None),
            recorded_barrier: Mutex::new(None),
            recorded_barrier_used: AtomicBool::new(false),
            stale_load_barrier: Mutex::new(None),
            stale_load_used: AtomicBool::new(false),
        })
    }

    fn record(&self) -> Option<WorkloadSagaRecord> {
        self.record
            .lock()
            .expect("controlled saga store lock should remain healthy")
            .clone()
    }

    fn replace(&self, record: WorkloadSagaRecord) {
        *self
            .record
            .lock()
            .expect("controlled saga store lock should remain healthy") = Some(record);
    }

    fn call_counts(&self) -> (usize, usize) {
        (
            self.load_calls.load(Ordering::Acquire),
            self.compare_and_swap_calls.load(Ordering::Acquire),
        )
    }

    fn fail_stopped_successor_ambiguously(&self) {
        *self
            .fault
            .lock()
            .expect("controlled saga fault lock should remain healthy") =
            StoreFault::AmbiguousStoppedSuccessorBeforeApply;
    }

    fn block_recorded_after_apply(&self, barrier: Arc<SemanticBarrier>) {
        *self
            .recorded_barrier
            .lock()
            .expect("controlled recorded barrier lock should remain healthy") = Some(barrier);
    }

    fn block_first_load_with_stale_snapshot(&self, barrier: Arc<SemanticBarrier>) {
        *self
            .stale_load_barrier
            .lock()
            .expect("controlled load barrier lock should remain healthy") = Some(barrier);
    }
}

impl WorkloadSagaStore for ControlledSagaStore {
    fn load<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            self.load_calls.fetch_add(1, Ordering::AcqRel);
            let snapshot = self.record();
            if snapshot.as_ref().is_some_and(|record| record.key() != key) {
                return Err(WorkloadSagaStoreError::Corrupt);
            }
            let barrier = self
                .stale_load_barrier
                .lock()
                .expect("controlled load barrier lock should remain healthy")
                .clone();
            if let Some(barrier) = barrier
                && !self.stale_load_used.swap(true, Ordering::AcqRel)
            {
                barrier.enter_and_wait().await;
            }
            Ok(snapshot)
        })
    }

    fn compare_and_swap<'a>(
        &'a self,
        expected: WorkloadSagaExpected,
        next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            self.compare_and_swap_calls.fetch_add(1, Ordering::AcqRel);
            let stopped_successor = next
                .successor_intent()
                .is_some_and(|intent| intent.desired_state() == DesiredWorkloadState::Stopped);
            if stopped_successor {
                let mut fault = self
                    .fault
                    .lock()
                    .expect("controlled saga fault lock should remain healthy");
                if *fault == StoreFault::AmbiguousStoppedSuccessorBeforeApply {
                    *fault = StoreFault::None;
                    return Err(WorkloadSagaStoreError::Ambiguous);
                }
            }

            let changed = {
                let mut current = self
                    .record
                    .lock()
                    .expect("controlled saga store lock should remain healthy");
                if current.as_ref() == Some(&next) {
                    false
                } else {
                    let matches = match (expected, current.as_ref()) {
                        (WorkloadSagaExpected::Missing, None) => true,
                        (WorkloadSagaExpected::Revision(expected), Some(record)) => {
                            expected == record.revision()
                        }
                        _ => false,
                    };
                    if !matches {
                        return Err(WorkloadSagaStoreError::Conflict {
                            expected,
                            observed: current.as_ref().map(WorkloadSagaRecord::revision),
                        });
                    }
                    *current = Some(next.clone());
                    true
                }
            };

            if changed
                && next.phase() == WorkloadSagaPhase::Recorded
                && !self.recorded_barrier_used.swap(true, Ordering::AcqRel)
            {
                let barrier = self
                    .recorded_barrier
                    .lock()
                    .expect("controlled recorded barrier lock should remain healthy")
                    .clone();
                if let Some(barrier) = barrier {
                    barrier.enter_and_wait().await;
                }
            }
            Ok(if changed {
                WorkloadSagaCommit::Applied
            } else {
                WorkloadSagaCommit::Unchanged
            })
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
    ) -> WorkloadSagaFuture<'a, nimbus_workloads::WorkloadRestartCandidatePage> {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TeardownBehavior {
    Succeed,
    DefiniteFailureAt(WorkloadTeardownStep),
    AmbiguousAt(WorkloadTeardownStep),
}

struct RecordingTeardownProvider {
    behavior: TeardownBehavior,
    calls: Mutex<Vec<(WorkloadTeardownStep, WorkloadTeardownCommandMode)>>,
}

impl RecordingTeardownProvider {
    fn new(behavior: TeardownBehavior) -> Arc<Self> {
        Arc::new(Self {
            behavior,
            calls: Mutex::new(Vec::new()),
        })
    }

    fn calls(&self) -> Vec<(WorkloadTeardownStep, WorkloadTeardownCommandMode)> {
        self.calls
            .lock()
            .expect("teardown call log should remain healthy")
            .clone()
    }

    fn observation(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownProviderObservation {
        self.calls
            .lock()
            .expect("teardown call log should remain healthy")
            .push((command.step(), command.mode()));
        let ambiguous = self.behavior == TeardownBehavior::AmbiguousAt(command.step());
        if ambiguous {
            let outcome = match command.mode() {
                WorkloadTeardownCommandMode::Execute => WorkloadTeardownProviderOutcome::Execute(
                    WorkloadTeardownExecuteOutcome::Ambiguous,
                ),
                WorkloadTeardownCommandMode::Inspect => WorkloadTeardownProviderOutcome::Inspect(
                    WorkloadTeardownInspectOutcome::Ambiguous,
                ),
            };
            return WorkloadTeardownProviderObservation::for_command(command, outcome);
        }
        let failing = self.behavior == TeardownBehavior::DefiniteFailureAt(command.step());
        let outcome = match (command.mode(), failing) {
            (WorkloadTeardownCommandMode::Execute, false) => {
                WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Succeeded(
                    Box::new(teardown_success(command.step(), command.subjects())),
                ))
            }
            (WorkloadTeardownCommandMode::Execute, true) => {
                WorkloadTeardownProviderOutcome::Execute(
                    WorkloadTeardownExecuteOutcome::DefiniteFailure(
                        WorkloadFailureEvidence::new(
                            "definition_retirement_cleanup_pending",
                            WorkloadOwnerEvidenceDigest::sha256("definition-retirement-failure"),
                        )
                        .expect("fixture failure evidence should validate"),
                    ),
                )
            }
            (WorkloadTeardownCommandMode::Inspect, false) => {
                WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::Satisfied(
                    Box::new(teardown_success(command.step(), command.subjects())),
                ))
            }
            (WorkloadTeardownCommandMode::Inspect, true) => {
                WorkloadTeardownProviderOutcome::Inspect(
                    WorkloadTeardownInspectOutcome::DefiniteFailure(
                        WorkloadFailureEvidence::new(
                            "definition_retirement_cleanup_pending",
                            WorkloadOwnerEvidenceDigest::sha256("definition-retirement-failure"),
                        )
                        .expect("fixture failure evidence should validate"),
                    ),
                )
            }
        };
        WorkloadTeardownProviderObservation::for_command(command, outcome)
    }
}

macro_rules! teardown_capability {
    ($trait_name:ident) => {
        impl $trait_name for RecordingTeardownProvider {
            fn execute<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadTeardownCommand,
            ) -> WorkloadTeardownCapabilityFuture<'a> {
                Box::pin(async move { self.observation(command) })
            }

            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadTeardownCommand,
            ) -> WorkloadTeardownCapabilityFuture<'a> {
                Box::pin(async move { self.observation(command) })
            }
        }
    };
}

teardown_capability!(FinalIngressWithdrawalCapability);
teardown_capability!(WorkloadExecutionDrainCapability);
teardown_capability!(WorkloadExecutionStopCapability);
teardown_capability!(NetworkDetachmentCapability);
teardown_capability!(NetworkReleaseCapability);

struct ControlledProvisionProvider {
    backend: Arc<ReadySandboxBackend>,
    phases: ProviderProvisionPhaseAdapter,
    activation_barrier: Option<Arc<SemanticBarrier>>,
    activation_blocked: AtomicBool,
}

impl ControlledProvisionProvider {
    fn new(
        backend: Arc<ReadySandboxBackend>,
        journal: ProviderCommandAttemptJournal,
        activation_barrier: Option<Arc<SemanticBarrier>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            backend,
            phases: ProviderProvisionPhaseAdapter::new(journal),
            activation_barrier,
            activation_blocked: AtomicBool::new(false),
        })
    }

    fn execute_success(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> nimbus_workloads::WorkloadProvisionInspectionResult {
        self.phases
            .execute(command, || ProviderProvisionEffectObservation::Succeeded {
                evidence: format!("definition-retirement:{:?}", command.step()).into_bytes(),
            })
    }

    fn inspect_success(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> nimbus_workloads::WorkloadProvisionInspectionResult {
        self.phases
            .inspect(command, || ProviderProvisionEffectObservation::Succeeded {
                evidence: format!("definition-retirement:{:?}", command.step()).into_bytes(),
            })
    }

    fn activate(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> ProviderProvisionEffectObservation {
        let spec =
            match nimbus_compute::workload_executable::decode_sandbox_spec(command.executable()) {
                Ok(spec) => spec,
                Err(error) => {
                    return ProviderProvisionEffectObservation::DefiniteFailure {
                        code: "definition_retirement_executable_invalid".to_owned(),
                        evidence: error.to_string().into_bytes(),
                    };
                }
            };
        let sandbox_id = SandboxId::new(command.execution().execution_id().as_str());
        match self.backend.activate_for_test(spec, sandbox_id.clone()) {
            Ok(handle) if handle.id == sandbox_id => {
                ProviderProvisionEffectObservation::Succeeded {
                    evidence: b"definition-retirement-activated".to_vec(),
                }
            }
            Ok(handle) => ProviderProvisionEffectObservation::DefiniteFailure {
                code: "definition_retirement_activation_crossed".to_owned(),
                evidence: handle.id.as_str().as_bytes().to_vec(),
            },
            Err(error) => ProviderProvisionEffectObservation::DefiniteFailure {
                code: "definition_retirement_activation_failed".to_owned(),
                evidence: error.to_string().into_bytes(),
            },
        }
    }

    fn inspect_activation(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> ProviderProvisionEffectObservation {
        let sandbox_id = SandboxId::new(command.execution().execution_id().as_str());
        if self
            .backend
            .activated_handle_for_test(&sandbox_id)
            .is_some()
        {
            ProviderProvisionEffectObservation::Succeeded {
                evidence: b"definition-retirement-activation-present".to_vec(),
            }
        } else {
            ProviderProvisionEffectObservation::Absent {
                evidence: b"definition-retirement-activation-absent".to_vec(),
            }
        }
    }
}

macro_rules! provision_effect_capability {
    ($trait_name:ident) => {
        impl $trait_name for ControlledProvisionProvider {
            fn execute<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.execute_success(command) })
            }

            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.inspect_success(command) })
            }
        }
    };
}

provision_effect_capability!(NetworkReservationCapability);
provision_effect_capability!(WorkloadPreparationCapability);
provision_effect_capability!(NetworkAttachmentCapability);
provision_effect_capability!(IngressPublicationCapability);

impl WorkloadActivationPrerequisiteCapability for ControlledProvisionProvider {
    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        Box::pin(async move { self.inspect_success(command) })
    }
}

impl WorkloadActivationCapability for ControlledProvisionProvider {
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        Box::pin(async move {
            if let Some(barrier) = self.activation_barrier.as_ref()
                && !self.activation_blocked.swap(true, Ordering::AcqRel)
            {
                barrier.enter_and_wait().await;
            }
            self.phases.execute(command, || self.activate(command))
        })
    }

    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        Box::pin(async move {
            self.phases
                .inspect(command, || self.inspect_activation(command))
        })
    }
}

impl WorkloadReadinessCapability for ControlledProvisionProvider {
    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        Box::pin(async move {
            self.phases
                .inspect(command, || self.inspect_activation(command))
        })
    }
}

impl IngressPublicationInspectionCapability for ControlledProvisionProvider {
    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        Box::pin(async move { self.inspect_success(command) })
    }
}

impl WorkloadExecutionObservationCapability for ControlledProvisionProvider {
    fn observe<'a>(
        &'a self,
        request: &'a WorkloadExecutionObservationRequest,
    ) -> WorkloadExecutionObservationFuture<'a> {
        Box::pin(async move {
            let sandbox_id = SandboxId::new(request.execution().execution_id().as_str());
            self.backend
                .activated_handle_for_test(&sandbox_id)
                .map(|handle| {
                    SandboxInspection::provider_authenticated_running(
                        handle,
                        SandboxExecutionAttemptId::new(
                            request.execution().attempt_id().to_string(),
                        )
                        .expect("fixture execution attempt should validate"),
                        b"definition-retirement-provider",
                    )
                })
                .map(WorkloadProviderObservation::Present)
                .unwrap_or(WorkloadProviderObservation::Absent)
        })
    }
}

impl WorkloadIngressObservationCapability for ControlledProvisionProvider {
    fn observe<'a>(
        &'a self,
        request: &'a WorkloadIngressObservationRequest,
    ) -> WorkloadIngressObservationFuture<'a> {
        Box::pin(async move {
            if request.compiled_plan().content().listeners().is_empty() {
                WorkloadProviderObservation::Present(Vec::new())
            } else {
                WorkloadProviderObservation::Ambiguous
            }
        })
    }
}

enum InitialSaga {
    Missing,
    PendingProvision,
    Observed,
}

struct RetirementHarness {
    _temp: tempfile::TempDir,
    compute: Arc<ComputeState>,
    manager: Arc<ServiceManager>,
    store: Arc<ControlledSagaStore>,
    teardown: Arc<RecordingTeardownProvider>,
    tenant_id: TenantId,
    definition: ServiceDefinition,
    observed_record: WorkloadSagaRecord,
}

impl RetirementHarness {
    async fn new(
        published: bool,
        initial: InitialSaga,
        teardown_behavior: TeardownBehavior,
        activation_barrier: Option<Arc<SemanticBarrier>>,
    ) -> Self {
        let temp = tempfile::tempdir().expect("retirement harness tempdir should create");
        let engine = Arc::new(Engine::new(temp.path()).expect("retirement harness engine creates"));
        let backend = Arc::new(ReadySandboxBackend::default());
        let manager = service_manager(backend.clone());
        let tenant_id = TenantId::new("definition-retirement-tenant")
            .expect("retirement tenant should validate");
        let mut spec = SandboxSpec::new(
            tenant_id.clone(),
            SandboxOwnerSpec::service(SERVICE_NAME),
            SandboxBackendKind::Krun,
            SandboxRootSpec::oci_image_reference("registry.example.com/worker:latest"),
            SandboxProcessSpec::new(vec!["worker".to_owned()]),
        );
        if published {
            spec = spec.with_port_binding(SandboxPortBinding::new(
                "api",
                EndpointProtocol::Tcp,
                15432,
                5432,
            ));
        }
        let definition = manager
            .create_service_definition(
                &tenant_id,
                SERVICE_NAME,
                ServiceBackend::sandbox(spec.clone()),
                BTreeMap::new(),
            )
            .expect("retirement service definition should create");
        let key = WorkloadSagaKey::new(
            tenant_id.clone(),
            nimbus_core::WorkloadId::new(SERVICE_NAME)
                .expect("service workload id should validate"),
        );
        let source_authority = Arc::new(ServiceManagerWorkloadProvisionSourceAuthority::new(
            manager.clone(),
        ));
        let source_identity = WorkloadProvisionSourceIdentity::sandbox_backed_service(SERVICE_NAME)
            .expect("service source identity should validate");
        let source = source_authority
            .current_source(&key, &source_identity)
            .await
            .expect("services should return exact source evidence");
        let bundle = provider_bundle(source.attachment_provider_id().clone());
        let selection = bundle.selection();
        let intent =
            running_service_intent(&tenant_id, &definition, &spec, source, &bundle, published);
        let reports = NetworkCapabilityRegistry::new([bundle])
            .expect("retirement provider reports should validate");
        let initial_record = WorkloadSagaRecord::new(key, intent)
            .expect("retirement initial saga record should validate");
        let pending_record = first_pending_provision(&initial_record);
        let observed_record = finish_provision(initial_record);
        let stored = match initial {
            InitialSaga::Missing => None,
            InitialSaga::PendingProvision => Some(pending_record),
            InitialSaga::Observed => Some(observed_record.clone()),
        };
        let store = ControlledSagaStore::new(stored);
        let teardown = RecordingTeardownProvider::new(teardown_behavior);
        let journal = ProviderCommandAttemptJournal::open(
            temp.path().join("provider-journal"),
            "definition-retirement-provider",
        )
        .expect("retirement provider journal should open");
        let provision =
            ControlledProvisionProvider::new(backend.clone(), journal, activation_barrier);
        let execution_provider_id = source_identity_execution_provider(&observed_record);
        let provision_capabilities = WorkloadProvisionCapabilityRegistry::new(
            [NetworkAttachmentProvisionCapabilities::new(
                selection.attachment_provider_id().clone(),
                provision.clone(),
            )],
            [WorkloadExecutionProvisionCapabilities::new(
                execution_provider_id.clone(),
                provision.clone(),
            )],
            [IngressProvisionCapabilities::new(
                selection.ingress_provider_id().clone(),
                provision,
            )],
        )
        .expect("retirement provision registry should validate");
        let teardown_capabilities = WorkloadTeardownCapabilityRegistry::new(
            [NetworkAttachmentTeardownCapabilities::new(
                selection.attachment_provider_id().clone(),
                teardown.clone(),
                teardown.clone(),
            )],
            [WorkloadExecutionTeardownCapabilities::new(
                execution_provider_id.clone(),
                teardown.clone(),
                teardown.clone(),
            )],
            [IngressTeardownCapabilities::new(
                selection.ingress_provider_id().clone(),
                teardown.clone(),
            )],
        )
        .expect("retirement teardown registry should validate");
        let teardown_capabilities = ExactWorkloadTeardownCapabilityRealm::new(
            teardown_capabilities,
            &selection,
            &execution_provider_id,
        )
        .expect("retirement exact teardown realm should validate");
        let network_manager = LocalNetworkManager::bootstrap(temp.path().join("network"))
            .expect("retirement network authority should bootstrap")
            .freeze(reports);
        let projection_sink: Arc<dyn WorkloadProjectionSink> =
            Arc::new(ServiceManagerWorkloadProjectionSink::new(manager.clone()));
        let saga_store: Arc<dyn WorkloadSagaStore> = store.clone();
        let source_owner: Arc<dyn nimbus_compute::workload_saga::WorkloadProvisionSourceAuthority> =
            source_authority;
        let compute = Arc::new(ComputeState::from_config(ComputeStateConfig {
            engine,
            workload_composition: ComputeWorkloadComposition::Managed {
                network_manager,
                local_node: NodeIdentity::new("definition-retirement-node")
                    .expect("fixture node should validate"),
                capability_selection: Box::new(selection),
                execution_provider_id,
                sovereignty: NetworkSovereigntyRequirements::new(
                    NetworkControlPlaneLocality::LocalOnly,
                    BTreeSet::new(),
                    true,
                ),
                saga_store,
                source_authority: source_owner,
                provision_capabilities: Box::new(provision_capabilities),
                restart_capabilities: Box::new(
                    WorkloadRestartCapabilityRegistry::new([])
                        .expect("empty restart registry should validate"),
                ),
                teardown_capabilities: Some(Box::new(teardown_capabilities)),
                desire_admission_guard: None,
                projection_sink,
            },
            deployment: DeploymentConfig::default(),
            control_plane: ControlPlaneConfig::router_options_default(),
            node_services: NodeServicesConfig::default().with_service_manager(manager.clone()),
            runtime: RuntimeGovernorConfig::default(),
        }));

        if matches!(initial, InitialSaga::Observed) {
            project_observed_service(&manager, &definition, &observed_record);
        }

        Self {
            _temp: temp,
            compute,
            manager,
            store,
            teardown,
            tenant_id,
            definition,
            observed_record,
        }
    }

    fn context(&self) -> TenantIsolationContext {
        TenantIsolationContext::system(self.tenant_id.clone(), "test.definition.delete")
    }

    async fn open_session(&self) -> String {
        self.manager
            .open_session_async(
                &self.tenant_id,
                SessionTarget::Service {
                    name: SERVICE_NAME.to_owned(),
                },
                vec!["stdio".to_owned()],
                Some(60_000),
            )
            .await
            .expect("retirement fixture session should open")
            .id
    }

    async fn delete(
        &self,
    ) -> Result<ServiceDefinition, nimbus_compute::ComputeResourceRetirementError> {
        self.delete_with_force(true).await
    }

    async fn delete_with_force(
        &self,
        force: bool,
    ) -> Result<ServiceDefinition, nimbus_compute::ComputeResourceRetirementError> {
        self.compute
            .resource_retirer()
            .expect("retirement harness has complete composition")
            .submit_definition_teardown(
                &self.context(),
                SERVICE_NAME,
                self.definition.generation,
                force,
            )
            .await
    }
}

fn provider_bundle(attachment_provider: NetworkProviderId) -> NetworkCapabilityBundle {
    let source_requirements =
        nimbus_sandbox::sandbox_network_plan_requirements(SandboxBackendKind::Krun);
    assert_eq!(
        &attachment_provider,
        source_requirements.required_attachment_provider_id(),
        "retirement attachment identity should match the sandbox source"
    );
    let lifecycle = NetworkLifecycleCapabilitySet::new([
        nimbus_network::NetworkLifecycleFeature::DurableInspect,
        nimbus_network::NetworkLifecycleFeature::Reconcile,
        nimbus_network::NetworkLifecycleFeature::Delete,
    ]);
    NetworkCapabilityBundle::new(
        NetworkAttachmentProviderRegistration::new(
            attachment_provider,
            source_requirements
                .capability_requirements()
                .attachment()
                .clone(),
            [NetworkAddressFamily::Ipv4, NetworkAddressFamily::Ipv6],
            lifecycle,
            NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
        ),
        crate::nimbus_owned_workload_ingress_registration(),
    )
}

fn running_service_intent(
    tenant_id: &TenantId,
    definition: &ServiceDefinition,
    spec: &SandboxSpec,
    source: nimbus_workloads::WorkloadProvisionSourceEvidence,
    bundle: &NetworkCapabilityBundle,
    published: bool,
) -> WorkloadSagaIntent {
    let executable = nimbus_compute::workload_executable::encode_sandbox_spec(spec)
        .expect("retirement sandbox spec should encode");
    let selection = bundle.selection();
    let identity = WorkloadNetworkPlanIdentity::new(
        tenant_id.clone(),
        format!("definition-retirement-{}", definition.name),
        NetworkResourceGeneration::new(definition.generation),
    )
    .expect("retirement network identity should validate");
    let attachment = WorkloadNetworkAttachmentBlueprint::new(&identity, "default")
        .expect("retirement attachment should validate");
    let requirements = NetworkCapabilityRequirements::new(
        bundle.attachment().attachment().clone(),
        bundle.ingress().endpoint().clone(),
        bundle.ingress().ingress().clone(),
        bundle.ingress().forwarding().clone(),
        nimbus_network::NetworkLifecycleRequirements::new(
            bundle.attachment().lifecycle().clone(),
            bundle.ingress().lifecycle().clone(),
        ),
        NetworkSovereigntyRequirements::new(
            NetworkControlPlaneLocality::LocalOnly,
            BTreeSet::new(),
            true,
        ),
    );
    let listeners = if published {
        vec![
            WorkloadNetworkListenerBlueprint::new(
                &identity,
                "api",
                EndpointProtocol::Tcp,
                std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
                WorkloadNetworkPortRequestMode::ProviderAssigned,
                WorkloadNetworkEndpointSemantics::new(
                    WorkloadNetworkForwardingBehavior::None,
                    NetworkTlsBehavior::Disabled,
                ),
                None,
            )
            .expect("retirement listener should validate"),
        ]
    } else {
        Vec::new()
    };
    let content = WorkloadNetworkPlanContent::new(
        identity,
        requirements,
        Some(selection),
        Some(bundle.selection_evidence()),
        Some(attachment),
        [],
        listeners,
        [],
        WorkloadActivationIntent::ActivateWhenAttached,
        if published {
            WorkloadPublicationIntent::PublishWhenReady
        } else {
            WorkloadPublicationIntent::Withheld
        },
    )
    .expect("retirement network content should validate");
    let compiled = CompiledWorkloadNetworkPlan::from_content(content)
        .expect("retirement compiled plan should validate");
    WorkloadSagaIntent::new_without_automatic_restart(
        DesiredWorkloadKind::Service,
        DesiredWorkloadState::Running,
        WorkloadGeneration::new(definition.generation),
        executable,
        source,
        WorkloadNetworkIntent::new(compiled),
        WorkloadActivationIntent::ActivateWhenAttached,
        if published {
            WorkloadPublicationIntent::PublishWhenReady
        } else {
            WorkloadPublicationIntent::Withheld
        },
        WorkloadAdmissionEvidence::new(
            format!("tid_{}", "1".repeat(64))
                .try_into()
                .expect("fixture decision id should validate"),
            format!("twu_{}", "2".repeat(64))
                .try_into()
                .expect("fixture workload uid should validate"),
            NodeIdentity::new("definition-retirement-node").expect("fixture node should validate"),
        ),
    )
    .expect("retirement running intent should validate")
}

fn first_pending_provision(record: &WorkloadSagaRecord) -> WorkloadSagaRecord {
    let WorkloadProvisionDecision::Proposed(proposed) =
        WorkloadProvisionDecision::plan(record).expect("initial provision should reduce")
    else {
        panic!("initial provision should produce a durable claim")
    };
    proposed.into_candidate()
}

fn finish_provision(mut record: WorkloadSagaRecord) -> WorkloadSagaRecord {
    for _ in 0..32 {
        match WorkloadProvisionDecision::plan(&record)
            .expect("retirement provision fixture should reduce")
        {
            WorkloadProvisionDecision::Wait => {
                assert_eq!(record.phase(), WorkloadSagaPhase::Observed);
                return record;
            }
            WorkloadProvisionDecision::Proposed(proposed) => {
                record = proposed.into_candidate();
                if let Some(WorkloadProvisionDisposition::DispatchPending(claim)) =
                    record.provision_disposition()
                {
                    let result = WorkloadProvisionEffectResult::Succeeded {
                        attempt_id: claim.attempt().attempt_id().clone(),
                        evidence: provision_success(
                            claim.attempt().step(),
                            claim.attempt().subjects(),
                        ),
                    };
                    let WorkloadProvisionDecision::Proposed(completed) =
                        WorkloadProvisionDecision::reduce(&record, result)
                            .expect("retirement provision success should reduce")
                    else {
                        panic!("retirement provision success should persist a candidate")
                    };
                    record = completed.into_candidate();
                }
            }
            WorkloadProvisionDecision::InspectExact(claim) => {
                let result = WorkloadProvisionEffectResult::Succeeded {
                    attempt_id: claim.attempt().attempt_id().clone(),
                    evidence: provision_success(claim.attempt().step(), claim.attempt().subjects()),
                };
                let WorkloadProvisionDecision::Proposed(completed) =
                    WorkloadProvisionDecision::reduce(&record, result)
                        .expect("retirement provision inspection should reduce")
                else {
                    panic!("retirement provision inspection should persist a candidate")
                };
                record = completed.into_candidate();
            }
            WorkloadProvisionDecision::DefiniteFailure => {
                panic!("all-success fixture should not reach definite provision failure")
            }
        }
    }
    panic!("retirement provision fixture exceeded its decision bound")
}

fn provision_success(
    step: WorkloadProvisionStep,
    subjects: &WorkloadProvisionSubjects,
) -> WorkloadProvisionSuccessEvidence {
    let evidence = WorkloadOwnerEvidenceDigest::sha256(format!("definition-retirement-{step:?}"));
    match (step, subjects) {
        (WorkloadProvisionStep::ReserveNetwork, WorkloadProvisionSubjects::Network(reference)) => {
            WorkloadProvisionSuccessEvidence::NetworkReserved {
                reference: reference.clone(),
                evidence,
            }
        }
        (
            WorkloadProvisionStep::PrepareWorkload,
            WorkloadProvisionSubjects::Execution(reference),
        ) => WorkloadProvisionSuccessEvidence::WorkloadPrepared {
            reference: reference.clone(),
            evidence,
        },
        (WorkloadProvisionStep::AttachNetwork, WorkloadProvisionSubjects::Network(reference)) => {
            WorkloadProvisionSuccessEvidence::NetworkAttached {
                reference: reference.clone(),
                evidence,
            }
        }
        (
            WorkloadProvisionStep::InspectActivationPrerequisites,
            WorkloadProvisionSubjects::Readiness { network, execution },
        ) => WorkloadProvisionSuccessEvidence::ActivationPrerequisitesReady {
            network: network.clone(),
            execution: execution.clone(),
            evidence,
        },
        (
            WorkloadProvisionStep::ActivateWorkload,
            WorkloadProvisionSubjects::Execution(reference),
        ) => WorkloadProvisionSuccessEvidence::WorkloadActivated {
            reference: reference.clone(),
            evidence,
        },
        (
            WorkloadProvisionStep::InspectWorkloadReadiness,
            WorkloadProvisionSubjects::Readiness { network, execution },
        ) => WorkloadProvisionSuccessEvidence::WorkloadReady {
            network: network.clone(),
            execution: execution.clone(),
            evidence,
        },
        (WorkloadProvisionStep::Publish, WorkloadProvisionSubjects::Publication(reference)) => {
            WorkloadProvisionSuccessEvidence::Published {
                reference: reference.clone(),
                evidence,
            }
        }
        (
            WorkloadProvisionStep::ObservePublication,
            WorkloadProvisionSubjects::Publication(reference),
        ) => WorkloadProvisionSuccessEvidence::PublicationObserved {
            reference: reference.clone(),
            evidence,
        },
        _ => panic!("retirement provision step and subjects should match"),
    }
}

fn teardown_success(
    step: WorkloadTeardownStep,
    subjects: &WorkloadTeardownSubjects,
) -> WorkloadTeardownSuccessEvidence {
    let evidence = WorkloadOwnerEvidenceDigest::sha256(format!("definition-retirement-{step:?}"));
    match (step, subjects) {
        (
            WorkloadTeardownStep::WithdrawPublication,
            WorkloadTeardownSubjects::Publication(reference),
        ) => WorkloadTeardownSuccessEvidence::PublicationAbsent {
            reference: reference.clone(),
            evidence,
        },
        (WorkloadTeardownStep::DrainExecution, WorkloadTeardownSubjects::Execution(reference)) => {
            WorkloadTeardownSuccessEvidence::ExecutionDrained {
                reference: reference.clone(),
                evidence,
            }
        }
        (WorkloadTeardownStep::StopExecution, WorkloadTeardownSubjects::Execution(reference)) => {
            WorkloadTeardownSuccessEvidence::ExecutionStopped {
                reference: reference.clone(),
                evidence,
            }
        }
        (WorkloadTeardownStep::DetachNetwork, WorkloadTeardownSubjects::Network(reference)) => {
            WorkloadTeardownSuccessEvidence::NetworkDetached {
                reference: reference.clone(),
                evidence,
            }
        }
        (WorkloadTeardownStep::ReleaseNetwork, WorkloadTeardownSubjects::Network(reference)) => {
            WorkloadTeardownSuccessEvidence::NetworkReleased {
                reference: reference.clone(),
                evidence,
            }
        }
        _ => panic!("retirement teardown step and subjects should match"),
    }
}

fn source_identity_execution_provider(record: &WorkloadSagaRecord) -> WorkloadExecutionProviderId {
    record
        .active_intent()
        .source()
        .execution_provider_id()
        .clone()
}

fn project_observed_service(
    manager: &ServiceManager,
    definition: &ServiceDefinition,
    record: &WorkloadSagaRecord,
) {
    let execution = record.current_execution_reference();
    let spec = definition
        .backend
        .sandbox_spec()
        .expect("retirement definition should remain sandbox-backed");
    let handle = ReadySandboxBackend::handle(
        spec,
        SandboxId::new(execution.execution_id().as_str()),
        SandboxStatus::Ready,
    );
    manager
        .project_service_definition_execution_observation(
            &definition.tenant_id,
            &definition.name,
            definition.generation,
            &definition.resource_version,
            &execution,
            handle,
        )
        .expect("retirement observed execution should project");
}

fn assert_source_observation_and_session_retained(harness: &RetirementHarness, session_id: &str) {
    assert_eq!(
        harness
            .manager
            .service_definition_for_tenant(&harness.tenant_id, SERVICE_NAME),
        Some(harness.definition.clone()),
        "definition bytes must remain until exact Recorded finalization"
    );
    let observation = harness
        .manager
        .service_definition_observation_for_tenant(&harness.tenant_id, SERVICE_NAME)
        .expect("exact observation must remain until finalization");
    assert_eq!(
        observation.execution,
        harness.observed_record.current_execution_reference()
    );
    assert_eq!(
        harness
            .manager
            .get_session(&harness.tenant_id, session_id)
            .expect("captured session should remain observable")
            .lifecycle_state,
        SessionLifecycleState::Open
    );
}

fn assert_service_source_reservation_available(harness: &RetirementHarness) {
    let prepared = harness
        .manager
        .prepare_sandbox_service_provision_source(&harness.tenant_id, SERVICE_NAME)
        .expect("definition source should remain readable");
    let decision = harness
        .context()
        .with_deployment_generation(harness.definition.generation)
        .with_workload_location(WorkloadLocation::new().with_node_id("definition-retirement-node"))
        .admit_decision(prepared.policy_input().clone())
        .expect("retained definition source should admit");
    harness
        .manager
        .reserve_sandbox_service_provision_source(&decision, prepared)
        .expect("failed deletion must not retain a source-retirement claim");
}

fn assert_exact_teardown_order(provider: &RecordingTeardownProvider) {
    let calls = provider.calls();
    assert_eq!(
        calls.iter().map(|(step, _)| *step).collect::<Vec<_>>(),
        ORDERED_TEARDOWN_STEPS
    );
    assert!(
        calls
            .iter()
            .all(|(_, mode)| *mode == WorkloadTeardownCommandMode::Execute)
    );
}

fn run_async_test(test: impl Future<Output = ()> + Send + 'static) {
    std::thread::Builder::new()
        .name("nimbus-definition-retirement-test".to_owned())
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("definition retirement fixture runtime should build")
                .block_on(test);
        })
        .expect("definition retirement fixture thread should start")
        .join()
        .expect("definition retirement fixture thread should complete");
}

#[test]
fn definition_delete_keeps_source_and_sessions_until_recorded_teardown() {
    run_async_test(async {
        let harness = Arc::new(
            RetirementHarness::new(true, InitialSaga::Observed, TeardownBehavior::Succeed, None)
                .await,
        );
        let session_id = harness.open_session().await;
        let recorded = Arc::new(SemanticBarrier::default());
        harness.store.block_recorded_after_apply(recorded.clone());

        let deleting = {
            let harness = harness.clone();
            tokio::spawn(async move { harness.delete().await })
        };
        recorded.wait_until_entered().await;

        assert_eq!(
            harness
                .store
                .record()
                .expect("record should remain")
                .phase(),
            WorkloadSagaPhase::Recorded
        );
        assert_source_observation_and_session_retained(&harness, &session_id);
        assert_exact_teardown_order(&harness.teardown);

        recorded.release();
        deleting
            .await
            .expect("definition deletion task should join")
            .expect("recorded deletion should complete");
        assert!(
            harness
                .manager
                .service_definition_for_tenant(&harness.tenant_id, SERVICE_NAME)
                .is_none()
        );
        assert!(
            harness
                .manager
                .service_definition_observation_for_tenant(&harness.tenant_id, SERVICE_NAME)
                .is_none()
        );
        let session = harness
            .manager
            .get_session(&harness.tenant_id, &session_id)
            .expect("force-deleted session remains inspectable");
        assert_eq!(session.lifecycle_state, SessionLifecycleState::Closed);
        assert_eq!(
            session.close_reason.as_deref(),
            Some("service_force_deleted")
        );
    });
}

#[test]
fn definition_delete_fences_and_joins_inflight_provision_before_removing_source() {
    run_async_test(async {
        let activation = Arc::new(SemanticBarrier::default());
        let harness = Arc::new(
            RetirementHarness::new(
                false,
                InitialSaga::Missing,
                TeardownBehavior::Succeed,
                Some(activation.clone()),
            )
            .await,
        );
        let provisioning = {
            let harness = harness.clone();
            tokio::spawn(async move {
                harness
                    .compute
                    .resource_provisioner()
                    .expect("fixture has a resource provisioner")
                    .provision_sandbox_service(
                        &harness.context(),
                        SERVICE_NAME,
                        &nimbus_compute::WorkloadProvisionCancellation::default(),
                    )
                    .await
            })
        };
        match tokio::time::timeout(
            std::time::Duration::from_secs(2),
            activation.entered.acquire(),
        )
        .await
        {
            Ok(Ok(permit)) => permit.forget(),
            Ok(Err(_)) => panic!("activation barrier should remain open"),
            Err(_) if provisioning.is_finished() => panic!(
                "provision finished before the activation barrier: {:?}",
                provisioning
                    .await
                    .expect("premature provision task should join")
            ),
            Err(_) => panic!("activation barrier should be entered before timeout"),
        }

        let prepared = harness
            .manager
            .prepare_sandbox_service_provision_source(&harness.tenant_id, SERVICE_NAME)
            .expect("exact provision source should remain readable while retirement fences starts");
        let decision = harness
            .context()
            .with_deployment_generation(harness.definition.generation)
            .with_workload_location(
                WorkloadLocation::new().with_node_id("definition-retirement-node"),
            )
            .admit_decision(prepared.policy_input().clone())
            .expect("retirement fixture source should admit");
        let deleting = {
            let harness = harness.clone();
            tokio::spawn(async move { harness.delete().await })
        };
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                match harness
                    .manager
                    .reserve_sandbox_service_provision_source(&decision, prepared.clone())
                {
                    Ok(_) => tokio::task::yield_now().await,
                    Err(error) if error.to_string().contains("retirement claim") => break,
                    Err(error) => panic!("unexpected provision source error: {error}"),
                }
            }
        })
        .await
        .expect("definition retirement claim should become visible");
        assert!(!deleting.is_finished());
        assert!(harness.teardown.calls().is_empty());
        assert_eq!(
            harness
                .manager
                .service_definition_for_tenant(&harness.tenant_id, SERVICE_NAME),
            Some(harness.definition.clone())
        );

        activation.release();
        provisioning
            .await
            .expect("provision task should join")
            .expect("the exact late provision should complete");
        deleting
            .await
            .expect("delete task should join")
            .expect("delete should retire the joined late success");
        assert!(
            harness
                .manager
                .service_definition_for_tenant(&harness.tenant_id, SERVICE_NAME)
                .is_none()
        );
        assert_exact_teardown_order(&harness.teardown);
    });
}

#[test]
fn force_delete_unresolved_submission_keeps_definition_and_makes_zero_stop_effects() {
    run_async_test(async {
        let harness =
            RetirementHarness::new(true, InitialSaga::Observed, TeardownBehavior::Succeed, None)
                .await;
        let session_id = harness.open_session().await;
        harness.store.fail_stopped_successor_ambiguously();

        let error = harness
            .delete()
            .await
            .expect_err("unresolved durable submission must fail closed");
        assert!(error.to_string().contains("ambiguous"));
        assert_source_observation_and_session_retained(&harness, &session_id);
        assert!(harness.teardown.calls().is_empty());
        assert_eq!(
            harness
                .store
                .record()
                .expect("running saga should remain")
                .phase(),
            WorkloadSagaPhase::Observed
        );
    });
}

#[test]
fn late_provision_result_after_force_delete_is_retired_before_definition_removal() {
    run_async_test(async {
        let harness = Arc::new(
            RetirementHarness::new(
                true,
                InitialSaga::PendingProvision,
                TeardownBehavior::Succeed,
                None,
            )
            .await,
        );
        let load = Arc::new(SemanticBarrier::default());
        harness
            .store
            .block_first_load_with_stale_snapshot(load.clone());
        let deleting = {
            let harness = harness.clone();
            tokio::spawn(async move { harness.delete().await })
        };
        load.wait_until_entered().await;
        project_observed_service(
            &harness.manager,
            &harness.definition,
            &harness.observed_record,
        );
        harness.store.replace(harness.observed_record.clone());
        assert!(
            harness
                .manager
                .service_definition_for_tenant(&harness.tenant_id, SERVICE_NAME)
                .is_some()
        );
        load.release();

        deleting
            .await
            .expect("late-result deletion should join")
            .expect("late provision success should be retired");
        assert_exact_teardown_order(&harness.teardown);
        assert!(
            harness
                .manager
                .service_definition_for_tenant(&harness.tenant_id, SERVICE_NAME)
                .is_none()
        );
    });
}

#[test]
fn definition_delete_cleanup_pending_keeps_definition_observation_and_sessions() {
    run_async_test(async {
        let harness = RetirementHarness::new(
            true,
            InitialSaga::Observed,
            TeardownBehavior::DefiniteFailureAt(WorkloadTeardownStep::DetachNetwork),
            None,
        )
        .await;
        let session_id = harness.open_session().await;

        harness
            .delete()
            .await
            .expect_err("cleanup-pending teardown must not finalize deletion");
        assert_source_observation_and_session_retained(&harness, &session_id);
        assert_eq!(
            harness
                .store
                .record()
                .expect("cleanup-pending saga should remain")
                .phase(),
            WorkloadSagaPhase::CleanupPending
        );
        assert_eq!(
            harness
                .teardown
                .calls()
                .into_iter()
                .map(|(step, _)| step)
                .collect::<Vec<_>>(),
            ORDERED_TEARDOWN_STEPS[..4]
        );
    });
}

#[test]
fn definition_delete_provider_ambiguity_keeps_definition_observation_and_sessions() {
    run_async_test(async {
        let harness = RetirementHarness::new(
            true,
            InitialSaga::Observed,
            TeardownBehavior::AmbiguousAt(WorkloadTeardownStep::DetachNetwork),
            None,
        )
        .await;
        let session_id = harness.open_session().await;

        harness
            .delete()
            .await
            .expect_err("provider ambiguity must remain replayable and must not finalize deletion");

        assert_source_observation_and_session_retained(&harness, &session_id);
        assert_eq!(
            harness.teardown.calls(),
            vec![
                (
                    WorkloadTeardownStep::WithdrawPublication,
                    WorkloadTeardownCommandMode::Execute,
                ),
                (
                    WorkloadTeardownStep::DrainExecution,
                    WorkloadTeardownCommandMode::Execute,
                ),
                (
                    WorkloadTeardownStep::StopExecution,
                    WorkloadTeardownCommandMode::Execute,
                ),
                (
                    WorkloadTeardownStep::DetachNetwork,
                    WorkloadTeardownCommandMode::Execute,
                ),
                (
                    WorkloadTeardownStep::DetachNetwork,
                    WorkloadTeardownCommandMode::Inspect,
                ),
            ]
        );
        assert_ne!(
            harness
                .store
                .record()
                .expect("ambiguous teardown record should remain")
                .phase(),
            WorkloadSagaPhase::Recorded
        );
    });
}

#[test]
fn definition_delete_cancellation_after_submission_is_replayable() {
    run_async_test(async {
        let harness = Arc::new(
            RetirementHarness::new(true, InitialSaga::Observed, TeardownBehavior::Succeed, None)
                .await,
        );
        let session_id = harness.open_session().await;
        let recorded = Arc::new(SemanticBarrier::default());
        harness.store.block_recorded_after_apply(recorded.clone());

        let deleting = {
            let harness = harness.clone();
            tokio::spawn(async move { harness.delete().await })
        };
        recorded.wait_until_entered().await;
        deleting.abort();
        assert!(
            deleting
                .await
                .expect_err("cancelled deletion waiter should abort")
                .is_cancelled()
        );
        assert_source_observation_and_session_retained(&harness, &session_id);
        assert_exact_teardown_order(&harness.teardown);

        recorded.release();
        let removed = harness
            .delete()
            .await
            .expect("exact retry should adopt the retained recorded teardown");
        assert_eq!(removed, harness.definition);
        assert_exact_teardown_order(&harness.teardown);
        assert!(
            harness
                .manager
                .service_definition_for_tenant(&harness.tenant_id, SERVICE_NAME)
                .is_none()
        );
        assert_eq!(
            harness
                .manager
                .get_session(&harness.tenant_id, &session_id)
                .expect("closed replayed session should remain inspectable")
                .lifecycle_state,
            SessionLifecycleState::Closed
        );
    });
}

#[test]
fn non_force_definition_delete_with_open_session_fails_before_mutation() {
    run_async_test(async {
        let harness =
            RetirementHarness::new(true, InitialSaga::Observed, TeardownBehavior::Succeed, None)
                .await;
        let session_id = harness.open_session().await;
        let before = harness
            .store
            .record()
            .expect("observed retirement record should exist");

        let error = harness
            .delete_with_force(false)
            .await
            .expect_err("non-force deletion must reject an open session");
        let nimbus_compute::ComputeResourceRetirementError::Source(nimbus_core::Error::Conflict {
            message,
            ..
        }) = error
        else {
            panic!("non-force session rejection must remain a source-policy conflict: {error:?}")
        };
        assert!(
            message.contains("open sessions"),
            "source-policy conflict should identify the retained open session: {message}"
        );
        assert_eq!(
            harness.store.call_counts(),
            (0, 0),
            "source policy must reject before reading or writing durable saga state"
        );
        assert_eq!(harness.store.record(), Some(before));
        assert!(harness.teardown.calls().is_empty());
        assert_source_observation_and_session_retained(&harness, &session_id);
        assert_service_source_reservation_available(&harness);
    });
}

#[test]
fn unstarted_dynamic_definition_delete_is_effect_free() {
    run_async_test(async {
        let harness =
            RetirementHarness::new(false, InitialSaga::Missing, TeardownBehavior::Succeed, None)
                .await;
        assert!(
            harness
                .manager
                .service_definition_observation_for_tenant(&harness.tenant_id, SERVICE_NAME)
                .is_none()
        );
        let session_error = harness
            .manager
            .open_session_async(
                &harness.tenant_id,
                SessionTarget::Service {
                    name: SERVICE_NAME.to_owned(),
                },
                vec!["stdio".to_owned()],
                Some(60_000),
            )
            .await
            .expect_err("an unobserved sandbox service cannot acquire a session");
        assert!(
            session_error
                .to_string()
                .contains("no observed ready generation")
        );

        let removed = harness
            .delete_with_force(false)
            .await
            .expect("unstarted dynamic definition should delete without provider effects");
        assert_eq!(removed, harness.definition);
        assert_eq!(
            harness.store.call_counts(),
            (1, 0),
            "unstarted deletion requires one read-only missing-saga lookup and no store write"
        );
        assert_eq!(harness.store.record(), None);
        assert!(harness.teardown.calls().is_empty());
        assert!(
            harness
                .manager
                .service_definition_for_tenant(&harness.tenant_id, SERVICE_NAME)
                .is_none()
        );
        assert!(
            harness
                .manager
                .service_definition_observation_for_tenant(&harness.tenant_id, SERVICE_NAME)
                .is_none()
        );
    });
}

#[test]
fn unstarted_static_catalog_definition_delete_returns_static_conflict() {
    run_async_test(async {
        let harness =
            RetirementHarness::new(false, InitialSaga::Missing, TeardownBehavior::Succeed, None)
                .await;
        let definition = harness
            .manager
            .service_definition_for_tenant(&harness.tenant_id, "db")
            .expect("fixture static catalog should expose db");
        assert_eq!(definition.source, ServiceDefinitionSource::StaticCatalog);

        let error = harness
            .compute
            .resource_retirer()
            .expect("retirement harness has complete composition")
            .submit_definition_teardown(&harness.context(), "db", definition.generation, true)
            .await
            .expect_err("static catalog definition must remain undeletable");
        assert!(matches!(
            error,
            nimbus_compute::ComputeResourceRetirementError::Source(
                nimbus_core::Error::Conflict { ref message, .. }
            ) if message.contains("static")
        ));
        assert_eq!(harness.store.call_counts(), (0, 0));
        assert!(harness.teardown.calls().is_empty());
        assert_eq!(
            harness
                .manager
                .service_definition_for_tenant(&harness.tenant_id, "db"),
            Some(definition)
        );
        assert_eq!(
            harness
                .manager
                .service_definition_for_tenant(&harness.tenant_id, SERVICE_NAME),
            Some(harness.definition.clone())
        );
    });
}
