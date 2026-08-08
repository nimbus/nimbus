use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use nimbus_compute::workload_saga::provision_provider::{
    ProviderProvisionEffectObservation, ProviderProvisionPhaseAdapter,
};
use nimbus_compute::workload_saga::{
    ConfirmedWorkloadProvisionCommand, IngressProvisionCapabilities, IngressPublicationCapability,
    IngressPublicationInspectionCapability, NetworkAttachmentCapability,
    NetworkAttachmentProvisionCapabilities, NetworkReservationCapability,
    WorkloadActivationCapability, WorkloadActivationPrerequisiteCapability,
    WorkloadExecutionProvisionCapabilities, WorkloadPreparationCapability,
    WorkloadProvisionCapabilityFuture, WorkloadProvisionCapabilityRegistry,
    WorkloadProvisionDriver, WorkloadProvisionRunDisposition, WorkloadProvisionSourceAuthority,
    WorkloadProvisionSourceAuthorityError, WorkloadProvisionSourceFuture,
    WorkloadReadinessCapability, WorkloadSagaCoordinator,
};
use nimbus_compute::{
    WorkloadExecutionObservationCapability, WorkloadExecutionObservationFuture,
    WorkloadExecutionObservationRequest, WorkloadIngressObservationCapability,
    WorkloadIngressObservationFuture, WorkloadIngressObservationRequest,
    WorkloadProviderObservation,
};
use nimbus_core::{TenantId, WorkloadId};
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
    WorkloadGeneration, WorkloadProvisionCommandMode, WorkloadProvisionInspectionResult,
    WorkloadProvisionSourceEvidence, WorkloadProvisionSourceGeneration,
    WorkloadProvisionSourceIdentity, WorkloadProvisionSourceResourceVersion, WorkloadProvisionStep,
    WorkloadPublicationIntent, WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaFuture,
    WorkloadSagaIntent, WorkloadSagaKey, WorkloadSagaPage, WorkloadSagaPageRequest,
    WorkloadSagaPhase, WorkloadSagaRecord, WorkloadSagaStore, WorkloadSagaTenantPage,
    WorkloadSagaTenantPageRequest,
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

fn process_driver(
    root: &Path,
    crash_cut: Option<CrashCut>,
    cut_marker: &Path,
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

struct ProcessProofProvider {
    phases: ProviderProvisionPhaseAdapter,
    effects: PathBuf,
    crash_cut: Option<CrashCut>,
    cut_marker: PathBuf,
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
    let intent = WorkloadSagaIntent::new(
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
