use std::fs::{File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use crate::backends::conmon::runtime_process::{
    RuntimeProcessIdentity, RuntimeProcessIdentityObservation, RuntimeProcessSignal,
    RuntimeProcessSignalOutcome,
};
use crate::{
    ProviderCommandAttemptJournal, ProviderCommandClaimDecision, ProviderCommandClaimInput,
    ProviderCommandObservationKind, SandboxExecutionTeardownCommand,
    SandboxExecutionTeardownObservation, SandboxExecutionTeardownOperation,
};

use super::*;

const ROOT_ENV: &str = "NIMBUS_NNC65D1_TEARDOWN_ROOT";
const ACTION_ENV: &str = "NIMBUS_NNC65D1_TEARDOWN_ACTION";
const CASE_ENV: &str = "NIMBUS_NNC65D1_TEARDOWN_CASE";
const ROLE_ENV: &str = "NIMBUS_NNC65D1_TEARDOWN_ROLE";
const CHILD_TEST: &str = "backends::container::runtime::teardown::tests::fresh_process::nnc6_5d1_execution_teardown_child";
const CHILD_TIMEOUT: Duration = Duration::from_secs(15);
const GATE_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(5);

#[derive(Clone)]
struct FileBackedRuntime {
    backend: ContainerSandboxBackend,
    witness: PathBuf,
    now_unix_millis: u64,
    terminal: bool,
    process_observation: RuntimeProcessIdentityObservation,
    absent_after_signal: bool,
}

impl FileBackedRuntime {
    fn live(backend: ContainerSandboxBackend, witness: PathBuf) -> Self {
        Self {
            backend,
            witness,
            now_unix_millis: 100,
            terminal: false,
            process_observation: RuntimeProcessIdentityObservation::ExactLive,
            absent_after_signal: false,
        }
    }

    fn terminal(backend: ContainerSandboxBackend, witness: PathBuf) -> Self {
        Self {
            terminal: true,
            ..Self::live(backend, witness)
        }
    }

    fn absent(backend: ContainerSandboxBackend, witness: PathBuf) -> Self {
        Self {
            process_observation: RuntimeProcessIdentityObservation::ExplicitlyAbsent,
            ..Self::live(backend, witness)
        }
    }

    fn live_until_signal(backend: ContainerSandboxBackend, witness: PathBuf) -> Self {
        Self {
            absent_after_signal: true,
            ..Self::live(backend, witness)
        }
    }
}

impl effects::ContainerExecutionTeardownRuntime for FileBackedRuntime {
    fn now_unix_millis(&self) -> crate::Result<u64> {
        Ok(self.now_unix_millis)
    }

    fn execution_is_terminal(&self, _manifest: &ContainerSandboxManifest) -> crate::Result<bool> {
        Ok(self.terminal)
    }

    fn capture_process(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> crate::Result<RuntimeProcessIdentity> {
        Ok(RuntimeProcessIdentity::fixture(
            manifest.handle.id.as_str(),
            "nnc6-5d1-process-fixture",
            42,
        ))
    }

    fn inspect_process(
        &self,
        _manifest: &ContainerSandboxManifest,
        _identity: &RuntimeProcessIdentity,
    ) -> crate::Result<RuntimeProcessIdentityObservation> {
        if self.absent_after_signal && self.witness.exists() {
            Ok(RuntimeProcessIdentityObservation::ExplicitlyAbsent)
        } else {
            Ok(self.process_observation)
        }
    }

    fn signal_process(
        &self,
        manifest: &ContainerSandboxManifest,
        _identity: &RuntimeProcessIdentity,
        signal: RuntimeProcessSignal,
    ) -> crate::Result<RuntimeProcessSignalOutcome> {
        let durable = self
            .backend
            .read_manifest(&manifest.handle.id)?
            .expect("a signal effect requires a durable manifest");
        match signal.number() {
            libc::SIGKILL => assert!(matches!(
                durable.execution_teardown.stop(),
                ContainerStopProgress::KillMayExist { .. }
            )),
            _ => assert!(matches!(
                durable.execution_teardown.stop(),
                ContainerStopProgress::TermMayExist { .. }
            )),
        }
        let mut witness = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.witness)
            .expect("the process signal witness should open");
        writeln!(
            witness,
            "pid={} sandbox={} signal={}",
            std::process::id(),
            manifest.handle.id,
            signal.number()
        )
        .and_then(|()| witness.sync_all())
        .expect("the process signal witness should become durable");
        sync_parent(&self.witness);
        Ok(RuntimeProcessSignalOutcome::Delivered)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CrashCase {
    BarrierPersisted,
    StopIntentPersisted,
    TermBeforeEffect,
    TermResponseLost,
    KillBeforeEffect,
    KillResponseLost,
    TerminalEvidencePersisted,
    ProviderResultLost,
}

impl CrashCase {
    const ALL: [Self; 8] = [
        Self::BarrierPersisted,
        Self::StopIntentPersisted,
        Self::TermBeforeEffect,
        Self::TermResponseLost,
        Self::KillBeforeEffect,
        Self::KillResponseLost,
        Self::TerminalEvidencePersisted,
        Self::ProviderResultLost,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::BarrierPersisted => "barrier-persisted",
            Self::StopIntentPersisted => "stop-intent-persisted",
            Self::TermBeforeEffect => "term-before-effect",
            Self::TermResponseLost => "term-response-lost",
            Self::KillBeforeEffect => "kill-before-effect",
            Self::KillResponseLost => "kill-response-lost",
            Self::TerminalEvidencePersisted => "terminal-evidence-persisted",
            Self::ProviderResultLost => "provider-result-lost",
        }
    }

    fn parse(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|case| case.label() == value)
            .unwrap_or_else(|| panic!("unknown NNC6.5d1 crash case {value:?}"))
    }

    const fn operation(self) -> SandboxExecutionTeardownOperation {
        match self {
            Self::BarrierPersisted | Self::ProviderResultLost => {
                SandboxExecutionTeardownOperation::Drain
            }
            Self::StopIntentPersisted
            | Self::TermBeforeEffect
            | Self::TermResponseLost
            | Self::KillBeforeEffect
            | Self::KillResponseLost
            | Self::TerminalEvidencePersisted => SandboxExecutionTeardownOperation::Stop,
        }
    }

    const fn needs_completed_drain(self) -> bool {
        !matches!(self, Self::BarrierPersisted | Self::ProviderResultLost)
    }

    const fn recovery_uses_next_epoch(self) -> bool {
        !matches!(
            self,
            Self::TerminalEvidencePersisted | Self::ProviderResultLost
        )
    }
}

#[test]
fn independent_process_contenders_publish_one_drain_and_one_stop_effect() {
    let fixture = TeardownFixture::attached("process-contention");
    let harness = harness_root(fixture.root.path());
    std::fs::create_dir_all(&harness).expect("the process harness directory should exist");
    prime_fresh_process_recovery(fixture.root.path());
    let network_files_before = fixture.durable_network_files();
    let network_manifest_before = fixture.network_authority();

    let drain = fixture.command(
        SandboxExecutionTeardownOperation::Drain,
        "process-contention",
        1,
    );
    let drain_outputs = run_contenders(fixture.root.path(), "contention-drain", "drain");
    assert_one_execute_and_one_adopt(&drain_outputs);
    assert_distinct_child_pids(&drain_outputs);
    assert_eq!(
        fixture
            .backend
            .attempt_idempotency_journal()
            .expect("the Container journal should reopen")
            .adopt_exact_attempt(drain.provider_claim())
            .expect("the exact drain journal entry should read")
            .expect("the exact drain journal entry should exist")
            .kind(),
        ProviderCommandObservationKind::Succeeded
    );
    assert!(matches!(
        fixture.manifest().execution_teardown.drain(),
        ContainerDrainProgress::Drained { fence, .. } if fence == drain.provider_claim()
    ));
    assert_eq!(fixture.durable_network_files(), network_files_before);
    assert_eq!(fixture.network_authority(), network_manifest_before);

    let stop = fixture.command(
        SandboxExecutionTeardownOperation::Stop,
        "process-contention",
        1,
    );
    let stop_outputs = run_contenders(fixture.root.path(), "contention-stop", "stop");
    assert_one_execute_and_one_adopt(&stop_outputs);
    assert_distinct_child_pids(&stop_outputs);
    assert_eq!(
        fixture
            .backend
            .attempt_idempotency_journal()
            .expect("the Container journal should reopen")
            .adopt_exact_attempt(stop.provider_claim())
            .expect("the exact stop journal entry should read")
            .expect("the exact stop journal entry should exist")
            .kind(),
        ProviderCommandObservationKind::InProgress
    );
    assert!(matches!(
        fixture.manifest().execution_teardown.stop(),
        ContainerStopProgress::TermMayExist { fence, .. } if fence == stop.provider_claim()
    ));
    let signals = signal_lines(fixture.root.path());
    assert_eq!(signals.len(), 1, "one process may emit the TERM effect");
    assert!(signals[0].contains(&format!("signal={}", libc::SIGTERM)));
    assert_eq!(fixture.durable_network_files(), network_files_before);
    assert_eq!(fixture.network_authority(), network_manifest_before);
}

#[test]
fn fresh_process_execution_teardown_crash_cut_matrix_converges() {
    for case in CrashCase::ALL {
        let fixture = TeardownFixture::attached(&format!("process-{}", case.label()));
        if case.needs_completed_drain() {
            let drain = fixture.command(
                SandboxExecutionTeardownOperation::Drain,
                &format!("matrix-{}", case.label()),
                1,
            );
            assert!(matches!(
                fixture.backend.execute_execution_teardown(&drain),
                SandboxExecutionTeardownObservation::Succeeded { .. }
            ));
        }
        std::fs::create_dir_all(harness_root(fixture.root.path()))
            .expect("the matrix process harness directory should exist");
        prime_fresh_process_recovery(fixture.root.path());
        let network_files_before = fixture.durable_network_files();
        let network_manifest_before = fixture.network_authority();

        let writer = run_child(fixture.root.path(), "matrix-writer", case.label(), "writer");
        assert_child_success(&writer, case.label(), "writer");
        let recovery = run_child(
            fixture.root.path(),
            "matrix-recovery",
            case.label(),
            "recovery",
        );
        assert_child_success(&recovery, case.label(), "recovery");
        assert_ne!(
            child_pid(&writer.stdout),
            child_pid(&recovery.stdout),
            "writer and recovery must be independent processes for {}",
            case.label()
        );
        assert!(
            recovery.stdout.contains("NNC65D1_INSPECT_FIRST:"),
            "recovery must inspect before it claims another epoch for {}:\n{}",
            case.label(),
            recovery.stdout
        );
        assert!(
            recovery.stdout.contains("NNC65D1_CONVERGED:succeeded"),
            "recovery must converge for {}:\n{}",
            case.label(),
            recovery.stdout
        );

        let manifest = fixture.manifest();
        match case.operation() {
            SandboxExecutionTeardownOperation::Drain => assert!(matches!(
                manifest.execution_teardown.drain(),
                ContainerDrainProgress::Drained { .. }
            )),
            SandboxExecutionTeardownOperation::Stop => assert!(matches!(
                manifest.execution_teardown.stop(),
                ContainerStopProgress::ExecutionStopped { .. }
            )),
        }
        if case.recovery_uses_next_epoch() {
            assert!(
                recovery.stdout.contains("NNC65D1_NEXT_EPOCH:2"),
                "exact recovery evidence must authorize only the next epoch for {}:\n{}",
                case.label(),
                recovery.stdout
            );
        } else {
            assert!(
                !recovery.stdout.contains("NNC65D1_NEXT_EPOCH:"),
                "terminal current-epoch evidence must not create another epoch for {}",
                case.label()
            );
        }

        let signals = signal_lines(fixture.root.path());
        let expected_signals = match case {
            CrashCase::TermResponseLost => vec![libc::SIGTERM],
            CrashCase::KillBeforeEffect | CrashCase::KillResponseLost => vec![libc::SIGKILL],
            _ => Vec::new(),
        };
        assert_eq!(
            signal_numbers(&signals),
            expected_signals,
            "recovery must not duplicate an unresolved signal for {}",
            case.label()
        );
        assert_eq!(
            fixture.durable_network_files(),
            network_files_before,
            "provider recovery must not mutate network authority for {}",
            case.label()
        );
        assert_eq!(
            fixture.network_authority(),
            network_manifest_before,
            "manifest network authority must stay byte-identical for {}",
            case.label()
        );
    }
}

#[test]
fn fresh_process_retry_claim_crashes_reconcile_before_manifest_progress() {
    let fixture = TeardownFixture::attached("process-retry-claim");
    std::fs::create_dir_all(harness_root(fixture.root.path()))
        .expect("the retry-claim process harness directory should exist");
    prime_fresh_process_recovery(fixture.root.path());
    let network_files_before = fixture.durable_network_files();
    let network_manifest_before = fixture.network_authority();
    let first = fixture.command(
        SandboxExecutionTeardownOperation::Drain,
        "process-retry-claim",
        1,
    );
    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("the retry-claim journal should open");
    assert!(matches!(
        journal
            .claim_dispatch_epoch(first.provider_claim())
            .expect("the first drain epoch should claim"),
        ProviderCommandClaimDecision::ExecuteClaimed(_)
    ));
    let mut manifest = fixture.manifest();
    manifest
        .execution_teardown
        .set_drain(ContainerDrainProgress::BarrierPersisted {
            fence: first.provider_claim().clone(),
        });
    fixture
        .backend
        .write_existing_workload_manifest(&manifest)
        .expect("the initial drain barrier should become durable");
    journal
        .record_observation(
            first.provider_claim(),
            ProviderCommandObservationKind::Absent,
            b"initial barrier has no external effect",
        )
        .expect("the initial exact absence should become durable");

    let mut process_ids = Vec::new();
    for epoch in [2_u64, 3] {
        let epoch = epoch.to_string();
        let claim = run_child(fixture.root.path(), "retry-claim", &epoch, "claim");
        assert_child_success(&claim, &epoch, "retry claimant");
        assert!(claim.stdout.contains("NNC65D1_RETRY_CLAIMED:"));
        process_ids.push(child_pid(&claim.stdout));

        let inspect = run_child(fixture.root.path(), "retry-inspect", &epoch, "inspect");
        assert_child_success(&inspect, &epoch, "retry inspector");
        assert!(inspect.stdout.contains("NNC65D1_RETRY_RECONCILED:absent"));
        process_ids.push(child_pid(&inspect.stdout));
    }
    let execute = run_child(fixture.root.path(), "retry-execute", "4", "execute");
    assert_child_success(&execute, "4", "retry executor");
    assert!(execute.stdout.contains("NNC65D1_RETRY_CONVERGED:succeeded"));
    process_ids.push(child_pid(&execute.stdout));
    process_ids.sort_unstable();
    process_ids.dedup();
    assert_eq!(
        process_ids.len(),
        5,
        "each retry claim, inspection, and final execution must use an independent process"
    );

    let final_command = fixture.command(
        SandboxExecutionTeardownOperation::Drain,
        "process-retry-claim",
        4,
    );
    assert!(matches!(
        fixture.manifest().execution_teardown.drain(),
        ContainerDrainProgress::Drained { fence, .. } if fence == final_command.provider_claim()
    ));
    assert!(signal_lines(fixture.root.path()).is_empty());
    assert_eq!(fixture.durable_network_files(), network_files_before);
    assert_eq!(fixture.network_authority(), network_manifest_before);
}

#[test]
#[ignore = "subprocess entry point; NNC6.5d1 parent tests supply exact durable roots"]
fn nnc6_5d1_execution_teardown_child() {
    let root = PathBuf::from(
        std::env::var_os(ROOT_ENV).expect("the NNC6.5d1 child requires its durable root"),
    );
    let action = required_env(ACTION_ENV);
    let case = required_env(CASE_ENV);
    let role = required_env(ROLE_ENV);
    println!("NNC65D1_PID:{}", std::process::id());
    match action.as_str() {
        "prime" => {
            let backend = reopen_backend(&root);
            let _ = backend
                .attempt_idempotency_journal()
                .expect("the priming process should open the shared journal");
            println!("NNC65D1_PRIMED:fresh-process-recovery");
        }
        "contend" => contention_child(&root, &case, &role),
        "matrix-writer" => matrix_writer_child(&root, CrashCase::parse(&case)),
        "matrix-recovery" => matrix_recovery_child(&root, CrashCase::parse(&case)),
        "retry-claim" => retry_claim_child(&root, parse_epoch(&case)),
        "retry-inspect" => retry_inspect_child(&root, parse_epoch(&case)),
        "retry-execute" => retry_execute_child(&root, parse_epoch(&case)),
        _ => panic!("unknown NNC6.5d1 child action {action:?}"),
    }
}

fn retry_claim_child(root: &Path, epoch: u64) {
    let backend = reopen_backend(root);
    let command = retry_claim_command(&backend, epoch);
    let journal = backend
        .attempt_idempotency_journal()
        .expect("the retry claimant should open the Container journal");
    assert!(matches!(
        journal
            .claim_dispatch_epoch(command.provider_claim())
            .expect("the exact retry epoch should claim"),
        ProviderCommandClaimDecision::ExecuteClaimed(_)
    ));
    println!("NNC65D1_RETRY_CLAIMED:{epoch}");
}

fn retry_inspect_child(root: &Path, epoch: u64) {
    let backend = reopen_backend(root);
    let command = retry_claim_command(&backend, epoch);
    let journal = backend
        .attempt_idempotency_journal()
        .expect("the retry inspector should open the Container journal");
    let durable = journal
        .adopt_exact_attempt(command.provider_claim())
        .expect("the exact claimed retry should read")
        .expect("the exact claimed retry should exist");
    assert_eq!(durable.kind(), ProviderCommandObservationKind::Claimed);
    let before = snapshot_files(root);
    let observation = backend.inspect_execution_teardown_with_observation(&command, &durable);
    assert!(matches!(
        observation,
        SandboxExecutionTeardownObservation::Absent { .. }
    ));
    assert_eq!(
        snapshot_files(root),
        before,
        "fresh-process retry inspection must not change a durable byte"
    );
    record_observation(&journal, &command, &observation);
    println!("NNC65D1_RETRY_RECONCILED:absent");
}

fn retry_execute_child(root: &Path, epoch: u64) {
    let backend = reopen_backend(root);
    let command = retry_claim_command(&backend, epoch);
    let journal = backend
        .attempt_idempotency_journal()
        .expect("the retry executor should open the Container journal");
    let execution = match journal
        .claim_dispatch_epoch(command.provider_claim())
        .expect("the final retry epoch should claim")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("the final retry epoch must receive execute authority")
        }
    };
    let observation = backend
        .execute_execution_teardown_with_claim(&command, execution)
        .expect("the final retry effect and result should publish atomically");
    assert_eq!(
        observation.kind(),
        ProviderCommandObservationKind::Succeeded
    );
    println!("NNC65D1_RETRY_CONVERGED:succeeded");
}

fn retry_claim_command(
    backend: &ContainerSandboxBackend,
    epoch: u64,
) -> SandboxExecutionTeardownCommand {
    command_for(
        backend,
        &crate::SandboxId::new("container-teardown-process-retry-claim"),
        SandboxExecutionTeardownOperation::Drain,
        "process-retry-claim",
        epoch,
    )
}

fn parse_epoch(value: &str) -> u64 {
    value
        .parse()
        .unwrap_or_else(|error| panic!("invalid NNC6.5d1 retry epoch {value:?}: {error}"))
}

fn contention_child(root: &Path, tag: &str, role: &str) {
    let ready = harness_root(root).join(format!("{tag}-{role}.ready"));
    persist_marker(&ready, std::process::id().to_string().as_bytes());
    wait_for_gate(&harness_root(root).join(format!("{tag}.go")));

    let operation = match tag {
        "contention-drain" => SandboxExecutionTeardownOperation::Drain,
        "contention-stop" => SandboxExecutionTeardownOperation::Stop,
        _ => panic!("unknown contention tag {tag:?}"),
    };
    let backend = reopen_backend(root);
    let id = crate::SandboxId::new("container-teardown-process-contention");
    let command = command_for(&backend, &id, operation, "process-contention", 1);
    let journal = backend
        .attempt_idempotency_journal()
        .expect("the Container journal should open in the child");
    match journal
        .claim_dispatch_epoch(command.provider_claim())
        .expect("the contending child claim should resolve")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(_) => {
            println!("NNC65D1_DECISION:execute");
            let runtime = FileBackedRuntime::live(backend.clone(), signal_path(root));
            let observation = backend
                .execute_execution_teardown_inner_with_runtime(&command, &runtime)
                .expect("the execute winner should run the exact teardown effect");
            record_observation(&journal, &command, &observation);
        }
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            println!("NNC65D1_DECISION:adopt");
        }
    }
}

fn matrix_writer_child(root: &Path, case: CrashCase) {
    let backend = reopen_backend(root);
    let id = matrix_sandbox_id(case);
    let command = command_for(
        &backend,
        &id,
        case.operation(),
        &format!("matrix-{}", case.label()),
        1,
    );
    let journal = backend
        .attempt_idempotency_journal()
        .expect("the matrix writer should open the Container journal");
    assert!(matches!(
        journal
            .claim_dispatch_epoch(command.provider_claim())
            .expect("the matrix writer should claim the first epoch"),
        ProviderCommandClaimDecision::ExecuteClaimed(_)
    ));

    match case {
        CrashCase::BarrierPersisted => {
            let mut manifest = read_manifest(&backend, &id);
            manifest
                .execution_teardown
                .set_drain(ContainerDrainProgress::BarrierPersisted {
                    fence: command.provider_claim().clone(),
                });
            backend
                .write_existing_workload_manifest(&manifest)
                .expect("the barrier crash cut should become durable");
        }
        CrashCase::ProviderResultLost => {
            assert!(matches!(
                backend.execute_execution_teardown(&command),
                SandboxExecutionTeardownObservation::Succeeded { .. }
            ));
        }
        CrashCase::TerminalEvidencePersisted => {
            let runtime = FileBackedRuntime::terminal(backend.clone(), signal_path(root));
            assert!(matches!(
                backend
                    .execute_execution_teardown_inner_with_runtime(&command, &runtime)
                    .expect("the terminal-evidence cut should persist"),
                SandboxExecutionTeardownObservation::Succeeded { .. }
            ));
        }
        CrashCase::StopIntentPersisted
        | CrashCase::TermBeforeEffect
        | CrashCase::TermResponseLost
        | CrashCase::KillBeforeEffect
        | CrashCase::KillResponseLost => persist_stop_cut(&backend, &command, case, root),
    }
    println!("NNC65D1_WRITER_CUT:{}", case.label());
}

fn persist_stop_cut(
    backend: &ContainerSandboxBackend,
    command: &SandboxExecutionTeardownCommand,
    case: CrashCase,
    root: &Path,
) {
    let mut manifest = read_manifest(backend, command.sandbox_id());
    manifest.shutdown_requested = true;
    super::super::super::synchronize_handle_status(&mut manifest, crate::SandboxStatus::Stopping);
    let process = RuntimeProcessIdentity::fixture(
        manifest.handle.id.as_str(),
        "nnc6-5d1-process-fixture",
        42,
    );
    let progress = match case {
        CrashCase::StopIntentPersisted => ContainerStopProgress::IntentPersisted {
            fence: command.provider_claim().clone(),
        },
        CrashCase::TermBeforeEffect | CrashCase::TermResponseLost => {
            ContainerStopProgress::TermMayExist {
                fence: command.provider_claim().clone(),
                process: process.clone(),
                grace_deadline_unix_millis: 1_000,
            }
        }
        CrashCase::KillBeforeEffect | CrashCase::KillResponseLost => {
            ContainerStopProgress::KillMayExist {
                fence: command.provider_claim().clone(),
                process: process.clone(),
                redelivery_not_before_unix_millis: 100,
            }
        }
        _ => unreachable!("only explicit stop crash cuts use this helper"),
    };
    manifest.execution_teardown.set_stop(progress);
    backend
        .write_existing_workload_manifest(&manifest)
        .expect("the stop crash cut should become durable before its effect");

    let signal = match case {
        CrashCase::TermResponseLost => Some(
            RuntimeProcessSignal::parse("TERM")
                .expect("the named TERM fixture signal should validate"),
        ),
        CrashCase::KillResponseLost => Some(RuntimeProcessSignal::kill()),
        _ => None,
    };
    if let Some(signal) = signal {
        let runtime = FileBackedRuntime::live(backend.clone(), signal_path(root));
        assert_eq!(
            runtime
                .signal_process(&manifest, &process, signal)
                .expect("the response-loss writer should perform the exact signal"),
            RuntimeProcessSignalOutcome::Delivered
        );
    }
}

fn matrix_recovery_child(root: &Path, case: CrashCase) {
    let backend = reopen_backend(root);
    let id = matrix_sandbox_id(case);
    let current = command_for(
        &backend,
        &id,
        case.operation(),
        &format!("matrix-{}", case.label()),
        1,
    );
    let journal = backend
        .attempt_idempotency_journal()
        .expect("the matrix recovery should reopen the Container journal");
    let durable = journal
        .adopt_exact_attempt(current.provider_claim())
        .expect("the recovery should inspect the exact durable provider claim")
        .expect("the writer must leave an exact provider claim");
    assert_eq!(durable.kind(), ProviderCommandObservationKind::Claimed);

    let runtime = recovery_runtime(&backend, root, case);
    let inspected = backend
        .inspect_execution_teardown_inner_with_runtime(&current, &runtime)
        .expect("fresh-process inspection should classify the crash cut");
    println!("NNC65D1_INSPECT_FIRST:{:?}", observation_kind(&inspected));
    record_observation(&journal, &current, &inspected);

    if matches!(
        inspected,
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ) {
        println!("NNC65D1_CONVERGED:succeeded");
        return;
    }
    assert!(
        matches!(
            inspected,
            SandboxExecutionTeardownObservation::Absent { .. }
                | SandboxExecutionTeardownObservation::RetryAuthorized { .. }
        ),
        "the crash cut must prove exact retry authority before retry: {inspected:?}"
    );

    let next = command_for(
        &backend,
        &id,
        case.operation(),
        &format!("matrix-{}", case.label()),
        2,
    );
    let next_execution = match journal
        .claim_dispatch_epoch(next.provider_claim())
        .expect("authoritative recovery evidence should permit the exact next epoch")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("the exact next epoch must receive execute authority")
        }
    };
    println!("NNC65D1_NEXT_EPOCH:2");
    let mut recovered = backend
        .execute_execution_teardown_inner_with_runtime_and_authorization(
            &next,
            &runtime,
            Some(next_execution.observation()),
        )
        .expect("the exact next epoch should converge the crash cut");
    if case == CrashCase::KillBeforeEffect {
        assert!(matches!(
            recovered,
            SandboxExecutionTeardownObservation::InProgress { .. }
        ));
        record_observation(&journal, &next, &recovered);
        let durable = journal
            .adopt_exact_attempt(next.provider_claim())
            .expect("the KILL redelivery observation should read")
            .expect("the KILL redelivery observation should exist");
        let inspected = backend
            .inspect_execution_teardown_inner_with_runtime_and_authorization(
                &next,
                &runtime,
                Some(&durable),
            )
            .expect("the exact KILL redelivery should become absent");
        assert!(matches!(
            inspected,
            SandboxExecutionTeardownObservation::RetryAuthorized { .. }
        ));
        record_observation(&journal, &next, &inspected);

        let final_command = command_for(
            &backend,
            &id,
            case.operation(),
            &format!("matrix-{}", case.label()),
            3,
        );
        let final_execution = match journal
            .claim_dispatch_epoch(final_command.provider_claim())
            .expect("exact KILL retry authority should permit the final epoch")
        {
            ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
            ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
                panic!("the final KILL epoch must receive execute authority")
            }
        };
        println!("NNC65D1_NEXT_EPOCH:3");
        recovered = backend
            .execute_execution_teardown_inner_with_runtime_and_authorization(
                &final_command,
                &runtime,
                Some(final_execution.observation()),
            )
            .expect("the exact KILL retry authority should converge");
        record_observation(&journal, &final_command, &recovered);
    }
    assert!(
        matches!(
            recovered,
            SandboxExecutionTeardownObservation::Succeeded { .. }
        ),
        "the exact next epoch must converge: {recovered:?}"
    );
    if case != CrashCase::KillBeforeEffect {
        record_observation(&journal, &next, &recovered);
    }
    println!("NNC65D1_CONVERGED:succeeded");
}

fn recovery_runtime(
    backend: &ContainerSandboxBackend,
    root: &Path,
    case: CrashCase,
) -> FileBackedRuntime {
    match case {
        CrashCase::StopIntentPersisted => {
            FileBackedRuntime::terminal(backend.clone(), signal_path(root))
        }
        CrashCase::KillBeforeEffect => {
            FileBackedRuntime::live_until_signal(backend.clone(), signal_path(root))
        }
        CrashCase::TermBeforeEffect | CrashCase::TermResponseLost | CrashCase::KillResponseLost => {
            FileBackedRuntime::absent(backend.clone(), signal_path(root))
        }
        CrashCase::BarrierPersisted
        | CrashCase::TerminalEvidencePersisted
        | CrashCase::ProviderResultLost => {
            FileBackedRuntime::live(backend.clone(), signal_path(root))
        }
    }
}

fn command_for(
    backend: &ContainerSandboxBackend,
    id: &crate::SandboxId,
    operation: SandboxExecutionTeardownOperation,
    attempt: &str,
    epoch: u64,
) -> SandboxExecutionTeardownCommand {
    let manifest = read_manifest(backend, id);
    let plan = manifest
        .provision_network_plan
        .as_ref()
        .expect("the process fixture must retain its exact provision plan");
    let claim = crate::ProviderCommandClaim::new(ProviderCommandClaimInput {
        authority_id: "authority-container-teardown".to_owned(),
        effect_subject: format!("{{\"sandbox\":\"{}\"}}", id),
        source_attempt_id: None,
        attempt_id: attempt.to_owned(),
        dispatch_epoch: epoch,
        workload_generation: plan.generation().as_u64(),
        restart_ordinal: 0,
        desired_digest: "1".repeat(64),
        source_digest: "2".repeat(64),
        network_plan_digest: plan.network_plan().digest().to_string(),
        provider_target_digest: "3".repeat(64),
        operation: operation.provider_operation(),
    })
    .expect("the deterministic process claim should validate");
    SandboxExecutionTeardownCommand::new(
        manifest.spec.tenant_id,
        id.clone(),
        manifest.execution_attempt_id,
        CONTAINER_EXECUTION_PROVIDER_KEY,
        operation,
        claim,
    )
    .expect("the deterministic process command should validate")
}

fn record_observation(
    journal: &ProviderCommandAttemptJournal,
    command: &SandboxExecutionTeardownCommand,
    observation: &SandboxExecutionTeardownObservation,
) {
    journal
        .record_observation_with_failure_code(
            command.provider_claim(),
            observation_kind(observation),
            observation.failure_code(),
            observation.evidence(),
        )
        .expect("the exact provider observation should become durable");
}

const fn observation_kind(
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

fn reopen_backend(root: &Path) -> ContainerSandboxBackend {
    ContainerSandboxBackend::new(
        super::super::super::ContainerSandboxBackendConfig::under_root(root),
    )
}

fn read_manifest(
    backend: &ContainerSandboxBackend,
    id: &crate::SandboxId,
) -> ContainerSandboxManifest {
    backend
        .read_manifest(id)
        .expect("the process fixture manifest should read")
        .expect("the process fixture manifest should exist")
}

fn matrix_sandbox_id(case: CrashCase) -> crate::SandboxId {
    crate::SandboxId::new(format!("container-teardown-process-{}", case.label()))
}

fn run_contenders(root: &Path, tag: &str, operation: &str) -> Vec<ChildOutput> {
    let mut children = (0..2)
        .map(|role| spawn_child(root, "contend", tag, &role.to_string()))
        .collect::<Vec<_>>();
    let ready_deadline = Instant::now() + GATE_TIMEOUT;
    loop {
        let ready = (0..2)
            .filter(|role| {
                harness_root(root)
                    .join(format!("{tag}-{role}.ready"))
                    .is_file()
            })
            .count();
        if ready == 2 {
            break;
        }
        assert!(
            Instant::now() < ready_deadline,
            "only {ready}/2 {operation} children reached the durable gate"
        );
        std::thread::sleep(POLL_INTERVAL);
    }
    persist_marker(&harness_root(root).join(format!("{tag}.go")), b"go");
    children.iter_mut().map(wait_for_child).collect()
}

fn prime_fresh_process_recovery(root: &Path) {
    let output = run_child(root, "prime", "prime", "prime");
    assert_child_success(&output, "prime", "fresh-process recovery primer");
    assert!(
        output
            .stdout
            .contains("NNC65D1_PRIMED:fresh-process-recovery"),
        "the fresh-process recovery primer must complete:\n{}",
        output.stdout
    );
}

fn spawn_child(root: &Path, action: &str, case: &str, role: &str) -> Child {
    Command::new(std::env::current_exe().expect("the sandbox test executable should resolve"))
        .args(["--exact", CHILD_TEST, "--ignored", "--nocapture"])
        .env(ROOT_ENV, root)
        .env(ACTION_ENV, action)
        .env(CASE_ENV, case)
        .env(ROLE_ENV, role)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("failed to spawn NNC6.5d1 {action} child: {error}"))
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

fn assert_child_success(output: &ChildOutput, case: &str, role: &str) {
    assert!(
        output.status.success(),
        "NNC6.5d1 {role} failed for {case}\nstdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr
    );
}

fn assert_one_execute_and_one_adopt(outputs: &[ChildOutput]) {
    for output in outputs {
        assert_child_success(output, "contention", "contender");
    }
    assert_eq!(
        outputs
            .iter()
            .filter(|output| output.stdout.contains("NNC65D1_DECISION:execute"))
            .count(),
        1,
        "one process must receive execute authority: {:#?}",
        outputs
            .iter()
            .map(|output| &output.stdout)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        outputs
            .iter()
            .filter(|output| output.stdout.contains("NNC65D1_DECISION:adopt"))
            .count(),
        1,
        "one process must adopt exact authority: {:#?}",
        outputs
            .iter()
            .map(|output| &output.stdout)
            .collect::<Vec<_>>()
    );
}

fn assert_distinct_child_pids(outputs: &[ChildOutput]) {
    assert_eq!(outputs.len(), 2);
    assert_ne!(
        child_pid(&outputs[0].stdout),
        child_pid(&outputs[1].stdout),
        "contenders must execute in independent OS processes"
    );
}

fn child_pid(stdout: &str) -> u32 {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("NNC65D1_PID:"))
        .unwrap_or_else(|| panic!("child output lacks its PID marker:\n{stdout}"))
        .parse()
        .expect("the child PID marker should be numeric")
}

fn wait_for_gate(path: &Path) {
    let deadline = Instant::now() + GATE_TIMEOUT;
    while !path.is_file() {
        assert!(
            Instant::now() < deadline,
            "child timed out waiting for the durable gate {}",
            path.display()
        );
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn persist_marker(path: &Path, bytes: &[u8]) {
    let mut file = File::create(path).expect("the process marker should create");
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .expect("the process marker should become durable");
    sync_parent(path);
}

fn sync_parent(path: &Path) {
    File::open(
        path.parent()
            .expect("the process marker should have a parent directory"),
    )
    .and_then(|directory| directory.sync_all())
    .expect("the process marker parent should sync");
}

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("the NNC6.5d1 child requires {name}"))
}

fn harness_root(root: &Path) -> PathBuf {
    root.join(".nnc6-5d1-process")
}

fn signal_path(root: &Path) -> PathBuf {
    harness_root(root).join("signals.log")
}

fn signal_lines(root: &Path) -> Vec<String> {
    let path = signal_path(root);
    if !path.is_file() {
        return Vec::new();
    }
    std::fs::read_to_string(path)
        .expect("the process signal witness should read")
        .lines()
        .map(str::to_owned)
        .collect()
}

fn signal_numbers(lines: &[String]) -> Vec<i32> {
    lines
        .iter()
        .map(|line| {
            line.split_whitespace()
                .find_map(|field| field.strip_prefix("signal="))
                .unwrap_or_else(|| panic!("signal witness lacks its number: {line:?}"))
                .parse()
                .expect("the signal witness number should parse")
        })
        .collect()
}
