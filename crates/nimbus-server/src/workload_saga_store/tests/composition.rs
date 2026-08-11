use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::time::Duration;

use nimbus_compute::workload_saga::{
    WorkloadSagaAction, WorkloadSagaCoordinator, WorkloadSagaDecision,
};
use nimbus_core::Error;
use nimbus_testing::{
    ProcessRoleSpec, SubprocessCrashCutHarness, run_crash_cut_child, run_crash_recovery_child,
};
use nimbus_workloads::{
    DesiredWorkloadState, ProposedWorkloadTeardownTransition, WorkloadSagaCommit,
    WorkloadSagaExpected, WorkloadSagaStore, WorkloadTeardownDecision, WorkloadTeardownStep,
};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Child;

use crate::config::transport::TransportConfig;
use crate::state::{
    AppState, AppStateConfig, ControlPlaneConfig, DeploymentConfig, NodeServicesConfig,
    RuntimeGovernorConfig,
};

use super::super::EngineWorkloadSagaStore;
use super::super::schema::{workload_saga_table, workload_saga_tenant};
use super::recovery::{PROCESS_MATRIX_EXPECTATIONS, process_matrix_histories, process_matrix_key};
use super::{engine, initial_record};

const CHILD_MODE: &str = "NIMBUS_NNC61D_CHILD_MODE";
const CHILD_ROOT: &str = "NIMBUS_NNC61D_CHILD_ROOT";
const CHILD_MODE_STALL: &str = "stall";
const CHILD_MODE_WRITE: &str = "write";
const CHILD_STALL_MARKER: &str = "NIMBUS_NNC61D_CHILD_STALLED";
const CHILD_TEST_NAME: &str = "workload_saga_store::tests::composition::fresh_process_recovers_durable_truth_without_snapshot_handoff";
const CHILD_TIMEOUT: Duration = Duration::from_secs(15);
const STALLED_CHILD_TIMEOUT: Duration = Duration::from_secs(2);
const RECOVERY_MATRIX_CHILD_TEST: &str =
    "workload_saga_store::tests::composition::workload_saga_recovery_matrix_child";
const RECOVERY_MATRIX_MODE_ENV: &str = "NIMBUS_NNC61E_RECOVERY_MATRIX_MODE";
const RECOVERY_MATRIX_WRITE_MODE: &str = "write";
const RECOVERY_MATRIX_READ_MODE: &str = "recover";
const RECOVERY_MATRIX_BOUNDARY: &str = "workload-saga.phase-matrix-durable";
const RECOVERY_MATRIX_OBSERVATION: &str =
    "matrix-30-f1c438180513c6064249057ab3d6715b1f1bd2ed05e306c5a0e7c839ef6a6544";
const RECOVERY_MATRIX_TIMEOUT: Duration = Duration::from_secs(20);
const RECOVERY_MATRIX_PID_PREFIX: &str = "NIMBUS_NNC61E_PROCESS_ID";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildCompletion {
    Exited,
    TimedOut,
    WaitFailed,
}

#[derive(Debug)]
struct ChildResult {
    completion: ChildCompletion,
    pid: u32,
    root: PathBuf,
    deadline: Duration,
    status: Option<ExitStatus>,
    cleanup: String,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_error: Option<String>,
    stderr_error: Option<String>,
    detail: String,
}

impl fmt::Display for ChildResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "child process: completion={:?} pid={} test={} root={} deadline={:?} status={} cleanup={} detail={} stdout_capture={} stderr_capture={}\nstdout:\n{}\nstderr:\n{}",
            self.completion,
            self.pid,
            CHILD_TEST_NAME,
            self.root.display(),
            self.deadline,
            self.status
                .map_or_else(|| "<none>".to_owned(), |status| status.to_string()),
            self.cleanup,
            self.detail,
            self.stdout_error.as_deref().unwrap_or("ok"),
            self.stderr_error.as_deref().unwrap_or("ok"),
            String::from_utf8_lossy(&self.stdout),
            String::from_utf8_lossy(&self.stderr),
        )
    }
}

#[derive(Debug)]
struct CapturedStream {
    bytes: Vec<u8>,
    error: Option<String>,
}

async fn capture_stream(mut stream: impl AsyncRead + Unpin) -> CapturedStream {
    let mut bytes = Vec::new();
    let error = stream
        .read_to_end(&mut bytes)
        .await
        .err()
        .map(|error| error.to_string());
    CapturedStream { bytes, error }
}

async fn finish_capture(
    reader: tokio::task::JoinHandle<CapturedStream>,
    stream_name: &str,
) -> CapturedStream {
    reader.await.unwrap_or_else(|error| CapturedStream {
        bytes: Vec::new(),
        error: Some(format!("{stream_name} reader task failed: {error}")),
    })
}

async fn terminate_and_reap(child: &mut Child) -> (Option<ExitStatus>, String) {
    let kill = child.start_kill();
    let reaped = child.wait().await;
    match (kill, reaped) {
        (Ok(()), Ok(status)) if child.id().is_none() => {
            (Some(status), "killed-and-reaped".to_owned())
        }
        (Ok(()), Ok(status)) => (
            Some(status),
            "kill-succeeded-but-child-handle-was-not-reaped".to_owned(),
        ),
        (Err(kill_error), Ok(status)) => (
            Some(status),
            format!("kill-failed-but-reaped: {kill_error}"),
        ),
        (Ok(()), Err(reap_error)) => (None, format!("killed-but-reap-failed: {reap_error}")),
        (Err(kill_error), Err(reap_error)) => (
            None,
            format!("kill-and-reap-failed: kill={kill_error}; reap={reap_error}"),
        ),
    }
}

async fn run_child(mode: &'static str, root: &Path, deadline: Duration) -> ChildResult {
    let mut command = tokio::process::Command::new(
        std::env::current_exe().expect("current test executable should resolve"),
    );
    command
        .args(["--exact", CHILD_TEST_NAME, "--nocapture"])
        .env(CHILD_MODE, mode)
        .env(CHILD_ROOT, root)
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap_or_else(|error| {
        panic!(
            "child process should launch: test={} root={} deadline={deadline:?}: {error}",
            CHILD_TEST_NAME,
            root.display()
        )
    });
    let pid = child
        .id()
        .expect("running child should expose a process id");
    let stdout = child
        .stdout
        .take()
        .expect("child stdout should be captured");
    let stderr = child
        .stderr
        .take()
        .expect("child stderr should be captured");
    let stdout_reader = tokio::spawn(capture_stream(stdout));
    let stderr_reader = tokio::spawn(capture_stream(stderr));

    let (completion, status, cleanup, detail) =
        match tokio::time::timeout(deadline, child.wait()).await {
            Ok(Ok(status)) => (
                ChildCompletion::Exited,
                Some(status),
                "exited-and-reaped".to_owned(),
                "child exited before its deadline".to_owned(),
            ),
            Ok(Err(error)) => {
                let (status, cleanup) = terminate_and_reap(&mut child).await;
                (
                    ChildCompletion::WaitFailed,
                    status,
                    cleanup,
                    format!("child wait failed: {error}"),
                )
            }
            Err(_) => {
                let (status, cleanup) = terminate_and_reap(&mut child).await;
                (
                    ChildCompletion::TimedOut,
                    status,
                    cleanup,
                    "child did not exit before its deadline".to_owned(),
                )
            }
        };
    let stdout = finish_capture(stdout_reader, "stdout").await;
    let stderr = finish_capture(stderr_reader, "stderr").await;
    ChildResult {
        completion,
        pid,
        root: root.to_owned(),
        deadline,
        status,
        cleanup,
        stdout: stdout.bytes,
        stderr: stderr.bytes,
        stdout_error: stdout.error,
        stderr_error: stderr.error,
        detail,
    }
}

#[test]
fn fresh_process_reopens_engine_and_plans_every_workload_saga_phase_without_snapshot_handoff() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let result = SubprocessCrashCutHarness::new(RECOVERY_MATRIX_TIMEOUT)
        .run(
            root.path(),
            RECOVERY_MATRIX_BOUNDARY,
            RECOVERY_MATRIX_OBSERVATION,
            recovery_matrix_child("phase-matrix-writer", RECOVERY_MATRIX_WRITE_MODE),
            recovery_matrix_child("phase-matrix-recovery", RECOVERY_MATRIX_READ_MODE),
        )
        .unwrap_or_else(|error| panic!("workload-saga phase recovery failed: {error}"));

    assert_eq!(result.boundary(), RECOVERY_MATRIX_BOUNDARY);
    assert_eq!(result.observation(), RECOVERY_MATRIX_OBSERVATION);
    assert_eq!(
        result.crash_diagnostic().cleanup(),
        "killed-at-boundary-and-reaped"
    );
    assert_eq!(result.crash_diagnostic().successful(), Some(false));
    assert_eq!(result.recovery_diagnostic().successful(), Some(true));
    assert_eq!(result.crash_diagnostic().role(), "phase-matrix-writer");
    assert_eq!(result.recovery_diagnostic().role(), "phase-matrix-recovery");

    let writer_pid = process_id(result.crash_diagnostic().stderr(), "writer");
    let recovery_pid = process_id(result.recovery_diagnostic().stderr(), "recovery");
    assert_ne!(
        writer_pid, recovery_pid,
        "recovery must execute in a distinct process"
    );
}

#[test]
#[ignore = "spawned only by the workload-saga phase recovery parent"]
fn workload_saga_recovery_matrix_child() {
    let mode =
        std::env::var(RECOVERY_MATRIX_MODE_ENV).expect("recovery matrix child mode should be set");
    match mode.as_str() {
        RECOVERY_MATRIX_WRITE_MODE => run_crash_cut_child(|context| {
            eprintln!("{RECOVERY_MATRIX_PID_PREFIX} writer {}", std::process::id());
            let runtime = recovery_matrix_runtime()?;
            let engine = Arc::new(
                nimbus_engine::Engine::new(context.state_root())
                    .map_err(|error| format!("writer Engine open failed: {error}"))?,
            );
            let store = EngineWorkloadSagaStore::new(engine);
            runtime.block_on(persist_process_matrix(&store))?;
            context.reach_boundary(RECOVERY_MATRIX_BOUNDARY)
        })
        .unwrap_or_else(|error| panic!("phase matrix writer failed: {error}")),
        RECOVERY_MATRIX_READ_MODE => run_crash_recovery_child(|context| {
            eprintln!(
                "{RECOVERY_MATRIX_PID_PREFIX} recovery {}",
                std::process::id()
            );
            let runtime = recovery_matrix_runtime()?;
            runtime.block_on(recover_process_matrix(context.state_root()))
        })
        .unwrap_or_else(|error| panic!("phase matrix recovery failed: {error}")),
        unknown => panic!("unknown recovery matrix child mode {unknown:?}"),
    }
}

fn recovery_matrix_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|error| format!("recovery matrix runtime failed: {error}"))
}

async fn persist_process_matrix(store: &EngineWorkloadSagaStore) -> Result<(), String> {
    let histories = process_matrix_histories();
    if histories.len() != PROCESS_MATRIX_EXPECTATIONS.len() {
        return Err(format!(
            "writer matrix cardinality mismatch: {} histories for {} expectations",
            histories.len(),
            PROCESS_MATRIX_EXPECTATIONS.len()
        ));
    }

    for (history, expectation) in histories.iter().zip(PROCESS_MATRIX_EXPECTATIONS) {
        let latest = history
            .last()
            .ok_or_else(|| format!("empty history for {}", expectation.label))?;
        if latest.key() != &process_matrix_key(expectation.label)
            || latest.phase() != expectation.phase
        {
            return Err(format!(
                "writer fixture mismatch for {}: key={} phase={:?}",
                expectation.label,
                latest.key().workload_id().as_str(),
                latest.phase()
            ));
        }
        for (index, record) in history.iter().enumerate() {
            let expected = index
                .checked_sub(1)
                .map_or(WorkloadSagaExpected::Missing, |previous| {
                    WorkloadSagaExpected::Revision(history[previous].revision())
                });
            let commit = store
                .compare_and_swap(expected, record.clone())
                .await
                .map_err(|error| {
                    format!(
                        "writer failed to persist {} {:?}: {error}",
                        expectation.label,
                        record.phase()
                    )
                })?;
            if commit != WorkloadSagaCommit::Applied {
                return Err(format!(
                    "writer did not newly persist {} {:?}: {commit:?}",
                    expectation.label,
                    record.phase()
                ));
            }
        }
    }
    Ok(())
}

async fn recover_process_matrix(root: &Path) -> Result<String, String> {
    let engine = Arc::new(
        nimbus_engine::Engine::new(root)
            .map_err(|error| format!("recovery Engine open failed: {error}"))?,
    );
    let store: Arc<dyn WorkloadSagaStore> = Arc::new(EngineWorkloadSagaStore::new(engine));
    let coordinator = WorkloadSagaCoordinator::new(store);
    let mut digest = Sha256::new();

    for expectation in PROCESS_MATRIX_EXPECTATIONS {
        let key = process_matrix_key(expectation.label);
        let record = coordinator
            .load(&key)
            .await
            .map_err(|error| format!("recovery load failed for {}: {error}", expectation.label))?
            .ok_or_else(|| format!("recovery omitted {}", expectation.label))?;
        if record.phase() != expectation.phase {
            return Err(format!(
                "recovery phase mismatch for {}: expected {:?}, observed {:?}",
                expectation.label,
                expectation.phase,
                record.phase()
            ));
        }

        let decision = WorkloadSagaDecision::for_record(&record)
            .map_err(|error| format!("decision failed for {}: {error}", expectation.label))?;
        let action = recovery_action_label(decision.action());
        if decision.key() != record.key()
            || decision.saga_id() != record.saga_id()
            || decision.revision() != record.revision()
            || decision.active_generation() != record.active_intent().generation()
            || decision.target_phase() != expectation.target
            || action != expectation.action
        {
            return Err(format!(
                "recovery decision mismatch for {}: target={:?} action={action}",
                expectation.label,
                decision.target_phase()
            ));
        }
        if matches!(
            decision.action(),
            WorkloadSagaAction::Teardown(WorkloadTeardownDecision::CleanupPending { .. })
        ) && record.teardown_disposition().is_none()
        {
            return Err(format!(
                "cleanup decision failed to retain durable teardown state for {}",
                expectation.label
            ));
        }

        digest.update(
            format!(
                "{}|{}|{}|{}|{}|{}|{}|{:?}\n",
                expectation.label,
                decision.key().tenant_id().as_str(),
                decision.key().workload_id().as_str(),
                decision.saga_id().as_str(),
                decision.revision().as_u64(),
                decision.active_generation().as_u64(),
                decision.target_phase().recovery_order(),
                decision.action(),
            )
            .as_bytes(),
        );
    }

    Ok(format!(
        "matrix-{}-{:x}",
        PROCESS_MATRIX_EXPECTATIONS.len(),
        digest.finalize()
    ))
}

fn recovery_action_label(action: &WorkloadSagaAction) -> &'static str {
    match action {
        WorkloadSagaAction::Provision(decision) => match decision {
            nimbus_compute::workload_saga::WorkloadProvisionDecision::Proposed(proposed) => {
                match proposed
                    .candidate()
                    .provision_disposition()
                    .and_then(nimbus_workloads::WorkloadProvisionDisposition::attempt)
                    .map(nimbus_workloads::WorkloadProvisionAttempt::step)
                {
                    Some(nimbus_workloads::WorkloadProvisionStep::ReserveNetwork) => {
                        "reserve-network"
                    }
                    Some(nimbus_workloads::WorkloadProvisionStep::PrepareWorkload) => {
                        "prepare-workload"
                    }
                    Some(nimbus_workloads::WorkloadProvisionStep::AttachNetwork) => {
                        "attach-network"
                    }
                    Some(
                        nimbus_workloads::WorkloadProvisionStep::InspectActivationPrerequisites,
                    ) => "inspect-activation-prerequisites",
                    Some(nimbus_workloads::WorkloadProvisionStep::ActivateWorkload) => {
                        "activate-workload"
                    }
                    Some(nimbus_workloads::WorkloadProvisionStep::InspectWorkloadReadiness) => {
                        "inspect-readiness"
                    }
                    Some(nimbus_workloads::WorkloadProvisionStep::Publish) => "publish",
                    Some(nimbus_workloads::WorkloadProvisionStep::ObservePublication) => {
                        "observe-publication"
                    }
                    None => "advance-without-effect",
                }
            }
            nimbus_compute::workload_saga::WorkloadProvisionDecision::InspectExact(_) => {
                "inspect-exact-attempt"
            }
            nimbus_compute::workload_saga::WorkloadProvisionDecision::DefiniteFailure
            | nimbus_compute::workload_saga::WorkloadProvisionDecision::Wait => "quiescent",
        },
        WorkloadSagaAction::Teardown(decision) => teardown_action_label(decision),
        WorkloadSagaAction::PromoteSuccessor { intent }
            if intent.desired_state() == DesiredWorkloadState::Running =>
        {
            "promote-successor-running"
        }
        WorkloadSagaAction::PromoteSuccessor { .. } => "promote-successor-stopped",
        WorkloadSagaAction::Quiescent => "quiescent",
    }
}

fn teardown_action_label(decision: &WorkloadTeardownDecision) -> &'static str {
    let step = match decision {
        WorkloadTeardownDecision::PersistCandidate(ProposedWorkloadTeardownTransition::Claim {
            attempt,
            ..
        }) => Some(attempt.step()),
        WorkloadTeardownDecision::InspectExact(claim) => Some(claim.attempt().step()),
        _ => None,
    };
    if let Some(step) = step {
        return match step {
            WorkloadTeardownStep::WithdrawPublication => "withdraw-publication",
            WorkloadTeardownStep::DrainExecution => "drain-workload",
            WorkloadTeardownStep::StopExecution => "stop-workload",
            WorkloadTeardownStep::DetachNetwork => "detach-network",
            WorkloadTeardownStep::ReleaseNetwork => "release-network",
        };
    }
    match decision {
        WorkloadTeardownDecision::PersistCandidate(ProposedWorkloadTeardownTransition::Claim {
            ..
        })
        | WorkloadTeardownDecision::InspectExact(_) => {
            unreachable!("effectful teardown decisions always have a step")
        }
        WorkloadTeardownDecision::PersistCandidate(
            ProposedWorkloadTeardownTransition::ResourceFree { .. },
        ) => "advance-without-effect",
        WorkloadTeardownDecision::PersistCandidate(
            ProposedWorkloadTeardownTransition::RecordTerminal,
        ) => "record-terminal-evidence",
        WorkloadTeardownDecision::CleanupPending { .. } => "inspect-cleanup",
        WorkloadTeardownDecision::RestartSettlementPending(_) => "restart-settlement-pending",
        WorkloadTeardownDecision::Quiescent => "quiescent",
    }
}

fn recovery_matrix_child(role: &str, mode: &str) -> ProcessRoleSpec {
    ProcessRoleSpec::new(
        role,
        std::env::current_exe().expect("current test executable should resolve"),
    )
    .arg("--exact")
    .arg(RECOVERY_MATRIX_CHILD_TEST)
    .arg("--ignored")
    .arg("--nocapture")
    .env(RECOVERY_MATRIX_MODE_ENV, mode)
}

fn process_id(stderr: &str, role: &str) -> u32 {
    stderr
        .lines()
        .find_map(|line| {
            line.strip_prefix(&format!("{RECOVERY_MATRIX_PID_PREFIX} {role} "))
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or_else(|| panic!("missing {role} child process id in stderr:\n{stderr}"))
}

#[tokio::test]
async fn fresh_store_instance_recovers_durable_truth_without_snapshot_handoff() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let writer_engine = engine(&root);
    let writer_engine_lifetime = Arc::downgrade(&writer_engine);
    let writer = EngineWorkloadSagaStore::new(Arc::clone(&writer_engine));
    let record = initial_record("fresh-store");
    writer
        .compare_and_swap(WorkloadSagaExpected::Missing, record.clone())
        .await
        .expect("record should persist");
    drop(writer);
    drop(writer_engine);
    assert!(
        writer_engine_lifetime.upgrade().is_none(),
        "the writer Engine must be gone before the durable root is reopened"
    );

    let reader = EngineWorkloadSagaStore::new(engine(&root));
    assert_eq!(reader.load(record.key()).await, Ok(Some(record)));
}

#[tokio::test]
async fn fresh_process_recovers_durable_truth_without_snapshot_handoff() {
    if let Some(mode) = std::env::var_os(CHILD_MODE) {
        let root = PathBuf::from(
            std::env::var_os(CHILD_ROOT).expect("child durable root should be provided"),
        );
        match mode.to_str().expect("child mode should be UTF-8") {
            CHILD_MODE_WRITE => {
                let engine = Arc::new(
                    nimbus_engine::Engine::new(&root)
                        .expect("child Engine should open the parent-owned durable root"),
                );
                let store = EngineWorkloadSagaStore::new(engine);
                let record = initial_record("fresh-process");
                assert_eq!(
                    store
                        .compare_and_swap(WorkloadSagaExpected::Missing, record)
                        .await,
                    Ok(nimbus_workloads::WorkloadSagaCommit::Applied)
                );
                return;
            }
            CHILD_MODE_STALL => {
                eprintln!("{CHILD_STALL_MARKER} root={}", root.display());
                std::future::pending::<()>().await;
                unreachable!("stalled child should only exit when its parent terminates it");
            }
            unknown => panic!("unknown child mode {unknown:?}"),
        }
    }

    let root = tempfile::tempdir().expect("fixture root should build");
    let child = run_child(CHILD_MODE_WRITE, root.path(), CHILD_TIMEOUT).await;
    assert_eq!(child.completion, ChildCompletion::Exited, "{child}");
    assert!(
        child.status.is_some_and(|status| status.success()),
        "{child}"
    );
    assert!(child.stdout_error.is_none(), "{child}");
    assert!(child.stderr_error.is_none(), "{child}");

    let record = initial_record("fresh-process");
    let reader = EngineWorkloadSagaStore::new(engine(&root));
    assert_eq!(reader.load(record.key()).await, Ok(Some(record)));
}

#[tokio::test]
async fn bounded_child_wait_terminates_and_reaps_stalled_child() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let child = run_child(CHILD_MODE_STALL, root.path(), STALLED_CHILD_TIMEOUT).await;

    assert_eq!(child.completion, ChildCompletion::TimedOut, "{child}");
    assert_eq!(child.cleanup, "killed-and-reaped", "{child}");
    assert!(
        child.status.is_some(),
        "killed child should be reaped: {child}"
    );
    assert!(child.stdout_error.is_none(), "{child}");
    assert!(child.stderr_error.is_none(), "{child}");
    assert!(
        String::from_utf8_lossy(&child.stderr).contains(CHILD_STALL_MARKER),
        "stalled child must reach its child-only branch before timeout: {child}"
    );
    let diagnostic = child.to_string();
    for field in [
        "pid=",
        "test=",
        "root=",
        "deadline=",
        "status=",
        "cleanup=killed-and-reaped",
        "stdout:",
        "stderr:",
    ] {
        assert!(
            diagnostic.contains(field),
            "timeout diagnostic should include {field:?}: {diagnostic}"
        );
    }
}

#[tokio::test]
async fn protocol_only_server_state_owns_no_workload_authority_or_saga_schema() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let engine = engine(&root);
    let state = AppState::from_config(AppStateConfig {
        workload: crate::workload_composition::ServerWorkloadProfile::protocol_only(Arc::clone(
            &engine,
        )),
        deployment: DeploymentConfig::default(),
        control_plane: ControlPlaneConfig::router_options_default(),
        node_services: NodeServicesConfig::default(),
        transport: TransportConfig::default(),
        runtime: RuntimeGovernorConfig::default(),
    });

    assert!(state.network_manager().is_none());
    assert!(state.workload_saga_coordinator().is_none());
    assert!(
        !engine
            .list_tenants_async()
            .await
            .expect("durable tenant inventory should read")
            .contains(&workload_saga_tenant().unwrap())
    );
    assert!(matches!(
        engine
            .get_table_schema_async(
                workload_saga_tenant().unwrap(),
                workload_saga_table().unwrap()
            )
            .await,
        Err(Error::TenantNotFound(_))
    ));
}
