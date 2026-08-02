use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::time::Duration;

use nimbus_core::Error;
use nimbus_network::{LocalNetworkManager, NetworkCapabilityRegistry};
use nimbus_workloads::{WorkloadSagaExpected, WorkloadSagaStore};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Child;

use crate::config::transport::TransportConfig;
use crate::state::{
    AppState, AppStateConfig, ControlPlaneConfig, DeploymentConfig, NodeServicesConfig,
    RuntimeGovernorConfig,
};

use super::super::EngineWorkloadSagaStore;
use super::super::schema::{workload_saga_table, workload_saga_tenant};
use super::{engine, initial_record};

const CHILD_MODE: &str = "NIMBUS_NNC61D_CHILD_MODE";
const CHILD_ROOT: &str = "NIMBUS_NNC61D_CHILD_ROOT";
const CHILD_MODE_STALL: &str = "stall";
const CHILD_MODE_WRITE: &str = "write";
const CHILD_STALL_MARKER: &str = "NIMBUS_NNC61D_CHILD_STALLED";
const CHILD_TEST_NAME: &str = "workload_saga_store::tests::composition::fresh_process_recovers_durable_truth_without_snapshot_handoff";
const CHILD_TIMEOUT: Duration = Duration::from_secs(15);
const STALLED_CHILD_TIMEOUT: Duration = Duration::from_secs(2);

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
async fn managed_server_state_retains_one_manager_and_engine_saga_coordinator() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let engine = engine(&root);
    let manager = LocalNetworkManager::open(
        root.path().join("network"),
        NetworkCapabilityRegistry::new([]).expect("empty capability registry should validate"),
    )
    .expect("network manager should open");
    let state = AppState::from_config(AppStateConfig {
        engine: Arc::clone(&engine),
        network_manager: Some(Arc::clone(&manager)),
        deployment: DeploymentConfig::default(),
        control_plane: ControlPlaneConfig::router_options_default(),
        node_services: NodeServicesConfig::default(),
        transport: TransportConfig::default(),
        runtime: RuntimeGovernorConfig::default(),
    });

    let retained_manager = state
        .network_manager()
        .expect("managed server state should retain its network manager");
    assert!(Arc::ptr_eq(&retained_manager, &manager));
    let first_coordinator = state
        .workload_saga_coordinator()
        .expect("managed server state should own a saga coordinator");
    let second_coordinator = state
        .workload_saga_coordinator()
        .expect("managed server state should retain one saga coordinator");
    assert!(Arc::ptr_eq(&first_coordinator, &second_coordinator));
    assert!(
        !manager.authority_path().exists(),
        "composition must not create network lifecycle state"
    );

    let missing = initial_record("managed-composition");
    assert_eq!(first_coordinator.load(missing.key()).await, Ok(None));
    engine
        .get_table_schema_async(
            workload_saga_tenant().unwrap(),
            workload_saga_table().unwrap(),
        )
        .await
        .expect("managed coordinator use should prepare the private saga table");
    assert!(
        !manager.authority_path().exists(),
        "saga-store use must not create network lifecycle state"
    );
}

#[tokio::test]
async fn protocol_only_server_state_owns_no_workload_authority_or_saga_schema() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let engine = engine(&root);
    let state = AppState::from_config(AppStateConfig {
        engine: Arc::clone(&engine),
        network_manager: None,
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
