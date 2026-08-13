use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use nimbus_compute::workload_saga::provision_provider::{
    ProviderProvisionEffectObservation, ProviderProvisionPhaseAdapter,
};
use nimbus_compute::workload_saga::{
    ConfirmedWorkloadProvisionCommand, ConfirmedWorkloadTeardownCommand,
    FinalIngressWithdrawalCapability, IngressProvisionCapabilities, IngressPublicationCapability,
    IngressPublicationInspectionCapability, IngressTeardownCapabilities,
    NetworkAttachmentCapability, NetworkAttachmentProvisionCapabilities,
    NetworkAttachmentTeardownCapabilities, NetworkDetachmentCapability, NetworkReleaseCapability,
    NetworkReservationCapability, WorkloadActivationCapability,
    WorkloadActivationPrerequisiteCapability, WorkloadExecutionDrainCapability,
    WorkloadExecutionProvisionCapabilities, WorkloadExecutionStopCapability,
    WorkloadExecutionTeardownCapabilities, WorkloadPreparationCapability,
    WorkloadProvisionCapabilityFuture, WorkloadProvisionCapabilityRegistry,
    WorkloadProvisionDriver, WorkloadProvisionRunDisposition, WorkloadProvisionSourceAuthority,
    WorkloadProvisionSourceAuthorityError, WorkloadProvisionSourceFuture,
    WorkloadReadinessCapability, WorkloadSagaCoordinator, WorkloadTeardownCancellationToken,
    WorkloadTeardownCapabilityFuture, WorkloadTeardownCapabilityRegistry,
    WorkloadTeardownExecuteOutcome, WorkloadTeardownInspectOutcome,
    WorkloadTeardownProviderObservation, WorkloadTeardownProviderOutcome,
    WorkloadTeardownRunDisposition, WorkloadTeardownRuntime,
    compensate_definite_provision_failure_once_for_test,
};
use nimbus_compute::{
    WorkloadExecutionObservationCapability, WorkloadExecutionObservationFuture,
    WorkloadExecutionObservationRequest, WorkloadIngressObservationCapability,
    WorkloadIngressObservationFuture, WorkloadIngressObservationRequest,
    WorkloadProviderObservation,
};
use nimbus_core::{TenantId, WorkloadId};
use nimbus_engine::Engine;
use nimbus_network::{
    NetworkAddressFamily, NetworkAttachmentCapabilitySet, NetworkAttachmentProviderRegistration,
    NetworkBindRealmKind, NetworkCapabilityBundle, NetworkCapabilityRegistry,
    NetworkControlPlaneLocality, NetworkEndpointCapabilitySet, NetworkExposure,
    NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet, NetworkManagementMode,
    NetworkPortAssignmentMode, NetworkProviderId, NetworkSovereigntyCapabilities, PortProtocol,
};
use nimbus_sandbox::ProviderCommandAttemptJournal;
use nimbus_testing::{
    ProcessRoleSpec, SubprocessCrashCutHarness, run_crash_cut_child, run_crash_recovery_child,
};
use nimbus_workloads::{
    DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity, WorkloadActivationIntent,
    WorkloadAdmissionEvidence, WorkloadExecutableEncoding, WorkloadExecutableIntent,
    WorkloadGeneration, WorkloadOwnerEvidenceDigest, WorkloadProvisionCommandMode,
    WorkloadProvisionDisposition, WorkloadProvisionInspectionResult,
    WorkloadProvisionSourceEvidence, WorkloadProvisionSourceGeneration,
    WorkloadProvisionSourceIdentity, WorkloadProvisionSourceResourceVersion, WorkloadProvisionStep,
    WorkloadPublicationIntent, WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaFuture,
    WorkloadSagaIntent, WorkloadSagaKey, WorkloadSagaPage, WorkloadSagaPageRequest,
    WorkloadSagaPhase, WorkloadSagaRecord, WorkloadSagaStore, WorkloadSagaTenantPage,
    WorkloadSagaTenantPageRequest, WorkloadTeardownCause, WorkloadTeardownCommandMode,
    WorkloadTeardownStep, WorkloadTeardownSubjects, WorkloadTeardownSuccessEvidence,
};

use super::super::EngineWorkloadSagaStore;
use super::compiled_network_plan;

const CHILD_TEST: &str =
    "workload_saga_store::tests::provision_driver_process::workload_provision_driver_process_child";
const CHILD_MODE_ENV: &str = "NIMBUS_NNC64_PROCESS_MODE";
const CHILD_CUT_ENV: &str = "NIMBUS_NNC64_PROCESS_CUT";
const CHILD_MODE_CRASH: &str = "crash";
const CHILD_MODE_RECOVER: &str = "recover";
const RECOVERY_OBSERVATION: &str = "observed-five-exact-effects";
const PROCESS_PID_PREFIX: &str = "NIMBUS_NNC64_PROCESS_PID";
const PROCESS_TIMEOUT: Duration = Duration::from_secs(20);
const CUT_MARKER_TIMEOUT: Duration = Duration::from_secs(10);
const PROVIDER_JOURNAL_NAMESPACE: &str = "nnc64-process-provider";
const COMPENSATION_CHILD_TEST: &str = "workload_saga_store::tests::provision_driver_process::workload_provision_compensation_process_child";
const COMPENSATION_MODE_ENV: &str = "NIMBUS_NNC65G_COMPENSATION_MODE";
const COMPENSATION_CUT_ENV: &str = "NIMBUS_NNC65G_COMPENSATION_CUT";
const COMPENSATION_OBSERVATION_PREFIX: &str = "failed-provision-compensation-recorded";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CrashCut {
    ClaimCas,
    OwnerClaim,
    Effect,
    ResultCas,
}

impl CrashCut {
    const ALL: [Self; 4] = [
        Self::ClaimCas,
        Self::OwnerClaim,
        Self::Effect,
        Self::ResultCas,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::ClaimCas => "after-claim-cas",
            Self::OwnerClaim => "after-owner-claim",
            Self::Effect => "after-effect",
            Self::ResultCas => "after-result-cas",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|candidate| candidate.name() == value)
            .ok_or_else(|| format!("unknown NNC6.4 process cut {value:?}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompensationCut {
    ResultBeforeCause,
    CauseResponseLoss,
}

impl CompensationCut {
    const ALL: [Self; 2] = [Self::ResultBeforeCause, Self::CauseResponseLoss];

    const fn name(self) -> &'static str {
        match self {
            Self::ResultBeforeCause => "result-before-cause",
            Self::CauseResponseLoss => "cause-response-loss",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|candidate| candidate.name() == value)
            .ok_or_else(|| format!("unknown NNC6.5g compensation cut {value:?}"))
    }

    fn observation(self) -> String {
        format!("{COMPENSATION_OBSERVATION_PREFIX}:{}", self.name())
    }
}

#[test]
fn fresh_process_reopens_engine_without_snapshot_handoff() {
    for cut in CrashCut::ALL {
        let root = tempfile::tempdir().expect("process proof root should build");
        let result = SubprocessCrashCutHarness::new(PROCESS_TIMEOUT)
            .run(
                root.path(),
                cut.name(),
                RECOVERY_OBSERVATION,
                child_role(&format!("{}-writer", cut.name()), CHILD_MODE_CRASH, cut),
                child_role(&format!("{}-recovery", cut.name()), CHILD_MODE_RECOVER, cut),
            )
            .unwrap_or_else(|error| panic!("{} process recovery failed: {error}", cut.name()));

        assert_eq!(result.boundary(), cut.name());
        assert_eq!(result.observation(), RECOVERY_OBSERVATION);
        assert_eq!(
            result.crash_diagnostic().cleanup(),
            "killed-at-boundary-and-reaped"
        );
        assert_eq!(result.crash_diagnostic().successful(), Some(false));
        assert_eq!(result.recovery_diagnostic().successful(), Some(true));
        assert_ne!(
            process_pid(result.crash_diagnostic().stderr(), "writer"),
            process_pid(result.recovery_diagnostic().stderr(), "recovery"),
            "{} must reopen in a genuinely distinct process",
            cut.name()
        );
    }
}

#[test]
fn failed_provision_compensation_reopens_result_and_cause_cuts() {
    for cut in CompensationCut::ALL {
        let root = tempfile::tempdir().expect("compensation process root should build");
        let result = SubprocessCrashCutHarness::new(PROCESS_TIMEOUT)
            .run(
                root.path(),
                cut.name(),
                &cut.observation(),
                compensation_child_role(&format!("{}-writer", cut.name()), CHILD_MODE_CRASH, cut),
                compensation_child_role(
                    &format!("{}-recovery", cut.name()),
                    CHILD_MODE_RECOVER,
                    cut,
                ),
            )
            .unwrap_or_else(|error| {
                panic!(
                    "{} compensation process recovery failed: {error}",
                    cut.name()
                )
            });

        assert_eq!(result.boundary(), cut.name());
        assert_eq!(result.observation(), cut.observation());
        assert_eq!(
            result.crash_diagnostic().cleanup(),
            "killed-at-boundary-and-reaped"
        );
        assert_eq!(result.recovery_diagnostic().successful(), Some(true));
        assert_ne!(
            process_pid(result.crash_diagnostic().stderr(), "writer"),
            process_pid(result.recovery_diagnostic().stderr(), "recovery"),
            "{} must reopen in a genuinely distinct process",
            cut.name()
        );
    }
}

#[test]
#[ignore = "spawned only by the NNC6.4 fresh-process proof parent"]
fn workload_provision_driver_process_child() {
    let mode = std::env::var(CHILD_MODE_ENV).expect("child process mode should be supplied");
    let cut = CrashCut::parse(
        &std::env::var(CHILD_CUT_ENV).expect("child process cut should be supplied"),
    )
    .expect("child process cut should be valid");
    match mode.as_str() {
        CHILD_MODE_CRASH => run_crash_cut_child(|context| {
            eprintln!("{PROCESS_PID_PREFIX}:writer:{}", std::process::id());
            let marker = context.state_root().join("cut-reached");
            std::thread::scope(|scope| -> Result<(), String> {
                scope.spawn(|| watch_cut_marker(context, &marker, cut));
                let runtime = process_runtime()?;
                let (driver, record) = process_driver(context.state_root(), Some(cut), &marker)?;
                let result = runtime.block_on(
                    driver.submit_and_drive(record.key().clone(), record.active_intent().clone()),
                );
                Err(format!(
                    "crash writer returned before {}: {result:?}",
                    cut.name()
                ))
            })
        })
        .unwrap_or_else(|error| panic!("NNC6.4 crash child failed: {error}")),
        CHILD_MODE_RECOVER => run_crash_recovery_child(|context| {
            eprintln!("{PROCESS_PID_PREFIX}:recovery:{}", std::process::id());
            let runtime = process_runtime()?;
            runtime.block_on(recover_process(context.state_root()))
        })
        .unwrap_or_else(|error| panic!("NNC6.4 recovery child failed: {error}")),
        unknown => panic!("unknown NNC6.4 process mode {unknown:?}"),
    }
}

#[test]
#[ignore = "spawned only by the NNC6.5g compensation fresh-process proof parent"]
fn workload_provision_compensation_process_child() {
    let mode = std::env::var(COMPENSATION_MODE_ENV)
        .expect("compensation child process mode should be supplied");
    let cut = CompensationCut::parse(
        &std::env::var(COMPENSATION_CUT_ENV)
            .expect("compensation child process cut should be supplied"),
    )
    .expect("compensation child process cut should be valid");
    match mode.as_str() {
        CHILD_MODE_CRASH => run_crash_cut_child(|context| {
            eprintln!("{PROCESS_PID_PREFIX}:writer:{}", std::process::id());
            let marker = context.state_root().join("compensation-cut-reached");
            std::thread::scope(|scope| -> Result<(), String> {
                scope.spawn(|| watch_compensation_cut(context, &marker, cut));
                process_runtime()?.block_on(write_compensation_cut(
                    context.state_root(),
                    &marker,
                    cut,
                ))
            })
        })
        .unwrap_or_else(|error| panic!("NNC6.5g compensation crash child failed: {error}")),
        CHILD_MODE_RECOVER => run_crash_recovery_child(|context| {
            eprintln!("{PROCESS_PID_PREFIX}:recovery:{}", std::process::id());
            process_runtime()?.block_on(recover_compensation_cut(context.state_root(), cut))
        })
        .unwrap_or_else(|error| panic!("NNC6.5g compensation recovery child failed: {error}")),
        unknown => panic!("unknown NNC6.5g compensation process mode {unknown:?}"),
    }
}

fn child_role(role: &str, mode: &str, cut: CrashCut) -> ProcessRoleSpec {
    ProcessRoleSpec::new(
        role,
        std::env::current_exe().expect("current test executable should resolve"),
    )
    .arg("--exact")
    .arg(CHILD_TEST)
    .arg("--ignored")
    .arg("--nocapture")
    .env(CHILD_MODE_ENV, mode)
    .env(CHILD_CUT_ENV, cut.name())
}

fn compensation_child_role(role: &str, mode: &str, cut: CompensationCut) -> ProcessRoleSpec {
    ProcessRoleSpec::new(
        role,
        std::env::current_exe().expect("current test executable should resolve"),
    )
    .arg("--exact")
    .arg(COMPENSATION_CHILD_TEST)
    .arg("--ignored")
    .arg("--nocapture")
    .env(COMPENSATION_MODE_ENV, mode)
    .env(COMPENSATION_CUT_ENV, cut.name())
}

fn process_pid(stderr: &str, role: &str) -> u32 {
    stderr
        .lines()
        .find_map(|line| {
            line.strip_prefix(&format!("{PROCESS_PID_PREFIX}:{role}:"))
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or_else(|| panic!("missing {role} process id in stderr:\n{stderr}"))
}

fn watch_cut_marker(
    context: &nimbus_testing::CrashCutChildContext,
    marker: &Path,
    cut: CrashCut,
) -> Result<(), String> {
    let deadline = Instant::now() + CUT_MARKER_TIMEOUT;
    while !marker.is_file() {
        if Instant::now() >= deadline {
            return Err(format!(
                "writer did not reach {} before {:?}",
                cut.name(),
                CUT_MARKER_TIMEOUT
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    context.reach_boundary(cut.name())
}

fn watch_compensation_cut(
    context: &nimbus_testing::CrashCutChildContext,
    marker: &Path,
    cut: CompensationCut,
) -> Result<(), String> {
    let deadline = Instant::now() + CUT_MARKER_TIMEOUT;
    while !marker.is_file() {
        if Instant::now() >= deadline {
            return Err(format!(
                "writer did not reach {} before {:?}",
                cut.name(),
                CUT_MARKER_TIMEOUT
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    context.reach_boundary(cut.name())
}

fn process_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|error| format!("NNC6.4 process runtime failed: {error}"))
}

async fn recover_process(root: &Path) -> Result<String, String> {
    let (driver, record) = process_driver(root, None, &root.join("unused-cut-marker"))?;
    let run = driver
        .resume(record.key())
        .await
        .map_err(|error| format!("fresh process resume failed: {error}"))?;
    if run.disposition() != WorkloadProvisionRunDisposition::Observed
        || run.record().phase() != WorkloadSagaPhase::Observed
    {
        return Err(format!(
            "fresh process did not converge to Observed: disposition={:?} phase={:?}",
            run.disposition(),
            run.record().phase()
        ));
    }
    let effects = fs::read_dir(root.join("effects"))
        .map_err(|error| format!("effect directory could not be read: {error}"))?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .count();
    if effects != 5 {
        return Err(format!(
            "expected five exact effect witnesses, observed {effects}"
        ));
    }
    Ok(RECOVERY_OBSERVATION.to_owned())
}

async fn write_compensation_cut(
    root: &Path,
    marker: &Path,
    cut: CompensationCut,
) -> Result<(), String> {
    let (driver, record) = process_driver_with_failure(
        root,
        None,
        &root.join("unused-provision-cut"),
        Some(WorkloadProvisionStep::PrepareWorkload),
    )?;
    let run = driver
        .submit_and_drive(record.key().clone(), record.active_intent().clone())
        .await
        .map_err(|error| format!("failed provision writer failed: {error}"))?;
    if run.disposition() != WorkloadProvisionRunDisposition::DefiniteFailure
        || !matches!(
            run.record().provision_disposition(),
            Some(WorkloadProvisionDisposition::DefiniteFailure { claim, .. })
                if claim.attempt().step() == WorkloadProvisionStep::PrepareWorkload
        )
    {
        return Err(format!(
            "failed provision writer did not persist exact result: disposition={:?} phase={:?}",
            run.disposition(),
            run.record().phase()
        ));
    }
    let failed = run.record().clone();
    drop(driver);

    match cut {
        CompensationCut::ResultBeforeCause => durable_stall(marker),
        CompensationCut::CauseResponseLoss => {
            let engine = Arc::new(
                Engine::new(root.join("engine"))
                    .map_err(|error| format!("cause writer Engine reopen failed: {error}"))?,
            );
            let store: Arc<dyn WorkloadSagaStore> = Arc::new(CrashAfterCauseStore {
                inner: EngineWorkloadSagaStore::new(engine),
                marker: marker.to_owned(),
            });
            let coordinator = Arc::new(WorkloadSagaCoordinator::new(store));
            let teardown = compensation_teardown_runtime(root, Arc::clone(&coordinator), &failed)?;
            let result =
                compensate_definite_provision_failure_once_for_test(coordinator, teardown, &failed)
                    .await;
            Err(format!(
                "cause-response writer returned before its cut: {result:?}"
            ))
        }
    }
}

async fn recover_compensation_cut(root: &Path, cut: CompensationCut) -> Result<String, String> {
    let engine = Arc::new(
        Engine::new(root.join("engine"))
            .map_err(|error| format!("compensation recovery Engine failed: {error}"))?,
    );
    let store: Arc<dyn WorkloadSagaStore> = Arc::new(EngineWorkloadSagaStore::new(engine));
    let coordinator = Arc::new(WorkloadSagaCoordinator::new(store));
    let key = process_record().key().clone();
    let current = coordinator
        .load(&key)
        .await
        .map_err(|error| format!("compensation recovery load failed: {error}"))?
        .ok_or_else(|| "compensation recovery record is missing".to_owned())?;
    let teardown = compensation_teardown_runtime(root, Arc::clone(&coordinator), &current)?;
    let run = match cut {
        CompensationCut::ResultBeforeCause => {
            if !matches!(
                current.provision_disposition(),
                Some(WorkloadProvisionDisposition::DefiniteFailure { claim, .. })
                    if claim.attempt().step() == WorkloadProvisionStep::PrepareWorkload
            ) || current.teardown_disposition().is_some()
            {
                return Err("result-before-cause recovery crossed durable failure".to_owned());
            }
            compensate_definite_provision_failure_once_for_test(
                Arc::clone(&coordinator),
                Arc::clone(&teardown),
                &current,
            )
            .await
            .map_err(|error| format!("result-before-cause compensation failed: {error}"))?
        }
        CompensationCut::CauseResponseLoss => {
            if current.phase() != WorkloadSagaPhase::WithdrawalCommitted
                || !matches!(
                    current
                        .teardown_disposition()
                        .map(|disposition| disposition.cause()),
                    Some(WorkloadTeardownCause::FailedProvision { claim, .. })
                        if claim.attempt().step() == WorkloadProvisionStep::PrepareWorkload
                )
            {
                return Err("cause-response recovery crossed durable cause".to_owned());
            }
            teardown
                .submit(key.clone(), &WorkloadTeardownCancellationToken::new())
                .await
                .map_err(|error| format!("cause-response teardown recovery failed: {error}"))?
        }
    };
    if run.disposition() != WorkloadTeardownRunDisposition::Completed
        || run.record().phase() != WorkloadSagaPhase::Recorded
    {
        return Err(format!(
            "compensation recovery did not record: disposition={:?} phase={:?}",
            run.disposition(),
            run.record().phase()
        ));
    }
    require_effect_count(&root.join("effects"), 1, "provision")?;
    require_effect_count(&root.join("teardown-effects"), 1, "teardown")?;
    Ok(cut.observation())
}

fn compensation_teardown_runtime(
    root: &Path,
    coordinator: Arc<WorkloadSagaCoordinator>,
    record: &WorkloadSagaRecord,
) -> Result<Arc<WorkloadTeardownRuntime>, String> {
    let source = Arc::new(ExactProcessSource {
        key: record.key().clone(),
        identity: record.active_intent().source().source_identity().clone(),
        evidence: record.active_intent().source().clone(),
    });
    let provider = Arc::new(CompensationTeardownProvider {
        effects: root.join("teardown-effects"),
    });
    fs::create_dir_all(&provider.effects)
        .map_err(|error| format!("teardown effect directory failed: {error}"))?;
    let selection = record
        .active_intent()
        .network()
        .compiled_plan()
        .content()
        .capability_selection()
        .cloned()
        .ok_or_else(|| "compensation record omitted network selection".to_owned())?;
    let capabilities = WorkloadTeardownCapabilityRegistry::new(
        [NetworkAttachmentTeardownCapabilities::new(
            selection.attachment_provider_id().clone(),
            provider.clone(),
            provider.clone(),
        )],
        [WorkloadExecutionTeardownCapabilities::new(
            execution_provider_id(),
            provider.clone(),
            provider.clone(),
        )],
        [IngressTeardownCapabilities::new(
            selection.ingress_provider_id().clone(),
            provider,
        )],
    )
    .map_err(|error| format!("compensation teardown registry failed: {error}"))?;
    Ok(Arc::new(WorkloadTeardownRuntime::new(
        coordinator,
        source,
        provider_reports(),
        Arc::new(capabilities),
    )))
}

fn require_effect_count(root: &Path, expected: usize, label: &str) -> Result<(), String> {
    let count = fs::read_dir(root)
        .map_err(|error| format!("{label} effect directory could not be read: {error}"))?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .count();
    if count == expected {
        Ok(())
    } else {
        Err(format!(
            "expected {expected} exact {label} effects, observed {count}"
        ))
    }
}

fn process_driver(
    root: &Path,
    crash_cut: Option<CrashCut>,
    cut_marker: &Path,
) -> Result<(WorkloadProvisionDriver, WorkloadSagaRecord), String> {
    process_driver_with_failure(root, crash_cut, cut_marker, None)
}

fn process_driver_with_failure(
    root: &Path,
    crash_cut: Option<CrashCut>,
    cut_marker: &Path,
    fail_step: Option<WorkloadProvisionStep>,
) -> Result<(WorkloadProvisionDriver, WorkloadSagaRecord), String> {
    fs::create_dir_all(root.join("effects"))
        .map_err(|error| format!("effect directory could not be created: {error}"))?;
    let record = process_record();
    let engine = Arc::new(
        nimbus_engine::Engine::new(root.join("engine"))
            .map_err(|error| format!("process Engine open failed: {error}"))?,
    );
    let engine_store = EngineWorkloadSagaStore::new(engine);
    let store: Arc<dyn WorkloadSagaStore> = if crash_cut == Some(CrashCut::ResultCas) {
        Arc::new(CrashAfterResultStore {
            inner: engine_store,
            marker: cut_marker.to_owned(),
        })
    } else {
        Arc::new(engine_store)
    };
    let coordinator = Arc::new(WorkloadSagaCoordinator::new(store));
    let journal =
        ProviderCommandAttemptJournal::open(root.join("provider"), PROVIDER_JOURNAL_NAMESPACE)
            .map_err(|error| format!("provider journal open failed: {error}"))?;
    let provider = Arc::new(ProcessProofProvider {
        phases: ProviderProvisionPhaseAdapter::new(journal),
        effects: root.join("effects"),
        crash_cut: crash_cut.filter(|cut| *cut != CrashCut::ResultCas),
        cut_marker: cut_marker.to_owned(),
        fail_step,
    });
    let capabilities = WorkloadProvisionCapabilityRegistry::new(
        [NetworkAttachmentProvisionCapabilities::new(
            attachment_provider_id(),
            provider.clone(),
        )],
        [WorkloadExecutionProvisionCapabilities::new(
            execution_provider_id(),
            provider.clone(),
        )],
        [IngressProvisionCapabilities::new(
            ingress_provider_id(),
            provider,
        )],
    )
    .map_err(|error| format!("process capability registry failed: {error}"))?;
    let source = Arc::new(ExactProcessSource {
        key: record.key().clone(),
        identity: record.active_intent().source().source_identity().clone(),
        evidence: record.active_intent().source().clone(),
    });
    let dispatcher = Arc::new(
        nimbus_compute::workload_saga::WorkloadProvisionDispatcher::new(
            source,
            provider_reports(),
            Arc::new(capabilities),
        ),
    );
    Ok((
        WorkloadProvisionDriver::new(coordinator, dispatcher),
        record,
    ))
}

struct CrashAfterResultStore {
    inner: EngineWorkloadSagaStore,
    marker: PathBuf,
}

struct CrashAfterCauseStore {
    inner: EngineWorkloadSagaStore,
    marker: PathBuf,
}

impl WorkloadSagaStore for CrashAfterCauseStore {
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
            let crash_after_commit = next.phase() == WorkloadSagaPhase::WithdrawalCommitted
                && matches!(
                    next.teardown_disposition()
                        .map(|disposition| disposition.cause()),
                    Some(WorkloadTeardownCause::FailedProvision { .. })
                );
            let result = self.inner.compare_and_swap(expected, next).await?;
            if crash_after_commit && result == WorkloadSagaCommit::Applied {
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

impl WorkloadSagaStore for CrashAfterResultStore {
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
            let crash_after_commit = next.phase() == WorkloadSagaPhase::NetworkReserved;
            let result = self.inner.compare_and_swap(expected, next).await?;
            if crash_after_commit && result == WorkloadSagaCommit::Applied {
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

struct CompensationTeardownProvider {
    effects: PathBuf,
}

impl CompensationTeardownProvider {
    fn observe(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownProviderObservation {
        let evidence = WorkloadOwnerEvidenceDigest::sha256(format!(
            "nnc65g-compensation:{:?}:{}",
            command.step(),
            command.attempt_id().as_str()
        ));
        let success = teardown_success(command, evidence);
        match command.mode() {
            WorkloadTeardownCommandMode::Execute => {
                let path = self.effects.join(format!(
                    "{}-{}",
                    teardown_step_name(command.step()),
                    command.attempt_id().as_str()
                ));
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .unwrap_or_else(|error| {
                        panic!(
                            "duplicate or failed teardown effect {}: {error}",
                            path.display()
                        )
                    });
                file.write_all(format!("{:?}", command.step()).as_bytes())
                    .and_then(|()| file.sync_all())
                    .expect("teardown effect marker should become durable");
                WorkloadTeardownProviderObservation::for_command(
                    command,
                    WorkloadTeardownProviderOutcome::Execute(
                        WorkloadTeardownExecuteOutcome::Succeeded(Box::new(success)),
                    ),
                )
            }
            WorkloadTeardownCommandMode::Inspect => {
                WorkloadTeardownProviderObservation::for_command(
                    command,
                    WorkloadTeardownProviderOutcome::Inspect(
                        WorkloadTeardownInspectOutcome::Satisfied(Box::new(success)),
                    ),
                )
            }
        }
    }
}

macro_rules! compensation_teardown_capability {
    ($trait_name:ident) => {
        impl $trait_name for CompensationTeardownProvider {
            fn execute<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadTeardownCommand,
            ) -> WorkloadTeardownCapabilityFuture<'a> {
                Box::pin(std::future::ready(self.observe(command)))
            }

            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadTeardownCommand,
            ) -> WorkloadTeardownCapabilityFuture<'a> {
                Box::pin(std::future::ready(self.observe(command)))
            }
        }
    };
}

compensation_teardown_capability!(FinalIngressWithdrawalCapability);
compensation_teardown_capability!(WorkloadExecutionDrainCapability);
compensation_teardown_capability!(WorkloadExecutionStopCapability);
compensation_teardown_capability!(NetworkDetachmentCapability);
compensation_teardown_capability!(NetworkReleaseCapability);

fn teardown_success(
    command: &ConfirmedWorkloadTeardownCommand,
    evidence: WorkloadOwnerEvidenceDigest,
) -> WorkloadTeardownSuccessEvidence {
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
        _ => panic!("compensation teardown step and subjects should match"),
    }
}

const fn teardown_step_name(step: WorkloadTeardownStep) -> &'static str {
    match step {
        WorkloadTeardownStep::WithdrawPublication => "withdraw",
        WorkloadTeardownStep::DrainExecution => "drain",
        WorkloadTeardownStep::StopExecution => "stop",
        WorkloadTeardownStep::DetachNetwork => "detach",
        WorkloadTeardownStep::ReleaseNetwork => "release",
    }
}

struct ProcessProofProvider {
    phases: ProviderProvisionPhaseAdapter,
    effects: PathBuf,
    crash_cut: Option<CrashCut>,
    cut_marker: PathBuf,
    fail_step: Option<WorkloadProvisionStep>,
}

impl WorkloadExecutionObservationCapability for ProcessProofProvider {
    fn observe<'a>(
        &'a self,
        _request: &'a WorkloadExecutionObservationRequest,
    ) -> WorkloadExecutionObservationFuture<'a> {
        Box::pin(async { WorkloadProviderObservation::Ambiguous })
    }
}

impl WorkloadIngressObservationCapability for ProcessProofProvider {
    fn observe<'a>(
        &'a self,
        _request: &'a WorkloadIngressObservationRequest,
    ) -> WorkloadIngressObservationFuture<'a> {
        Box::pin(async { WorkloadProviderObservation::Ambiguous })
    }
}

impl ProcessProofProvider {
    fn execute_effect(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionInspectionResult {
        if command.step() == WorkloadProvisionStep::ReserveNetwork
            && command.mode() == WorkloadProvisionCommandMode::Execute
            && self.crash_cut == Some(CrashCut::ClaimCas)
        {
            durable_stall(&self.cut_marker);
        }
        self.phases.execute(command, || {
            if command.step() == WorkloadProvisionStep::ReserveNetwork
                && self.crash_cut == Some(CrashCut::OwnerClaim)
            {
                durable_stall(&self.cut_marker);
            }
            let observation = self.create_effect(command);
            if command.step() == WorkloadProvisionStep::ReserveNetwork
                && self.crash_cut == Some(CrashCut::Effect)
            {
                durable_stall(&self.cut_marker);
            }
            observation
        })
    }

    fn inspect_effect(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionInspectionResult {
        self.phases.inspect(command, || {
            if self.effect_path(command).is_file() {
                ProviderProvisionEffectObservation::Succeeded {
                    evidence: effect_evidence(command),
                }
            } else {
                ProviderProvisionEffectObservation::Absent {
                    evidence: format!("{}-absent", step_name(command.step())).into_bytes(),
                }
            }
        })
    }

    fn inspect_ready(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionInspectionResult {
        self.phases
            .inspect(command, || ProviderProvisionEffectObservation::Succeeded {
                evidence: effect_evidence(command),
            })
    }

    fn create_effect(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> ProviderProvisionEffectObservation {
        if self.fail_step == Some(command.step()) {
            return ProviderProvisionEffectObservation::DefiniteFailure {
                code: "process_definite_failure".to_owned(),
                evidence: format!("{}-definite-failure", step_name(command.step())).into_bytes(),
            };
        }
        let path = self.effect_path(command);
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return ProviderProvisionEffectObservation::DefiniteFailure {
                    code: "duplicate_external_effect".to_owned(),
                    evidence: path.display().to_string().into_bytes(),
                };
            }
            Err(error) => {
                return ProviderProvisionEffectObservation::Ambiguous {
                    evidence: error.to_string().into_bytes(),
                };
            }
        };
        let evidence = effect_evidence(command);
        if file
            .write_all(&evidence)
            .and_then(|()| file.sync_all())
            .is_err()
        {
            return ProviderProvisionEffectObservation::Ambiguous { evidence };
        }
        ProviderProvisionEffectObservation::Succeeded { evidence }
    }

    fn effect_path(&self, command: &ConfirmedWorkloadProvisionCommand) -> PathBuf {
        self.effects.join(format!(
            "{}-{}",
            step_name(command.step()),
            command.attempt_id().as_str()
        ))
    }
}

macro_rules! effect_capability {
    ($trait_name:ident) => {
        impl $trait_name for ProcessProofProvider {
            fn execute<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.execute_effect(command) })
            }

            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.inspect_effect(command) })
            }
        }
    };
}

effect_capability!(NetworkReservationCapability);
effect_capability!(WorkloadPreparationCapability);
effect_capability!(NetworkAttachmentCapability);
effect_capability!(WorkloadActivationCapability);
effect_capability!(IngressPublicationCapability);

impl WorkloadActivationPrerequisiteCapability for ProcessProofProvider {
    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        Box::pin(async move { self.inspect_ready(command) })
    }
}

impl WorkloadReadinessCapability for ProcessProofProvider {
    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        Box::pin(async move { self.inspect_ready(command) })
    }
}

impl IngressPublicationInspectionCapability for ProcessProofProvider {
    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        Box::pin(async move { self.inspect_ready(command) })
    }
}

fn durable_stall(marker: &Path) -> ! {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(marker)
        .expect("cut marker must be created exactly once");
    file.write_all(b"reached")
        .and_then(|()| file.sync_all())
        .expect("cut marker must become durable");
    loop {
        std::thread::park();
    }
}

fn effect_evidence(command: &ConfirmedWorkloadProvisionCommand) -> Vec<u8> {
    format!(
        "{}:{}:{}",
        step_name(command.step()),
        command.attempt_id().as_str(),
        command.dispatch_epoch().as_u64()
    )
    .into_bytes()
}

const fn step_name(step: WorkloadProvisionStep) -> &'static str {
    match step {
        WorkloadProvisionStep::ReserveNetwork => "reserve",
        WorkloadProvisionStep::PrepareWorkload => "prepare",
        WorkloadProvisionStep::AttachNetwork => "attach",
        WorkloadProvisionStep::InspectActivationPrerequisites => "activation-prerequisites",
        WorkloadProvisionStep::ActivateWorkload => "activate",
        WorkloadProvisionStep::InspectWorkloadReadiness => "workload-readiness",
        WorkloadProvisionStep::Publish => "publish",
        WorkloadProvisionStep::ObservePublication => "publication-observation",
    }
}

fn process_record() -> WorkloadSagaRecord {
    let tenant_id = TenantId::new("tenant-nnc64-process").expect("process tenant should validate");
    let key = WorkloadSagaKey::new(
        tenant_id.clone(),
        WorkloadId::new("workload-nnc64-process").expect("process workload should validate"),
    );
    let executable = WorkloadExecutableIntent::new(
        WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
        r#"{"fixture":"nnc64-process"}"#,
    )
    .expect("process executable should validate");
    let source = WorkloadProvisionSourceEvidence::standalone_sandbox(
        WorkloadProvisionSourceIdentity::standalone_sandbox("nnc64-process", "fixture")
            .expect("process source identity should validate"),
        WorkloadProvisionSourceGeneration::new(1),
        WorkloadProvisionSourceResourceVersion::new("nnc64-process-v1")
            .expect("process source version should validate"),
        executable.content_digest(),
        attachment_provider_id(),
        execution_provider_id(),
    )
    .expect("process source should validate");
    let intent = WorkloadSagaIntent::new_without_automatic_restart(
        DesiredWorkloadKind::Sandbox,
        DesiredWorkloadState::Running,
        WorkloadGeneration::new(1),
        executable,
        source,
        nimbus_workloads::WorkloadNetworkIntent::new(compiled_network_plan(
            &tenant_id,
            "nnc64-process",
            1,
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::PublishWhenReady,
        )),
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
        WorkloadAdmissionEvidence::new(
            format!("tid_{}", "3".repeat(64))
                .try_into()
                .expect("process decision id should validate"),
            format!("twu_{}", "4".repeat(64))
                .try_into()
                .expect("process workload uid should validate"),
            NodeIdentity::new("node-nnc64-process").expect("process node should validate"),
        ),
    )
    .expect("process intent should validate");
    WorkloadSagaRecord::new(key, intent).expect("process record should validate")
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
    NetworkCapabilityRegistry::new([bundle]).expect("process provider report should validate")
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
