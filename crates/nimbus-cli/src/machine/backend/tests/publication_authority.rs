use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use nimbus::{
    EndpointProtocol, PublishedEndpoint, SandboxBackend, SandboxBackendKind, SandboxHandle,
    SandboxId, SandboxPortBinding, SandboxSpec, SandboxStatus, TenantId,
};
use nimbus_machine::MachineForwarderAuthority;
use nimbus_machine::api::{
    MachineApiServiceSandboxImageStartRequest, MachineApiServiceSandboxStopRequest,
};
use nimbus_network::{
    ListenerId, LocalNetworkStateStore, LocalPortLeaseAuthority, NetworkLeaseEpoch, NetworkPlanId,
    NetworkResourceGeneration, NetworkResourceId, PortBindRealm, PortBindTarget, PortBindingSpec,
    PortLeaseAccounting, PortLeaseFence, PortLeasePhase, PortLeaseRecord, PortLeaseRequest,
    PortProtocol, PortPublicationIntent, PortRequestMode,
};
use nimbus_sandbox::{
    MachinePortForwardOutcome, MachinePortForwardReceipt, SandboxCleanupObservation,
    SandboxExecutionObservation, SandboxInspection, SandboxRestartAssessment,
    SandboxRestartBlocker,
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;

use super::{image_spec, short_socket_tempdir, test_forwarder_authority};
use crate::machine::{ForwardedMachineApiSandboxBackend, MachineApiClient};

const HTTP_IO_TIMEOUT: Duration = Duration::from_secs(2);
const MUTATION_TIMEOUT_PROOF: Duration = Duration::from_millis(40);
const FRESH_PROCESS_CHILD_ENV: &str = "NIMBUS_NNC46E_PUBLICATION_CHILD";
const FRESH_PROCESS_ROOT_ENV: &str = "NIMBUS_NNC46E_PUBLICATION_ROOT";
const FRESH_PROCESS_SOCKET_ENV: &str = "NIMBUS_NNC46E_PUBLICATION_SOCKET";
const FRESH_PROCESS_HANDLE_ENV: &str = "NIMBUS_NNC46E_PUBLICATION_HANDLE";
const FRESH_PROCESS_TEST_NAME: &str = "machine::backend::tests::publication_authority::ambiguous_forwarded_stop_retains_parent_publication_fence_after_fresh_process_recovery";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn parent_publication_conflict_fails_before_machine_api_io() {
    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("machine-api.sock");
    let parent_root = temp_dir.path().join("parent-network");
    let parent = LocalPortLeaseAuthority::open(&parent_root).expect("parent authority should open");
    let authority = test_forwarder_authority();
    let tenant = tenant_id();
    let spec = publication_spec(&tenant, "database");
    let existing = standalone_publication_request(
        &tenant,
        "existing-parent-sandbox",
        &spec.port_bindings[0],
        authority.generation(),
    );
    let existing_record = parent
        .reserve(existing.clone())
        .expect("existing owner should reserve the conflicting parent port");
    let parent_bytes_before = authority_bytes(&parent_root);
    let server = ScriptedMachineApi::bind(
        socket_path.clone(),
        parent.clone(),
        [ScriptedReply::ExactStart],
    );
    let backend = forwarded_backend(
        MachineApiClient::new_for_test(socket_path).with_forwarder_authority(authority),
        &parent,
    );

    let result = backend.start(spec).await;
    let report = server.finish().await;

    let error = result.expect_err("the parent conflict must reject before Machine API I/O");
    let rendered = error.to_string();
    assert!(
        rendered.contains(existing.lease_id().as_str())
            && rendered.contains(resource_id_text(existing.owner_id())),
        "the conflict must identify the exact durable lease and owner without guessing: {rendered}"
    );
    assert_eq!(
        report.requests.len(),
        0,
        "a same-parent conflict must emit zero Machine API requests"
    );
    assert_eq!(
        parent
            .inspect(existing.lease_id())
            .expect("existing owner should inspect"),
        Some(existing_record),
        "the conflicting durable owner must remain byte-for-byte authoritative"
    );
    assert_eq!(authority_bytes(&parent_root), parent_bytes_before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn same_numeric_guest_and_parent_publications_use_distinct_authority_roots() {
    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("machine-api.sock");
    let parent_root = temp_dir.path().join("parent-network");
    let guest_root = temp_dir.path().join("guest-network");
    let parent = LocalPortLeaseAuthority::open(&parent_root).expect("parent authority should open");
    let guest = LocalPortLeaseAuthority::open(&guest_root).expect("guest authority should open");
    let authority = test_forwarder_authority();
    let tenant = tenant_id();
    let spec = publication_spec(&tenant, "database");
    let guest_request = standalone_publication_request(
        &tenant,
        "guest-wildcard-proxy",
        &spec.port_bindings[0],
        authority.generation(),
    );
    let guest_record = guest
        .reserve(guest_request.clone())
        .expect("guest host-realm port should reserve");
    let server = ScriptedMachineApi::bind(
        socket_path.clone(),
        parent.clone(),
        [ScriptedReply::ExactStart],
    );
    let backend = forwarded_backend(
        MachineApiClient::new_for_test(socket_path).with_forwarder_authority(authority),
        &parent,
    );

    let started = backend.start(spec).await;
    let report = server.finish().await;
    let started = started.expect("the separate parent root must not conflict with the guest root");
    let parent_records = assert_complete_active_batch(&parent, &started);

    assert_eq!(report.requests.len(), 1);
    assert!(
        parent_records
            .iter()
            .any(|record| record.reserved_port() == guest_record.reserved_port()),
        "the separate parent authority must admit the same numeric host port"
    );
    assert_eq!(
        guest
            .inspect(guest_request.lease_id())
            .expect("guest record should inspect"),
        Some(guest_record),
        "the parent transition must not mutate the guest authority root"
    );
    assert_ne!(authority_bytes(&parent_root), authority_bytes(&guest_root));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forwarded_start_activates_parent_publication_only_from_exact_complete_evidence() {
    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("machine-api.sock");
    let parent = LocalPortLeaseAuthority::open(temp_dir.path().join("parent-network"))
        .expect("parent authority should open");
    let authority = test_forwarder_authority();
    let tenant = tenant_id();
    let server = ScriptedMachineApi::bind(
        socket_path.clone(),
        parent.clone(),
        [ScriptedReply::ExactStart],
    );
    let backend = forwarded_backend(
        MachineApiClient::new_for_test(socket_path).with_forwarder_authority(authority.clone()),
        &parent,
    );

    let started = backend.start(publication_spec(&tenant, "database")).await;
    let report = server.finish().await;
    let started = started.expect("the exact complete receipt should start the sandbox");
    let records = assert_complete_active_batch(&parent, &started);

    assert_eq!(report.requests.len(), 1);
    assert!(report.requests[0].path.contains("image-start"));
    assert!(records.iter().all(|record| {
        record.request().generation() == authority.generation()
            && record.binding().is_some()
            && record.adoption_claim().is_some()
            && record.bind_claim().is_none()
    }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ambiguous_forwarded_start_retains_unpublished_parent_fence() {
    for reply in [
        ScriptedReply::PartialStart,
        ScriptedReply::StaleStart,
        ScriptedReply::CrossedStart,
        ScriptedReply::UntypedStart,
        ScriptedReply::EofStart,
        ScriptedReply::LostStartResponse,
        ScriptedReply::TimeoutStart,
        ScriptedReply::RefusedStart,
    ] {
        let case = AmbiguousStartCase::run(reply).await;
        assert!(
            case.result.is_err(),
            "{} must not report the forwarded workload ready",
            reply.label()
        );
        assert_eq!(
            case.records.len(),
            2,
            "{} must retain the complete two-port parent batch",
            reply.label()
        );
        assert!(
            case.records.iter().all(|record| {
                matches!(
                    record.phase(),
                    PortLeasePhase::Reserved | PortLeasePhase::CleanupPending
                ) && record.binding().is_none()
                    && record.adoption_claim().is_none()
                    && record.bind_claim().is_some()
                    && record.request().generation() == case.generation
            }),
            "{} must remain unpublished while retaining the provider claim and generation: {:?}",
            reply.label(),
            case.records
        );
        assert_eq!(
            plan_ids(&case.records).len(),
            1,
            "{} must retain one exact durable plan",
            reply.label()
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exact_forwarded_stop_withdraws_before_io_and_releases_after_receipt() {
    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("machine-api.sock");
    let parent = LocalPortLeaseAuthority::open(temp_dir.path().join("parent-network"))
        .expect("parent authority should open");
    let authority = test_forwarder_authority();
    let tenant = tenant_id();
    let server = ScriptedMachineApi::bind(
        socket_path.clone(),
        parent.clone(),
        [ScriptedReply::ExactStart, ScriptedReply::ExactStop],
    );
    let backend = forwarded_backend(
        MachineApiClient::new_for_test(socket_path).with_forwarder_authority(authority.clone()),
        &parent,
    );

    let started = backend.start(publication_spec(&tenant, "database")).await;
    let stop = match &started {
        Ok(handle) => backend.stop(&handle.id).await,
        Err(error) => Err(error.clone()),
    };
    let report = server.finish().await;
    let started = started.expect("exact start should establish the parent batch");
    stop.expect("exact authenticated absence should stop the workload");

    assert_eq!(report.requests.len(), 2);
    assert_eq!(
        report.requests[1].phases_before_response,
        vec![PortLeasePhase::Withdrawing; 2],
        "the complete parent batch must be Withdrawing before the stop request is answered: {:?}",
        report.requests[1]
    );
    let records = records_for_started_handle(&parent, &started);
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|record| {
        record.phase() == PortLeasePhase::Released
            && record.binding().is_some()
            && record.active_lifetime().is_none()
    }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn staged_attempt_with_released_exact_plan_converges_before_replacement() {
    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("machine-api.sock");
    let parent_root = temp_dir.path().join("parent-network");
    let parent = LocalPortLeaseAuthority::open(&parent_root).expect("parent authority should open");
    let authority = test_forwarder_authority();
    let tenant = tenant_id();
    let spec = publication_spec(&tenant, "database");
    let server = ScriptedMachineApi::bind(
        socket_path.clone(),
        parent.clone(),
        [ScriptedReply::ExactStart],
    );
    let crashed = forwarded_backend(
        MachineApiClient::new_for_test(socket_path.clone())
            .with_forwarder_authority(authority.clone()),
        &parent,
    );
    let staged = crashed
        .publication_intents
        .stage_service_attempt(
            &tenant,
            spec.service_name()
                .expect("publication fixture must have service identity"),
            &authority,
            &spec.port_bindings,
        )
        .expect("crash-cut attempt should stage");
    let claims =
        super::super::publication_claims(&staged).expect("staged attempt should derive claims");
    let reservation = parent
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&claims)
        .expect("crash-cut attempt should reserve its exact plan");
    let (_, lifetimes) = reservation.into_parts();
    let mut released = parent
        .release_provider_managed_claim_batch_after_confirmed_absence_with_lifetimes(
            &claims, &lifetimes,
        )
        .expect("exact absence should release the plan before the simulated crash");
    assert_eq!(released.len(), spec.port_bindings.len());
    assert!(
        released
            .iter()
            .all(|record| record.phase() == PortLeasePhase::Released),
        "the crash cut requires every exact lease terminal while its intent remains staged"
    );
    assert_eq!(
        crashed
            .publication_intents
            .load_plan(&staged.plan_id)
            .expect("staged crash-cut intent should load")
            .expect("staged crash-cut intent must exist")
            .phase,
        super::super::MachinePublicationIntentPhase::Staged,
        "the simulated crash must occur before the intent terminal write"
    );
    drop(lifetimes);
    drop(crashed);

    let recovered = forwarded_backend(
        MachineApiClient::new_for_test(socket_path).with_forwarder_authority(authority),
        &parent,
    );
    let started = recovered.start(spec).await;
    let report = server.finish().await;
    let started = started.expect(
        "fresh recovery must terminalize the exact released attempt and stage a replacement",
    );

    assert_eq!(
        recovered
            .publication_intents
            .load_plan(&staged.plan_id)
            .expect("recovered crash-cut intent should load")
            .expect("recovered crash-cut intent must remain recorded")
            .phase,
        super::super::MachinePublicationIntentPhase::Terminal
    );
    assert_ne!(
        started.id, staged.sandbox_id,
        "recovery must start a new fenced attempt instead of reusing terminal identity"
    );
    let mut terminal_records = parent
        .list_plan(&staged.plan_id)
        .expect("terminal crash-cut plan should list");
    terminal_records
        .sort_by(|left, right| left.request().lease_id().cmp(right.request().lease_id()));
    released.sort_by(|left, right| left.request().lease_id().cmp(right.request().lease_id()));
    assert_eq!(
        terminal_records, released,
        "recovery must authenticate and preserve the exact terminal lease records"
    );
    assert_eq!(
        report.requests.len(),
        1,
        "terminal crash recovery must emit only the replacement start request"
    );
    assert_complete_active_batch(&parent, &started);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ambiguous_forwarded_stop_retains_parent_publication_fence_after_fresh_process_recovery() {
    if std::env::var_os(FRESH_PROCESS_CHILD_ENV).is_some() {
        run_forwarded_start_crash_child().await;
        unreachable!("the crash child exits without running destructors");
    }

    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("machine-api.sock");
    let parent_root = temp_dir.path().join("parent-network");
    let parent = LocalPortLeaseAuthority::open(&parent_root).expect("parent authority should open");
    let authority = test_forwarder_authority();
    let tenant = tenant_id();
    let server = ScriptedMachineApi::bind(
        socket_path.clone(),
        parent.clone(),
        [
            ScriptedReply::ExactStart,
            ScriptedReply::EofStop,
            ScriptedReply::ExactStop,
        ],
    );
    let handle_path = temp_dir.path().join("started-sandbox-id");
    let mut child_command =
        Command::new(std::env::current_exe().expect("test executable path should resolve"));
    child_command
        .args(["--exact", FRESH_PROCESS_TEST_NAME, "--nocapture"])
        .env(FRESH_PROCESS_CHILD_ENV, "1")
        .env(FRESH_PROCESS_ROOT_ENV, &parent_root)
        .env(FRESH_PROCESS_SOCKET_ENV, &socket_path)
        .env(FRESH_PROCESS_HANDLE_ENV, &handle_path);
    let child = run_bounded_child(&mut child_command, Duration::from_secs(20));
    assert!(
        child.status.success(),
        "fresh-process start child failed: stdout={} stderr={}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    let started_id = SandboxId::new(
        fs::read_to_string(&handle_path)
            .expect("crash child should record its parent-selected sandbox ID"),
    );

    let reopened =
        LocalPortLeaseAuthority::open(&parent_root).expect("fresh parent authority should reopen");
    let client =
        MachineApiClient::new_for_test(socket_path).with_forwarder_authority(authority.clone());
    let fresh_backend = forwarded_backend(client, &reopened);
    let stop = fresh_backend.stop(&started_id).await;

    assert!(stop.is_err(), "EOF cannot prove exact provider absence");
    let started = SandboxHandle::new(
        tenant.clone(),
        started_id,
        "database",
        SandboxBackendKind::Container,
        SandboxStatus::Ready,
        Vec::new(),
    );
    let records = records_for_started_handle(&reopened, &started);
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|record| {
        record.phase() == PortLeasePhase::CleanupPending
            && record.binding().is_some()
            && record.adoption_claim().is_some()
            && record.active_lifetime().is_some()
            && record.request().generation() == authority.generation()
    }));

    let conflict = standalone_publication_request(
        &tenant,
        "replacement-sandbox",
        &publication_spec(&tenant, "replacement").port_bindings[0],
        authority
            .generation()
            .checked_next()
            .expect("test generation should advance"),
    );
    assert!(
        reopened.reserve(conflict).is_err(),
        "ambiguous cleanup must retain the host-port conflict across reopen"
    );

    fresh_backend
        .stop(&started.id)
        .await
        .expect("an exact retry must converge the fresh-process cleanup");
    let report = server.finish().await;
    assert_eq!(report.requests.len(), 3);
    assert_eq!(
        report.requests[1].phases_before_response,
        vec![PortLeasePhase::CleanupPending; 2],
        "fresh-process recovery must durably fence the whole batch before ambiguous stop I/O: {:?}",
        report.requests[1]
    );
    assert_eq!(
        report.requests[2].phases_before_response,
        vec![PortLeasePhase::CleanupPending; 2],
        "the exact retry must preserve the full fence until its response: {:?}",
        report.requests[2]
    );
    assert!(
        records_for_started_handle(&reopened, &started)
            .iter()
            .all(|record| record.phase() == PortLeasePhase::Released),
        "exact absence must terminally release every recovered batch member"
    );
}

fn run_bounded_child(command: &mut Command, timeout: Duration) -> Output {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("fresh-process start child should spawn");
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .expect("finished fresh-process child output should collect");
            }
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let kill_error = child.kill().err();
                let output = child
                    .wait_with_output()
                    .expect("timed-out fresh-process child output should collect");
                panic!(
                    "fresh-process start child exceeded {timeout:?}; kill_error={kill_error:?}; \
                     stdout={} stderr={}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Err(error) => {
                let kill_error = child.kill().err();
                let output = child
                    .wait_with_output()
                    .expect("failed fresh-process child output should collect");
                panic!(
                    "fresh-process start child status failed: {error}; kill_error={kill_error:?}; \
                     stdout={} stderr={}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
    }
}

async fn run_forwarded_start_crash_child() {
    let parent_root = PathBuf::from(
        std::env::var_os(FRESH_PROCESS_ROOT_ENV).expect("child root must be present"),
    );
    let socket_path = PathBuf::from(
        std::env::var_os(FRESH_PROCESS_SOCKET_ENV).expect("child socket must be present"),
    );
    let handle_path = PathBuf::from(
        std::env::var_os(FRESH_PROCESS_HANDLE_ENV).expect("child handle path must be present"),
    );
    let parent =
        LocalPortLeaseAuthority::open(parent_root).expect("child parent authority should open");
    let backend = forwarded_backend(
        MachineApiClient::new_for_test(socket_path)
            .with_forwarder_authority(test_forwarder_authority()),
        &parent,
    );
    let started = backend
        .start(publication_spec(&tenant_id(), "database"))
        .await
        .expect("child exact start should succeed");
    fs::write(handle_path, started.id.as_str()).expect("child should record the sandbox ID");
    std::process::exit(0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stale_machine_generation_cannot_activate_or_release_current_publication() {
    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("machine-api.sock");
    let parent = LocalPortLeaseAuthority::open(temp_dir.path().join("parent-network"))
        .expect("parent authority should open");
    let generation = NetworkResourceGeneration::new(9);
    let current_authority = authority_at_generation(generation);
    let tenant = tenant_id();
    let server = ScriptedMachineApi::bind(
        socket_path.clone(),
        parent.clone(),
        [
            ScriptedReply::ExactStart,
            ScriptedReply::StaleStop,
            ScriptedReply::ExactStop,
        ],
    );
    let current = forwarded_backend(
        MachineApiClient::new_for_test(socket_path.clone())
            .with_forwarder_authority(current_authority.clone()),
        &parent,
    );

    let started = current.start(publication_spec(&tenant, "database")).await;
    let started = started.expect("current generation should activate");
    let before = records_for_started_handle(&parent, &started);
    assert_eq!(
        before.len(),
        2,
        "current-generation activation must establish the complete parent batch"
    );
    let stop = current.stop(&started.id).await;

    assert!(
        stop.is_err(),
        "stale guest receipt must not release current publication authority"
    );
    let after = records_for_started_handle(&parent, &started);
    assert_eq!(after.len(), before.len());
    assert!(after.iter().zip(&before).all(|(current, prior)| {
        current.phase() != PortLeasePhase::Released
            && current.request().generation() == current_authority.generation()
            && current.binding() == prior.binding()
            && current.adoption_claim() == prior.adoption_claim()
    }));

    current
        .stop(&started.id)
        .await
        .expect("the exact current-generation retry must converge");
    let report = server.finish().await;
    assert_eq!(report.requests.len(), 3);
    assert!(
        records_for_started_handle(&parent, &started)
            .iter()
            .all(|record| record.phase() == PortLeasePhase::Released),
        "only current-generation exact absence may release the publication"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forwarded_inspect_leaves_parent_publication_authority_byte_unchanged() {
    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("machine-api.sock");
    let parent_root = temp_dir.path().join("parent-network");
    let parent = LocalPortLeaseAuthority::open(&parent_root).expect("parent authority should open");
    let authority = test_forwarder_authority();
    let tenant = tenant_id();
    let server = ScriptedMachineApi::bind(
        socket_path.clone(),
        parent.clone(),
        [ScriptedReply::ExactStart, ScriptedReply::Inspect],
    );
    let backend = forwarded_backend(
        MachineApiClient::new_for_test(socket_path).with_forwarder_authority(authority),
        &parent,
    );

    let started = backend.start(publication_spec(&tenant, "database")).await;
    let before = authority_bytes_if_exists(&parent_root);
    let inspected = match &started {
        Ok(handle) => backend.inspect(&handle.id).await,
        Err(error) => Err(error.clone()),
    };
    let report = server.finish().await;
    let started = started.expect("exact start should establish the parent batch");
    assert_complete_active_batch(&parent, &started);
    assert_eq!(report.requests.len(), 2);
    let inspected = inspected
        .expect("inspect should succeed")
        .expect("started handle should inspect");

    assert_eq!(
        inspected,
        scripted_inspection(&started),
        "the forwarded adapter must preserve every typed field and the exact comparison version"
    );
    assert_eq!(
        before,
        authority_bytes_if_exists(&parent_root),
        "the parent authority bytes must not change across the read path"
    );
}

/// Compose the production parent state machine over an isolated primitive.
///
/// The production constructor additionally retains a process-owned
/// `HostMachineNetworkAuthority`; this test-only constructor keeps durable
/// batch behavior observable without claiming the process-global manager.
fn forwarded_backend(
    client: MachineApiClient,
    parent: &LocalPortLeaseAuthority,
) -> ForwardedMachineApiSandboxBackend {
    ForwardedMachineApiSandboxBackend::new_for_test(client, parent.clone())
        .expect("forwarded parent publication backend should compose")
}

struct AmbiguousStartCase {
    result: Result<SandboxHandle, nimbus::SandboxError>,
    records: Vec<PortLeaseRecord>,
    generation: NetworkResourceGeneration,
}

impl AmbiguousStartCase {
    async fn run(reply: ScriptedReply) -> Self {
        let temp_dir = short_socket_tempdir();
        let socket_path = temp_dir.path().join(format!("{}.sock", reply.label()));
        let parent = LocalPortLeaseAuthority::open(
            temp_dir
                .path()
                .join(format!("parent-network-{}", reply.label())),
        )
        .expect("parent authority should open");
        let authority = test_forwarder_authority();
        let generation = authority.generation();
        let client = MachineApiClient::new_for_test(socket_path.clone())
            .with_mutation_io_timeout_for_test(MUTATION_TIMEOUT_PROOF)
            .with_forwarder_authority(authority);
        let server = (!matches!(reply, ScriptedReply::RefusedStart))
            .then(|| ScriptedMachineApi::bind(socket_path, parent.clone(), [reply]));
        let backend = forwarded_backend(client, &parent);

        let result = backend
            .start(publication_spec(&tenant_id(), "database"))
            .await;
        if let Some(server) = server {
            let _ = server.finish().await;
        }

        Self {
            result,
            records: parent.list().expect("parent records should list"),
            generation,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScriptedReply {
    ExactStart,
    PartialStart,
    StaleStart,
    CrossedStart,
    UntypedStart,
    EofStart,
    LostStartResponse,
    TimeoutStart,
    RefusedStart,
    ExactStop,
    StaleStop,
    EofStop,
    Inspect,
}

impl ScriptedReply {
    fn label(self) -> &'static str {
        match self {
            Self::ExactStart => "exact-start",
            Self::PartialStart => "partial-start",
            Self::StaleStart => "stale-start",
            Self::CrossedStart => "crossed-start",
            Self::UntypedStart => "untyped-start",
            Self::EofStart => "eof-start",
            Self::LostStartResponse => "lost-start-response",
            Self::TimeoutStart => "timeout-start",
            Self::RefusedStart => "refused-start",
            Self::ExactStop => "exact-stop",
            Self::StaleStop => "stale-stop",
            Self::EofStop => "eof-stop",
            Self::Inspect => "inspect",
        }
    }
}

struct ScriptedMachineApi {
    shutdown: watch::Sender<bool>,
    join: tokio::task::JoinHandle<Result<ScriptedReport, String>>,
}

impl ScriptedMachineApi {
    fn bind(
        socket_path: PathBuf,
        authority: LocalPortLeaseAuthority,
        replies: impl IntoIterator<Item = ScriptedReply>,
    ) -> Self {
        let listener = UnixListener::bind(&socket_path).expect("scripted Machine API should bind");
        let replies = replies.into_iter().collect::<VecDeque<_>>();
        let (shutdown, shutdown_rx) = watch::channel(false);
        let join = tokio::spawn(run_scripted_machine_api(
            listener,
            authority,
            replies,
            shutdown_rx,
        ));
        Self { shutdown, join }
    }

    async fn finish(self) -> ScriptedReport {
        let _ = self.shutdown.send(true);
        self.join
            .await
            .expect("scripted Machine API task should join")
            .expect("scripted Machine API should complete without fixture errors")
    }
}

#[derive(Debug)]
struct ScriptedReport {
    requests: Vec<ObservedRequest>,
}

#[derive(Debug)]
struct ObservedRequest {
    path: String,
    phases_before_response: Vec<PortLeasePhase>,
}

#[derive(Clone)]
struct LastStarted {
    handle: SandboxHandle,
    bindings: Vec<SandboxPortBinding>,
}

async fn run_scripted_machine_api(
    listener: UnixListener,
    authority: LocalPortLeaseAuthority,
    mut replies: VecDeque<ScriptedReply>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<ScriptedReport, String> {
    let mut requests = Vec::new();
    let mut last_started = None;
    loop {
        let accepted = tokio::select! {
            changed = shutdown.changed() => {
                changed.map_err(|error| error.to_string())?;
                break;
            }
            accepted = listener.accept() => accepted.map_err(|error| error.to_string())?,
        };
        let reply = replies
            .pop_front()
            .ok_or_else(|| "scripted Machine API received an unexpected request".to_owned())?;
        handle_scripted_request(
            accepted.0,
            reply,
            &authority,
            &mut last_started,
            &mut requests,
            shutdown.clone(),
        )
        .await?;
    }
    Ok(ScriptedReport { requests })
}

async fn handle_scripted_request(
    mut stream: UnixStream,
    reply: ScriptedReply,
    authority: &LocalPortLeaseAuthority,
    last_started: &mut Option<LastStarted>,
    requests: &mut Vec<ObservedRequest>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), String> {
    let request = read_http_request(&mut stream).await?;
    requests.push(ObservedRequest {
        path: request.path.clone(),
        phases_before_response: authority
            .list()
            .map_err(|error| error.to_string())?
            .iter()
            .map(PortLeaseRecord::phase)
            .collect(),
    });

    match reply {
        ScriptedReply::ExactStart
        | ScriptedReply::PartialStart
        | ScriptedReply::StaleStart
        | ScriptedReply::CrossedStart
        | ScriptedReply::UntypedStart => {
            let (body, started) = start_response(&request.body, reply)?;
            *last_started = Some(started);
            write_json_response(&mut stream, &body).await?;
        }
        ScriptedReply::EofStart | ScriptedReply::EofStop => {}
        ScriptedReply::LostStartResponse => {
            let (body, started) = start_response(&request.body, ScriptedReply::ExactStart)?;
            *last_started = Some(started);
            write_incomplete_json_response(&mut stream, &body).await?;
        }
        ScriptedReply::TimeoutStart => {
            shutdown
                .changed()
                .await
                .map_err(|error| error.to_string())?;
        }
        ScriptedReply::ExactStop => {
            let body = stop_response(&request.body, last_started, false)?;
            write_json_response(&mut stream, &body).await?;
        }
        ScriptedReply::StaleStop => {
            let body = stop_response(&request.body, last_started, true)?;
            write_json_response(&mut stream, &body).await?;
        }
        ScriptedReply::Inspect => {
            let started = last_started
                .as_ref()
                .ok_or_else(|| "inspect script requires a prior start".to_owned())?;
            let inspection = scripted_inspection(&started.handle);
            write_json_response(
                &mut stream,
                &json!({
                    "sandbox_id": started.handle.id,
                    "inspection": inspection,
                }),
            )
            .await?;
        }
        ScriptedReply::RefusedStart => {
            return Err("refusal is represented by no listening socket".to_owned());
        }
    }
    Ok(())
}

fn start_response(
    request_body: &Value,
    reply: ScriptedReply,
) -> Result<(Value, LastStarted), String> {
    let request: MachineApiServiceSandboxImageStartRequest =
        serde_json::from_value(request_body.clone()).map_err(|error| error.to_string())?;
    let handle = sandbox_handle(&request.sandbox_id, &request.spec);
    let mut response_authority = request.forwarder_authority.clone();
    let mut receipts = request
        .spec
        .port_bindings
        .iter()
        .map(|binding| {
            receipt(
                MachinePortForwardOutcome::Exposed,
                &request.spec.tenant_id,
                &request.sandbox_id,
                binding,
                &request.forwarder_authority,
            )
        })
        .collect::<Vec<_>>();

    match reply {
        ScriptedReply::ExactStart => {}
        ScriptedReply::PartialStart => {
            receipts.pop();
        }
        ScriptedReply::StaleStart => {
            response_authority = authority_at_generation(
                request
                    .forwarder_authority
                    .generation()
                    .as_u64()
                    .checked_sub(1)
                    .map(NetworkResourceGeneration::new)
                    .unwrap_or_else(|| NetworkResourceGeneration::new(u64::MAX)),
            );
            for receipt in &mut receipts {
                receipt.provider_generation = response_authority.generation();
            }
        }
        ScriptedReply::CrossedStart => {
            receipts[0].sandbox_id = SandboxId::new("crossed-sandbox-incarnation");
        }
        ScriptedReply::UntypedStart => {
            return Ok((
                json!({
                    "handle": handle.clone(),
                    "forwarder_authority": request.forwarder_authority.clone(),
                    "publication_evidence": request.spec.port_bindings.clone(),
                }),
                LastStarted {
                    handle,
                    bindings: request.spec.port_bindings,
                },
            ));
        }
        other => return Err(format!("unexpected start response script {other:?}")),
    }

    Ok((
        json!({
            "handle": handle.clone(),
            "forwarder_authority": response_authority,
            "publication_evidence": receipts,
        }),
        LastStarted {
            handle,
            bindings: request.spec.port_bindings,
        },
    ))
}

fn stop_response(
    request_body: &Value,
    last_started: &Option<LastStarted>,
    stale: bool,
) -> Result<Value, String> {
    let request: MachineApiServiceSandboxStopRequest =
        serde_json::from_value(request_body.clone()).map_err(|error| error.to_string())?;
    let started = last_started
        .as_ref()
        .ok_or_else(|| "stop script requires a prior start".to_owned())?;
    let authority = if stale {
        authority_at_generation(
            request
                .forwarder_authority
                .generation()
                .as_u64()
                .checked_sub(1)
                .map(NetworkResourceGeneration::new)
                .unwrap_or_else(|| NetworkResourceGeneration::new(u64::MAX)),
        )
    } else {
        request.forwarder_authority
    };
    let receipts = started
        .bindings
        .iter()
        .enumerate()
        .map(|(index, binding)| {
            receipt(
                if index == 0 {
                    MachinePortForwardOutcome::Withdrawn
                } else {
                    MachinePortForwardOutcome::ExactAlreadyAbsent
                },
                &started.handle.tenant_id,
                &started.handle.id,
                binding,
                &authority,
            )
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "sandbox_id": started.handle.id,
        "tenant_id": started.handle.tenant_id,
        "stopped": true,
        "forwarder_authority": authority,
        "confirmed_absent_evidence": receipts,
    }))
}

fn scripted_inspection(handle: &SandboxHandle) -> SandboxInspection {
    SandboxInspection::provider_reported(handle.clone()).with_provider_projection(
        handle.clone(),
        SandboxExecutionObservation::Exited { exit_code: 42 },
        SandboxRestartAssessment::Candidate {
            exit_code: 42,
            completed_restarts: 1,
            retry_delay_millis: 2_000,
            persisted_not_before_millis: Some(9_000),
            blocker: Some(SandboxRestartBlocker::StartupReconciliationUnavailable),
        },
        SandboxCleanupObservation::Retained,
    )
}

fn receipt(
    outcome: MachinePortForwardOutcome,
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    binding: &SandboxPortBinding,
    authority: &MachineForwarderAuthority,
) -> MachinePortForwardReceipt {
    MachinePortForwardReceipt {
        outcome,
        tenant_id: tenant_id.clone(),
        sandbox_id: sandbox_id.clone(),
        binding: binding.clone(),
        provider_instance: authority.provider_instance().clone(),
        provider_generation: authority.generation(),
    }
}

struct HttpRequest {
    path: String,
    body: Value,
}

async fn read_http_request(stream: &mut UnixStream) -> Result<HttpRequest, String> {
    let mut bytes = Vec::new();
    let mut header_end = None;
    let mut content_length = None;
    loop {
        let mut chunk = [0_u8; 4096];
        let read = tokio::time::timeout(HTTP_IO_TIMEOUT, stream.read(&mut chunk))
            .await
            .map_err(|_| "scripted Machine API request read timed out".to_owned())?
            .map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        if header_end.is_none()
            && let Some(offset) = find_bytes(&bytes, b"\r\n\r\n")
        {
            let end = offset + 4;
            let headers = std::str::from_utf8(&bytes[..end]).map_err(|error| error.to_string())?;
            content_length = Some(parse_content_length(headers)?);
            header_end = Some(end);
        }
        if let (Some(end), Some(length)) = (header_end, content_length)
            && bytes.len() >= end + length
        {
            break;
        }
    }

    let header_end = header_end.ok_or_else(|| "Machine API request had no headers".to_owned())?;
    let headers = std::str::from_utf8(&bytes[..header_end]).map_err(|error| error.to_string())?;
    let mut request_line = headers
        .lines()
        .next()
        .ok_or_else(|| "Machine API request had no request line".to_owned())?
        .split_whitespace();
    let _method = request_line
        .next()
        .ok_or_else(|| "Machine API request had no method".to_owned())?;
    let path = request_line
        .next()
        .ok_or_else(|| "Machine API request had no path".to_owned())?
        .to_owned();
    let length = content_length.unwrap_or(0);
    let body = if length == 0 {
        json!({})
    } else {
        serde_json::from_slice(&bytes[header_end..header_end + length])
            .map_err(|error| error.to_string())?
    };
    Ok(HttpRequest { path, body })
}

fn parse_content_length(headers: &str) -> Result<usize, String> {
    headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim())
        })
        .unwrap_or("0")
        .parse()
        .map_err(|error| format!("invalid content length: {error}"))
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

async fn write_json_response(stream: &mut UnixStream, body: &Value) -> Result<(), String> {
    let body = serde_json::to_vec(body).map_err(|error| error.to_string())?;
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .await
        .map_err(|error| error.to_string())?;
    stream
        .write_all(&body)
        .await
        .map_err(|error| error.to_string())
}

async fn write_incomplete_json_response(
    stream: &mut UnixStream,
    body: &Value,
) -> Result<(), String> {
    let body = serde_json::to_vec(body).map_err(|error| error.to_string())?;
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .await
        .map_err(|error| error.to_string())?;
    let partial = body.len().min(3);
    stream
        .write_all(&body[..partial])
        .await
        .map_err(|error| error.to_string())
}

fn publication_spec(tenant_id: &TenantId, service_name: &str) -> SandboxSpec {
    image_spec(tenant_id, service_name, "docker://busybox:latest").with_port_binding(
        SandboxPortBinding::new("postgres", EndpointProtocol::Tcp, 15432, 5432),
    )
}

fn tenant_id() -> TenantId {
    TenantId::new("tenant-a").expect("fixture tenant should validate")
}

fn sandbox_handle(id: &SandboxId, spec: &SandboxSpec) -> SandboxHandle {
    let endpoints = spec
        .port_bindings
        .iter()
        .map(|binding| {
            PublishedEndpoint::new(
                binding.name.clone(),
                binding.protocol,
                binding.host_socket_addr(),
            )
        })
        .collect();
    SandboxHandle::new(
        spec.tenant_id.clone(),
        id.clone(),
        spec.display_name(),
        SandboxBackendKind::Container,
        SandboxStatus::Ready,
        endpoints,
    )
}

fn authority_at_generation(generation: NetworkResourceGeneration) -> MachineForwarderAuthority {
    let base = test_forwarder_authority();
    MachineForwarderAuthority::new(base.provider_instance().clone(), generation)
}

fn standalone_publication_request(
    tenant_id: &TenantId,
    sandbox_incarnation: &str,
    binding: &SandboxPortBinding,
    generation: NetworkResourceGeneration,
) -> PortLeaseRequest {
    let listener =
        ListenerId::for_tenant_workload_listener(tenant_id, sandbox_incarnation, &binding.name);
    PortLeaseRequest::new(
        nimbus_network::PortLeaseId::for_listener(&listener),
        listener.into(),
        Some(tenant_id.clone()),
        PortLeaseFence::new(generation, NetworkLeaseEpoch::new(1)),
        PortLeaseAccounting::TenantPublished,
        PortPublicationIntent::host(binding.host_address),
        PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_wildcard(),
            if binding.host_address.is_loopback() {
                nimbus_network::PortExposure::Loopback
            } else {
                nimbus_network::PortExposure::Public
            },
            PortRequestMode::Exact(
                NonZeroU16::new(binding.host_port).expect("fixture port must be non-zero"),
            ),
        ),
    )
}

fn assert_complete_active_batch(
    authority: &LocalPortLeaseAuthority,
    handle: &SandboxHandle,
) -> Vec<PortLeaseRecord> {
    let records = records_for_started_handle(authority, handle);
    assert_eq!(
        records.len(),
        2,
        "the exact two-port publication plan must be durable"
    );
    assert!(
        records
            .iter()
            .all(|record| record.phase() == PortLeasePhase::Active),
        "exact evidence must activate every plan member: {records:?}"
    );
    let plans = plan_ids(&records);
    assert_eq!(plans.len(), 1);
    assert_eq!(
        handle.id.as_str(),
        format!(
            "machine-api:{}",
            plans.iter().next().expect("one plan should exist")
        ),
        "the parent-selected sandbox identity must be the tenant-qualified plan identity"
    );
    records
}

fn records_for_started_handle(
    authority: &LocalPortLeaseAuthority,
    handle: &SandboxHandle,
) -> Vec<PortLeaseRecord> {
    let expected_plan = handle
        .id
        .as_str()
        .strip_prefix("machine-api:")
        .and_then(|value| value.parse::<NetworkPlanId>().ok());
    match expected_plan {
        Some(plan_id) => authority
            .list_plan(&plan_id)
            .expect("started plan should list"),
        None => authority.list().expect("parent records should list"),
    }
}

fn plan_ids(records: &[PortLeaseRecord]) -> BTreeSet<NetworkPlanId> {
    records
        .iter()
        .map(|record| {
            record
                .request()
                .plan_id()
                .expect("machine publication must carry one plan identity")
                .clone()
        })
        .collect()
}

fn resource_id_text(resource_id: &NetworkResourceId) -> &str {
    match resource_id {
        NetworkResourceId::Attachment(id) => id.as_str(),
        NetworkResourceId::Segment(id) => id.as_str(),
        NetworkResourceId::PublishedEndpoint(id) => id.as_str(),
        NetworkResourceId::Listener(id) => id.as_str(),
        NetworkResourceId::IngressRoute(id) => id.as_str(),
        NetworkResourceId::PortLease(id) => id.as_str(),
    }
}

fn authority_bytes(root: &Path) -> Vec<u8> {
    fs::read(LocalNetworkStateStore::authority_path_for(root))
        .expect("authority state should exist")
}

fn authority_bytes_if_exists(root: &Path) -> Option<Vec<u8>> {
    fs::read(LocalNetworkStateStore::authority_path_for(root)).ok()
}
