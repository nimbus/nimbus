//! Stitched parent/guest subprocess recovery for forwarded teardown.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nimbus_compute::workload_executable::decode_sandbox_spec;
use nimbus_compute::workload_saga::{
    WorkloadSagaCoordinator, WorkloadTeardownCancellationToken, WorkloadTeardownCapabilityRegistry,
    WorkloadTeardownRuntime,
};
use nimbus_machine::api::{
    MachineApiWorkloadTeardownObservation, MachineApiWorkloadTeardownPhaseRequest,
    MachineApiWorkloadTeardownPhaseResponse,
};
use nimbus_network::{
    ListenerId, LocalPortLeaseAuthority, NetworkProviderHandle, NetworkResourceGeneration,
};
use nimbus_node::{
    HostExecutionDrainProvider, HostExecutionStopProvider, HostLifecycleBackend,
    HostLifecycleFuture, HostLifecyclePlan, HostLifecycleRequest, HostLifecycleStatus,
    HostTeardownExecuteClaim, HostTeardownExecuteObservation, HostTeardownFuture,
    HostTeardownInspectClaim, HostTeardownInspectObservation,
};
use nimbus_sandbox::{
    MachinePortForwardOutcome, MachinePortForwardReceipt, MachinePortForwardingRetirement,
    MachinePortForwardingRetirementObservation, ProviderCommandObservation,
    ProviderCommandOperation, ProviderCommandStartedClaimDecision, SandboxExecutionAttemptId,
    SandboxId, SandboxPortBinding, SandboxProvisionDependencyListener, SandboxProvisionNetworkPlan,
    backends::container::{ContainerSandboxBackend, ContainerSandboxBackendConfig},
    sandbox_network_plan_requirements,
};
use nimbus_workloads::{
    LocalEnforcementBinding, WorkloadOwnerEvidenceDigest, WorkloadSagaCommit, WorkloadSagaExpected,
    WorkloadSagaFuture, WorkloadSagaPage, WorkloadSagaPageRequest, WorkloadSagaPhase,
    WorkloadSagaRecord, WorkloadSagaStore, WorkloadSagaStoreError, WorkloadSagaTenantPage,
    WorkloadSagaTenantPageRequest, WorkloadTeardownStep, WorkloadTeardownSuccessEvidence,
};
use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;
use crate::machine::api::{
    GuestNodeWorkloadService, MachineApiNodeWorkloadFacade, guest_teardown_provider_claim_for_test,
    sandbox_network_plan_for_teardown_test,
};

const CHILD_TEST: &str = "machine::backend::provision::tests::teardown_substitution::process_recovery::forwarded_two_realm_process_child";
const ROLE_ENV: &str = "NIMBUS_NNC65D4_TWO_REALM_ROLE";
const PARENT_ROOT_ENV: &str = "NIMBUS_NNC65D4_TWO_REALM_PARENT_ROOT";
const GUEST_ROOT_ENV: &str = "NIMBUS_NNC65D4_TWO_REALM_GUEST_ROOT";
const SOCKET_ENV: &str = "NIMBUS_NNC65D4_TWO_REALM_SOCKET";
const REQUEST_ENV: &str = "NIMBUS_NNC65D4_TWO_REALM_REQUEST";
const RESULT_ENV: &str = "NIMBUS_NNC65D4_TWO_REALM_RESULT";
const READY_ENV: &str = "NIMBUS_NNC65D4_TWO_REALM_READY";
const EXPECTED_CALLS_ENV: &str = "NIMBUS_NNC65D4_TWO_REALM_EXPECTED_CALLS";
const STORE_NAME: &str = "two-realm-saga.json";
const FORWARDING_STATE_NAME: &str = "two-realm-forwarding-state";
const GUEST_EFFECT_LOG_NAME: &str = "two-realm-guest-effects.log";
const WAIT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy)]
enum FrozenGuestCut {
    TerminalResponseLost,
    PreparedBeforeEffect,
}

#[test]
fn forwarded_two_realm_fresh_process_matrix_recovers_every_frozen_cut() {
    let mut recovered_cuts = 0;
    for cut in [
        FrozenGuestCut::TerminalResponseLost,
        FrozenGuestCut::PreparedBeforeEffect,
    ] {
        run_frozen_cut(cut);
        recovered_cuts += 1;
    }
    assert_eq!(recovered_cuts, 2, "both frozen two-realm cuts must recover");
}

fn run_frozen_cut(cut: FrozenGuestCut) {
    let fixture = Fixture::new([ResponseMode::Exact(
        MachineApiWorkloadProvisionObservation::Succeeded {
            evidence: b"two-realm-parent-published".to_vec(),
        },
    )]);
    let provision = fixture.adapter(MachineProvider::Krunkit);
    let publish = teardown_test_runtime().block_on(fixture.publish_command());
    assert!(matches!(
        teardown_test_runtime().block_on(IngressPublicationCapability::execute(
            &provision,
            publish.command(),
        )),
        WorkloadProvisionInspectionResult::Succeeded { .. }
    ));
    drop(provision);
    assert_eq!(fixture.server.finish().len(), 1);

    let parent_root = fixture.root.path();
    let original_socket = parent_root.join("machine-api.sock");
    if original_socket.exists() {
        fs::remove_file(&original_socket).expect("provision socket should unlink");
    }
    let transport = TempDir::new().expect("two-realm transport root should exist");
    let socket = transport.path().join("guest-teardown.sock");
    let guest = TempDir::new().expect("guest durable root should exist");
    let guest_root = guest.path();
    fs::write(parent_root.join(FORWARDING_STATE_NAME), b"present")
        .expect("parent forwarding state should initialize");

    let observed = advance_to_phase(
        WorkloadSagaRecord::new(workload_key(), fixture.intent.clone())
            .expect("two-realm saga should validate"),
        WorkloadSagaPhase::Observed,
    );
    write_saga(parent_root, &begin_exact_teardown(&observed));

    let guest_before_parent = snapshot_tree(guest_root);
    run_child(
        "parent-run",
        parent_root,
        guest_root,
        &socket,
        None,
        transport.path().join("parent-initial.json"),
    );
    assert_eq!(
        snapshot_tree(guest_root),
        guest_before_parent,
        "parent-only withdrawal and request staging must leave the guest root byte-stable"
    );
    let pending = read_saga(parent_root);
    let disposition = pending
        .teardown_disposition()
        .expect("ambiguous remote request should remain in teardown");
    assert_eq!(
        disposition
            .claim()
            .expect("ambiguous remote request should retain its exact claim")
            .attempt()
            .step(),
        WorkloadTeardownStep::DrainExecution
    );
    assert!(disposition.requires_inspection());

    let request = find_parent_drain_request(parent_root);
    let request_path = transport.path().join("prepared-drain.json");
    fs::write(&request_path, serde_json::to_vec(&request).unwrap())
        .expect("exact prepared request should persist outside both authority roots");
    let parent_before_guest = snapshot_tree(parent_root);
    let guest_role = match cut {
        FrozenGuestCut::TerminalResponseLost => "guest-dispatch",
        FrozenGuestCut::PreparedBeforeEffect => "guest-start",
    };
    run_child(
        guest_role,
        parent_root,
        guest_root,
        &socket,
        Some(&request_path),
        transport.path().join("guest-cut.json"),
    );
    assert_eq!(
        snapshot_tree(parent_root),
        parent_before_guest,
        "a guest-only durable operation must leave every parent-root byte unchanged"
    );

    let expected_calls = match cut {
        FrozenGuestCut::TerminalResponseLost => 1,
        FrozenGuestCut::PreparedBeforeEffect => 2,
    };
    let ready = transport.path().join("guest-ready");
    let server_result = transport.path().join("guest-server.json");
    let mut server = spawn_guest_server(
        parent_root,
        guest_root,
        &socket,
        &ready,
        &server_result,
        expected_calls,
    );
    wait_for_path(&ready);
    run_child(
        "parent-run",
        parent_root,
        guest_root,
        &socket,
        None,
        transport.path().join("parent-recovery.json"),
    );
    let status = wait_for_child(&mut server, "guest recovery server");
    assert!(status.success(), "guest recovery server failed: {status}");

    let calls: Value =
        serde_json::from_slice(&fs::read(&server_result).expect("guest calls should persist"))
            .expect("guest calls should decode");
    let calls = calls.as_array().expect("guest calls should be an array");
    assert_eq!(calls.len(), expected_calls);
    assert_eq!(calls[0]["mode"], "inspect");
    assert_eq!(calls[0]["dispatchEpoch"], "0");
    if matches!(cut, FrozenGuestCut::PreparedBeforeEffect) {
        assert_eq!(calls[1]["mode"], "execute");
        assert_eq!(calls[1]["dispatchEpoch"], "1");
    }

    let recovered = read_saga(parent_root);
    let recovered_disposition = recovered
        .teardown_disposition()
        .expect("the next remote phase should remain recoverable");
    assert_eq!(recovered_disposition.context().completed().len(), 2);
    assert_eq!(
        recovered_disposition
            .claim()
            .expect("the next failed remote phase should retain a claim")
            .attempt()
            .step(),
        WorkloadTeardownStep::StopExecution
    );
    assert!(recovered_disposition.requires_inspection());
    let effects = fs::read_to_string(guest_root.join(GUEST_EFFECT_LOG_NAME))
        .expect("guest effect log should exist");
    assert_eq!(
        effects
            .lines()
            .filter(|line| *line == "drain_execute")
            .count(),
        1,
        "each frozen cut must converge on one guest drain effect"
    );
    assert_eq!(
        effects
            .lines()
            .filter(|line| *line == "stop_execute")
            .count(),
        0,
        "the bounded proof must not cross into a later guest phase"
    );
}

#[test]
#[ignore = "subprocess entry point; the parent test supplies exact durable roots"]
fn forwarded_two_realm_process_child() {
    let role = std::env::var(ROLE_ENV).expect("two-realm role must be supplied");
    let parent_root = env_path(PARENT_ROOT_ENV);
    let guest_root = env_path(GUEST_ROOT_ENV);
    let socket = env_path(SOCKET_ENV);
    let result_path = env_path(RESULT_ENV);
    match role.as_str() {
        "parent-run" => run_parent_process(&parent_root, &socket, &result_path),
        "guest-dispatch" | "guest-start" => {
            let request_path = env_path(REQUEST_ENV);
            let request: MachineApiWorkloadTeardownPhaseRequest =
                serde_json::from_slice(&fs::read(request_path).unwrap()).unwrap();
            run_guest_cut(&guest_root, &request, role == "guest-start", &result_path);
        }
        "guest-server" => {
            let ready = env_path(READY_ENV);
            let expected_calls = std::env::var(EXPECTED_CALLS_ENV)
                .unwrap()
                .parse::<usize>()
                .unwrap();
            run_guest_server(&guest_root, &socket, &ready, expected_calls, &result_path);
        }
        other => panic!("unknown two-realm child role {other}"),
    }
}

fn run_parent_process(parent_root: &Path, socket: &Path, result_path: &Path) {
    let authority = forwarder_authority();
    let (intent, _, provider_reports) = workload_intent_with_listener_count(&authority, 1);
    let client = MachineApiClient::new_for_test(socket).with_forwarder_authority(authority.clone());
    let ports = LocalPortLeaseAuthority::open(parent_root).expect("parent ports should reopen");
    let provision = Arc::new(
        ForwardedMachineProvisionAdapter::new_for_test(
            client,
            ports,
            source_plan(MachineProvider::Krunkit, authority.clone()),
        )
        .expect("parent provision authorities should reopen"),
    );
    let forwarding = Arc::new(DurableForwarding::new(
        &authority,
        parent_root.join(FORWARDING_STATE_NAME),
    ));
    let adapter = Arc::new(
        ForwardedMachineTeardownAdapter::new_for_test(provision, forwarding)
            .expect("parent teardown authorities should reopen"),
    );
    let (attachment, execution, ingress) = adapter.registrations().into_parts();
    let registry = WorkloadTeardownCapabilityRegistry::new([attachment], [execution], [ingress])
        .expect("fresh parent registry should contain all five capabilities");
    let store = Arc::new(ProcessTeardownStore::new(parent_root.join(STORE_NAME)));
    let runtime = WorkloadTeardownRuntime::new(
        Arc::new(WorkloadSagaCoordinator::new(store)),
        Arc::new(StaticSource(intent.source().clone())),
        provider_reports,
        Arc::new(registry),
    );
    let run = teardown_test_runtime()
        .block_on(runtime.submit(workload_key(), &WorkloadTeardownCancellationToken::new()));
    fs::write(
        result_path,
        serde_json::to_vec(&json!({ "ok": run.is_ok() })).unwrap(),
    )
    .expect("parent child result should persist");
}

fn run_guest_cut(
    guest_root: &Path,
    request: &MachineApiWorkloadTeardownPhaseRequest,
    start_only: bool,
    result_path: &Path,
) {
    let (service, backend) = guest_service(guest_root, request);
    let command = request.command();
    let output = if start_only {
        let claim = guest_teardown_provider_claim_for_test(
            command,
            request.forwarder_authority(),
            command.claim().attempt().required_node(),
        )
        .expect("guest provider claim should lower");
        let decision = backend
            .attempt_idempotency_journal()
            .expect("guest Container journal should open")
            .claim_dispatch_epoch_started(&claim, &serde_json::to_vec(command).unwrap())
            .expect("guest prepared start should persist");
        assert!(matches!(
            decision,
            ProviderCommandStartedClaimDecision::ExecuteStarted(_)
        ));
        json!({ "outcome": "started" })
    } else {
        let result = teardown_test_runtime()
            .block_on(service.teardown_phase(command, request.forwarder_authority()))
            .expect("guest exact phase should execute");
        assert!(matches!(
            result.observation(),
            MachineApiWorkloadTeardownObservation::Execute(
                MachineApiWorkloadTeardownExecuteObservation::Succeeded { .. }
            )
        ));
        json!({ "outcome": "terminal" })
    };
    fs::write(result_path, serde_json::to_vec(&output).unwrap())
        .expect("guest cut result should persist");
}

fn run_guest_server(
    guest_root: &Path,
    socket: &Path,
    ready: &Path,
    expected_calls: usize,
    result_path: &Path,
) {
    let listener = UnixListener::bind(socket).expect("guest recovery server should bind");
    listener
        .set_nonblocking(true)
        .expect("guest recovery server should use bounded accept");
    fs::write(ready, b"ready").expect("guest recovery readiness should persist");
    let mut service = None;
    let mut calls = Vec::with_capacity(expected_calls);
    for index in 0..expected_calls {
        let mut stream = accept_teardown_connection(&listener, WAIT, index, expected_calls);
        let request = read_teardown_request(&mut stream);
        let (current, _) = service.get_or_insert_with(|| guest_service(guest_root, &request));
        let result = teardown_test_runtime()
            .block_on(current.teardown_phase(request.command(), request.forwarder_authority()))
            .expect("guest recovery phase should dispatch");
        calls.push(json!({
            "mode": request.command().mode(),
            "dispatchEpoch": request.command().dispatch_epoch(),
        }));
        write_guest_response(&mut stream, &request, result);
        if index + 1 == expected_calls {
            fs::remove_file(socket).expect("closed guest server should reject later phases");
        }
    }
    fs::write(result_path, serde_json::to_vec(&calls).unwrap())
        .expect("guest recovery calls should persist");
}

fn guest_service(
    guest_root: &Path,
    request: &MachineApiWorkloadTeardownPhaseRequest,
) -> (Arc<GuestNodeWorkloadService>, Arc<ContainerSandboxBackend>) {
    fs::create_dir_all(guest_root).expect("guest state root should exist");
    let runtime_path = guest_root.join("runtime-state-fixture");
    if !runtime_path.exists() {
        fs::write(
            &runtime_path,
            format!(
                "#!/bin/sh\nprintf '%s\\n' '{{\"id\":\"{}\",\"status\":\"running\"}}'\n",
                request
                    .command()
                    .execution_locator()
                    .execution_id()
                    .as_str()
            ),
        )
        .expect("guest runtime fixture should persist");
        fs::set_permissions(&runtime_path, fs::Permissions::from_mode(0o755))
            .expect("guest runtime fixture should be executable");
    }
    let mut config =
        ContainerSandboxBackendConfig::plan_only(guest_root.join("bundles"), guest_root);
    config.runtime_path = runtime_path;
    config.use_buildah_unshare = false;
    let backend = Arc::new(ContainerSandboxBackend::new(config));
    let command = request.command();
    let sandbox_id = SandboxId::new(command.execution_locator().execution_id().as_str());
    let state_view =
        nimbus_sandbox::backends::container::ContainerSandboxStateView::new(guest_root);
    if state_view
        .inspect(&sandbox_id)
        .expect("guest manifest inspection should stay readable")
        .is_none()
    {
        let authority = forwarder_authority();
        let (intent, _, _) = workload_intent_with_listener_count(&authority, 1);
        let mut spec = decode_sandbox_spec(intent.executable()).expect("guest spec should decode");
        spec.owner = nimbus::SandboxOwnerSpec::service("two-realm-service");
        let lowered = sandbox_network_plan_for_teardown_test(
            command.compiled_network_plan(),
            command.claim().attempt().generation(),
            &spec,
        )
        .unwrap_or_else(|observation| panic!("guest plan should lower: {observation:?}"));
        let dependency = SandboxProvisionDependencyListener::new(
            ListenerId::for_tenant_workload_listener(
                command.claim().attempt().key().tenant_id(),
                "machine-workload-incarnation",
                "egress-pep",
            ),
            "egress-pep",
            sandbox_network_plan_requirements(nimbus::SandboxBackendKind::Container)
                .pep_provider_id()
                .clone(),
        );
        let plan = SandboxProvisionNetworkPlan::new(
            lowered.network_plan().clone(),
            lowered.tenant_id().clone(),
            lowered.generation(),
            lowered.attachment_id().clone(),
            lowered.listeners().iter().cloned(),
            lowered
                .dependency_listeners()
                .iter()
                .cloned()
                .chain([dependency]),
        )
        .expect("guest dependency-complete plan should validate");
        let execution_attempt = SandboxExecutionAttemptId::new(
            command.execution_locator().attempt_id().as_str().to_owned(),
        )
        .expect("guest execution attempt should validate");
        backend
            .reserve_provision_network(spec, sandbox_id.clone(), execution_attempt.clone(), plan)
            .expect("guest PlanOnly reservation should persist");
        backend
            .prepare_provision_workload(&sandbox_id, &execution_attempt)
            .expect("guest PlanOnly manifest should persist");
    }
    let host = Arc::new(ProcessGuestHost {
        effect_log: guest_root.join(GUEST_EFFECT_LOG_NAME),
    });
    let service = Arc::new(GuestNodeWorkloadService::new_for_teardown_test(
        command.claim().attempt().required_node().clone(),
        host,
        Arc::clone(&backend),
        guest_root,
    ));
    (service, backend)
}

struct ProcessGuestHost {
    effect_log: PathBuf,
}

impl ProcessGuestHost {
    fn record(&self, event: &str) {
        let mut log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.effect_log)
            .expect("guest effect log should open");
        writeln!(log, "{event}").expect("guest effect should persist");
    }
}

impl HostLifecycleBackend for ProcessGuestHost {
    fn validate(
        &self,
        _binding: &LocalEnforcementBinding,
        _request: HostLifecycleRequest,
    ) -> nimbus_core::Result<HostLifecyclePlan> {
        Err(nimbus_core::Error::PermissionDenied(
            "two-realm guest uses exact teardown only".to_owned(),
        ))
    }

    fn inspect<'a>(
        &'a self,
        _execution_id: nimbus_workloads::WorkloadExecutionId,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async {
            Err(nimbus_core::Error::PermissionDenied(
                "two-realm guest uses exact inspection only".to_owned(),
            ))
        })
    }
}

impl HostExecutionDrainProvider for ProcessGuestHost {
    fn execute_drain<'a>(
        &'a self,
        claim: HostTeardownExecuteClaim,
    ) -> HostTeardownFuture<'a, HostTeardownExecuteObservation> {
        Box::pin(async move {
            self.record("drain_execute");
            HostTeardownExecuteObservation::Succeeded(Box::new(execution_success(
                WorkloadTeardownStep::DrainExecution,
                claim.execution(),
            )))
        })
    }

    fn inspect_drain<'a>(
        &'a self,
        claim: HostTeardownInspectClaim,
    ) -> HostTeardownFuture<'a, HostTeardownInspectObservation> {
        Box::pin(async move {
            self.record("drain_inspect");
            HostTeardownInspectObservation::Satisfied(Box::new(execution_success(
                WorkloadTeardownStep::DrainExecution,
                claim.execution(),
            )))
        })
    }
}

impl HostExecutionStopProvider for ProcessGuestHost {
    fn execute_stop<'a>(
        &'a self,
        claim: HostTeardownExecuteClaim,
    ) -> HostTeardownFuture<'a, HostTeardownExecuteObservation> {
        Box::pin(async move {
            self.record("stop_execute");
            HostTeardownExecuteObservation::Succeeded(Box::new(execution_success(
                WorkloadTeardownStep::StopExecution,
                claim.execution(),
            )))
        })
    }

    fn inspect_stop<'a>(
        &'a self,
        claim: HostTeardownInspectClaim,
    ) -> HostTeardownFuture<'a, HostTeardownInspectObservation> {
        Box::pin(async move {
            self.record("stop_inspect");
            HostTeardownInspectObservation::Satisfied(Box::new(execution_success(
                WorkloadTeardownStep::StopExecution,
                claim.execution(),
            )))
        })
    }
}

fn execution_success(
    step: WorkloadTeardownStep,
    execution: &nimbus_workloads::WorkloadExecutionReference,
) -> WorkloadTeardownSuccessEvidence {
    let evidence = WorkloadOwnerEvidenceDigest::sha256(format!("two-realm-{step:?}"));
    match step {
        WorkloadTeardownStep::DrainExecution => WorkloadTeardownSuccessEvidence::ExecutionDrained {
            reference: execution.clone(),
            evidence,
        },
        WorkloadTeardownStep::StopExecution => WorkloadTeardownSuccessEvidence::ExecutionStopped {
            reference: execution.clone(),
            evidence,
        },
        _ => unreachable!("the two-realm host owns only execution teardown"),
    }
}

struct ProcessTeardownStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl ProcessTeardownStore {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    fn read(&self) -> Result<WorkloadSagaRecord, WorkloadSagaStoreError> {
        serde_json::from_slice(&fs::read(&self.path).map_err(store_error)?)
            .map_err(|_| WorkloadSagaStoreError::Corrupt)
    }

    fn write(&self, record: &WorkloadSagaRecord) -> Result<(), WorkloadSagaStoreError> {
        let bytes = serde_json::to_vec(record).map_err(|_| WorkloadSagaStoreError::Corrupt)?;
        let stage = self.path.with_extension("json.stage");
        fs::write(&stage, bytes).map_err(store_error)?;
        fs::rename(stage, &self.path).map_err(store_error)
    }
}

impl WorkloadSagaStore for ProcessTeardownStore {
    fn load<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            let _guard = self
                .lock
                .lock()
                .expect("process store lock should be healthy");
            let record = self.read()?;
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
            let _guard = self
                .lock
                .lock()
                .expect("process store lock should be healthy");
            let current = self.read()?;
            if current == next {
                return Ok(WorkloadSagaCommit::Unchanged);
            }
            let observed = Some(current.revision());
            if expected != WorkloadSagaExpected::Revision(current.revision()) {
                return Err(WorkloadSagaStoreError::Conflict { expected, observed });
            }
            self.write(&next)?;
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

struct DurableForwarding {
    provider_instance: NetworkProviderHandle,
    provider_generation: NetworkResourceGeneration,
    state_path: PathBuf,
}

impl DurableForwarding {
    fn new(authority: &MachineForwarderAuthority, state_path: PathBuf) -> Self {
        Self {
            provider_instance: authority.provider_instance().clone(),
            provider_generation: authority.generation(),
            state_path,
        }
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

impl MachinePortForwardingRetirement for DurableForwarding {
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
        let state = fs::read_to_string(&self.state_path).map_err(|error| {
            nimbus_sandbox::SandboxError::OperationFailed {
                message: format!("two-realm forwarding state is unavailable: {error}"),
            }
        })?;
        Ok(if state == "absent" {
            MachinePortForwardingRetirementObservation::Absent(self.receipts(
                MachinePortForwardOutcome::ExactAlreadyAbsent,
                tenant_id,
                sandbox_id,
                bindings,
            ))
        } else {
            MachinePortForwardingRetirementObservation::Present(self.receipts(
                MachinePortForwardOutcome::Exposed,
                tenant_id,
                sandbox_id,
                bindings,
            ))
        })
    }

    fn withdraw_batch(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
    ) -> nimbus_sandbox::Result<Vec<MachinePortForwardReceipt>> {
        fs::write(&self.state_path, b"absent").map_err(|error| {
            nimbus_sandbox::SandboxError::OperationFailed {
                message: format!("two-realm forwarding state could not persist: {error}"),
            }
        })?;
        Ok(self.receipts(
            MachinePortForwardOutcome::Withdrawn,
            tenant_id,
            sandbox_id,
            bindings,
        ))
    }
}

fn find_parent_drain_request(root: &Path) -> MachineApiWorkloadTeardownPhaseRequest {
    fn visit(path: &Path, candidates: &mut Vec<MachineApiWorkloadTeardownPhaseRequest>) {
        if path.is_dir() {
            for child in fs::read_dir(path).unwrap() {
                visit(&child.unwrap().path(), candidates);
            }
            return;
        }
        if path.extension().is_none_or(|extension| extension != "json") {
            return;
        }
        let Ok(bytes) = fs::read(path) else {
            return;
        };
        let Ok(envelope) = serde_json::from_slice::<Value>(&bytes) else {
            return;
        };
        let Ok(observation) = serde_json::from_value::<ProviderCommandObservation>(
            envelope.get("observation").cloned().unwrap_or(Value::Null),
        ) else {
            return;
        };
        if observation.claim().operation() != ProviderCommandOperation::DrainExecution {
            return;
        }
        let Some(prepared) = observation.prepared_request() else {
            return;
        };
        if let Ok(request) = serde_json::from_slice(prepared) {
            candidates.push(request);
        }
    }
    let mut candidates = Vec::new();
    visit(
        &root.join(".nimbus-provider-command-attempts"),
        &mut candidates,
    );
    assert_eq!(
        candidates.len(),
        1,
        "one exact parent drain request should persist"
    );
    candidates.pop().unwrap()
}

fn write_guest_response(
    stream: &mut UnixStream,
    request: &MachineApiWorkloadTeardownPhaseRequest,
    result: nimbus_machine::api::MachineApiWorkloadTeardownPhaseResult,
) {
    let response = MachineApiWorkloadTeardownPhaseResponse::for_request(request, result)
        .expect("guest recovery response should correlate");
    let body = serde_json::to_vec(&response).expect("guest response should encode");
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

fn run_child(
    role: &str,
    parent_root: &Path,
    guest_root: &Path,
    socket: &Path,
    request: Option<&Path>,
    result: PathBuf,
) {
    let mut command = child_command(role, parent_root, guest_root, socket, &result);
    if let Some(request) = request {
        command.env(REQUEST_ENV, request);
    }
    let output = command.output().expect("two-realm child should run");
    assert!(
        output.status.success(),
        "two-realm {role} child failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn spawn_guest_server(
    parent_root: &Path,
    guest_root: &Path,
    socket: &Path,
    ready: &Path,
    result: &Path,
    expected_calls: usize,
) -> Child {
    let mut command = child_command("guest-server", parent_root, guest_root, socket, result);
    command
        .env(READY_ENV, ready)
        .env(EXPECTED_CALLS_ENV, expected_calls.to_string());
    command.spawn().expect("guest recovery server should spawn")
}

fn wait_for_child(child: &mut Child, description: &str) -> ExitStatus {
    let deadline = Instant::now() + WAIT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                child.kill().unwrap_or_else(|error| {
                    panic!("could not terminate timed-out {description}: {error}")
                });
                let status = child.wait().unwrap_or_else(|error| {
                    panic!("could not reap timed-out {description}: {error}")
                });
                panic!("timed out waiting for {description}; terminated child with {status}");
            }
            Err(error) => panic!("could not inspect {description}: {error}"),
        }
    }
}

fn child_command(
    role: &str,
    parent_root: &Path,
    guest_root: &Path,
    socket: &Path,
    result: &Path,
) -> Command {
    let mut command =
        Command::new(std::env::current_exe().expect("test executable should resolve"));
    command
        .args(["--exact", CHILD_TEST, "--ignored", "--nocapture"])
        .env(ROLE_ENV, role)
        .env(PARENT_ROOT_ENV, parent_root)
        .env(GUEST_ROOT_ENV, guest_root)
        .env(SOCKET_ENV, socket)
        .env(RESULT_ENV, result);
    command
}

fn wait_for_path(path: &Path) {
    let deadline = Instant::now() + WAIT;
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn env_path(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name} must be supplied")))
}

fn write_saga(root: &Path, record: &WorkloadSagaRecord) {
    fs::write(root.join(STORE_NAME), serde_json::to_vec(record).unwrap())
        .expect("two-realm saga should persist");
}

fn read_saga(root: &Path) -> WorkloadSagaRecord {
    serde_json::from_slice(&fs::read(root.join(STORE_NAME)).expect("two-realm saga should exist"))
        .expect("two-realm saga should decode")
}

fn snapshot_tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn collect(root: &Path, path: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        if !path.exists() {
            return;
        }
        if path.is_file() {
            out.insert(
                path.strip_prefix(root).unwrap().to_path_buf(),
                fs::read(path).unwrap(),
            );
            return;
        }
        for child in fs::read_dir(path).unwrap() {
            collect(root, &child.unwrap().path(), out);
        }
    }
    let mut snapshot = BTreeMap::new();
    collect(root, root, &mut snapshot);
    snapshot
}

fn store_error(_error: std::io::Error) -> WorkloadSagaStoreError {
    WorkloadSagaStoreError::Unavailable
}
