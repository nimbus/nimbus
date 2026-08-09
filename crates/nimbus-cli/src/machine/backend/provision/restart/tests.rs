//! Behavioral proofs for parent-side forwarded restart publication authority.

use std::collections::VecDeque;
use std::io::{Read as _, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use nimbus::SandboxPortBinding;
use nimbus_compute::workload_saga::{
    ConfirmedWorkloadRestartCommand, RestartPublicationCapability,
    RestartPublicationObservationCapability, WorkloadRestartAdmissionDecision,
    WorkloadRestartAdmissionRequest, WorkloadRestartCommandMode, WorkloadRestartCommandOutcome,
    WorkloadRestartCommandResult, WorkloadRestartDecision, WorkloadSagaCoordinator,
    apply_restart_result, decide_restart_admission, decide_restart_progress,
};
use nimbus_machine::{
    MachineProvider,
    api::{
        MachineApiWorkloadRestartObservation, MachineApiWorkloadRestartPhaseRequest,
        MachineApiWorkloadRestartPhaseResponse,
    },
};
use nimbus_network::{LocalPortLeaseAuthority, NetworkPlanId, PortLeasePhase, PortLeaseRequest};
use nimbus_workloads::{
    WorkloadInspectionVersion, WorkloadRestartCandidatePage, WorkloadRestartCandidatePageRequest,
    WorkloadRestartEvidenceDigest, WorkloadRestartNotBeforeUnixMillis, WorkloadRestartPolicy,
    WorkloadRestartStep, WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaFuture,
    WorkloadSagaIntent, WorkloadSagaPage, WorkloadSagaPageRequest, WorkloadSagaPhase,
    WorkloadSagaRecord, WorkloadSagaStore, WorkloadSagaStoreError, WorkloadSagaTenantPage,
    WorkloadSagaTenantPageRequest,
};
use tempfile::TempDir;

use super::super::tests::{
    advance_to_phase, forwarder_authority, source_plan, workload_intent, workload_key,
};
use super::*;

struct CommitScriptStore {
    commits: Mutex<VecDeque<WorkloadSagaCommit>>,
}

impl CommitScriptStore {
    fn new(commits: impl IntoIterator<Item = WorkloadSagaCommit>) -> Arc<Self> {
        Arc::new(Self {
            commits: Mutex::new(commits.into_iter().collect()),
        })
    }
}

impl WorkloadSagaStore for CommitScriptStore {
    fn load<'a>(
        &'a self,
        _key: &'a nimbus_workloads::WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async { Ok(None) })
    }

    fn compare_and_swap<'a>(
        &'a self,
        _expected: WorkloadSagaExpected,
        _next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            self.commits
                .lock()
                .expect("commit script lock should be healthy")
                .pop_front()
                .ok_or(WorkloadSagaStoreError::Corrupt)
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
        request: WorkloadRestartCandidatePageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadRestartCandidatePage> {
        Box::pin(async move { WorkloadRestartCandidatePage::new(&request, Vec::new(), false) })
    }

    fn list_for_tenant<'a>(
        &'a self,
        tenant_id: &'a nimbus::TenantId,
        request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage> {
        Box::pin(async move { WorkloadSagaTenantPage::new(tenant_id, &request, Vec::new(), false) })
    }
}

struct RestartFixture {
    _root: TempDir,
    server: FakeRestartMachineApi,
    port_authority: LocalPortLeaseAuthority,
    observed: WorkloadSagaRecord,
}

impl RestartFixture {
    fn new(responses: impl IntoIterator<Item = MachineApiWorkloadRestartObservation>) -> Self {
        let root = TempDir::new().expect("restart fixture root should exist");
        let authority = forwarder_authority();
        let server = FakeRestartMachineApi::start(root.path().join("machine-api.sock"), responses);
        let port_authority =
            LocalPortLeaseAuthority::open(root.path()).expect("fixture port authority should open");
        let (intent, _, _) = workload_intent(&authority);
        let observed = advance_to_phase(
            WorkloadSagaRecord::new(workload_key(), intent)
                .expect("restart fixture saga should validate"),
            WorkloadSagaPhase::Observed,
        );
        Self {
            _root: root,
            server,
            port_authority,
            observed,
        }
    }

    fn adapter(&self) -> ForwardedMachineProvisionAdapter {
        let authority = forwarder_authority();
        let client =
            crate::machine::client::MachineApiClient::new_for_test(self.server.socket_path())
                .with_forwarder_authority(authority.clone());
        ForwardedMachineProvisionAdapter::new_for_test(
            client,
            self.port_authority.clone(),
            source_plan(MachineProvider::Krunkit, authority),
        )
        .expect("forwarded restart adapter should open")
    }

    async fn command(
        &self,
        request_label: &str,
        target: WorkloadRestartStep,
        inspect_target: bool,
    ) -> ConfirmedWorkloadRestartCommand {
        command_through(&self.observed, request_label, target, inspect_target).await
    }
}

async fn command_through(
    observed: &WorkloadSagaRecord,
    request_label: &str,
    target: WorkloadRestartStep,
    inspect_target: bool,
) -> ConfirmedWorkloadRestartCommand {
    let request = WorkloadRestartAdmissionRequest::for_explicit(
        observed,
        request_label,
        WorkloadRestartNotBeforeUnixMillis::new(0),
    )
    .expect("explicit restart request should validate");
    command_through_request(observed, &request, target, inspect_target).await
}

async fn command_through_request(
    observed: &WorkloadSagaRecord,
    request: &WorkloadRestartAdmissionRequest,
    target: WorkloadRestartStep,
    inspect_target: bool,
) -> ConfirmedWorkloadRestartCommand {
    let WorkloadRestartAdmissionDecision::Transition(admitted) =
        decide_restart_admission(observed, request).expect("restart should admit")
    else {
        panic!("a new restart request should transition");
    };
    let mut current = *admitted;

    for _ in 0..32 {
        let WorkloadRestartDecision::Proposed(proposed) =
            decide_restart_progress(&current, WorkloadRestartNotBeforeUnixMillis::new(0))
                .expect("restart state should reduce")
        else {
            panic!("restart fixture should propose its next durable transition");
        };
        if proposed.action_after_confirmation().is_none() {
            current = proposed.into_candidate();
            continue;
        }
        let step = proposed
            .candidate()
            .restart_state()
            .active()
            .and_then(|active| active.disposition().claim())
            .expect("effect proposal should retain an exact claim")
            .step();
        let store = if inspect_target && step == target {
            CommitScriptStore::new([WorkloadSagaCommit::Unchanged, WorkloadSagaCommit::Applied])
        } else if matches!(
            step,
            WorkloadRestartStep::InspectActivationPrerequisites
                | WorkloadRestartStep::InspectReadiness
                | WorkloadRestartStep::ObservePublication
        ) {
            CommitScriptStore::new([WorkloadSagaCommit::Applied, WorkloadSagaCommit::Applied])
        } else {
            CommitScriptStore::new([WorkloadSagaCommit::Applied])
        };
        let confirmed = WorkloadSagaCoordinator::new(store)
            .claim_restart_command(&current, &proposed)
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "restart command should confirm at {step:?} (target {target:?}, inspect={inspect_target}): {error:?}"
                )
            });
        let durable = confirmed
            .confirmed_record()
            .expect("confirmed restart should retain durable truth")
            .clone();
        let command = confirmed
            .command()
            .expect("effect proposal should issue one command")
            .clone();
        if step == target {
            assert_eq!(
                command.mode(),
                if inspect_target {
                    WorkloadRestartCommandMode::Inspect
                } else {
                    WorkloadRestartCommandMode::Execute
                }
            );
            return command;
        }
        let result = WorkloadRestartCommandResult::for_command(
            &command,
            WorkloadRestartCommandOutcome::Succeeded {
                evidence: WorkloadRestartEvidenceDigest::sha256(format!(
                    "fixture-success-{step:?}"
                )),
            },
        );
        let WorkloadRestartDecision::Proposed(completed) =
            apply_restart_result(&durable, &command, result)
                .expect("fixture command success should reduce")
        else {
            panic!("successful restart command should produce a durable candidate");
        };
        current = completed.into_candidate();
    }
    panic!("restart command fixture exceeded its transition bound")
}

#[allow(dead_code, reason = "shared by sibling restart route tests")]
pub(crate) async fn confirmed_restart_command_for_test(
    request_label: &str,
    target: WorkloadRestartStep,
    inspect_target: bool,
) -> ConfirmedWorkloadRestartCommand {
    let authority = forwarder_authority();
    let (intent, _, _) = workload_intent(&authority);
    let observed = advance_to_phase(
        WorkloadSagaRecord::new(workload_key(), intent)
            .expect("shared restart fixture saga should validate"),
        WorkloadSagaPhase::Observed,
    );
    command_through(&observed, request_label, target, inspect_target).await
}

#[allow(dead_code, reason = "shared by sibling restart route tests")]
pub(crate) async fn confirmed_automatic_restart_command_for_test(
    target: WorkloadRestartStep,
    inspect_target: bool,
) -> ConfirmedWorkloadRestartCommand {
    let authority = forwarder_authority();
    let (intent, _, _) = workload_intent(&authority);
    let intent = WorkloadSagaIntent::new_with_restart_policy(
        intent.kind(),
        intent.desired_state(),
        intent.generation(),
        intent.executable().clone(),
        intent.source().clone(),
        WorkloadRestartPolicy::OnFailure { max_restarts: 3 },
        intent.network().clone(),
        intent.activation(),
        intent.publication(),
        intent.admission().clone(),
    )
    .expect("automatic restart fixture intent should validate");
    let observed = advance_to_phase(
        WorkloadSagaRecord::new(workload_key(), intent)
            .expect("automatic restart fixture saga should validate"),
        WorkloadSagaPhase::Observed,
    );
    let request = WorkloadRestartAdmissionRequest::for_automatic(
        &observed,
        1,
        WorkloadInspectionVersion::from_bytes([0x64; 32]),
        WorkloadRestartNotBeforeUnixMillis::new(0),
    );
    command_through_request(&observed, &request, target, inspect_target).await
}

fn succeeded(label: &str) -> MachineApiWorkloadRestartObservation {
    MachineApiWorkloadRestartObservation::Succeeded {
        evidence: WorkloadRestartEvidenceDigest::sha256(label),
    }
}

fn absent(label: &str) -> MachineApiWorkloadRestartObservation {
    MachineApiWorkloadRestartObservation::AuthenticatedAbsent {
        evidence: WorkloadRestartEvidenceDigest::sha256(label),
    }
}

fn assert_succeeded(
    observation: &nimbus_compute::workload_saga::WorkloadRestartProviderObservation,
) {
    assert!(
        format!("{observation:?}").contains("Succeeded"),
        "expected successful provider observation, got {observation:?}"
    );
}

fn assert_absent(observation: &nimbus_compute::workload_saga::WorkloadRestartProviderObservation) {
    assert!(
        format!("{observation:?}").contains("AuthenticatedAbsent"),
        "expected authenticated absence, got {observation:?}"
    );
}

fn validated_publication(
    adapter: &ForwardedMachineProvisionAdapter,
    command: &ConfirmedWorkloadRestartCommand,
) -> ValidatedForwardedRestart {
    match adapter.validate_restart_phase(command, WorkloadRestartStep::Publish, command.mode()) {
        Ok(validated) => validated,
        Err(_) => panic!("fixture publication command should authenticate"),
    }
}

fn requests(validated: &ValidatedForwardedRestart) -> Vec<PortLeaseRequest> {
    validated
        .members
        .iter()
        .map(|member| member.request().clone())
        .collect()
}

fn assert_rebind_ready(authority: &LocalPortLeaseAuthority, plan_id: &NetworkPlanId) {
    let records = authority
        .list_plan(plan_id)
        .expect("parent publication plan should inspect");
    assert!(
        !records.is_empty(),
        "fixture publication needs a parent lease"
    );
    assert!(records.iter().all(|record| {
        record.phase() == PortLeasePhase::Reserved
            && record.binding().is_none()
            && record.bind_claim().is_none()
    }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exact_publish_replay_invokes_one_machine_api_effect() {
    let fixture = RestartFixture::new([succeeded("published-once")]);
    let adapter = fixture.adapter();
    let command = fixture
        .command("publish-replay", WorkloadRestartStep::Publish, false)
        .await;

    let first = RestartPublicationCapability::execute(&adapter, &command).await;
    let replay = RestartPublicationCapability::execute(&adapter, &command).await;

    assert_succeeded(&first);
    assert_succeeded(&replay);
    let calls = fixture.server.finish();
    assert_eq!(calls.len(), 1, "exact replay must adopt the durable result");
    let forwarded = calls[0].command();
    assert_eq!(forwarded.command_id(), command.command_id());
    assert_eq!(forwarded.transition_id(), command.transition_id());
    assert_eq!(forwarded.request_id(), command.request_id());
    assert_eq!(forwarded.source_attempt_id(), command.source_attempt_id());
    assert_eq!(forwarded.attempt_id(), command.attempt_id());
    assert_eq!(forwarded.restart_epoch(), command.restart_epoch());
    assert_eq!(forwarded.dispatch_epoch(), command.dispatch_epoch());
    assert_eq!(forwarded.provider_selection(), command.provider_selection());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn publish_absence_execute_and_inspection_leave_parent_rebind_ready() {
    let fixture = RestartFixture::new([
        absent("publish-execute-absent"),
        absent("publish-inspect-absent"),
    ]);
    let adapter = fixture.adapter();
    let execute = fixture
        .command("publish-absence", WorkloadRestartStep::Publish, false)
        .await;
    let inspect = fixture
        .command("publish-absence", WorkloadRestartStep::Publish, true)
        .await;
    let plan_id = execute.compiled_network_plan().plan().plan_id().clone();

    let executed = RestartPublicationCapability::execute(&adapter, &execute).await;
    let records_after_execute = fixture
        .port_authority
        .list_plan(&plan_id)
        .expect("parent publication plan should inspect after absence");
    assert!(
        format!("{executed:?}").contains("Ambiguous"),
        "execute-time absence must require exact inspection, got {executed:?}; records: {records_after_execute:?}"
    );
    assert_rebind_ready(&fixture.port_authority, &plan_id);

    let inspected = RestartPublicationCapability::inspect(&adapter, &inspect).await;
    assert_absent(&inspected);
    assert_rebind_ready(&fixture.port_authority, &plan_id);
    assert_eq!(fixture.server.finish().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fresh_adapter_observation_is_read_only_and_requires_execute_recovery() {
    let fixture = RestartFixture::new([
        succeeded("publish-before-process-loss"),
        succeeded("observe-after-process-loss"),
    ]);
    let publish = fixture
        .command("fresh-adapter", WorkloadRestartStep::Publish, false)
        .await;
    let observe = fixture
        .command(
            "fresh-adapter",
            WorkloadRestartStep::ObservePublication,
            true,
        )
        .await;
    let first = fixture.adapter();
    assert_succeeded(&RestartPublicationCapability::execute(&first, &publish).await);
    drop(first);

    let restarted = fixture.adapter();
    let plan_id = publish.compiled_network_plan().plan().plan_id();
    let before = fixture
        .port_authority
        .list_plan(plan_id)
        .expect("parent publication plan should inspect before observation");
    let observed = RestartPublicationObservationCapability::inspect(&restarted, &observe).await;
    assert_absent(&observed);
    assert_eq!(
        fixture
            .port_authority
            .list_plan(plan_id)
            .expect("parent publication plan should inspect after observation"),
        before,
        "inspection must not mutate durable parent publication state"
    );
    let validated = validated_publication(&restarted, &publish);
    let recoveries = fixture
        .port_authority
        .recover_dead_lifetimes(&requests(&validated))
        .expect("inspection must not reclaim the dead parent lifetime");
    drop(recoveries);
    assert_eq!(fixture.server.finish().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn crossed_activation_plan_does_not_drop_existing_parent_lifetime() {
    let fixture = RestartFixture::new([]);
    let adapter = fixture.adapter();
    let command = fixture
        .command("activation-mismatch", WorkloadRestartStep::Publish, false)
        .await;
    let validated = validated_publication(&adapter, &command);
    adapter
        .reserve_parent_batch_for(&validated.plan_id, &validated.members)
        .unwrap_or_else(|_| panic!("exact parent batch should reserve"));
    adapter
        .activate_parent_batch_for(&validated.plan_id, &validated.members)
        .unwrap_or_else(|_| panic!("exact parent batch should activate"));

    let exact_requests = requests(&validated);
    let mut crossed_members = validated.members.clone();
    let crossed = crossed_members
        .first_mut()
        .expect("fixture publication should contain one parent member");
    let binding = crossed.binding();
    crossed.replace_binding_for_test(SandboxPortBinding::new(
        "crossed-restart-listener",
        binding.protocol,
        binding.host_port,
        binding.guest_port,
    ));
    assert!(
        adapter
            .activate_parent_batch_for(&validated.plan_id, &crossed_members)
            .is_err(),
        "crossed activation must fail closed"
    );
    assert!(
        fixture
            .port_authority
            .recover_dead_lifetimes(&exact_requests)
            .is_err(),
        "a crossed activation must not release the exact live lifetime"
    );
    assert_eq!(fixture.server.finish().len(), 0);
}

struct FakeRestartMachineApi {
    socket_path: PathBuf,
    calls: Arc<Mutex<Vec<MachineApiWorkloadRestartPhaseRequest>>>,
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}

impl FakeRestartMachineApi {
    fn start(
        path: PathBuf,
        responses: impl IntoIterator<Item = MachineApiWorkloadRestartObservation>,
    ) -> Self {
        let listener = UnixListener::bind(&path).expect("fake restart Machine API should bind");
        listener
            .set_nonblocking(true)
            .expect("fake restart Machine API should become nonblocking");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_calls = calls.clone();
        let worker_stop = stop.clone();
        let mut responses = responses.into_iter().collect::<VecDeque<_>>();
        let worker = thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_nonblocking(false)
                            .expect("accepted fake restart stream should block");
                        let request = read_request(&mut stream);
                        worker_calls
                            .lock()
                            .expect("fake restart call log should be healthy")
                            .push(request.clone());
                        let observation = responses
                            .pop_front()
                            .unwrap_or(MachineApiWorkloadRestartObservation::Ambiguous);
                        write_response(&mut stream, &request, observation);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("fake restart Machine API accept failed: {error}"),
                }
            }
        });
        Self {
            socket_path: path,
            calls,
            stop,
            worker: Mutex::new(Some(worker)),
        }
    }

    fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    fn finish(&self) -> Vec<MachineApiWorkloadRestartPhaseRequest> {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self
            .worker
            .lock()
            .expect("fake restart worker lock should be healthy")
            .take()
        {
            worker
                .join()
                .expect("fake restart Machine API should stop cleanly");
        }
        self.calls
            .lock()
            .expect("fake restart call log should be healthy")
            .clone()
    }
}

impl Drop for FakeRestartMachineApi {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self
            .worker
            .get_mut()
            .expect("fake restart worker lock should be healthy")
            .take()
        {
            let _ = worker.join();
        }
    }
}

fn read_request(stream: &mut UnixStream) -> MachineApiWorkloadRestartPhaseRequest {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("fake restart stream timeout should configure");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 4096];
        let read = stream.read(&mut chunk).expect("fake request should read");
        assert!(read > 0, "fake request closed before headers");
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(end) = find_bytes(&bytes, b"\r\n\r\n") {
            break end + 4;
        }
        assert!(Instant::now() < deadline, "fake request headers timed out");
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
        .expect("request should carry content length");
    while bytes.len() < header_end + content_length {
        let mut chunk = [0_u8; 4096];
        let read = stream
            .read(&mut chunk)
            .expect("fake request body should read");
        assert!(read > 0, "fake request closed before its body");
        bytes.extend_from_slice(&chunk[..read]);
    }
    serde_json::from_slice(&bytes[header_end..header_end + content_length])
        .expect("strict restart request should decode")
}

fn write_response(
    stream: &mut UnixStream,
    request: &MachineApiWorkloadRestartPhaseRequest,
    observation: MachineApiWorkloadRestartObservation,
) {
    let response = MachineApiWorkloadRestartPhaseResponse::for_request(request, observation)
        .expect("fake restart response should validate");
    let body = serde_json::to_vec(&response).expect("fake restart response should encode");
    write!(
        stream,
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    )
    .expect("fake restart response headers should write");
    stream
        .write_all(&body)
        .expect("fake restart response body should write");
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
