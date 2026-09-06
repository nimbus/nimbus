//! Fresh-process Krun attachment checkpoint and contention proofs.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use crate::backends::oci::network::{
    HostManagedAttachmentCheckpointTestProbe, HostManagedAttachmentDetachPhase,
    HostManagedAttachmentReleasePhase, HostManagedAttachmentTeardownCheckpoint,
};
use crate::{
    ProviderCommandAttemptJournal, ProviderCommandClaimInput, SandboxNetworkTeardownCommandInput,
    SandboxNetworkTeardownIdentity, SandboxNetworkTeardownIdentityInput,
};

use super::*;

const ROOT_ENV: &str = "NIMBUS_NNC65D3_KRUN_ROOT";
const ACTION_ENV: &str = "NIMBUS_NNC65D3_KRUN_ACTION";
const OPERATION_ENV: &str = "NIMBUS_NNC65D3_KRUN_OPERATION";
const PHASE_ENV: &str = "NIMBUS_NNC65D3_KRUN_PHASE";
const ROLE_ENV: &str = "NIMBUS_NNC65D3_KRUN_ROLE";
const SANDBOX_ENV: &str = "NIMBUS_NNC65D3_KRUN_SANDBOX";
const PEP_PORT_ENV: &str = "NIMBUS_NNC65D3_KRUN_PEP_PORT";
const CHILD_TEST: &str = "backends::krun::vm::teardown::tests::network_teardown::fresh_process::nnc6_5d3_krun_network_child";
const CRASH_EXIT: i32 = 87;
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
fn fresh_process_network_teardown_krun_recovers_every_durable_checkpoint() {
    for case in DETACH_CASES.iter().chain(RELEASE_CASES) {
        if std::env::var("NIMBUS_NNC65D3_KRUN_ONLY_CHECKPOINT")
            .is_ok_and(|label| label != case.label)
        {
            continue;
        }
        let operation = operation_for(case.checkpoint);
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
        assert!(stdout.contains("NNC65D3_KRUN_INSPECT:"), "{stdout}");
        assert!(
            stdout.contains("NNC65D3_KRUN_CONVERGED:succeeded"),
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
                backend.startup_network_reconciliation_error.is_none(),
                "terminal Krun replay at {} must accept the released desired tombstone after its provider retry witness is retired: {:?}",
                case.label,
                backend.startup_network_reconciliation_error
            );
        }
        let command = network_command_from_backend(&backend, &fixture.id, operation);
        let manifest = backend
            .read_manifest(&fixture.id)
            .expect("recovered Krun manifest should read")
            .expect("recovered Krun manifest should exist");
        match operation {
            SandboxNetworkTeardownOperation::Detach => assert_eq!(
                manifest.network_teardown.detach_phase(),
                HostManagedAttachmentDetachPhase::Detached
            ),
            SandboxNetworkTeardownOperation::Release => assert_eq!(
                manifest.network_teardown.release_phase(),
                HostManagedAttachmentReleasePhase::Released
            ),
        }
        assert_eq!(
            backend
                .attempt_idempotency_journal()
                .expect("recovered Krun journal should open")
                .adopt_exact_attempt(command.provider_claim())
                .expect("recovered Krun result should read")
                .expect("recovered Krun result should exist")
                .kind(),
            ProviderCommandObservationKind::Succeeded
        );
    }
}

#[test]
fn fresh_process_krun_network_contenders_publish_one_result_per_operation() {
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
        );
        wait_for_path(
            &fixture
                .root
                .path()
                .join(format!("ready-{}-second", operation_label(operation))),
        );
        std::fs::write(&gate, b"start\n").expect("Krun contention gate should open");
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
fn fresh_process_interrupted_adoption_converges_and_replays_without_writes() {
    for (label, cut) in [
        (
            "launch-reserved",
            InterruptedAdoptionAllocatorCut::LaunchReserved,
        ),
        (
            "adoption-intent-reserved",
            InterruptedAdoptionAllocatorCut::AdoptionIntentReserved,
        ),
        ("adopted", InterruptedAdoptionAllocatorCut::Adopted),
        (
            "reservation-cleanup-pending",
            InterruptedAdoptionAllocatorCut::ReservationCleanupPending,
        ),
        ("absent", InterruptedAdoptionAllocatorCut::Absent),
    ] {
        let fixture = prepared_interrupted_adoption(label, cut);
        let recovery = run_child(
            fixture.root.path(),
            &fixture.id,
            fixture.pep_port,
            "recover-adoption",
            SandboxNetworkTeardownOperation::Detach,
            None,
            None,
        );
        assert_success(&recovery, label);
        assert!(
            String::from_utf8_lossy(&recovery.stdout)
                .contains("NNC65G_KRUN_ADOPTION_RECOVERED:released"),
            "{}",
            String::from_utf8_lossy(&recovery.stdout)
        );

        let before_replay = snapshot_files(fixture.root.path());
        let replay = run_child(
            fixture.root.path(),
            &fixture.id,
            fixture.pep_port,
            "replay-adoption",
            SandboxNetworkTeardownOperation::Detach,
            None,
            None,
        );
        assert_success(&replay, label);
        assert!(
            String::from_utf8_lossy(&replay.stdout)
                .contains("NNC65G_KRUN_ADOPTION_REPLAY:byte-stable"),
            "{}",
            String::from_utf8_lossy(&replay.stdout)
        );
        assert_ne!(child_pid(&recovery.stdout), child_pid(&replay.stdout));
        assert_eq!(
            snapshot_files(fixture.root.path()),
            before_replay,
            "terminal exact replay must not change a durable byte for allocator cut {cut:?}",
        );

        let backend = reopen_backend(fixture.root.path(), fixture.pep_port);
        let terminal = backend
            .read_manifest(&fixture.id)
            .expect("recovered adopting manifest should read")
            .expect("recovered adopting manifest should exist");
        assert_eq!(terminal.launch_authority, KrunLaunchAuthority::Released);
        assert_eq!(
            terminal.network_teardown.release_phase(),
            HostManagedAttachmentReleasePhase::Released
        );
        let config = terminal
            .network_config
            .as_ref()
            .expect("terminal adopting manifest retains identity evidence");
        assert_eq!(
            backend
                .segment_allocator
                .inspect_attachment_reservation(
                    &terminal.spec.tenant_id,
                    &config.attachment_id,
                    &config.reservation_claim,
                )
                .expect("terminal adopting allocator state should inspect")
                .state(),
            NetworkAttachmentReservationState::Absent
        );
        assert!(
            backend
                .port_lease_coordinator()
                .port_lease_records_snapshot(
                    &terminal.port_leases,
                    "terminal interrupted-adoption listeners",
                )
                .expect("terminal interrupted-adoption listeners should inspect")
                .iter()
                .all(|record| record.phase() == PortLeasePhase::Released)
        );
    }
}

#[test]
#[ignore = "subprocess entry point; NNC6.5d3 parent supplies exact durable roots"]
fn nnc6_5d3_krun_network_child() {
    let root = PathBuf::from(required_env(ROOT_ENV));
    let sandbox_id = SandboxId::new(required_env(SANDBOX_ENV));
    let operation = parse_operation(&required_env(OPERATION_ENV));
    let pep_port = required_env(PEP_PORT_ENV)
        .parse()
        .expect("Krun PEP port should parse");
    println!("NNC65D3_KRUN_PID:{}", std::process::id());
    match required_env(ACTION_ENV).as_str() {
        "crash" => crash_child(
            &root,
            &sandbox_id,
            pep_port,
            operation,
            &required_env(PHASE_ENV),
        ),
        "recover" => recovery_child(&root, &sandbox_id, pep_port, operation),
        "recover-adoption" => recover_interrupted_adoption_child(&root, &sandbox_id, pep_port),
        "replay-adoption" => replay_interrupted_adoption_child(&root, &sandbox_id, pep_port),
        "contend" => contention_child(
            &root,
            &sandbox_id,
            pep_port,
            operation,
            &required_env(ROLE_ENV),
        ),
        action => panic!("unknown Krun network child action {action:?}"),
    }
}

struct PreparedFixture {
    root: tempfile::TempDir,
    id: SandboxId,
    pep_port: u16,
    artifact_sentinel: Option<PathBuf>,
    runtime_artifacts: Option<[PathBuf; 3]>,
    /// Carried over from the fixture that reserved `pep_port`. Every child
    /// process below rebinds that exact port, so the claim is held for the
    /// prepared fixture's whole life rather than read.
    _port_window: PortWindow,
}

fn prepared_fixture(label: &str, operation: SandboxNetworkTeardownOperation) -> PreparedFixture {
    let fixture = NetworkTeardownFixture::attached(label);
    let stop = fixture.stop_execution(label);
    if operation == SandboxNetworkTeardownOperation::Release {
        let detach = fixture.network_command(&stop, SandboxNetworkTeardownOperation::Detach, 1);
        assert_eq!(
            execute_network(&fixture.backend, &detach).kind(),
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
        std::fs::create_dir_all(&rootfs_path).expect("Krun owned rootfs should create");
        let sentinel = rootfs_path.join("provider-finality-sentinel");
        std::fs::write(&sentinel, b"owned").expect("Krun artifact sentinel should persist");
        manifest.launch_artifact = Some(KrunLaunchArtifact::Rootfs(
            crate::backends::oci::materializer::MaterializedImageRootfs {
                image_reference: "registry.example.com/nimbus/finality:test".to_owned(),
                rootfs_path,
            },
        ));
        fixture
            .backend
            .write_manifest(&manifest)
            .expect("Krun release fixture should retain its owned launch artifact");
        Some(sentinel)
    } else {
        None
    };
    let runtime_artifacts = if operation == SandboxNetworkTeardownOperation::Release {
        Some(retain_stale_runtime_artifacts(&fixture.manifest()))
    } else {
        None
    };
    let pep_port = fixture
        .manifest()
        .egress_proxy
        .as_ref()
        .expect("prepared Krun fixture should retain its PEP")
        .port;
    let NetworkTeardownFixture {
        root,
        backend,
        runtime,
        id,
        port_window,
        ..
    } = fixture;
    drop(runtime);
    drop(backend);
    PreparedFixture {
        root,
        id,
        pep_port,
        artifact_sentinel,
        runtime_artifacts,
        _port_window: port_window,
    }
}

fn prepared_interrupted_adoption(
    label: &str,
    cut: InterruptedAdoptionAllocatorCut,
) -> PreparedFixture {
    let fixture =
        NetworkTeardownFixture::interrupted_adoption(&format!("fresh-process-{label}"), cut);
    let pep_port = fixture
        .manifest()
        .egress_proxy
        .as_ref()
        .expect("interrupted-adoption fixture should retain its PEP reservation")
        .port;
    let NetworkTeardownFixture {
        root,
        backend,
        runtime,
        id,
        port_window,
        ..
    } = fixture;
    drop(runtime);
    drop(backend);
    PreparedFixture {
        root,
        id,
        pep_port,
        artifact_sentinel: None,
        runtime_artifacts: None,
        _port_window: port_window,
    }
}

fn crash_child(
    root: &Path,
    sandbox_id: &SandboxId,
    pep_port: u16,
    operation: SandboxNetworkTeardownOperation,
    phase: &str,
) {
    let backend = reopen_backend(root, pep_port).with_network_teardown_checkpoint_test_probe(
        HostManagedAttachmentCheckpointTestProbe::exit_after(
            parse_checkpoint(operation, phase),
            CRASH_EXIT,
        ),
    );
    let command = network_command_from_backend(&backend, sandbox_id, operation);
    let journal = backend
        .attempt_idempotency_journal()
        .expect("Krun crash writer journal should open");
    let execution = match journal
        .claim_dispatch_epoch(command.provider_claim())
        .expect("Krun crash writer should claim")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("fresh Krun command claimed twice")
        }
    };
    let result = backend.execute_network_teardown_with_claim(&command, execution);
    panic!("Krun checkpoint {phase} did not terminate the writer process: {result:?}");
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
            .expect("startup-finalized Krun manifest should read")
            .expect("startup-finalized Krun manifest should exist");
        assert!(startup_terminal.has_terminal_network_finality());
    }
    let command = network_command_from_backend(&backend, sandbox_id, operation);
    let journal = backend
        .attempt_idempotency_journal()
        .expect("Krun recovery journal should open");
    let claimed = journal
        .adopt_exact_attempt(command.provider_claim())
        .expect("crashed Krun claim should read")
        .expect("crashed Krun claim should exist");
    assert_eq!(claimed.kind(), ProviderCommandObservationKind::Claimed);
    let before = snapshot_files(root);
    let inspected = backend.inspect_network_teardown_with_observation(&command, &claimed);
    assert_eq!(
        snapshot_files(root),
        before,
        "Krun Inspect must be byte-stable"
    );
    assert!(matches!(
        inspected,
        crate::SandboxNetworkTeardownObservation::InProgress { .. }
            | crate::SandboxNetworkTeardownObservation::RetryAuthorized { .. }
            | crate::SandboxNetworkTeardownObservation::Succeeded { .. }
    ));
    println!("NNC65D3_KRUN_INSPECT:{inspected:?}");
    let execution = journal
        .resume_current_claim(&claimed)
        .expect("fresh Krun process should recover the claimed effect");
    let result = backend
        .execute_network_teardown_with_claim(&command, execution)
        .expect("recovered Krun execution should publish");
    assert_eq!(result.kind(), ProviderCommandObservationKind::Succeeded);
    if operation == SandboxNetworkTeardownOperation::Release {
        let terminal = backend
            .read_manifest(sandbox_id)
            .expect("terminal Krun manifest should read")
            .expect("terminal Krun manifest should exist");
        assert!(terminal.has_terminal_network_finality());
    }
    println!("NNC65D3_KRUN_CONVERGED:succeeded");
}

fn recover_interrupted_adoption_child(root: &Path, sandbox_id: &SandboxId, pep_port: u16) {
    let backend = reopen_backend(root, pep_port);
    let journal = backend
        .attempt_idempotency_journal()
        .expect("interrupted-adoption recovery journal should open");
    for operation in [
        SandboxExecutionTeardownOperation::Drain,
        SandboxExecutionTeardownOperation::Stop,
    ] {
        let command = execution_command_from_backend(&backend, sandbox_id, operation);
        let execution = match journal
            .claim_dispatch_epoch(command.provider_claim())
            .expect("interrupted-adoption execution command should claim")
        {
            ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
            ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
                panic!("fresh interrupted-adoption execution command claimed twice")
            }
        };
        let result = backend
            .execute_execution_teardown_with_claim(&command, execution)
            .expect("interrupted-adoption execution result should publish");
        assert_eq!(result.kind(), ProviderCommandObservationKind::Succeeded);
    }
    for operation in [
        SandboxNetworkTeardownOperation::Detach,
        SandboxNetworkTeardownOperation::Release,
    ] {
        let command = network_command_from_backend(&backend, sandbox_id, operation);
        let execution = match journal
            .claim_dispatch_epoch(command.provider_claim())
            .expect("interrupted-adoption network command should claim")
        {
            ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
            ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
                panic!("fresh interrupted-adoption network command claimed twice")
            }
        };
        let result = backend
            .execute_network_teardown_with_claim(&command, execution)
            .expect("interrupted-adoption network result should publish");
        assert_eq!(result.kind(), ProviderCommandObservationKind::Succeeded);
    }
    println!("NNC65G_KRUN_ADOPTION_RECOVERED:released");
}

fn replay_interrupted_adoption_child(root: &Path, sandbox_id: &SandboxId, pep_port: u16) {
    let before = snapshot_files(root);
    let backend = reopen_backend(root, pep_port);
    let journal = backend
        .attempt_idempotency_journal()
        .expect("interrupted-adoption replay journal should open");
    for operation in [
        SandboxExecutionTeardownOperation::Drain,
        SandboxExecutionTeardownOperation::Stop,
    ] {
        let command = execution_command_from_backend(&backend, sandbox_id, operation);
        assert_eq!(
            journal
                .adopt_exact_attempt(command.provider_claim())
                .expect("terminal execution result should read")
                .expect("terminal execution result should exist")
                .kind(),
            ProviderCommandObservationKind::Succeeded
        );
    }
    for operation in [
        SandboxNetworkTeardownOperation::Detach,
        SandboxNetworkTeardownOperation::Release,
    ] {
        let command = network_command_from_backend(&backend, sandbox_id, operation);
        assert_eq!(
            journal
                .adopt_exact_attempt(command.provider_claim())
                .expect("terminal network result should read")
                .expect("terminal network result should exist")
                .kind(),
            ProviderCommandObservationKind::Succeeded
        );
    }
    assert_eq!(
        snapshot_files(root),
        before,
        "fresh-process terminal replay must be byte-stable"
    );
    println!("NNC65G_KRUN_ADOPTION_REPLAY:byte-stable");
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
        .expect("Krun contender journal should open");
    let ready = root.join(format!("ready-{}-{role}", operation_label(operation)));
    std::fs::write(&ready, b"ready\n").expect("Krun contender readiness should persist");
    wait_for_path(&root.join(format!("gate-{}", operation_label(operation))));
    match journal
        .claim_dispatch_epoch(command.provider_claim())
        .expect("Krun contender should reach the exact stream")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => {
            let result = backend
                .execute_network_teardown_with_claim(&command, execution)
                .expect("winning Krun contender should publish");
            assert_eq!(result.kind(), ProviderCommandObservationKind::Succeeded);
            println!("NNC65D3_KRUN_ROLE:execute");
        }
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            wait_for_contender_success(&journal, command.provider_claim());
            println!("NNC65D3_KRUN_ROLE:adopt");
        }
    }
}

fn network_command_from_backend(
    backend: &KrunSandboxBackend,
    sandbox_id: &SandboxId,
    operation: SandboxNetworkTeardownOperation,
) -> SandboxNetworkTeardownCommand {
    let manifest = backend
        .read_manifest(sandbox_id)
        .expect("exact Krun manifest should read")
        .expect("exact Krun manifest should exist");
    let stop_claim = match manifest.execution_teardown.stop() {
        KrunStopProgress::ExecutionStopped { fence, .. } => fence,
        progress => panic!("Krun recovery requires ExecutionStopped, got {progress:?}"),
    };
    let plan = manifest
        .provision_network_plan
        .as_ref()
        .expect("Krun recovery requires the compiled plan");
    let identity = SandboxNetworkTeardownIdentity::new(SandboxNetworkTeardownIdentityInput {
        tenant_id: manifest.spec.tenant_id.clone(),
        sandbox_id: manifest.handle.id.clone(),
        execution_attempt_id: manifest.execution_attempt_id.clone(),
        attachment_id: plan.attachment_id().clone(),
        network_plan: plan.network_plan().clone(),
        provider_registration_key: KRUN_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY.to_owned(),
        provider_source_digest: NetworkCapabilitySourceDigest::from_bytes([9; 32]),
    })
    .expect("recovered Krun identity should validate");
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
    .expect("recovered Krun claim should validate");
    SandboxNetworkTeardownCommand::new(SandboxNetworkTeardownCommandInput {
        identity,
        operation,
        provider_claim: claim,
    })
    .expect("recovered Krun command should validate")
}

fn execution_command_from_backend(
    backend: &KrunSandboxBackend,
    sandbox_id: &SandboxId,
    operation: SandboxExecutionTeardownOperation,
) -> SandboxExecutionTeardownCommand {
    let manifest = backend
        .read_manifest(sandbox_id)
        .expect("exact Krun manifest should read")
        .expect("exact Krun manifest should exist");
    let plan = manifest
        .provision_network_plan
        .as_ref()
        .expect("Krun recovery requires the compiled plan");
    let claim = ProviderCommandClaim::new(ProviderCommandClaimInput {
        authority_id: "authority-krun-network-teardown".to_owned(),
        effect_subject: format!("{{\"sandbox\":\"{sandbox_id}\"}}"),
        source_attempt_id: None,
        attempt_id: sandbox_id
            .as_str()
            .strip_prefix("krun-network-adopting-fresh-process-")
            .unwrap_or_else(|| sandbox_id.as_str())
            .to_owned(),
        dispatch_epoch: 1,
        workload_generation: plan.generation().as_u64(),
        restart_ordinal: 0,
        desired_digest: "1".repeat(64),
        source_digest: "2".repeat(64),
        network_plan_digest: plan.network_plan().digest().to_string(),
        provider_target_digest: "3".repeat(64),
        operation: operation.provider_operation(),
    })
    .expect("recovered Krun execution claim should validate");
    SandboxExecutionTeardownCommand::new(
        manifest.spec.tenant_id,
        manifest.handle.id,
        manifest.execution_attempt_id,
        "nimbus-sandbox.krun-execution",
        operation,
        claim,
    )
    .expect("recovered Krun execution command should validate")
}

fn reopen_backend(root: &Path, pep_port: u16) -> KrunSandboxBackend {
    let mut config = KrunSandboxBackendConfig::under_root(root);
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    config.published_port_range = pep_port..=pep_port;
    config.netavark_path = PathBuf::from("/usr/bin/true");
    KrunSandboxBackend::new(config)
        .with_egress_pin_provider(Arc::new(FixedOciEgressPinProvider::ready()))
}

fn operation_for(
    checkpoint: HostManagedAttachmentTeardownCheckpoint,
) -> SandboxNetworkTeardownOperation {
    match checkpoint {
        HostManagedAttachmentTeardownCheckpoint::Detach(_) => {
            SandboxNetworkTeardownOperation::Detach
        }
        HostManagedAttachmentTeardownCheckpoint::Release(_) => {
            SandboxNetworkTeardownOperation::Release
        }
    }
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
        .unwrap_or_else(|| panic!("unknown Krun network checkpoint {label:?}"))
        .checkpoint
}

fn parse_operation(value: &str) -> SandboxNetworkTeardownOperation {
    match value {
        "detach" => SandboxNetworkTeardownOperation::Detach,
        "release" => SandboxNetworkTeardownOperation::Release,
        _ => panic!("unknown Krun network operation {value:?}"),
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
    command.output().expect("Krun network child should start")
}

fn spawn_contender(
    root: &Path,
    sandbox_id: &SandboxId,
    pep_port: u16,
    operation: SandboxNetworkTeardownOperation,
    role: &str,
) -> Child {
    let mut command = child_command(root, sandbox_id, pep_port, "contend", operation);
    command
        .env(ROLE_ENV, role)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Krun network contender should start")
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
            .expect("first Krun child status should read")
            .is_some();
        let second_done = second
            .try_wait()
            .expect("second Krun child status should read")
            .is_some();
        if first_done && second_done {
            return [
                first
                    .wait_with_output()
                    .expect("first Krun child output should read"),
                second
                    .wait_with_output()
                    .expect("second Krun child output should read"),
            ];
        }
        if started.elapsed() >= TIMEOUT {
            let _ = first.kill();
            let _ = second.kill();
            let first_output = first
                .wait_with_output()
                .expect("timed-out first Krun child output should read");
            let second_output = second
                .wait_with_output()
                .expect("timed-out second Krun child output should read");
            panic!(
                "Krun network {operation:?} contenders timed out\nfirst stdout:\n{}\nfirst stderr:\n{}\nsecond stdout:\n{}\nsecond stderr:\n{}",
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
            .expect("Krun contender result should remain readable")
            .expect("Krun contender result should remain present");
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
            "winning Krun contender published unexpected result {current:?}"
        );
        assert!(
            started.elapsed() < TIMEOUT,
            "winning Krun contender did not publish before timeout"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn child_pid(bytes: &[u8]) -> u32 {
    String::from_utf8_lossy(bytes)
        .lines()
        .find_map(|line| line.strip_prefix("NNC65D3_KRUN_PID:"))
        .expect("Krun child output should include its PID")
        .parse()
        .expect("Krun child PID should parse")
}

fn assert_success(output: &Output, label: &str) {
    assert!(
        output.status.success(),
        "Krun network child failed for {label}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("required environment variable {name} is absent"))
}
