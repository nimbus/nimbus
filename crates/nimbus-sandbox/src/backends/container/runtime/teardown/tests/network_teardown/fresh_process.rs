//! Fresh-process Container attachment checkpoint and contention proofs.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use crate::backends::container::ContainerSandboxBackendConfig;
use crate::backends::container::runtime::ContainerLaunchArtifact;
use crate::backends::oci::network::{
    FixedOciEgressPinProvider, HostManagedAttachmentCheckpointTestProbe,
    HostManagedAttachmentDetachPhase, HostManagedAttachmentReleasePhase,
    HostManagedAttachmentTeardownCheckpoint,
};
use crate::{ProviderCommandAttemptJournal, SandboxId};

use super::*;

const ROOT_ENV: &str = "NIMBUS_NNC65D3_CONTAINER_ROOT";
const ACTION_ENV: &str = "NIMBUS_NNC65D3_CONTAINER_ACTION";
const OPERATION_ENV: &str = "NIMBUS_NNC65D3_CONTAINER_OPERATION";
const PHASE_ENV: &str = "NIMBUS_NNC65D3_CONTAINER_PHASE";
const ROLE_ENV: &str = "NIMBUS_NNC65D3_CONTAINER_ROLE";
const SANDBOX_ENV: &str = "NIMBUS_NNC65D3_CONTAINER_SANDBOX";
const PEP_PORT_ENV: &str = "NIMBUS_NNC65D3_CONTAINER_PEP_PORT";
const CHILD_TEST: &str = "backends::container::runtime::teardown::tests::network_teardown::fresh_process::nnc6_5d3_container_network_child";
const CRASH_EXIT: i32 = 86;
const TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy)]
struct CrashCase {
    label: &'static str,
    checkpoint: HostManagedAttachmentTeardownCheckpoint,
}

const DETACH_CASES: &[CrashCase] = &[
    detach_case(
        "attachment-deleting",
        HostManagedAttachmentDetachPhase::AttachmentDeleting,
    ),
    detach_case(
        "segment-quarantined",
        HostManagedAttachmentDetachPhase::SegmentQuarantined,
    ),
    detach_case(
        "pep-stop-may-exist",
        HostManagedAttachmentDetachPhase::PepStopMayExist,
    ),
    detach_case(
        "pep-retained",
        HostManagedAttachmentDetachPhase::PepRetained,
    ),
    detach_case(
        "listener-stop-may-exist",
        HostManagedAttachmentDetachPhase::ListenerStopMayExist,
    ),
    detach_case(
        "provider-delete-may-exist",
        HostManagedAttachmentDetachPhase::ProviderDeleteMayExist,
    ),
    detach_case(
        "provider-absent",
        HostManagedAttachmentDetachPhase::ProviderAbsent,
    ),
    detach_case(
        "namespace-remove-may-exist",
        HostManagedAttachmentDetachPhase::NamespaceRemoveMayExist,
    ),
    detach_case(
        "namespace-absent",
        HostManagedAttachmentDetachPhase::NamespaceAbsent,
    ),
    detach_case(
        "listeners-retained",
        HostManagedAttachmentDetachPhase::ListenersRetained,
    ),
    detach_case("detached", HostManagedAttachmentDetachPhase::Detached),
];

const RELEASE_CASES: &[CrashCase] = &[
    release_case(
        "release-authenticated",
        HostManagedAttachmentReleasePhase::ReleaseAuthenticated,
    ),
    release_case(
        "pep-release-may-exist",
        HostManagedAttachmentReleasePhase::PepReleaseMayExist,
    ),
    release_case(
        "pep-released",
        HostManagedAttachmentReleasePhase::PepReleased,
    ),
    release_case(
        "listener-release-may-exist",
        HostManagedAttachmentReleasePhase::ListenerReleaseMayExist,
    ),
    release_case(
        "listeners-released",
        HostManagedAttachmentReleasePhase::ListenersReleased,
    ),
    release_case(
        "ipam-release-may-exist",
        HostManagedAttachmentReleasePhase::IpamReleaseMayExist,
    ),
    release_case(
        "ipam-released",
        HostManagedAttachmentReleasePhase::IpamReleased,
    ),
    release_case(
        "segment-release-may-exist",
        HostManagedAttachmentReleasePhase::SegmentReleaseMayExist,
    ),
    release_case(
        "segment-released",
        HostManagedAttachmentReleasePhase::SegmentReleased,
    ),
    release_case(
        "attachment-release-may-exist",
        HostManagedAttachmentReleasePhase::AttachmentReleaseMayExist,
    ),
    release_case("released", HostManagedAttachmentReleasePhase::Released),
];

const fn detach_case(label: &'static str, phase: HostManagedAttachmentDetachPhase) -> CrashCase {
    CrashCase {
        label,
        checkpoint: HostManagedAttachmentTeardownCheckpoint::Detach(phase),
    }
}

const fn release_case(label: &'static str, phase: HostManagedAttachmentReleasePhase) -> CrashCase {
    CrashCase {
        label,
        checkpoint: HostManagedAttachmentTeardownCheckpoint::Release(phase),
    }
}

#[test]
fn fresh_process_network_teardown_container_recovers_every_durable_checkpoint() {
    for case in DETACH_CASES.iter().chain(RELEASE_CASES) {
        if std::env::var("NIMBUS_NNC65D3_ONLY_CHECKPOINT").is_ok_and(|label| label != case.label) {
            continue;
        }
        let operation = match case.checkpoint {
            HostManagedAttachmentTeardownCheckpoint::Detach(_) => {
                SandboxNetworkTeardownOperation::Detach
            }
            HostManagedAttachmentTeardownCheckpoint::Release(_) => {
                SandboxNetworkTeardownOperation::Release
            }
        };
        let fixture = prepared_fixture(&format!("crash-{}", case.label), operation);
        let writer = run_child(
            fixture.root.path(),
            &fixture.id,
            fixture.pep_port,
            "crash",
            operation,
            Some(case.label),
            None,
        );
        assert_eq!(
            writer.status.code(),
            Some(CRASH_EXIT),
            "writer must die at {}\nstdout:\n{}\nstderr:\n{}",
            case.label,
            String::from_utf8_lossy(&writer.stdout),
            String::from_utf8_lossy(&writer.stderr),
        );

        let recovery = run_child(
            fixture.root.path(),
            &fixture.id,
            fixture.pep_port,
            "recover",
            operation,
            Some(case.label),
            None,
        );
        assert_success(&recovery, case.label);
        let stdout = String::from_utf8_lossy(&recovery.stdout);
        assert!(stdout.contains("NNC65D3_CONTAINER_INSPECT:"), "{stdout}");
        assert!(
            stdout.contains("NNC65D3_CONTAINER_CONVERGED:succeeded"),
            "{stdout}"
        );
        assert_ne!(child_pid(&writer.stdout), child_pid(&recovery.stdout));
        if let Some(sentinel) = fixture.artifact_sentinel.as_ref() {
            assert!(
                !sentinel.exists(),
                "recovery at {} must remove the exact provider-owned launch artifact",
                case.label
            );
        }
        if let Some(runtime_artifacts) = fixture.runtime_artifacts.as_ref() {
            assert!(
                runtime_artifacts.iter().all(|path| !path.exists()),
                "recovery at {} must remove the exact stale runtime receipts",
                case.label
            );
        }

        let backend = reopen_backend(fixture.root.path(), fixture.pep_port);
        if operation == SandboxNetworkTeardownOperation::Release {
            assert!(
                backend.startup_reconciliation_error.is_none(),
                "terminal Container replay at {} must accept the released desired tombstone after its provider retry witness is retired: {:?}",
                case.label,
                backend.startup_reconciliation_error
            );
        }
        let command = network_command_from_backend(&backend, &fixture.id, operation);
        let manifest = backend
            .read_manifest(command.sandbox_id())
            .expect("recovered manifest should read")
            .expect("recovered manifest should exist");
        match operation {
            SandboxNetworkTeardownOperation::Detach => {
                assert_eq!(
                    manifest.network_teardown.detach_phase(),
                    HostManagedAttachmentDetachPhase::Detached
                );
                assert!(manifest.network_teardown.detached_proof().is_some());
            }
            SandboxNetworkTeardownOperation::Release => assert_eq!(
                manifest.network_teardown.release_phase(),
                HostManagedAttachmentReleasePhase::Released
            ),
        }
        assert_eq!(
            backend
                .attempt_idempotency_journal()
                .expect("recovered journal should open")
                .adopt_exact_attempt(command.provider_claim())
                .expect("recovered result should read")
                .expect("recovered result should exist")
                .kind(),
            ProviderCommandObservationKind::Succeeded
        );
    }
}

#[test]
fn fresh_process_container_network_contenders_publish_one_result_per_operation() {
    let fixture = prepared_fixture(
        "process-contention",
        SandboxNetworkTeardownOperation::Detach,
    );
    for operation in [
        SandboxNetworkTeardownOperation::Detach,
        SandboxNetworkTeardownOperation::Release,
    ] {
        let gate = fixture
            .root
            .path()
            .join(format!("gate-{}", operation_label(operation)));
        let first = spawn_contender(
            fixture.root.path(),
            &fixture.id,
            fixture.pep_port,
            operation,
            "first",
            &gate,
        );
        wait_for_path(
            &fixture
                .root
                .path()
                .join(format!("ready-{}-first", operation_label(operation))),
        );
        let second = spawn_contender(
            fixture.root.path(),
            &fixture.id,
            fixture.pep_port,
            operation,
            "second",
            &gate,
        );
        wait_for_path(
            &fixture
                .root
                .path()
                .join(format!("ready-{}-second", operation_label(operation))),
        );
        std::fs::write(&gate, b"start\n").expect("contention gate should open");
        let outputs = wait_contenders(first, second, operation);
        for output in &outputs {
            assert_success(output, operation_label(operation));
        }
        assert_eq!(
            outputs
                .iter()
                .filter(|output| String::from_utf8_lossy(&output.stdout).contains("ROLE:execute"))
                .count(),
            1
        );
        assert_eq!(
            outputs
                .iter()
                .filter(|output| String::from_utf8_lossy(&output.stdout).contains("ROLE:adopt"))
                .count(),
            1
        );
        assert_ne!(child_pid(&outputs[0].stdout), child_pid(&outputs[1].stdout));
    }
}

#[test]
#[ignore = "subprocess entry point; NNC6.5d3 parent supplies exact durable roots"]
fn nnc6_5d3_container_network_child() {
    let root = PathBuf::from(required_env(ROOT_ENV));
    let action = required_env(ACTION_ENV);
    let operation = parse_operation(&required_env(OPERATION_ENV));
    let sandbox_id = SandboxId::new(required_env(SANDBOX_ENV));
    let pep_port = required_env(PEP_PORT_ENV)
        .parse()
        .expect("Container PEP port should parse");
    println!("NNC65D3_CONTAINER_PID:{}", std::process::id());
    match action.as_str() {
        "crash" => crash_child(
            &root,
            &sandbox_id,
            pep_port,
            operation,
            &required_env(PHASE_ENV),
        ),
        "recover" => recovery_child(&root, &sandbox_id, pep_port, operation),
        "contend" => contention_child(
            &root,
            &sandbox_id,
            pep_port,
            operation,
            &required_env(ROLE_ENV),
        ),
        _ => panic!("unknown Container network child action {action:?}"),
    }
}

struct PreparedFixture {
    root: tempfile::TempDir,
    id: SandboxId,
    pep_port: u16,
    /// The child processes below re-open the backend on `pep_port` and bind
    /// the egress proxy there, so this parent keeps the window claimed for the
    /// whole run rather than releasing it with the fixture it came from.
    _port_window: Option<PortWindow>,
    artifact_sentinel: Option<PathBuf>,
    runtime_artifacts: Option<[PathBuf; 3]>,
}

fn prepared_fixture(label: &str, operation: SandboxNetworkTeardownOperation) -> PreparedFixture {
    let fixture = TeardownFixture::attached(label);
    let drain = fixture.command(SandboxExecutionTeardownOperation::Drain, label, 1);
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&drain),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    let stop = fixture.command(SandboxExecutionTeardownOperation::Stop, label, 1);
    let runtime = fixture.runtime_for_terminal_stop(&stop);
    let pep_port = fixture
        .manifest()
        .egress_proxy
        .as_ref()
        .expect("prepared fixture should retain its PEP")
        .port;
    if operation == SandboxNetworkTeardownOperation::Release {
        let detach = network_command(&fixture, &stop, SandboxNetworkTeardownOperation::Detach, 1);
        assert_eq!(
            execute_network(&fixture, &detach).kind(),
            ProviderCommandObservationKind::Succeeded
        );
    }
    let artifact_sentinel = if operation == SandboxNetworkTeardownOperation::Release {
        let mut manifest = fixture.manifest();
        let artifact_root = crate::artifact_paths::rootfs_root(
            &fixture.backend.config.workload_state_root,
            &manifest.spec.tenant_id,
            &fixture.id,
        )
        .join(fixture.id.as_str());
        let rootfs_path = artifact_root.join("rootfs");
        std::fs::create_dir_all(&rootfs_path).expect("Container owned rootfs should create");
        let sentinel = rootfs_path.join("provider-finality-sentinel");
        std::fs::write(&sentinel, b"owned").expect("Container artifact sentinel should persist");
        manifest.launch_artifact = Some(ContainerLaunchArtifact::Rootfs(
            crate::backends::oci::materializer::MaterializedImageRootfs {
                image_reference: "registry.example.com/nimbus/finality:test".to_owned(),
                rootfs_path,
            },
        ));
        fixture
            .backend
            .write_existing_workload_manifest(&manifest)
            .expect("Container release fixture should retain its owned launch artifact");
        Some(sentinel)
    } else {
        None
    };
    let runtime_artifacts = if operation == SandboxNetworkTeardownOperation::Release {
        let mut manifest = fixture.manifest();
        Some(retain_stale_runtime_artifacts(
            &fixture,
            &mut manifest,
            "fresh-process-release",
        ))
    } else {
        None
    };
    let TeardownFixture {
        root,
        port_window,
        backend,
        id,
        ..
    } = fixture;
    drop(runtime);
    drop(backend);
    PreparedFixture {
        root,
        id,
        pep_port,
        _port_window: port_window,
        artifact_sentinel,
        runtime_artifacts,
    }
}

fn crash_child(
    root: &Path,
    sandbox_id: &SandboxId,
    pep_port: u16,
    operation: SandboxNetworkTeardownOperation,
    phase: &str,
) {
    let checkpoint = parse_checkpoint(operation, phase);
    let backend = reopen_backend(root, pep_port).with_network_teardown_checkpoint_test_probe(
        HostManagedAttachmentCheckpointTestProbe::exit_after(checkpoint, CRASH_EXIT),
    );
    let command = network_command_from_backend(&backend, sandbox_id, operation);
    let journal = backend
        .attempt_idempotency_journal()
        .expect("crash writer journal should open");
    let execution = match journal
        .claim_dispatch_epoch(command.provider_claim())
        .expect("crash writer should claim")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("fresh crash command claimed twice")
        }
    };
    let result = backend.execute_network_teardown_with_claim(&command, execution);
    let inspection = result.as_ref().ok().map(|observation| {
        backend.inspect_network_teardown_with_observation(&command, observation)
    });
    panic!(
        "checkpoint {phase} did not terminate the writer process: {result:?}; inspection={inspection:?}"
    );
}

fn recovery_child(
    root: &Path,
    sandbox_id: &SandboxId,
    pep_port: u16,
    operation: SandboxNetworkTeardownOperation,
) {
    let backend = reopen_backend(root, pep_port);
    if operation == SandboxNetworkTeardownOperation::Release
        && required_env(PHASE_ENV) == "released"
    {
        let startup_terminal = backend
            .read_manifest(sandbox_id)
            .expect("startup-finalized Container manifest should read")
            .expect("startup-finalized Container manifest should exist");
        assert!(startup_terminal.has_terminal_network_finality());
    }
    let command = network_command_from_backend(&backend, sandbox_id, operation);
    let journal = backend
        .attempt_idempotency_journal()
        .expect("recovery journal should open");
    let claimed = journal
        .adopt_exact_attempt(command.provider_claim())
        .expect("crashed current claim should read")
        .expect("crashed current claim should exist");
    assert_eq!(claimed.kind(), ProviderCommandObservationKind::Claimed);
    let before = snapshot_files(root);
    let inspected = backend.inspect_network_teardown_with_observation(&command, &claimed);
    assert_eq!(snapshot_files(root), before, "Inspect must be byte-stable");
    assert!(matches!(
        inspected,
        SandboxNetworkTeardownObservation::InProgress { .. }
            | SandboxNetworkTeardownObservation::RetryAuthorized { .. }
            | SandboxNetworkTeardownObservation::Succeeded { .. }
    ));
    println!("NNC65D3_CONTAINER_INSPECT:{inspected:?}");
    let execution = journal
        .resume_current_claim(&claimed)
        .expect("fresh process should recover the exact claimed effect");
    let result = backend
        .execute_network_teardown_with_claim(&command, execution)
        .expect("recovered execution should publish");
    assert_eq!(result.kind(), ProviderCommandObservationKind::Succeeded);
    if operation == SandboxNetworkTeardownOperation::Release {
        let terminal = backend
            .read_manifest(sandbox_id)
            .expect("terminal Container manifest should read")
            .expect("terminal Container manifest should exist");
        assert!(terminal.has_terminal_network_finality());
    }
    println!("NNC65D3_CONTAINER_CONVERGED:succeeded");
}

fn contention_child(
    root: &Path,
    sandbox_id: &SandboxId,
    pep_port: u16,
    operation: SandboxNetworkTeardownOperation,
    role: &str,
) {
    let backend = reopen_backend(root, pep_port);
    let command = network_command_from_backend(&backend, sandbox_id, operation);
    let journal = backend
        .attempt_idempotency_journal()
        .expect("contender journal should open");
    let ready = root.join(format!("ready-{}-{role}", operation_label(operation)));
    std::fs::write(&ready, b"ready\n").expect("contender readiness should persist");
    wait_for_path(&root.join(format!("gate-{}", operation_label(operation))));
    match journal
        .claim_dispatch_epoch(command.provider_claim())
        .expect("contender should reach the exact stream")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => {
            let result = backend
                .execute_network_teardown_with_claim(&command, execution)
                .expect("winning contender should publish");
            if result.kind() != ProviderCommandObservationKind::Succeeded {
                let inspected =
                    backend.inspect_network_teardown_with_observation(&command, &result);
                panic!(
                    "winning Container contender returned {result:?}; exact inspection: {inspected:?}; evidence: {}",
                    String::from_utf8_lossy(inspected.evidence())
                );
            }
            println!("NNC65D3_CONTAINER_ROLE:execute");
        }
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            wait_for_contender_success(&journal, command.provider_claim());
            println!("NNC65D3_CONTAINER_ROLE:adopt");
        }
    }
}

fn network_command_from_backend(
    backend: &ContainerSandboxBackend,
    sandbox_id: &SandboxId,
    operation: SandboxNetworkTeardownOperation,
) -> SandboxNetworkTeardownCommand {
    let manifest = backend
        .read_manifest(sandbox_id)
        .expect("exact Container manifest should read")
        .expect("exact Container manifest should exist");
    let stop_claim = match manifest.execution_teardown.stop() {
        ContainerStopProgress::ExecutionStopped { fence, .. } => fence,
        progress => panic!("network recovery requires ExecutionStopped, got {progress:?}"),
    };
    let plan = manifest
        .provision_network_plan
        .as_ref()
        .expect("network recovery requires the compiled plan");
    let identity = SandboxNetworkTeardownIdentity::new(SandboxNetworkTeardownIdentityInput {
        tenant_id: manifest.spec.tenant_id.clone(),
        sandbox_id: manifest.handle.id.clone(),
        execution_attempt_id: manifest.execution_attempt_id.clone(),
        attachment_id: plan.attachment_id().clone(),
        network_plan: plan.network_plan().clone(),
        provider_registration_key: CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY.to_owned(),
        provider_source_digest: NetworkCapabilitySourceDigest::from_bytes([9; 32]),
    })
    .expect("recovered identity should validate");
    let claim = ProviderCommandClaim::new(ProviderCommandClaimInput {
        authority_id: stop_claim.authority_id().to_owned(),
        effect_subject: identity.provider_effect_subject(),
        source_attempt_id: stop_claim.source_attempt_id().map(str::to_owned),
        attempt_id: stop_claim.attempt_id().to_owned(),
        dispatch_epoch: 1,
        workload_generation: stop_claim.workload_generation(),
        restart_ordinal: stop_claim.restart_ordinal(),
        desired_digest: stop_claim.desired_digest().to_owned(),
        source_digest: stop_claim.source_digest().to_owned(),
        network_plan_digest: stop_claim.network_plan_digest().to_owned(),
        provider_target_digest: identity.provider_target_digest(),
        operation: operation.provider_operation(),
    })
    .expect("recovered provider claim should validate");
    SandboxNetworkTeardownCommand::new(SandboxNetworkTeardownCommandInput {
        identity,
        operation,
        provider_claim: claim,
    })
    .expect("recovered network command should validate")
}

fn reopen_backend(root: &Path, pep_port: u16) -> ContainerSandboxBackend {
    let mut config = ContainerSandboxBackendConfig::under_root(root);
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    config.published_port_range = pep_port..=pep_port;
    config.netavark_path = PathBuf::from("/usr/bin/true");
    ContainerSandboxBackend::new(config)
        .with_egress_pin_provider(Arc::new(FixedOciEgressPinProvider::ready()))
}

fn parse_checkpoint(
    operation: SandboxNetworkTeardownOperation,
    label: &str,
) -> HostManagedAttachmentTeardownCheckpoint {
    let cases = match operation {
        SandboxNetworkTeardownOperation::Detach => DETACH_CASES,
        SandboxNetworkTeardownOperation::Release => RELEASE_CASES,
    };
    cases
        .iter()
        .find(|case| case.label == label)
        .unwrap_or_else(|| panic!("unknown Container network checkpoint {label:?}"))
        .checkpoint
}

fn parse_operation(value: &str) -> SandboxNetworkTeardownOperation {
    match value {
        "detach" => SandboxNetworkTeardownOperation::Detach,
        "release" => SandboxNetworkTeardownOperation::Release,
        _ => panic!("unknown Container network operation {value:?}"),
    }
}

const fn operation_label(operation: SandboxNetworkTeardownOperation) -> &'static str {
    match operation {
        SandboxNetworkTeardownOperation::Detach => "detach",
        SandboxNetworkTeardownOperation::Release => "release",
    }
}

fn run_child(
    root: &Path,
    sandbox_id: &SandboxId,
    pep_port: u16,
    action: &str,
    operation: SandboxNetworkTeardownOperation,
    phase: Option<&str>,
    role: Option<&str>,
) -> Output {
    let mut command = child_command(root, sandbox_id, pep_port, action, operation);
    if let Some(phase) = phase {
        command.env(PHASE_ENV, phase);
    }
    if let Some(role) = role {
        command.env(ROLE_ENV, role);
    }
    command
        .output()
        .expect("Container network child should start")
}

fn spawn_contender(
    root: &Path,
    sandbox_id: &SandboxId,
    pep_port: u16,
    operation: SandboxNetworkTeardownOperation,
    role: &str,
    _gate: &Path,
) -> Child {
    let mut command = child_command(root, sandbox_id, pep_port, "contend", operation);
    command
        .env(ROLE_ENV, role)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Container network contender should start")
}

fn child_command(
    root: &Path,
    sandbox_id: &SandboxId,
    pep_port: u16,
    action: &str,
    operation: SandboxNetworkTeardownOperation,
) -> Command {
    let mut command =
        Command::new(std::env::current_exe().expect("test executable should resolve"));
    command
        .arg("--exact")
        .arg(CHILD_TEST)
        .arg("--ignored")
        .arg("--nocapture")
        .env(ROOT_ENV, root)
        .env(SANDBOX_ENV, sandbox_id.as_str())
        .env(PEP_PORT_ENV, pep_port.to_string())
        .env(ACTION_ENV, action)
        .env(OPERATION_ENV, operation_label(operation));
    command
}

fn wait_contenders(
    mut first: Child,
    mut second: Child,
    operation: SandboxNetworkTeardownOperation,
) -> [Output; 2] {
    let started = Instant::now();
    loop {
        let first_done = first
            .try_wait()
            .expect("first child status should read")
            .is_some();
        let second_done = second
            .try_wait()
            .expect("second child status should read")
            .is_some();
        if first_done && second_done {
            return [
                first
                    .wait_with_output()
                    .expect("first child output should read"),
                second
                    .wait_with_output()
                    .expect("second child output should read"),
            ];
        }
        if started.elapsed() >= TIMEOUT {
            let _ = first.kill();
            let _ = second.kill();
            let first_output = first
                .wait_with_output()
                .expect("timed-out first child output should read");
            let second_output = second
                .wait_with_output()
                .expect("timed-out second child output should read");
            panic!(
                "Container network {operation:?} contenders timed out\nfirst stdout:\n{}\nfirst stderr:\n{}\nsecond stdout:\n{}\nsecond stderr:\n{}",
                String::from_utf8_lossy(&first_output.stdout),
                String::from_utf8_lossy(&first_output.stderr),
                String::from_utf8_lossy(&second_output.stdout),
                String::from_utf8_lossy(&second_output.stderr),
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn wait_for_path(path: &Path) {
    let started = Instant::now();
    while !path.exists() {
        assert!(
            started.elapsed() < TIMEOUT,
            "timed out waiting for {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn wait_for_contender_success(
    journal: &ProviderCommandAttemptJournal,
    claim: &ProviderCommandClaim,
) {
    let started = Instant::now();
    loop {
        let current = journal
            .adopt_exact_attempt(claim)
            .expect("contender result should remain readable")
            .expect("contender result should remain present");
        if current.kind() == ProviderCommandObservationKind::Succeeded {
            return;
        }
        assert!(
            matches!(
                current.kind(),
                ProviderCommandObservationKind::Claimed
                    | ProviderCommandObservationKind::InProgress
                    | ProviderCommandObservationKind::Ambiguous
            ),
            "winning contender published unexpected result {current:?}"
        );
        assert!(
            started.elapsed() < TIMEOUT,
            "winning Container contender did not publish before timeout"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn child_pid(bytes: &[u8]) -> u32 {
    String::from_utf8_lossy(bytes)
        .lines()
        .find_map(|line| line.strip_prefix("NNC65D3_CONTAINER_PID:"))
        .expect("child output should include its PID")
        .parse()
        .expect("child PID should parse")
}

fn assert_success(output: &Output, label: &str) {
    assert!(
        output.status.success(),
        "Container network child failed for {label}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("required environment variable {name} is absent"))
}
