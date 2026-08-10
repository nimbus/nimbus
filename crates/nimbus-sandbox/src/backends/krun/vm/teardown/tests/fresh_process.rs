//! Fresh-process recovery proofs for exact Krun execution teardown.

use std::fs::{self, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::backends::conmon::runtime_process::{
    RuntimeProcessIdentity, RuntimeProcessIdentityObservation, RuntimeProcessSignal,
    RuntimeProcessSignalOutcome,
};
use crate::{
    ProviderCommandClaim, ProviderCommandClaimDecision, ProviderCommandClaimInput,
    ProviderCommandObservationKind, ProviderCommandOperation, SandboxExecutionTeardownCommand,
    SandboxExecutionTeardownObservation, SandboxExecutionTeardownOperation,
};

use super::*;

const ROOT_ENV: &str = "NIMBUS_NNC65D2_TEARDOWN_ROOT";
const ACTION_ENV: &str = "NIMBUS_NNC65D2_TEARDOWN_ACTION";
const CASE_ENV: &str = "NIMBUS_NNC65D2_TEARDOWN_CASE";
const ROLE_ENV: &str = "NIMBUS_NNC65D2_TEARDOWN_ROLE";
const CHILD_TEST: &str =
    "backends::krun::vm::teardown::tests::fresh_process::krun_execution_teardown_process_child";
const CHILD_TIMEOUT: Duration = Duration::from_secs(15);
const GATE_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(5);

#[derive(Clone)]
struct FileBackedRuntime {
    backend: KrunSandboxBackend,
    witness: PathBuf,
    terminal: bool,
    process_observation: RuntimeProcessIdentityObservation,
}

impl FileBackedRuntime {
    fn live(backend: KrunSandboxBackend, witness: PathBuf) -> Self {
        Self {
            backend,
            witness,
            terminal: false,
            process_observation: RuntimeProcessIdentityObservation::ExactLive,
        }
    }

    fn terminal(backend: KrunSandboxBackend, witness: PathBuf) -> Self {
        Self {
            terminal: true,
            ..Self::live(backend, witness)
        }
    }

    fn absent(backend: KrunSandboxBackend, witness: PathBuf) -> Self {
        Self {
            process_observation: RuntimeProcessIdentityObservation::ExplicitlyAbsent,
            ..Self::live(backend, witness)
        }
    }
}

impl KrunExecutionTeardownRuntime for FileBackedRuntime {
    fn now_unix_millis(&self) -> crate::Result<u64> {
        Ok(10_000)
    }

    fn observe_execution_terminal(
        &self,
        _manifest: &KrunSandboxManifest,
    ) -> crate::Result<KrunExecutionTerminalObservation> {
        Ok(if self.terminal {
            KrunExecutionTerminalObservation::ExactExit { exit_code: 0 }
        } else {
            KrunExecutionTerminalObservation::NotObserved
        })
    }

    fn capture_process(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> crate::Result<RuntimeProcessIdentity> {
        let attempt_id = match &manifest.creator_handoff {
            KrunCreatorHandoffState::RuntimeObserved { receipt }
            | KrunCreatorHandoffState::Pending { receipt } => receipt.attempt_id(),
            KrunCreatorHandoffState::Quiesced { proof } => proof.attempt_id(),
            KrunCreatorHandoffState::SpawnIntent { attempt_id } => attempt_id,
            KrunCreatorHandoffState::NotSpawned => "missing-creator",
        };
        Ok(RuntimeProcessIdentity::fixture(
            manifest.handle.id.as_str(),
            attempt_id,
            42,
        ))
    }

    fn inspect_process(
        &self,
        _manifest: &KrunSandboxManifest,
        _identity: &RuntimeProcessIdentity,
    ) -> crate::Result<RuntimeProcessIdentityObservation> {
        Ok(self.process_observation)
    }

    fn signal_process(
        &self,
        manifest: &KrunSandboxManifest,
        _identity: &RuntimeProcessIdentity,
        signal: RuntimeProcessSignal,
    ) -> crate::Result<RuntimeProcessSignalOutcome> {
        let durable = self
            .backend
            .read_manifest(&manifest.handle.id)?
            .expect("fresh-process signal requires durable state");
        match signal.number() {
            libc::SIGKILL => assert!(matches!(
                durable.execution_teardown.stop(),
                KrunStopProgress::KillMayExist { .. }
            )),
            _ => assert!(matches!(
                durable.execution_teardown.stop(),
                KrunStopProgress::GracefulSignalMayExist { .. }
            )),
        }
        let mut witness = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.witness)
            .expect("fresh-process signal witness should open");
        writeln!(
            witness,
            "pid={} sandbox={} signal={}",
            std::process::id(),
            manifest.handle.id,
            signal.number(),
        )
        .and_then(|()| witness.sync_all())
        .expect("fresh-process signal witness should become durable");
        sync_parent(&self.witness);
        Ok(RuntimeProcessSignalOutcome::Delivered)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CrashCase {
    ClaimPersisted,
    BarrierPersisted,
    StopIntentPersisted,
    GracefulBoundaryPersisted,
    GracefulResponseLost,
    KillBoundaryPersisted,
    KillResponseLost,
    TerminalManifestPersisted,
    ProviderResultPersisted,
}

impl CrashCase {
    const ALL: [Self; 9] = [
        Self::ClaimPersisted,
        Self::BarrierPersisted,
        Self::StopIntentPersisted,
        Self::GracefulBoundaryPersisted,
        Self::GracefulResponseLost,
        Self::KillBoundaryPersisted,
        Self::KillResponseLost,
        Self::TerminalManifestPersisted,
        Self::ProviderResultPersisted,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::ClaimPersisted => "claim-persisted",
            Self::BarrierPersisted => "barrier-persisted",
            Self::StopIntentPersisted => "stop-intent-persisted",
            Self::GracefulBoundaryPersisted => "graceful-boundary-persisted",
            Self::GracefulResponseLost => "graceful-response-lost",
            Self::KillBoundaryPersisted => "kill-boundary-persisted",
            Self::KillResponseLost => "kill-response-lost",
            Self::TerminalManifestPersisted => "terminal-manifest-persisted",
            Self::ProviderResultPersisted => "provider-result-persisted",
        }
    }

    fn parse(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|case| case.label() == value)
            .unwrap_or_else(|| panic!("unknown NNC6.5d2 crash case {value:?}"))
    }

    const fn operation(self) -> SandboxExecutionTeardownOperation {
        match self {
            Self::ClaimPersisted | Self::BarrierPersisted | Self::ProviderResultPersisted => {
                SandboxExecutionTeardownOperation::Drain
            }
            Self::StopIntentPersisted
            | Self::GracefulBoundaryPersisted
            | Self::GracefulResponseLost
            | Self::KillBoundaryPersisted
            | Self::KillResponseLost
            | Self::TerminalManifestPersisted => SandboxExecutionTeardownOperation::Stop,
        }
    }

    const fn is_stop(self) -> bool {
        matches!(self.operation(), SandboxExecutionTeardownOperation::Stop)
    }

    const fn seeded_signal_count(self) -> usize {
        match self {
            Self::GracefulResponseLost | Self::KillResponseLost => 1,
            _ => 0,
        }
    }
}

#[test]
fn fresh_process_krun_execution_teardown_contenders_share_one_claim_and_signal() {
    let fixture = TeardownFixture::new("process-contention");
    fixture.drain("process-contention", 1);
    let harness = harness_root(fixture.root.path());
    fs::create_dir_all(&harness).expect("fresh-process harness should exist");
    let network_before = fixture.network_authority();

    let mut left = spawn_child(fixture.root.path(), "contend", "process-contention", "left");
    let mut right = spawn_child(
        fixture.root.path(),
        "contend",
        "process-contention",
        "right",
    );
    wait_for_path(&harness.join("process-contention-left.ready"));
    wait_for_path(&harness.join("process-contention-right.ready"));
    persist_marker(&harness.join("process-contention.go"), b"go");
    let outputs = [wait_for_child(&mut left), wait_for_child(&mut right)];

    for output in &outputs {
        assert_child_success(output, "fresh-process contention");
    }
    assert_eq!(
        outputs
            .iter()
            .filter(|output| output.stdout.contains("NNC65D2_DECISION:execute"))
            .count(),
        1,
    );
    assert_eq!(
        outputs
            .iter()
            .filter(|output| output.stdout.contains("NNC65D2_DECISION:adopt"))
            .count(),
        1,
    );
    assert_ne!(child_pid(&outputs[0].stdout), child_pid(&outputs[1].stdout));
    assert_eq!(signal_lines(fixture.root.path()).len(), 1);
    assert_eq!(fixture.network_authority(), network_before);
}

#[test]
fn fresh_process_krun_execution_teardown_recovers_all_provider_crash_cuts() {
    for case in CrashCase::ALL {
        let fixture = TeardownFixture::new(case.label());
        let network_before = fixture.network_authority();
        seed_crash_cut(&fixture, case);
        let output = run_child(fixture.root.path(), "recover", case.label(), "recovery");
        assert_child_success(&output, case.label());
        assert!(output.stdout.contains("NNC65D2_RECOVERED:succeeded"));
        assert_eq!(
            signal_lines(fixture.root.path()).len(),
            case.seeded_signal_count(),
            "recovery must not redeliver a may-exist signal for {}",
            case.label(),
        );
        assert_eq!(fixture.network_authority(), network_before);
        let manifest = fixture.manifest();
        if case.is_stop() {
            assert!(matches!(
                manifest.execution_teardown.stop(),
                KrunStopProgress::ExecutionStopped { .. }
            ));
        } else {
            assert!(matches!(
                manifest.execution_teardown.drain(),
                KrunDrainProgress::Drained { .. }
            ));
        }
    }
}

#[test]
#[ignore = "subprocess entry point; NNC6.5d2 parent tests supply exact durable roots"]
fn krun_execution_teardown_process_child() {
    let root = PathBuf::from(
        std::env::var_os(ROOT_ENV).expect("the NNC6.5d2 child requires its durable root"),
    );
    let action = required_env(ACTION_ENV);
    let case = required_env(CASE_ENV);
    let role = required_env(ROLE_ENV);
    println!("NNC65D2_PID:{}", std::process::id());
    match action.as_str() {
        "contend" => contention_child(&root, &case, &role),
        "recover" => recovery_child(&root, CrashCase::parse(&case)),
        _ => panic!("unknown NNC6.5d2 child action {action:?}"),
    }
}

fn seed_crash_cut(fixture: &TeardownFixture, case: CrashCase) {
    if case.is_stop() {
        fixture.drain("process-recovery", 1);
    }
    let command = fixture.command(case.operation(), "process-recovery", 1);
    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("crash-cut journal should open");
    let _execution = claim_teardown_execution(&journal, &command);
    let mut manifest = fixture.manifest();
    match case {
        CrashCase::ClaimPersisted => {}
        CrashCase::BarrierPersisted => {
            manifest
                .execution_teardown
                .set_drain(KrunDrainProgress::BarrierPersisted {
                    fence: command.provider_claim().clone(),
                });
        }
        CrashCase::StopIntentPersisted => {
            manifest.shutdown_requested = true;
            manifest.status = SandboxStatus::Stopping;
            manifest.handle.status = SandboxStatus::Stopping;
            manifest
                .execution_teardown
                .set_stop(KrunStopProgress::IntentPersisted {
                    fence: command.provider_claim().clone(),
                });
        }
        CrashCase::GracefulBoundaryPersisted | CrashCase::GracefulResponseLost => {
            set_stopping(&mut manifest);
            manifest
                .execution_teardown
                .set_stop(KrunStopProgress::GracefulSignalMayExist {
                    fence: command.provider_claim().clone(),
                    process: process_identity(&manifest),
                    graceful_signal: "SIGTERM".to_owned(),
                    grace_deadline_unix_millis: 1_000,
                });
        }
        CrashCase::KillBoundaryPersisted | CrashCase::KillResponseLost => {
            set_stopping(&mut manifest);
            manifest
                .execution_teardown
                .set_stop(KrunStopProgress::KillMayExist {
                    fence: command.provider_claim().clone(),
                    process: process_identity(&manifest),
                    redelivery_not_before_unix_millis: 20_000,
                });
        }
        CrashCase::TerminalManifestPersisted => {
            set_stopping(&mut manifest);
            manifest
                .execution_teardown
                .set_stop(KrunStopProgress::ExecutionStopped {
                    fence: command.provider_claim().clone(),
                    evidence: b"terminal manifest persisted".to_vec(),
                });
        }
        CrashCase::ProviderResultPersisted => {
            manifest
                .execution_teardown
                .set_drain(KrunDrainProgress::Drained {
                    fence: command.provider_claim().clone(),
                    evidence: b"provider result persisted".to_vec(),
                });
        }
    }
    fixture.write_manifest(&manifest);
    if matches!(
        case,
        CrashCase::GracefulResponseLost | CrashCase::KillResponseLost
    ) {
        persist_marker(&signal_path(fixture.root.path()), b"seeded-signal\n");
    }
    if case == CrashCase::ProviderResultPersisted {
        journal
            .record_observation(
                command.provider_claim(),
                ProviderCommandObservationKind::Succeeded,
                b"provider result persisted",
            )
            .expect("provider-result crash cut should persist");
    }
}

fn set_stopping(manifest: &mut KrunSandboxManifest) {
    manifest.shutdown_requested = true;
    manifest.status = SandboxStatus::Stopping;
    manifest.handle.status = SandboxStatus::Stopping;
}

fn process_identity(manifest: &KrunSandboxManifest) -> RuntimeProcessIdentity {
    let attempt_id = match &manifest.creator_handoff {
        KrunCreatorHandoffState::RuntimeObserved { receipt } => receipt.attempt_id(),
        other => panic!("crash fixture requires runtime-observed creator, got {other:?}"),
    };
    RuntimeProcessIdentity::fixture(manifest.handle.id.as_str(), attempt_id, 42)
}

fn contention_child(root: &Path, case: &str, role: &str) {
    let harness = harness_root(root);
    persist_marker(&harness.join(format!("{case}-{role}.ready")), b"ready");
    wait_for_path(&harness.join(format!("{case}.go")));

    let base = reopen_backend(root);
    let runtime = Arc::new(FileBackedRuntime::live(base.clone(), signal_path(root)));
    let backend = base.with_teardown_runtime_provider(runtime);
    let manifest = only_manifest(&backend);
    let command = command_for_manifest(&manifest, SandboxExecutionTeardownOperation::Stop, case, 1);
    let journal = backend
        .attempt_idempotency_journal()
        .expect("contending child journal should open");
    match journal
        .claim_dispatch_epoch(command.provider_claim())
        .expect("contending child should claim or adopt")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => {
            println!("NNC65D2_DECISION:execute");
            let observation = backend
                .execute_execution_teardown_with_claim(&command, execution)
                .expect("execute winner should publish its provider observation");
            assert_eq!(
                observation.kind(),
                ProviderCommandObservationKind::InProgress
            );
        }
        ProviderCommandClaimDecision::AdoptExactAttempt(observation) => {
            println!("NNC65D2_DECISION:adopt");
            assert_eq!(
                observation.kind(),
                ProviderCommandObservationKind::InProgress
            );
        }
    }
}

fn recovery_child(root: &Path, case: CrashCase) {
    let base = reopen_backend(root);
    let witness = signal_path(root);
    let runtime = if case == CrashCase::StopIntentPersisted {
        Arc::new(FileBackedRuntime::terminal(base.clone(), witness))
    } else if case.is_stop() {
        Arc::new(FileBackedRuntime::absent(base.clone(), witness))
    } else {
        Arc::new(FileBackedRuntime::live(base.clone(), witness))
    };
    let backend = base.with_teardown_runtime_provider(runtime);
    let manifest = only_manifest(&backend);
    let first = command_for_manifest(&manifest, case.operation(), "process-recovery", 1);
    let journal = backend
        .attempt_idempotency_journal()
        .expect("recovery child journal should open");
    let durable = match journal
        .claim_dispatch_epoch(first.provider_claim())
        .expect("recovery child should adopt its crash-cut claim")
    {
        ProviderCommandClaimDecision::AdoptExactAttempt(observation) => observation,
        ProviderCommandClaimDecision::ExecuteClaimed(_) => {
            panic!("crash-cut claim must already be durable")
        }
    };
    if durable.kind() == ProviderCommandObservationKind::Succeeded {
        println!("NNC65D2_RECOVERED:succeeded");
        return;
    }
    assert_eq!(durable.kind(), ProviderCommandObservationKind::Claimed);
    let inspected = backend.inspect_execution_teardown_with_observation(&first, &durable);
    let kind = observation_kind(&inspected);
    journal
        .record_observation_with_failure_code(
            first.provider_claim(),
            kind,
            inspected.failure_code(),
            inspected.evidence(),
        )
        .expect("recovery inspection should become durable");
    if matches!(
        inspected,
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ) {
        println!("NNC65D2_RECOVERED:succeeded");
        return;
    }
    assert!(matches!(
        inspected,
        SandboxExecutionTeardownObservation::Absent { .. }
            | SandboxExecutionTeardownObservation::RetryAuthorized { .. }
    ));
    let retry = command_for_manifest(&manifest, case.operation(), "process-recovery", 2);
    let execution = match journal
        .claim_dispatch_epoch(retry.provider_claim())
        .expect("recovery retry should receive adjacent authority")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("recovery retry must receive execute authority")
        }
    };
    let result = backend
        .execute_execution_teardown_with_claim(&retry, execution)
        .expect("fresh-process recovery should publish its provider result");
    assert_eq!(result.kind(), ProviderCommandObservationKind::Succeeded);
    println!("NNC65D2_RECOVERED:succeeded");
}

fn observation_kind(
    observation: &SandboxExecutionTeardownObservation,
) -> ProviderCommandObservationKind {
    match observation {
        SandboxExecutionTeardownObservation::Succeeded { .. } => {
            ProviderCommandObservationKind::Succeeded
        }
        SandboxExecutionTeardownObservation::DefiniteFailure { .. } => {
            ProviderCommandObservationKind::DefiniteFailure
        }
        SandboxExecutionTeardownObservation::Absent { .. } => {
            ProviderCommandObservationKind::Absent
        }
        SandboxExecutionTeardownObservation::RetryAuthorized { .. } => {
            ProviderCommandObservationKind::RetryAuthorized
        }
        SandboxExecutionTeardownObservation::InProgress { .. } => {
            ProviderCommandObservationKind::InProgress
        }
        SandboxExecutionTeardownObservation::Ambiguous { .. } => {
            ProviderCommandObservationKind::Ambiguous
        }
    }
}

fn command_for_manifest(
    manifest: &KrunSandboxManifest,
    operation: SandboxExecutionTeardownOperation,
    attempt: &str,
    epoch: u64,
) -> SandboxExecutionTeardownCommand {
    let plan = manifest
        .provision_network_plan
        .as_ref()
        .expect("fresh-process manifest should retain its plan");
    let claim = ProviderCommandClaim::new(ProviderCommandClaimInput {
        authority_id: "authority-krun-teardown".to_owned(),
        effect_subject: format!("{{\"sandbox\":\"{}\"}}", manifest.handle.id),
        source_attempt_id: None,
        attempt_id: attempt.to_owned(),
        dispatch_epoch: epoch,
        workload_generation: plan.generation().as_u64(),
        restart_ordinal: 0,
        desired_digest: "1".repeat(64),
        source_digest: "2".repeat(64),
        network_plan_digest: plan.network_plan().digest().to_string(),
        provider_target_digest: "3".repeat(64),
        operation: match operation {
            SandboxExecutionTeardownOperation::Drain => ProviderCommandOperation::DrainExecution,
            SandboxExecutionTeardownOperation::Stop => ProviderCommandOperation::StopExecution,
        },
    })
    .expect("fresh-process provider claim should validate");
    SandboxExecutionTeardownCommand::new(
        manifest.spec.tenant_id.clone(),
        manifest.handle.id.clone(),
        manifest.execution_attempt_id.clone(),
        "nimbus-sandbox.krun-execution",
        operation,
        claim,
    )
    .expect("fresh-process command should validate")
}

fn reopen_backend(root: &Path) -> KrunSandboxBackend {
    let mut config = KrunSandboxBackendConfig::under_root(root);
    config.stop_timeout = Duration::from_millis(100);
    KrunSandboxBackend::new(config)
}

fn only_manifest(backend: &KrunSandboxBackend) -> KrunSandboxManifest {
    let mut manifests = Vec::new();
    let tenants = backend.config.workload_state_root.join("tenants");
    for tenant in fs::read_dir(&tenants).expect("fresh-process tenant root should read") {
        let sandboxes = tenant
            .expect("fresh-process tenant entry should read")
            .path()
            .join("sandboxes");
        for sandbox in fs::read_dir(sandboxes).expect("fresh-process sandbox root should read") {
            let id = crate::SandboxId::new(
                sandbox
                    .expect("fresh-process sandbox entry should read")
                    .file_name()
                    .to_string_lossy()
                    .into_owned(),
            );
            if let Some(manifest) = backend
                .read_manifest(&id)
                .expect("fresh-process manifest should read")
            {
                manifests.push(manifest);
            }
        }
    }
    assert_eq!(
        manifests.len(),
        1,
        "fresh-process root must own one manifest"
    );
    manifests.pop().expect("one manifest should exist")
}

fn spawn_child(root: &Path, action: &str, case: &str, role: &str) -> Child {
    Command::new(std::env::current_exe().expect("sandbox test executable should resolve"))
        .args(["--exact", CHILD_TEST, "--ignored", "--nocapture"])
        .env(ROOT_ENV, root)
        .env(ACTION_ENV, action)
        .env(CASE_ENV, case)
        .env(ROLE_ENV, role)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("failed to spawn NNC6.5d2 {action} child: {error}"))
}

fn run_child(root: &Path, action: &str, case: &str, role: &str) -> ChildOutput {
    let mut child = spawn_child(root, action, case, role);
    wait_for_child(&mut child)
}

fn wait_for_child(child: &mut Child) -> ChildOutput {
    let deadline = Instant::now() + CHILD_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL_INTERVAL),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return collect_child(child, None, "child exceeded its 15-second bound");
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return collect_child(child, None, &format!("child wait failed: {error}"));
            }
        }
    };
    collect_child(child, Some(status), "")
}

fn collect_child(child: &mut Child, status: Option<ExitStatus>, wait_error: &str) -> ChildOutput {
    let mut stdout = String::new();
    child
        .stdout
        .as_mut()
        .expect("child stdout should be piped")
        .read_to_string(&mut stdout)
        .expect("child stdout should read");
    let mut stderr = String::new();
    child
        .stderr
        .as_mut()
        .expect("child stderr should be piped")
        .read_to_string(&mut stderr)
        .expect("child stderr should read");
    let status = status.unwrap_or_else(|| {
        panic!("{wait_error}\nchild stdout:\n{stdout}\nchild stderr:\n{stderr}")
    });
    ChildOutput {
        status,
        stdout,
        stderr,
    }
}

struct ChildOutput {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

fn assert_child_success(output: &ChildOutput, context: &str) {
    assert!(
        output.status.success(),
        "NNC6.5d2 child failed for {context}\nstdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr,
    );
}

fn wait_for_path(path: &Path) {
    let deadline = Instant::now() + GATE_TIMEOUT;
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for semantic gate {}",
            path.display(),
        );
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn persist_marker(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("marker parent should exist");
    }
    let mut file = fs::File::create(path).expect("marker should create");
    file.write_all(bytes).expect("marker should write");
    file.sync_all().expect("marker should sync");
    sync_parent(path);
}

fn sync_parent(path: &Path) {
    fs::File::open(path.parent().expect("path should have a parent"))
        .and_then(|parent| parent.sync_all())
        .expect("marker parent should sync");
}

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("fresh-process child requires {name}"))
}

fn harness_root(root: &Path) -> PathBuf {
    root.join("nnc6.5d2-process-harness")
}

fn signal_path(root: &Path) -> PathBuf {
    harness_root(root).join("signals.log")
}

fn signal_lines(root: &Path) -> Vec<String> {
    match fs::read_to_string(signal_path(root)) {
        Ok(contents) => contents.lines().map(str::to_owned).collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => panic!("signal witness should read: {error}"),
    }
}

fn child_pid(stdout: &str) -> u32 {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("NNC65D2_PID:"))
        .expect("child must report its process id")
        .parse()
        .expect("child process id should parse")
}
