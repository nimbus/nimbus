use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nimbus_compute::workload_saga::{
    ConfirmedWorkloadTeardownCommand, FinalIngressWithdrawalCapability,
    IngressTeardownCapabilities, NetworkAttachmentTeardownCapabilities,
    NetworkDetachmentCapability, NetworkReleaseCapability, WorkloadExecutionDrainCapability,
    WorkloadExecutionStopCapability, WorkloadExecutionTeardownCapabilities,
    WorkloadProvisionSourceAuthority, WorkloadProvisionSourceAuthorityError,
    WorkloadProvisionSourceFuture, WorkloadSagaCoordinator, WorkloadTeardownCancellationToken,
    WorkloadTeardownCapabilityFuture, WorkloadTeardownCapabilityRegistry,
    WorkloadTeardownExecuteOutcome, WorkloadTeardownInspectOutcome,
    WorkloadTeardownProviderObservation, WorkloadTeardownProviderOutcome,
    WorkloadTeardownRunDisposition, WorkloadTeardownRuntime,
};
use nimbus_core::TenantId;
use nimbus_network::{
    NetworkAddressFamily, NetworkAttachmentCapabilitySet, NetworkAttachmentProviderRegistration,
    NetworkBindRealmKind, NetworkCapabilityBundle, NetworkCapabilityRegistry,
    NetworkControlPlaneLocality, NetworkEndpointCapabilitySet, NetworkExposure,
    NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet, NetworkManagementMode,
    NetworkPortAssignmentMode, NetworkProviderId, NetworkSovereigntyCapabilities, PortProtocol,
};
use nimbus_process_harness::{
    ProcessRoleSpec, SubprocessCrashCutHarness, run_crash_cut_child, run_crash_recovery_child,
};
use nimbus_workloads::{
    DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity, WorkloadActivationIntent,
    WorkloadAdmissionEvidence, WorkloadExecutableEncoding, WorkloadExecutableIntent,
    WorkloadFailureEvidence, WorkloadGeneration, WorkloadOwnerEvidenceDigest,
    WorkloadProvisionSourceEvidence, WorkloadProvisionSourceGeneration,
    WorkloadProvisionSourceIdentity, WorkloadProvisionSourceResourceVersion,
    WorkloadPublicationIntent, WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaFuture,
    WorkloadSagaIntent, WorkloadSagaIntentUpdate, WorkloadSagaKey, WorkloadSagaPage,
    WorkloadSagaPageRequest, WorkloadSagaPhase, WorkloadSagaRecord, WorkloadSagaStore,
    WorkloadSagaTenantPage, WorkloadSagaTenantPageRequest, WorkloadTeardownCommandMode,
    WorkloadTeardownDisposition, WorkloadTeardownStep, WorkloadTeardownSubjects,
    WorkloadTeardownSuccessEvidence,
};

use super::super::EngineWorkloadSagaStore;
use super::{compiled_network_plan, recovery::provision_history};

const CHILD_TEST: &str =
    "workload_saga_store::tests::teardown_driver_process::workload_teardown_driver_process_child";
const CHILD_MODE_ENV: &str = "NIMBUS_NNC65B_PROCESS_MODE";
const CHILD_KIND_ENV: &str = "NIMBUS_NNC65B_PROCESS_KIND";
const CHILD_STEP_ENV: &str = "NIMBUS_NNC65B_PROCESS_STEP";
const CHILD_MODE_CRASH: &str = "crash";
const CHILD_MODE_RECOVER: &str = "recover";
const PROCESS_PID_PREFIX: &str = "NIMBUS_NNC65B_PROCESS_PID";
const PROCESS_TIMEOUT: Duration = Duration::from_secs(20);
const CUT_MARKER_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CrashKind {
    Claim,
    Effect,
}

impl CrashKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Claim => "claim",
            Self::Effect => "effect",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "claim" => Ok(Self::Claim),
            "effect" => Ok(Self::Effect),
            _ => Err(format!("unknown NNC6.5b process kind {value:?}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TeardownCase {
    Withdraw,
    Drain,
    Stop,
    Detach,
    Release,
}

impl TeardownCase {
    const ALL: [Self; 5] = [
        Self::Withdraw,
        Self::Drain,
        Self::Stop,
        Self::Detach,
        Self::Release,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::Withdraw => "withdraw",
            Self::Drain => "drain",
            Self::Stop => "stop",
            Self::Detach => "detach",
            Self::Release => "release",
        }
    }

    const fn step(self) -> WorkloadTeardownStep {
        match self {
            Self::Withdraw => WorkloadTeardownStep::WithdrawPublication,
            Self::Drain => WorkloadTeardownStep::DrainExecution,
            Self::Stop => WorkloadTeardownStep::StopExecution,
            Self::Detach => WorkloadTeardownStep::DetachNetwork,
            Self::Release => WorkloadTeardownStep::ReleaseNetwork,
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|candidate| candidate.name() == value)
            .ok_or_else(|| format!("unknown NNC6.5b process step {value:?}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CrashSpec {
    kind: CrashKind,
    case: TeardownCase,
}

impl CrashSpec {
    fn boundary(self) -> String {
        format!("after-{}-{}", self.kind.name(), self.case.name())
    }

    fn observation(self) -> String {
        format!(
            "teardown-recovered:{}:{}:five-effects",
            self.kind.name(),
            self.case.name()
        )
    }
}

#[test]
fn teardown_driver_process_crash_after_each_claim_inspects_before_retry() {
    run_process_matrix(CrashKind::Claim);
}

#[test]
fn teardown_driver_process_crash_after_each_effect_never_reexecutes() {
    run_process_matrix(CrashKind::Effect);
}

fn run_process_matrix(kind: CrashKind) {
    for case in TeardownCase::ALL {
        let spec = CrashSpec { kind, case };
        let root = tempfile::tempdir().expect("teardown process proof root should build");
        let result = SubprocessCrashCutHarness::new(PROCESS_TIMEOUT)
            .run(
                root.path(),
                &spec.boundary(),
                &spec.observation(),
                child_role("writer", CHILD_MODE_CRASH, spec),
                child_role("recovery", CHILD_MODE_RECOVER, spec),
            )
            .unwrap_or_else(|error| panic!("{} process recovery failed: {error}", spec.boundary()));

        assert_eq!(result.boundary(), spec.boundary());
        assert_eq!(result.observation(), spec.observation());
        assert_eq!(
            result.crash_diagnostic().cleanup(),
            "killed-at-boundary-and-reaped"
        );
        assert_eq!(result.crash_diagnostic().successful(), Some(false));
        assert_eq!(result.recovery_diagnostic().successful(), Some(true));
        assert_ne!(
            process_pid(result.crash_diagnostic().stderr(), "writer"),
            process_pid(result.recovery_diagnostic().stderr(), "recovery"),
            "{} must reopen in a distinct process",
            spec.boundary()
        );
    }
}

#[test]
#[ignore = "spawned only by the NNC6.5b teardown process proof parents"]
fn workload_teardown_driver_process_child() {
    let mode = std::env::var(CHILD_MODE_ENV).expect("child process mode should be supplied");
    let spec = CrashSpec {
        kind: CrashKind::parse(
            &std::env::var(CHILD_KIND_ENV).expect("child process kind should be supplied"),
        )
        .expect("child process kind should be valid"),
        case: TeardownCase::parse(
            &std::env::var(CHILD_STEP_ENV).expect("child process step should be supplied"),
        )
        .expect("child process step should be valid"),
    };
    match mode.as_str() {
        CHILD_MODE_CRASH => run_crash_cut_child(|context| {
            eprintln!("{PROCESS_PID_PREFIX}:writer:{}", std::process::id());
            let marker = context.state_root().join("teardown-cut-reached");
            std::thread::scope(|scope| -> Result<(), String> {
                scope.spawn(|| watch_cut_marker(context, &marker, spec));
                let runtime = process_runtime()?;
                runtime.block_on(async {
                    let process =
                        process_driver(context.state_root(), Some(spec), &marker, true).await?;
                    let result = process
                        .runtime
                        .submit(
                            process.key.clone(),
                            &WorkloadTeardownCancellationToken::new(),
                        )
                        .await;
                    Err(format!(
                        "crash writer returned before {}: {result:?}",
                        spec.boundary()
                    ))
                })
            })
        })
        .unwrap_or_else(|error| panic!("NNC6.5b crash child failed: {error}")),
        CHILD_MODE_RECOVER => run_crash_recovery_child(|context| {
            eprintln!("{PROCESS_PID_PREFIX}:recovery:{}", std::process::id());
            let runtime = process_runtime()?;
            runtime.block_on(recover_process(context.state_root(), spec))
        })
        .unwrap_or_else(|error| panic!("NNC6.5b recovery child failed: {error}")),
        unknown => panic!("unknown NNC6.5b process mode {unknown:?}"),
    }
}

fn child_role(role: &str, mode: &str, spec: CrashSpec) -> ProcessRoleSpec {
    ProcessRoleSpec::new(
        role,
        std::env::current_exe().expect("current test executable should resolve"),
    )
    .arg("--exact")
    .arg(CHILD_TEST)
    .arg("--ignored")
    .arg("--nocapture")
    .env(CHILD_MODE_ENV, mode)
    .env(CHILD_KIND_ENV, spec.kind.name())
    .env(CHILD_STEP_ENV, spec.case.name())
}

fn process_pid(stderr: &str, role: &str) -> u32 {
    stderr
        .lines()
        .find_map(|line| {
            line.strip_prefix(&format!("{PROCESS_PID_PREFIX}:{role}:"))
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or_else(|| panic!("missing {role} process ID in stderr:\n{stderr}"))
}

fn watch_cut_marker(
    context: &nimbus_process_harness::CrashCutChildContext,
    marker: &Path,
    spec: CrashSpec,
) -> Result<(), String> {
    let deadline = Instant::now() + CUT_MARKER_TIMEOUT;
    while !marker.is_file() {
        if Instant::now() >= deadline {
            return Err(format!(
                "writer did not reach {} before {:?}",
                spec.boundary(),
                CUT_MARKER_TIMEOUT
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    context.reach_boundary(&spec.boundary())
}

fn process_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|error| format!("NNC6.5b process runtime failed: {error}"))
}

struct ProcessDriver {
    runtime: WorkloadTeardownRuntime,
    store: Arc<dyn WorkloadSagaStore>,
    provider: Arc<ProcessProofProvider>,
    key: WorkloadSagaKey,
}

async fn process_driver(
    root: &Path,
    crash: Option<CrashSpec>,
    cut_marker: &Path,
    seed: bool,
) -> Result<ProcessDriver, String> {
    let history = process_history();
    let withdrawal = history
        .last()
        .expect("teardown process history should not be empty")
        .clone();
    let engine = Arc::new(
        nimbus_engine::Engine::new(root.join("engine"))
            .map_err(|error| format!("teardown process Engine open failed: {error}"))?,
    );
    let engine_store = EngineWorkloadSagaStore::new(engine);
    if seed {
        persist_history(&engine_store, &history).await?;
    }
    let store: Arc<dyn WorkloadSagaStore> =
        if crash.is_some_and(|spec| spec.kind == CrashKind::Claim) {
            Arc::new(CrashAfterClaimStore {
                inner: engine_store,
                spec: crash.expect("claim crash spec should exist"),
                marker: cut_marker.to_owned(),
            })
        } else {
            Arc::new(engine_store)
        };
    let provider = Arc::new(ProcessProofProvider {
        journal: DurableCapabilityJournal::new(root.join("teardown-capability-journal")),
        crash: crash.filter(|spec| spec.kind == CrashKind::Effect),
        marker: cut_marker.to_owned(),
        calls: Mutex::new(Vec::new()),
    });
    let capabilities = WorkloadTeardownCapabilityRegistry::new(
        [NetworkAttachmentTeardownCapabilities::new(
            attachment_provider_id(),
            provider.clone(),
            provider.clone(),
        )],
        [WorkloadExecutionTeardownCapabilities::new(
            execution_provider_id(),
            provider.clone(),
            provider.clone(),
        )],
        [IngressTeardownCapabilities::new(
            ingress_provider_id(),
            provider.clone(),
        )],
    )
    .map_err(|error| format!("teardown process capability registry failed: {error}"))?;
    let source = Arc::new(ExactProcessSource {
        key: withdrawal.key().clone(),
        identity: withdrawal
            .active_intent()
            .source()
            .source_identity()
            .clone(),
        evidence: withdrawal.active_intent().source().clone(),
    });
    let coordinator = Arc::new(WorkloadSagaCoordinator::new(Arc::clone(&store)));
    Ok(ProcessDriver {
        runtime: WorkloadTeardownRuntime::new(
            coordinator,
            source,
            provider_reports(),
            Arc::new(capabilities),
        ),
        store,
        provider,
        key: withdrawal.key().clone(),
    })
}

async fn recover_process(root: &Path, spec: CrashSpec) -> Result<String, String> {
    let process = process_driver(root, None, &root.join("unused-cut-marker"), false).await?;
    let before = process
        .store
        .load(&process.key)
        .await
        .map_err(|error| format!("recovery preflight load failed: {error}"))?
        .ok_or_else(|| "recovery preflight omitted durable teardown state".to_owned())?;
    let claim = before
        .teardown_disposition()
        .and_then(WorkloadTeardownDisposition::claim)
        .ok_or_else(|| "recovery preflight omitted the exact durable claim".to_owned())?;
    if claim.attempt().step() != spec.case.step()
        || !matches!(
            before.teardown_disposition(),
            Some(WorkloadTeardownDisposition::DispatchPending { .. })
        )
    {
        return Err(format!(
            "recovery preflight crossed the cut claim: phase={:?} step={:?}",
            before.phase(),
            claim.attempt().step()
        ));
    }
    let attempt_id = claim.attempt().attempt_id().clone();
    let initial_epoch = claim.dispatch_epoch();

    let run = process
        .runtime
        .submit(
            process.key.clone(),
            &WorkloadTeardownCancellationToken::new(),
        )
        .await
        .map_err(|error| format!("fresh-process teardown resume failed: {error}"))?;
    if run.disposition() != WorkloadTeardownRunDisposition::Completed
        || run.record().phase() != WorkloadSagaPhase::Recorded
    {
        return Err(format!(
            "fresh process did not converge: disposition={:?} phase={:?}",
            run.disposition(),
            run.record().phase()
        ));
    }
    let calls = process.provider.calls();
    let first = calls
        .first()
        .ok_or_else(|| "recovery made no teardown capability call".to_owned())?;
    if first.step != spec.case.step()
        || first.mode != WorkloadTeardownCommandMode::Inspect
        || first.attempt_id != attempt_id
        || first.epoch != initial_epoch.as_u64()
    {
        return Err(format!(
            "recovery did not inspect the exact cut claim first: {first:?}"
        ));
    }
    let target_calls: Vec<_> = calls
        .iter()
        .filter(|call| call.step == spec.case.step())
        .collect();
    match spec.kind {
        CrashKind::Claim => {
            if target_calls.len() != 2
                || target_calls[0].mode != WorkloadTeardownCommandMode::Inspect
                || target_calls[1].mode != WorkloadTeardownCommandMode::Execute
                || target_calls[1].attempt_id != attempt_id
                || target_calls[1].epoch != initial_epoch.as_u64() + 1
            {
                return Err(format!(
                    "claim-cut recovery did not inspect then retry the same attempt once: {target_calls:?}"
                ));
            }
        }
        CrashKind::Effect => {
            if target_calls.len() != 1
                || target_calls[0].mode != WorkloadTeardownCommandMode::Inspect
            {
                return Err(format!(
                    "effect-cut recovery re-executed a recorded effect: {target_calls:?}"
                ));
            }
        }
    }
    let effects = process.provider.journal.count()?;
    if effects != 5 {
        return Err(format!(
            "expected five exact teardown effect witnesses, observed {effects}"
        ));
    }
    Ok(spec.observation())
}

struct CrashAfterClaimStore {
    inner: EngineWorkloadSagaStore,
    spec: CrashSpec,
    marker: PathBuf,
}

impl WorkloadSagaStore for CrashAfterClaimStore {
    fn load<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        self.inner.load(key)
    }

    fn compare_and_swap<'a>(
        &'a self,
        expected: WorkloadSagaExpected,
        next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            let cut = matches!(
                next.teardown_disposition(),
                Some(WorkloadTeardownDisposition::DispatchPending { claim, .. })
                    if claim.attempt().step() == self.spec.case.step()
            );
            let result = self.inner.compare_and_swap(expected, next).await?;
            if cut && result == WorkloadSagaCommit::Applied {
                durable_stall(&self.marker);
            }
            Ok(result)
        })
    }

    fn list_recoverable<'a>(
        &'a self,
        request: WorkloadSagaPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaPage> {
        self.inner.list_recoverable(request)
    }

    fn list_restart_candidates<'a>(
        &'a self,
        request: nimbus_workloads::WorkloadRestartCandidatePageRequest,
    ) -> nimbus_workloads::WorkloadSagaFuture<'a, nimbus_workloads::WorkloadRestartCandidatePage>
    {
        self.inner.list_restart_candidates(request)
    }

    fn list_for_tenant<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage> {
        self.inner.list_for_tenant(tenant_id, request)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProviderCall {
    step: WorkloadTeardownStep,
    mode: WorkloadTeardownCommandMode,
    attempt_id: nimbus_workloads::WorkloadTeardownAttemptId,
    epoch: u64,
}

struct ProcessProofProvider {
    journal: DurableCapabilityJournal,
    crash: Option<CrashSpec>,
    marker: PathBuf,
    calls: Mutex<Vec<ProviderCall>>,
}

impl ProcessProofProvider {
    fn calls(&self) -> Vec<ProviderCall> {
        self.calls
            .lock()
            .expect("teardown process call log should remain healthy")
            .clone()
    }

    fn observe(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownProviderObservation {
        self.calls
            .lock()
            .expect("teardown process call log should remain healthy")
            .push(ProviderCall {
                step: command.step(),
                mode: command.mode(),
                attempt_id: command.attempt_id().clone(),
                epoch: command.dispatch_epoch().as_u64(),
            });
        let outcome = match command.mode() {
            WorkloadTeardownCommandMode::Execute => match self.journal.record(command) {
                Ok(()) => {
                    if self
                        .crash
                        .is_some_and(|spec| spec.case.step() == command.step())
                    {
                        durable_stall(&self.marker);
                    }
                    WorkloadTeardownProviderOutcome::Execute(
                        WorkloadTeardownExecuteOutcome::Succeeded(Box::new(success_evidence(
                            command,
                        ))),
                    )
                }
                Err(JournalRecordError::Duplicate) => WorkloadTeardownProviderOutcome::Execute(
                    WorkloadTeardownExecuteOutcome::DefiniteFailure(
                        WorkloadFailureEvidence::new(
                            "duplicate_teardown_effect",
                            WorkloadOwnerEvidenceDigest::sha256(format!(
                                "{}:{}",
                                step_name(command.step()),
                                command.attempt_id().as_str()
                            )),
                        )
                        .expect("duplicate teardown evidence should validate"),
                    ),
                ),
                Err(JournalRecordError::Io) => WorkloadTeardownProviderOutcome::Execute(
                    WorkloadTeardownExecuteOutcome::Ambiguous,
                ),
            },
            WorkloadTeardownCommandMode::Inspect => {
                if self.journal.contains(command) {
                    WorkloadTeardownProviderOutcome::Inspect(
                        WorkloadTeardownInspectOutcome::Satisfied(Box::new(success_evidence(
                            command,
                        ))),
                    )
                } else {
                    WorkloadTeardownProviderOutcome::Inspect(
                        WorkloadTeardownInspectOutcome::NotCompleted(
                            WorkloadOwnerEvidenceDigest::sha256(format!(
                                "{}:{}:absent",
                                step_name(command.step()),
                                command.attempt_id().as_str()
                            )),
                        ),
                    )
                }
            }
        };
        WorkloadTeardownProviderObservation::for_command(command, outcome)
    }
}

macro_rules! teardown_capability {
    ($trait_name:ident) => {
        impl $trait_name for ProcessProofProvider {
            fn execute<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadTeardownCommand,
            ) -> WorkloadTeardownCapabilityFuture<'a> {
                Box::pin(async move { self.observe(command) })
            }

            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadTeardownCommand,
            ) -> WorkloadTeardownCapabilityFuture<'a> {
                Box::pin(async move { self.observe(command) })
            }
        }
    };
}

teardown_capability!(FinalIngressWithdrawalCapability);
teardown_capability!(WorkloadExecutionDrainCapability);
teardown_capability!(WorkloadExecutionStopCapability);
teardown_capability!(NetworkDetachmentCapability);
teardown_capability!(NetworkReleaseCapability);

#[derive(Debug, Clone)]
struct DurableCapabilityJournal {
    root: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JournalRecordError {
    Duplicate,
    Io,
}

impl DurableCapabilityJournal {
    fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn record(&self, command: &ConfirmedWorkloadTeardownCommand) -> Result<(), JournalRecordError> {
        fs::create_dir_all(&self.root).map_err(|_| JournalRecordError::Io)?;
        let path = self.path(command);
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(JournalRecordError::Duplicate);
            }
            Err(_) => return Err(JournalRecordError::Io),
        };
        let evidence = format!(
            "{}:{}:{}",
            step_name(command.step()),
            command.attempt_id().as_str(),
            command.dispatch_epoch().as_u64()
        );
        file.write_all(evidence.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|_| JournalRecordError::Io)?;
        Ok(())
    }

    fn contains(&self, command: &ConfirmedWorkloadTeardownCommand) -> bool {
        self.path(command).is_file()
    }

    fn count(&self) -> Result<usize, String> {
        let entries = fs::read_dir(&self.root)
            .map_err(|error| format!("teardown capability journal could not be read: {error}"))?;
        Ok(entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .count())
    }

    fn path(&self, command: &ConfirmedWorkloadTeardownCommand) -> PathBuf {
        self.root.join(format!(
            "{}-{}",
            step_name(command.step()),
            command.attempt_id().as_str()
        ))
    }
}

struct ExactProcessSource {
    key: WorkloadSagaKey,
    identity: WorkloadProvisionSourceIdentity,
    evidence: WorkloadProvisionSourceEvidence,
}

impl WorkloadProvisionSourceAuthority for ExactProcessSource {
    fn current_source<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
        identity: &'a WorkloadProvisionSourceIdentity,
    ) -> WorkloadProvisionSourceFuture<'a> {
        Box::pin(async move {
            if key != &self.key || identity != &self.identity {
                return Err(WorkloadProvisionSourceAuthorityError::Corrupt);
            }
            Ok(self.evidence.clone())
        })
    }
}

async fn persist_history(
    store: &EngineWorkloadSagaStore,
    history: &[WorkloadSagaRecord],
) -> Result<(), String> {
    for (index, record) in history.iter().enumerate() {
        let expected = index
            .checked_sub(1)
            .map_or(WorkloadSagaExpected::Missing, |previous| {
                WorkloadSagaExpected::Revision(history[previous].revision())
            });
        let commit = store
            .compare_and_swap(expected, record.clone())
            .await
            .map_err(|error| format!("teardown seed transition failed: {error}"))?;
        if commit != WorkloadSagaCommit::Applied {
            return Err(format!(
                "teardown seed transition into {:?} was not applied",
                record.phase()
            ));
        }
    }
    Ok(())
}

fn process_history() -> Vec<WorkloadSagaRecord> {
    let mut history = provision_history(
        "nnc65b-process",
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let observed = history
        .last()
        .expect("teardown process fixture should reach Observed");
    let WorkloadSagaIntentUpdate::Transition(withdrawal) = observed
        .apply_intent(stopped_intent(observed))
        .expect("stopped successor should start teardown")
    else {
        panic!("stopped successor should change the durable record");
    };
    history.push(*withdrawal);
    history
}

fn stopped_intent(observed: &WorkloadSagaRecord) -> WorkloadSagaIntent {
    let executable = WorkloadExecutableIntent::new(
        WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
        r#"{"fixture":"nnc65b-process-stopped"}"#,
    )
    .expect("teardown process executable should validate");
    let source = WorkloadProvisionSourceEvidence::standalone_sandbox(
        WorkloadProvisionSourceIdentity::standalone_sandbox(
            observed.key().workload_id().as_str(),
            "nnc65b-process-stopped",
        )
        .expect("teardown process source identity should validate"),
        WorkloadProvisionSourceGeneration::new(2),
        WorkloadProvisionSourceResourceVersion::new("nnc65b-process-v2")
            .expect("teardown process source version should validate"),
        executable.content_digest(),
        attachment_provider_id(),
        execution_provider_id(),
    )
    .expect("teardown process source should validate");
    WorkloadSagaIntent::new_without_automatic_restart(
        DesiredWorkloadKind::Sandbox,
        DesiredWorkloadState::Stopped,
        WorkloadGeneration::new(2),
        executable,
        source,
        nimbus_workloads::WorkloadNetworkIntent::new(compiled_network_plan(
            observed.key().tenant_id(),
            "nnc65b-process-stopped",
            2,
            WorkloadActivationIntent::PrepareOnly,
            WorkloadPublicationIntent::Withheld,
        )),
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
        WorkloadAdmissionEvidence::new(
            format!("tid_{}", "1".repeat(64))
                .try_into()
                .expect("teardown process decision ID should validate"),
            format!("twu_{}", "2".repeat(64))
                .try_into()
                .expect("teardown process workload UID should validate"),
            NodeIdentity::new("node-recovery").expect("teardown process node should validate"),
        ),
    )
    .expect("teardown process stopped intent should validate")
}

fn success_evidence(command: &ConfirmedWorkloadTeardownCommand) -> WorkloadTeardownSuccessEvidence {
    let evidence = WorkloadOwnerEvidenceDigest::sha256(format!(
        "{}:{}",
        step_name(command.step()),
        command.attempt_id().as_str()
    ));
    match (command.step(), command.subjects()) {
        (
            WorkloadTeardownStep::WithdrawPublication,
            WorkloadTeardownSubjects::Publication(reference),
        ) => WorkloadTeardownSuccessEvidence::PublicationAbsent {
            reference: reference.clone(),
            evidence,
        },
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
        _ => panic!("teardown process step and subjects should match"),
    }
}

fn provider_reports() -> NetworkCapabilityRegistry {
    let attachment =
        NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []);
    let endpoint = NetworkEndpointCapabilitySet::new(
        [NetworkAddressFamily::Ipv4],
        [NetworkBindRealmKind::Host],
        [NetworkExposure::Loopback],
        [PortProtocol::Tcp],
        [NetworkPortAssignmentMode::ProviderAssigned],
    );
    let lifecycle = NetworkLifecycleCapabilitySet::new([]);
    let bundle = NetworkCapabilityBundle::new(
        NetworkAttachmentProviderRegistration::new(
            attachment_provider_id(),
            attachment,
            [NetworkAddressFamily::Ipv4],
            lifecycle.clone(),
            NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
        ),
        NetworkIngressProviderRegistration::new(
            ingress_provider_id(),
            endpoint,
            NetworkIngressCapabilitySet::new([]),
            NetworkForwardingCapabilitySet::new([]),
            lifecycle,
            NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
        ),
    );
    NetworkCapabilityRegistry::new([bundle])
        .expect("teardown process provider report should validate")
}

fn attachment_provider_id() -> NetworkProviderId {
    NetworkProviderId::for_registration_key("fixture-attachment")
}

fn ingress_provider_id() -> NetworkProviderId {
    NetworkProviderId::for_registration_key("fixture-ingress")
}

fn execution_provider_id() -> nimbus_workloads::WorkloadExecutionProviderId {
    nimbus_workloads::WorkloadExecutionProviderId::for_registration_key("fixture-execution")
}

const fn step_name(step: WorkloadTeardownStep) -> &'static str {
    match step {
        WorkloadTeardownStep::WithdrawPublication => "withdraw",
        WorkloadTeardownStep::DrainExecution => "drain",
        WorkloadTeardownStep::StopExecution => "stop",
        WorkloadTeardownStep::DetachNetwork => "detach",
        WorkloadTeardownStep::ReleaseNetwork => "release",
    }
}

fn durable_stall(marker: &Path) -> ! {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(marker)
        .expect("teardown cut marker should be created exactly once");
    file.write_all(b"reached")
        .and_then(|()| file.sync_all())
        .expect("teardown cut marker should become durable");
    loop {
        std::thread::park();
    }
}
