//! End-to-end compute substitution for the forwarded-machine teardown sink.

use std::io::{Read as _, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use nimbus_compute::workload_saga::{
    IngressPublicationCapability, WorkloadSagaCoordinator, WorkloadTeardownCancellationToken,
    WorkloadTeardownCapabilityRegistry, WorkloadTeardownRunDisposition, WorkloadTeardownRuntime,
};
use nimbus_machine::api::{
    MachineApiNetworkReleaseAbsenceEvidence, MachineApiWorkloadTeardownExecuteObservation,
    MachineApiWorkloadTeardownObservation, MachineApiWorkloadTeardownPhaseRequest,
    MachineApiWorkloadTeardownPhaseResponse, MachineApiWorkloadTeardownPhaseResult,
};
use nimbus_network::{NetworkProviderHandle, NetworkResourceGeneration, PortLeasePhase};
use nimbus_sandbox::{
    MachinePortForwardOutcome, MachinePortForwardReceipt, MachinePortForwardingRetirement,
    MachinePortForwardingRetirementObservation, SandboxId, SandboxPortBinding,
};
use nimbus_workloads::{
    DesiredWorkloadState, WorkloadActivationIntent, WorkloadOwnerEvidenceDigest,
    WorkloadPublicationIntent, WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaFuture,
    WorkloadSagaIntent, WorkloadSagaIntentUpdate, WorkloadSagaPage, WorkloadSagaPageRequest,
    WorkloadSagaPhase, WorkloadSagaRecord, WorkloadSagaStore, WorkloadSagaStoreError,
    WorkloadSagaTenantPage, WorkloadSagaTenantPageRequest, WorkloadTeardownCommandMode,
    WorkloadTeardownStep, WorkloadTeardownSubjects, WorkloadTeardownSuccessEvidence,
};

use super::*;
use crate::machine::ForwardedMachineApiSandboxBackend;
use crate::machine::backend::teardown::ForwardedMachineTeardownAdapter;

#[path = "teardown_substitution/process_recovery.rs"]
mod process_recovery;

const GUEST_SERVER_WAIT: Duration = Duration::from_secs(10);

#[test]
fn real_forwarded_teardown_registry_runs_all_five_phases_through_compute_cas() {
    let evidence = teardown_test_runtime().block_on(run_real_forwarded_teardown_registry(1));
    assert_eq!(evidence.initial_port_phases, [PortLeasePhase::Active]);
    assert_eq!(evidence.final_port_phases, [PortLeasePhase::Released]);
    assert_eq!(
        evidence.forwarding_operations,
        ["withdraw", "inspect", "inspect"],
        "parent forwarding must prove withdrawal before guest work and prove absence again at release"
    );
}

#[test]
fn real_forwarded_teardown_registry_inspects_all_five_phases_without_fallback() {
    let evidence = teardown_test_runtime().block_on(run_real_forwarded_teardown_registry_scenario(
        2,
        RegistryTeardownScenario::InspectAfterAmbiguity,
    ));
    assert_eq!(
        evidence.forwarding_operations,
        [
            "withdraw", "inspect", "inspect", "withdraw", "inspect", "inspect"
        ],
        "parent withdrawal must inspect before its adjacent retry"
    );
    assert_eq!(evidence.guest_calls.len(), 8);
    for (index, step) in [
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownStep::StopExecution,
        WorkloadTeardownStep::DetachNetwork,
        WorkloadTeardownStep::ReleaseNetwork,
    ]
    .into_iter()
    .enumerate()
    {
        let execute = &evidence.guest_calls[index * 2];
        let inspect = &evidence.guest_calls[index * 2 + 1];
        assert_eq!(
            (execute.step, execute.mode),
            (step, WorkloadTeardownCommandMode::Execute)
        );
        assert_eq!(
            (inspect.step, inspect.mode),
            (step, WorkloadTeardownCommandMode::Inspect)
        );
        assert_eq!(execute.dispatch_epoch, inspect.dispatch_epoch);
        assert_eq!(execute.prior_receipt_count, index + 1);
        assert_eq!(inspect.prior_receipt_count, index + 1);
    }
}

#[test]
fn forwarded_parent_sibling_matrix_retains_complete_batch_until_exact_absence() {
    let evidence = teardown_test_runtime().block_on(run_real_forwarded_teardown_registry_scenario(
        2,
        RegistryTeardownScenario::InspectAfterAmbiguity,
    ));
    assert_eq!(
        evidence.initial_port_phases,
        [PortLeasePhase::Active, PortLeasePhase::Active]
    );
    assert_eq!(
        evidence.final_port_phases,
        [PortLeasePhase::Released, PortLeasePhase::Released]
    );
    assert_eq!(
        evidence.partial_authority_checks, 2,
        "both the failed Execute and exact Inspect must leave the complete parent batch byte-stable"
    );
}

#[test]
fn forwarded_parent_response_loss_recovers_with_exact_inspect_before_retry() {
    let evidence = teardown_test_runtime().block_on(run_real_forwarded_teardown_registry_scenario(
        2,
        RegistryTeardownScenario::InspectAfterAmbiguity,
    ));
    assert_eq!(
        evidence
            .guest_calls
            .iter()
            .filter(|call| call.mode == WorkloadTeardownCommandMode::Execute)
            .count(),
        4,
        "response loss must not repeat a remote Execute automatically"
    );
    assert_eq!(
        evidence
            .guest_calls
            .iter()
            .filter(|call| call.mode == WorkloadTeardownCommandMode::Inspect)
            .count(),
        4,
        "each lost remote response must recover through exact Inspect"
    );
}

#[test]
fn forwarded_parent_release_requires_exact_guest_and_provider_absence() {
    let evidence = teardown_test_runtime().block_on(run_real_forwarded_teardown_registry_scenario(
        2,
        RegistryTeardownScenario::MissingReleaseAbsence,
    ));
    assert!(!evidence.completed);
    assert_eq!(
        evidence.final_port_phases,
        [
            PortLeasePhase::CleanupPending,
            PortLeasePhase::CleanupPending
        ],
        "a release response without independent absence must retain the complete parent batch"
    );
    assert_eq!(
        evidence
            .guest_calls
            .iter()
            .map(|call| (call.step, call.mode))
            .collect::<Vec<_>>(),
        [
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
                WorkloadTeardownStep::ReleaseNetwork,
                WorkloadTeardownCommandMode::Execute,
            ),
        ]
    );
}

#[test]
fn forwarded_zero_listener_teardown_runs_all_five_phases_without_synthetic_port() {
    let empty = teardown_test_runtime().block_on(run_real_forwarded_teardown_registry(0));
    assert!(empty.initial_port_phases.is_empty());
    assert!(empty.final_port_phases.is_empty());
    assert_eq!(
        empty.forwarding_operations,
        ["inspect", "inspect"],
        "empty withdrawal and release must record explicit absence without a synthetic forwarding member"
    );
}

fn teardown_test_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .thread_stack_size(8 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("real teardown fixture runtime should build")
}

struct RegistryRunEvidence {
    completed: bool,
    initial_port_phases: Vec<PortLeasePhase>,
    final_port_phases: Vec<PortLeasePhase>,
    forwarding_operations: Vec<&'static str>,
    partial_authority_checks: usize,
    guest_calls: Vec<GuestTeardownCall>,
}

async fn run_real_forwarded_teardown_registry(listener_count: usize) -> RegistryRunEvidence {
    run_real_forwarded_teardown_registry_scenario(listener_count, RegistryTeardownScenario::Exact)
        .await
}

#[derive(Clone, Copy)]
enum RegistryTeardownScenario {
    Exact,
    InspectAfterAmbiguity,
    MissingReleaseAbsence,
}

async fn run_real_forwarded_teardown_registry_scenario(
    listener_count: usize,
    scenario: RegistryTeardownScenario,
) -> RegistryRunEvidence {
    let responses = (listener_count > 0).then_some(ResponseMode::Exact(
        MachineApiWorkloadProvisionObservation::Succeeded {
            evidence: b"published-before-teardown".to_vec(),
        },
    ));
    let fixture = Fixture::with_listener_count(responses, listener_count);
    let provision = Arc::new(fixture.adapter(MachineProvider::Krunkit));
    if listener_count == 0 {
        let prepare = fixture
            .command_at_phase(WorkloadSagaPhase::NetworkReserved)
            .await;
        provision
            .validate_exact_phase(
                prepare.command(),
                WorkloadProvisionStep::PrepareWorkload,
                nimbus_workloads::WorkloadProvisionCommandMode::Execute,
            )
            .ok()
            .expect("zero-listener prepare must stage exact empty retirement authority");
    } else {
        let publish = fixture.publish_command().await;
        assert!(matches!(
            IngressPublicationCapability::execute(provision.as_ref(), publish.command()).await,
            WorkloadProvisionInspectionResult::Succeeded { .. }
        ));
    }
    let machine_api_path = fixture.server.socket_path().to_path_buf();
    assert_eq!(
        fixture.server.finish().len(),
        usize::from(listener_count > 0)
    );
    std::fs::remove_file(&machine_api_path)
        .expect("finished provision API socket should unlink before teardown");
    let initial_port_phases = fixture
        .port_authority
        .list_plan(fixture.compiled_plan.plan().plan_id())
        .expect("active parent port batch should remain inspectable")
        .into_iter()
        .map(|record| record.phase())
        .collect::<Vec<_>>();

    let guest = match scenario {
        RegistryTeardownScenario::Exact => ExactGuestTeardownApi::start(machine_api_path, 4),
        RegistryTeardownScenario::InspectAfterAmbiguity => {
            ExactGuestTeardownApi::response_loss_then_inspect(machine_api_path)
        }
        RegistryTeardownScenario::MissingReleaseAbsence => {
            ExactGuestTeardownApi::missing_release_absence(machine_api_path)
        }
    };
    let forwarding = Arc::new(match scenario {
        RegistryTeardownScenario::Exact | RegistryTeardownScenario::MissingReleaseAbsence => {
            RecordingForwarding::new(&fixture.authority, listener_count > 0)
        }
        RegistryTeardownScenario::InspectAfterAmbiguity => {
            RecordingForwarding::partial_then_absent(
                &fixture.authority,
                fixture.port_authority.authority_path(),
            )
        }
    });
    let adapter = Arc::new(
        ForwardedMachineTeardownAdapter::new_for_test(Arc::clone(&provision), forwarding.clone())
            .expect("forwarded teardown adapter should reuse exact parent authorities"),
    );
    let (attachment, execution, ingress) = adapter.registrations().into_parts();
    let registry = WorkloadTeardownCapabilityRegistry::new([attachment], [execution], [ingress])
        .expect("one real forwarded adapter should register all five exact roles once");

    let observed = advance_to_phase(
        WorkloadSagaRecord::new(workload_key(), fixture.intent.clone())
            .expect("fixture saga should validate"),
        WorkloadSagaPhase::Observed,
    );
    let initial = begin_exact_teardown(&observed);
    let store = Arc::new(ExactTeardownStore::new(initial.clone()));
    let runtime = WorkloadTeardownRuntime::new(
        Arc::new(WorkloadSagaCoordinator::new(store.clone())),
        Arc::new(StaticSource(initial.active_intent().source().clone())),
        fixture.provider_reports.clone(),
        Arc::new(registry),
    );

    let run = match runtime
        .submit(
            initial.key().clone(),
            &WorkloadTeardownCancellationToken::new(),
        )
        .await
    {
        Ok(run) => run,
        Err(error) => panic!(
            "the real forwarded registry should complete teardown: {error:?}; cas={}; forwarding={:?}; durable={:#?}",
            store.cas_count(),
            forwarding.operations(),
            store.record(),
        ),
    };

    let completed = run.disposition() == WorkloadTeardownRunDisposition::Completed;
    if matches!(scenario, RegistryTeardownScenario::MissingReleaseAbsence) {
        assert!(
            !completed,
            "unauthenticated release absence must stop the run"
        );
    } else {
        assert!(
            completed,
            "real registry stopped early; cas={}; forwarding={:?}; guest={:?}; durable={:#?}",
            store.cas_count(),
            forwarding.operations(),
            guest.calls(),
            run.record(),
        );
        assert_eq!(run.record().phase(), WorkloadSagaPhase::Recorded);
    }
    assert_eq!(store.record(), *run.record());
    assert!(
        store.cas_count() >= 10,
        "five claims and five results must cross compute's durable CAS"
    );
    let guest_calls = guest.finish();
    if matches!(scenario, RegistryTeardownScenario::Exact) {
        assert_eq!(
            guest_calls.iter().map(|call| call.step).collect::<Vec<_>>(),
            [
                WorkloadTeardownStep::DrainExecution,
                WorkloadTeardownStep::StopExecution,
                WorkloadTeardownStep::DetachNetwork,
                WorkloadTeardownStep::ReleaseNetwork,
            ]
        );
        assert!(
            guest_calls
                .iter()
                .all(|call| call.mode == WorkloadTeardownCommandMode::Execute)
        );
    }
    let final_port_phases = fixture
        .port_authority
        .list_plan(fixture.compiled_plan.plan().plan_id())
        .expect("terminal parent port batch should remain inspectable")
        .into_iter()
        .map(|record| record.phase())
        .collect();
    RegistryRunEvidence {
        completed,
        initial_port_phases,
        final_port_phases,
        forwarding_operations: forwarding.operations(),
        partial_authority_checks: forwarding.partial_authority_checks(),
        guest_calls,
    }
}

#[test]
fn exact_teardown_capabilities_fail_closed_without_provision_authority() {
    let fixture = Fixture::new([]);
    let backend = ForwardedMachineApiSandboxBackend::new_for_test(
        MachineApiClient::new_for_test(fixture.server.socket_path())
            .with_forwarder_authority(fixture.authority.clone()),
        fixture.port_authority.clone(),
    )
    .expect("legacy read backend should open without exact provision composition");

    let error = match backend.teardown_capabilities() {
        Ok(_) => panic!("exact teardown composition must not manufacture missing authority"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("exact provision authority"));
    assert_eq!(fixture.server.finish().len(), 0);
}

fn begin_exact_teardown(observed: &WorkloadSagaRecord) -> WorkloadSagaRecord {
    let active = observed.active_intent();
    let generation = nimbus_workloads::WorkloadGeneration::new(active.generation().as_u64() + 1);
    let active_network = active.network().compiled_plan().content();
    let identity = WorkloadNetworkPlanIdentity::new(
        active_network.identity().tenant_id().clone(),
        active_network.identity().workload_incarnation_key(),
        NetworkResourceGeneration::new(generation.as_u64()),
    )
    .expect("fixture stopped network identity should validate");
    let network = WorkloadNetworkIntent::new(
        CompiledWorkloadNetworkPlan::from_content(
            WorkloadNetworkPlanContent::new(
                identity,
                active_network.capability_requirements().clone(),
                active_network.capability_selection().cloned(),
                active_network.capability_selection_evidence().cloned(),
                active_network.attachment().cloned(),
                active_network.routes().iter().cloned(),
                active_network.listeners().iter().cloned(),
                active_network.dependency_listeners().iter().cloned(),
                WorkloadActivationIntent::PrepareOnly,
                WorkloadPublicationIntent::Withheld,
            )
            .expect("fixture stopped network content should validate"),
        )
        .expect("fixture stopped network plan should compile"),
    );
    let stopped = WorkloadSagaIntent::new_without_automatic_restart(
        active.kind(),
        DesiredWorkloadState::Stopped,
        generation,
        active.executable().clone(),
        active.source().clone(),
        network,
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
        active.admission().clone(),
    )
    .expect("fixture stopped intent should validate");
    let WorkloadSagaIntentUpdate::Transition(record) = observed
        .apply_intent(stopped)
        .expect("higher-generation stopped intent should begin teardown")
    else {
        panic!("higher-generation stopped intent must transition");
    };
    *record
}

struct ExactTeardownStore {
    record: Mutex<WorkloadSagaRecord>,
    cas_count: Mutex<usize>,
}

impl ExactTeardownStore {
    fn new(record: WorkloadSagaRecord) -> Self {
        Self {
            record: Mutex::new(record),
            cas_count: Mutex::new(0),
        }
    }

    fn record(&self) -> WorkloadSagaRecord {
        self.record
            .lock()
            .expect("exact teardown store lock should be healthy")
            .clone()
    }

    fn cas_count(&self) -> usize {
        *self
            .cas_count
            .lock()
            .expect("exact teardown count lock should be healthy")
    }
}

impl WorkloadSagaStore for ExactTeardownStore {
    fn load<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            let record = self.record();
            Ok((record.key() == key).then_some(record))
        })
    }

    fn compare_and_swap<'a>(
        &'a self,
        expected: WorkloadSagaExpected,
        next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            next.validate()?;
            let mut current = self
                .record
                .lock()
                .expect("exact teardown store lock should be healthy");
            if *current == next {
                return Ok(WorkloadSagaCommit::Unchanged);
            }
            let observed = Some(current.revision());
            if expected != WorkloadSagaExpected::Revision(current.revision()) {
                return Err(WorkloadSagaStoreError::Conflict { expected, observed });
            }
            *current = next;
            *self
                .cas_count
                .lock()
                .expect("exact teardown count lock should be healthy") += 1;
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

struct RecordingForwarding {
    provider_instance: NetworkProviderHandle,
    provider_generation: NetworkResourceGeneration,
    state: Mutex<RecordingForwardingState>,
    partial_once: bool,
    authority_path: Option<PathBuf>,
    authority_before: Option<Vec<u8>>,
    partial_authority_checks: Mutex<usize>,
    operations: Mutex<Vec<&'static str>>,
}

#[derive(Clone, Copy)]
enum RecordingForwardingState {
    Present,
    Partial,
    Absent,
}

impl RecordingForwarding {
    fn new(authority: &MachineForwarderAuthority, present: bool) -> Self {
        Self {
            provider_instance: authority.provider_instance().clone(),
            provider_generation: authority.generation(),
            state: Mutex::new(if present {
                RecordingForwardingState::Present
            } else {
                RecordingForwardingState::Absent
            }),
            partial_once: false,
            authority_path: None,
            authority_before: None,
            partial_authority_checks: Mutex::new(0),
            operations: Mutex::new(Vec::new()),
        }
    }

    fn partial_then_absent(
        authority: &MachineForwarderAuthority,
        authority_path: &std::path::Path,
    ) -> Self {
        Self {
            provider_instance: authority.provider_instance().clone(),
            provider_generation: authority.generation(),
            state: Mutex::new(RecordingForwardingState::Present),
            partial_once: true,
            authority_path: Some(authority_path.to_path_buf()),
            authority_before: Some(
                std::fs::read(authority_path)
                    .expect("partial forwarding fixture should snapshot parent authority"),
            ),
            partial_authority_checks: Mutex::new(0),
            operations: Mutex::new(Vec::new()),
        }
    }

    fn operations(&self) -> Vec<&'static str> {
        self.operations
            .lock()
            .expect("forwarding operation log should be healthy")
            .clone()
    }

    fn partial_authority_checks(&self) -> usize {
        *self
            .partial_authority_checks
            .lock()
            .expect("partial authority count should be healthy")
    }

    fn assert_partial_authority_unchanged(&self) {
        let (Some(path), Some(expected)) = (&self.authority_path, &self.authority_before) else {
            return;
        };
        assert_eq!(
            std::fs::read(path).expect("partial parent authority should remain readable"),
            *expected,
            "partial forwarding absence must not mutate any parent lease sibling"
        );
        *self
            .partial_authority_checks
            .lock()
            .expect("partial authority count should be healthy") += 1;
    }

    fn receipts(
        &self,
        outcome: MachinePortForwardOutcome,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
    ) -> Vec<MachinePortForwardReceipt> {
        bindings
            .iter()
            .cloned()
            .map(|binding| MachinePortForwardReceipt {
                outcome,
                tenant_id: tenant_id.clone(),
                sandbox_id: sandbox_id.clone(),
                binding,
                provider_instance: self.provider_instance.clone(),
                provider_generation: self.provider_generation,
            })
            .collect()
    }
}

impl MachinePortForwardingRetirement for RecordingForwarding {
    fn provider_instance(&self) -> &NetworkProviderHandle {
        &self.provider_instance
    }

    fn provider_generation(&self) -> NetworkResourceGeneration {
        self.provider_generation
    }

    fn inspect_batch(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
    ) -> nimbus_sandbox::Result<MachinePortForwardingRetirementObservation> {
        self.operations
            .lock()
            .expect("forwarding operation log should be healthy")
            .push("inspect");
        match *self
            .state
            .lock()
            .expect("forwarding state lock should be healthy")
        {
            RecordingForwardingState::Present => Ok(
                MachinePortForwardingRetirementObservation::Present(self.receipts(
                    MachinePortForwardOutcome::Exposed,
                    tenant_id,
                    sandbox_id,
                    bindings,
                )),
            ),
            RecordingForwardingState::Partial => {
                self.assert_partial_authority_unchanged();
                let (absent, present) = bindings.split_at(1);
                Ok(MachinePortForwardingRetirementObservation::Partial {
                    present: self.receipts(
                        MachinePortForwardOutcome::Exposed,
                        tenant_id,
                        sandbox_id,
                        present,
                    ),
                    absent: self.receipts(
                        MachinePortForwardOutcome::ExactAlreadyAbsent,
                        tenant_id,
                        sandbox_id,
                        absent,
                    ),
                })
            }
            RecordingForwardingState::Absent => Ok(
                MachinePortForwardingRetirementObservation::Absent(self.receipts(
                    MachinePortForwardOutcome::ExactAlreadyAbsent,
                    tenant_id,
                    sandbox_id,
                    bindings,
                )),
            ),
        }
    }

    fn withdraw_batch(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
    ) -> nimbus_sandbox::Result<Vec<MachinePortForwardReceipt>> {
        self.operations
            .lock()
            .expect("forwarding operation log should be healthy")
            .push("withdraw");
        let mut state = self
            .state
            .lock()
            .expect("forwarding state lock should be healthy");
        *state = if self.partial_once && matches!(*state, RecordingForwardingState::Present) {
            RecordingForwardingState::Partial
        } else {
            RecordingForwardingState::Absent
        };
        Ok(self.receipts(
            MachinePortForwardOutcome::Withdrawn,
            tenant_id,
            sandbox_id,
            bindings,
        ))
    }
}

struct ExactGuestTeardownApi {
    calls: Arc<Mutex<Vec<GuestTeardownCall>>>,
    worker: thread::JoinHandle<()>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GuestTeardownCall {
    step: WorkloadTeardownStep,
    mode: WorkloadTeardownCommandMode,
    dispatch_epoch: u64,
    prior_receipt_count: usize,
}

#[derive(Clone, Copy)]
enum GuestReply {
    Success,
    LoseResponse,
    MissingReleaseAbsence,
}

impl ExactGuestTeardownApi {
    fn start(path: PathBuf, expected_calls: usize) -> Self {
        Self::start_scripted(path, vec![GuestReply::Success; expected_calls])
    }

    fn response_loss_then_inspect(path: PathBuf) -> Self {
        let mut replies = Vec::with_capacity(8);
        for _ in 0..4 {
            replies.push(GuestReply::LoseResponse);
            replies.push(GuestReply::Success);
        }
        Self::start_scripted(path, replies)
    }

    fn missing_release_absence(path: PathBuf) -> Self {
        Self::start_scripted(
            path,
            vec![
                GuestReply::Success,
                GuestReply::Success,
                GuestReply::Success,
                GuestReply::MissingReleaseAbsence,
            ],
        )
    }

    fn start_scripted(path: PathBuf, replies: Vec<GuestReply>) -> Self {
        let listener = UnixListener::bind(path).expect("exact guest teardown API should bind");
        listener
            .set_nonblocking(true)
            .expect("exact guest teardown API should use bounded accept");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let worker_calls = Arc::clone(&calls);
        let worker = thread::spawn(move || {
            let expected_calls = replies.len();
            for (index, reply) in replies.into_iter().enumerate() {
                let mut stream =
                    accept_teardown_connection(&listener, GUEST_SERVER_WAIT, index, expected_calls);
                let request = read_teardown_request(&mut stream);
                worker_calls
                    .lock()
                    .expect("guest teardown calls should be healthy")
                    .push(GuestTeardownCall {
                        step: request.command().step(),
                        mode: request.command().mode(),
                        dispatch_epoch: request.command().dispatch_epoch().as_u64(),
                        prior_receipt_count: request
                            .command()
                            .prior_receipt_prefix()
                            .receipts()
                            .len(),
                    });
                match reply {
                    GuestReply::Success => write_teardown_success(&mut stream, &request),
                    GuestReply::MissingReleaseAbsence => {
                        write_teardown_success_without_release_absence(&mut stream, &request)
                    }
                    GuestReply::LoseResponse => {}
                }
            }
        });
        Self { calls, worker }
    }

    fn calls(&self) -> Vec<GuestTeardownCall> {
        self.calls
            .lock()
            .expect("guest teardown calls should be healthy")
            .clone()
    }

    fn finish(self) -> Vec<GuestTeardownCall> {
        let Self { calls, worker } = self;
        worker
            .join()
            .expect("exact guest teardown API should stop cleanly");
        calls
            .lock()
            .expect("guest teardown calls should be healthy")
            .clone()
    }
}

fn accept_teardown_connection(
    listener: &UnixListener,
    wait: Duration,
    accepted_calls: usize,
    expected_calls: usize,
) -> UnixStream {
    let deadline = Instant::now() + wait;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream
                    .set_nonblocking(false)
                    .expect("accepted guest teardown stream should use bounded blocking reads");
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for guest teardown request {}/{expected_calls}",
                    accepted_calls + 1,
                );
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("exact guest teardown API could not accept: {error}"),
        }
    }
}

#[test]
fn exact_guest_teardown_accept_fails_within_its_deadline_when_a_call_is_missing() {
    let root = tempfile::tempdir().expect("bounded accept root should exist");
    let listener = UnixListener::bind(root.path().join("missing-call.sock"))
        .expect("bounded accept listener should bind");
    listener
        .set_nonblocking(true)
        .expect("bounded accept listener should be nonblocking");
    let started = Instant::now();
    let result = std::panic::catch_unwind(|| {
        accept_teardown_connection(&listener, Duration::from_millis(25), 0, 1)
    });
    assert!(result.is_err(), "a missing scripted call must fail closed");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "a missing scripted call must not strand the test worker"
    );
}

fn read_teardown_request(stream: &mut UnixStream) -> MachineApiWorkloadTeardownPhaseRequest {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("guest teardown request timeout should configure");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 4096];
        let read = stream
            .read(&mut chunk)
            .expect("guest teardown request should read");
        assert!(read > 0, "guest teardown request closed before headers");
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(end) = find_bytes(&bytes, b"\r\n\r\n") {
            break end + 4;
        }
        assert!(
            Instant::now() < deadline,
            "guest teardown headers timed out"
        );
    };
    let headers = std::str::from_utf8(&bytes[..header_end]).expect("headers should be UTF-8");
    let content_length = headers
        .lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().expect("length should parse"))
            })
        })
        .expect("teardown request should carry content length");
    while bytes.len() < header_end + content_length {
        let mut chunk = [0_u8; 4096];
        let read = stream
            .read(&mut chunk)
            .expect("guest teardown request body should read");
        assert!(read > 0, "guest teardown request closed before its body");
        bytes.extend_from_slice(&chunk[..read]);
    }
    serde_json::from_slice(&bytes[header_end..header_end + content_length])
        .expect("strict guest teardown request should decode")
}

fn write_teardown_success(
    stream: &mut UnixStream,
    request: &MachineApiWorkloadTeardownPhaseRequest,
) {
    let success = success_evidence(request.command().step(), request.command().subjects());
    let observation = match request.command().mode() {
        WorkloadTeardownCommandMode::Execute => MachineApiWorkloadTeardownObservation::Execute(
            MachineApiWorkloadTeardownExecuteObservation::Succeeded {
                evidence: Box::new(success),
            },
        ),
        WorkloadTeardownCommandMode::Inspect => MachineApiWorkloadTeardownObservation::Inspect(
            nimbus_machine::api::MachineApiWorkloadTeardownInspectObservation::Satisfied {
                evidence: Box::new(success),
            },
        ),
    };
    let release_absence =
        (request.command().step() == WorkloadTeardownStep::ReleaseNetwork).then(|| {
            MachineApiNetworkReleaseAbsenceEvidence::new(
                WorkloadOwnerEvidenceDigest::sha256("guest-provider-absent"),
                WorkloadOwnerEvidenceDigest::sha256("guest-publication-absent"),
            )
        });
    let result =
        MachineApiWorkloadTeardownPhaseResult::new(request.command(), observation, release_absence)
            .expect("exact guest teardown result should validate");
    let response = MachineApiWorkloadTeardownPhaseResponse::for_request(request, result)
        .expect("exact guest teardown response should validate");
    let body = serde_json::to_vec(&response).expect("guest teardown response should encode");
    write!(
        stream,
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    )
    .expect("guest teardown response headers should write");
    stream
        .write_all(&body)
        .expect("guest teardown response body should write");
}

fn write_teardown_success_without_release_absence(
    stream: &mut UnixStream,
    request: &MachineApiWorkloadTeardownPhaseRequest,
) {
    let success = WorkloadTeardownSuccessEvidence::NetworkReleased {
        reference: match request.command().subjects() {
            WorkloadTeardownSubjects::Network(reference) => reference.clone(),
            _ => panic!("release request should carry a network reference"),
        },
        evidence: WorkloadOwnerEvidenceDigest::sha256("guest-release-without-absence"),
    };
    let observation = MachineApiWorkloadTeardownObservation::Execute(
        MachineApiWorkloadTeardownExecuteObservation::Succeeded {
            evidence: Box::new(success),
        },
    );
    let result = MachineApiWorkloadTeardownPhaseResult::new(
        request.command(),
        observation,
        Some(MachineApiNetworkReleaseAbsenceEvidence::new(
            WorkloadOwnerEvidenceDigest::sha256("guest-provider-absent"),
            WorkloadOwnerEvidenceDigest::sha256("guest-publication-absent"),
        )),
    )
    .expect("valid response should be constructible before its absence is removed");
    let response = MachineApiWorkloadTeardownPhaseResponse::for_request(request, result)
        .expect("valid response should correlate before mutation");
    let mut response = serde_json::to_value(response).expect("response should encode");
    response
        .as_object_mut()
        .expect("response should be an object")
        .remove("releaseAbsence");
    let body = serde_json::to_vec(&response).expect("invalid response fixture should encode");
    write!(
        stream,
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    )
    .expect("guest response headers should write");
    stream
        .write_all(&body)
        .expect("guest response body should write");
}

fn success_evidence(
    step: WorkloadTeardownStep,
    subjects: &WorkloadTeardownSubjects,
) -> WorkloadTeardownSuccessEvidence {
    let evidence = WorkloadOwnerEvidenceDigest::sha256(format!("guest-{step:?}"));
    match (step, subjects) {
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
        _ => panic!("guest API received an unsupported teardown success step"),
    }
}
