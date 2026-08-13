use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nimbus_compute::workload_saga::restart_provider_command::{
    ProviderRestartEffectObservation, ProviderRestartPhaseAdapter,
};
use nimbus_compute::workload_saga::{
    ConfirmedWorkloadRestartCommand, ConfirmedWorkloadTeardownCommand,
    FinalIngressWithdrawalCapability, IngressTeardownCapabilities,
    NetworkAttachmentTeardownCapabilities, NetworkDetachmentCapability, NetworkReleaseCapability,
    NetworkRestartAttachmentCapability, RestartPublicationCapability,
    RestartPublicationObservationCapability, RestartPublicationWithdrawalCapability,
    WorkloadExecutionDrainCapability, WorkloadExecutionQuiescenceCapability,
    WorkloadExecutionStopCapability, WorkloadExecutionTeardownCapabilities,
    WorkloadProvisionSourceAuthority, WorkloadProvisionSourceAuthorityError,
    WorkloadProvisionSourceFuture, WorkloadRestartActivationCapability,
    WorkloadRestartActivationPrerequisiteCapability, WorkloadRestartCapabilities,
    WorkloadRestartCapabilityFuture, WorkloadRestartCapabilityRegistry, WorkloadRestartCommandMode,
    WorkloadRestartDecision, WorkloadRestartPreparationCapability,
    WorkloadRestartReadinessCapability, WorkloadSagaConfirmation, WorkloadSagaCoordinator,
    WorkloadTeardownCancellationToken, WorkloadTeardownCapabilityFuture,
    WorkloadTeardownCapabilityRegistry, WorkloadTeardownExecuteOutcome,
    WorkloadTeardownInspectOutcome, WorkloadTeardownProviderObservation,
    WorkloadTeardownProviderOutcome, WorkloadTeardownRunDisposition, WorkloadTeardownRuntime,
    decide_restart_progress, settle_restart_for_teardown_once_for_test,
};
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
    WorkloadOwnerEvidenceDigest, WorkloadPhaseDetail, WorkloadProvisionSourceEvidence,
    WorkloadProvisionSourceIdentity, WorkloadRestartDisposition, WorkloadRestartEffectResult,
    WorkloadRestartNotBeforeUnixMillis, WorkloadRestartStep, WorkloadSagaCommit,
    WorkloadSagaExpected, WorkloadSagaKey, WorkloadSagaPhase, WorkloadSagaRecord,
    WorkloadSagaStore, WorkloadTeardownCommandMode, WorkloadTeardownStep, WorkloadTeardownSubjects,
    WorkloadTeardownSuccessEvidence, WorkloadTerminalEvidenceDigest, WorkloadTerminalObservation,
};

use super::super::EngineWorkloadSagaStore;
use super::{restart_process::publication_withdrawal_history, valid_competing_successor};

const CHILD_TEST: &str = "workload_saga_store::tests::restart_settlement_process::workload_restart_settlement_process_child";
const MODE_ENV: &str = "NIMBUS_NNC65G_RESTART_SETTLEMENT_MODE";
const WRITE_MODE: &str = "write";
const RECOVER_MODE: &str = "recover";
const BOUNDARY: &str = "restart-effect-and-successor-durable-before-result";
const OBSERVATION: &str = "restart-settlement-recovered:recorded:one-execute";
const PID_PREFIX: &str = "NIMBUS_NNC65G_RESTART_SETTLEMENT_PID";
const TIMEOUT: Duration = Duration::from_secs(20);
const LABEL: &str = "nnc65g-restart-settlement";
const PROVIDER_NAMESPACE: &str = "nnc65g-restart-settlement";

#[test]
fn restart_settlement_reopens_without_duplicate_execute() {
    let root = tempfile::tempdir().expect("restart settlement process root should build");
    let result = SubprocessCrashCutHarness::new(TIMEOUT)
        .run(
            root.path(),
            BOUNDARY,
            OBSERVATION,
            child("restart-settlement-writer", WRITE_MODE),
            child("restart-settlement-recovery", RECOVER_MODE),
        )
        .unwrap_or_else(|error| panic!("restart settlement process proof failed: {error}"));

    assert_eq!(result.boundary(), BOUNDARY);
    assert_eq!(result.observation(), OBSERVATION);
    assert_eq!(
        result.crash_diagnostic().cleanup(),
        "killed-at-boundary-and-reaped"
    );
    assert_eq!(result.recovery_diagnostic().cleanup(), "exited-and-reaped");
    let writer_pid = process_id(result.crash_diagnostic().stderr(), "writer");
    let recovery_pid = process_id(result.recovery_diagnostic().stderr(), "recovery");
    assert_ne!(
        writer_pid, recovery_pid,
        "recovery must use a fresh process"
    );
}

#[test]
#[ignore = "spawned only by the restart settlement process proof parent"]
fn workload_restart_settlement_process_child() {
    let mode = std::env::var(MODE_ENV).expect("restart settlement child mode should be set");
    match mode.as_str() {
        WRITE_MODE => run_crash_cut_child(|context| {
            eprintln!("{PID_PREFIX} writer {}", std::process::id());
            runtime()?.block_on(write_crash_state(context.state_root()))?;
            context.reach_boundary(BOUNDARY)
        })
        .unwrap_or_else(|error| panic!("restart settlement writer failed: {error}")),
        RECOVER_MODE => run_crash_recovery_child(|context| {
            eprintln!("{PID_PREFIX} recovery {}", std::process::id());
            runtime()?.block_on(recover_to_recorded(context.state_root()))
        })
        .unwrap_or_else(|error| panic!("restart settlement recovery failed: {error}")),
        unknown => panic!("unknown restart settlement child mode {unknown:?}"),
    }
}

async fn write_crash_state(root: &Path) -> Result<(), String> {
    let engine =
        Arc::new(Engine::new(root).map_err(|error| format!("writer Engine open failed: {error}"))?);
    let store = Arc::new(EngineWorkloadSagaStore::new(engine));
    let history = publication_withdrawal_history(LABEL);
    persist_history(&store, &history).await?;
    let loaded = history.last().expect("restart history should not be empty");
    let WorkloadRestartDecision::Proposed(proposed) =
        decide_restart_progress(loaded, WorkloadRestartNotBeforeUnixMillis::new(500))
            .map_err(|error| format!("writer restart decision failed: {error}"))?
    else {
        return Err("writer restart decision did not propose a command".to_owned());
    };
    let coordinator = WorkloadSagaCoordinator::new(store.clone());
    let confirmed = coordinator
        .claim_restart_command(loaded, &proposed)
        .await
        .map_err(|error| format!("writer restart claim failed: {error}"))?;
    if confirmed.confirmation() != WorkloadSagaConfirmation::AppliedByThisCall {
        return Err(format!(
            "writer did not win exact Execute authority: {:?}",
            confirmed.confirmation()
        ));
    }
    let command = confirmed
        .command()
        .ok_or_else(|| "writer confirmation omitted restart command".to_owned())?;
    if command.mode() != WorkloadRestartCommandMode::Execute {
        return Err("writer command was not Execute".to_owned());
    }
    let provider = RestartProcessProvider::open(root)?;
    provider.observe(command, WorkloadRestartCommandMode::Execute);
    if !effect_marker(root).is_file() {
        return Err("writer restart effect marker is missing".to_owned());
    }

    let claimed = confirmed
        .confirmed_record()
        .ok_or_else(|| "writer confirmation omitted durable claim".to_owned())?;
    let successor = valid_competing_successor(claimed);
    if !matches!(
        successor
            .restart_state()
            .active()
            .map(|active| active.disposition()),
        Some(WorkloadRestartDisposition::InspectionRequired { .. })
    ) {
        return Err("stopped successor did not fence the issued restart for inspection".to_owned());
    }
    let commit = store
        .compare_and_swap(
            WorkloadSagaExpected::Revision(claimed.revision()),
            successor,
        )
        .await
        .map_err(|error| format!("writer stopped-successor CAS failed: {error}"))?;
    if commit != WorkloadSagaCommit::Applied {
        return Err(format!(
            "writer stopped-successor CAS was not applied: {commit:?}"
        ));
    }
    Ok(())
}

async fn recover_to_recorded(root: &Path) -> Result<String, String> {
    let engine = Arc::new(
        Engine::new(root).map_err(|error| format!("recovery Engine open failed: {error}"))?,
    );
    let store: Arc<dyn WorkloadSagaStore> = Arc::new(EngineWorkloadSagaStore::new(engine));
    let coordinator = Arc::new(WorkloadSagaCoordinator::new(store));
    let key = publication_withdrawal_history(LABEL)
        .last()
        .expect("restart recovery history should not be empty")
        .key()
        .clone();
    let before = coordinator
        .load(&key)
        .await
        .map_err(|error| format!("restart recovery preflight failed: {error}"))?
        .ok_or_else(|| "restart recovery durable record is missing".to_owned())?;
    let active = before
        .restart_state()
        .active()
        .ok_or_else(|| "restart recovery lost its active restart".to_owned())?;
    if !matches!(
        active.disposition(),
        WorkloadRestartDisposition::InspectionRequired { .. }
    ) {
        return Err("restart recovery did not reopen inspection-only authority".to_owned());
    }
    let target_attempt = active.admission().attempt_id().clone();
    let source_attempt = active.admission().source_attempt_id().clone();
    let source = Arc::new(ExactProcessSource::for_record(&before));
    let reports = provider_reports();
    let selection = before
        .active_intent()
        .network()
        .compiled_plan()
        .content()
        .capability_selection()
        .cloned()
        .ok_or_else(|| "restart recovery fixture omitted network selection".to_owned())?;
    let provider = RestartProcessProvider::open(root)?;
    let restart_capabilities =
        WorkloadRestartCapabilityRegistry::new([WorkloadRestartCapabilities::new(
            execution_provider_id(),
            Some(selection.clone()),
            provider.clone(),
            provider.clone(),
            provider,
        )])
        .map_err(|error| format!("restart recovery registry failed: {error}"))?;
    let settled = settle_restart_for_teardown_once_for_test(
        Arc::clone(&coordinator),
        source.clone(),
        reports.clone(),
        Arc::new(restart_capabilities),
        &key,
        WorkloadRestartNotBeforeUnixMillis::new(500),
    )
    .await?;
    if !settled {
        return Err("restart recovery remained pending after exact inspection".to_owned());
    }
    if unexpected_inspect_marker(root).exists() {
        return Err("restart recovery invoked the effect closure during inspection".to_owned());
    }
    if marker_count(root)? != 1 {
        return Err("restart recovery did not preserve exactly one Execute effect".to_owned());
    }

    let withdrawal = coordinator
        .load(&key)
        .await
        .map_err(|error| format!("settled withdrawal load failed: {error}"))?
        .ok_or_else(|| "settled withdrawal is missing".to_owned())?;
    let settlement = withdrawal
        .teardown_disposition()
        .and_then(|disposition| disposition.context().restart_settlement())
        .ok_or_else(|| "withdrawal omitted exact restart settlement".to_owned())?;
    if settlement.claim().step() != WorkloadRestartStep::WithdrawPublication
        || !matches!(
            settlement.result(),
            WorkloadRestartEffectResult::Succeeded { .. }
        )
        || settlement.source_execution().attempt_id() != &source_attempt
        || settlement.target_execution().attempt_id() != &target_attempt
    {
        return Err("withdrawal crossed exact restart settlement evidence".to_owned());
    }

    let teardown_provider = Arc::new(TeardownProcessProvider::default());
    let teardown_capabilities = teardown_capabilities(teardown_provider.clone(), &selection)?;
    let teardown = WorkloadTeardownRuntime::new(
        Arc::clone(&coordinator),
        source,
        reports,
        Arc::new(teardown_capabilities),
    );
    let run = teardown
        .submit(key.clone(), &WorkloadTeardownCancellationToken::new())
        .await
        .map_err(|error| format!("restart settlement teardown failed: {error}"))?;
    if run.disposition() != WorkloadTeardownRunDisposition::Completed
        || run.record().phase() != WorkloadSagaPhase::Recorded
    {
        return Err(format!(
            "restart settlement teardown did not record: disposition={:?} phase={:?}",
            run.disposition(),
            run.record().phase()
        ));
    }
    let WorkloadPhaseDetail::Recorded(recorded) = run.record().phase_detail() else {
        return Err("restart settlement terminal detail is not Recorded".to_owned());
    };
    let terminal = recorded
        .terminal_execution_reference()
        .ok_or_else(|| "restart settlement terminal execution is missing".to_owned())?;
    if terminal.attempt_id() != &source_attempt || terminal.attempt_id() == &target_attempt {
        return Err(
            "pre-target restart settlement did not retain the exact source execution".to_owned(),
        );
    }
    let observations = teardown_provider.observations();
    if observations.len() != 5 {
        return Err(format!(
            "teardown produced {} terminal observations, expected five",
            observations.len()
        ));
    }
    let observations_only = WorkloadTerminalEvidenceDigest::for_observations(&observations)
        .map_err(|error| format!("observations-only digest failed: {error}"))?;
    if recorded.terminal_evidence_digest() == observations_only {
        return Err("Recorded digest omitted the restart settlement domain".to_owned());
    }
    Ok(OBSERVATION.to_owned())
}

struct RestartProcessProvider {
    phases: ProviderRestartPhaseAdapter,
    effect_marker: PathBuf,
    unexpected_inspect_marker: PathBuf,
}

impl RestartProcessProvider {
    fn open(root: &Path) -> Result<Arc<Self>, String> {
        let journal =
            ProviderCommandAttemptJournal::open(root.join("restart-provider"), PROVIDER_NAMESPACE)
                .map_err(|error| format!("restart provider journal failed: {error}"))?;
        Ok(Arc::new(Self {
            phases: ProviderRestartPhaseAdapter::new(journal),
            effect_marker: effect_marker(root),
            unexpected_inspect_marker: unexpected_inspect_marker(root),
        }))
    }

    fn observe(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
        mode: WorkloadRestartCommandMode,
    ) -> nimbus_compute::workload_saga::WorkloadRestartProviderObservation {
        let success = || ProviderRestartEffectObservation::Succeeded {
            evidence: b"nnc65g-restart-quiescence".to_vec(),
        };
        match mode {
            WorkloadRestartCommandMode::Execute => self.phases.execute(command, || {
                create_marker(&self.effect_marker, b"execute")
                    .expect("restart Execute marker should be created once");
                success()
            }),
            WorkloadRestartCommandMode::Inspect => self.phases.inspect(command, || {
                create_marker(&self.unexpected_inspect_marker, b"inspect-effect")
                    .expect("unexpected restart inspection marker should be unique");
                success()
            }),
        }
    }
}

macro_rules! restart_effect_capability {
    ($trait_name:ident) => {
        impl $trait_name for RestartProcessProvider {
            fn execute(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                Box::pin(std::future::ready(
                    self.observe(command, WorkloadRestartCommandMode::Execute),
                ))
            }

            fn inspect(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                Box::pin(std::future::ready(
                    self.observe(command, WorkloadRestartCommandMode::Inspect),
                ))
            }
        }
    };
}

macro_rules! restart_inspection_capability {
    ($trait_name:ident) => {
        impl $trait_name for RestartProcessProvider {
            fn inspect(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                Box::pin(std::future::ready(
                    self.observe(command, WorkloadRestartCommandMode::Inspect),
                ))
            }
        }
    };
}

restart_effect_capability!(RestartPublicationWithdrawalCapability);
restart_effect_capability!(WorkloadExecutionQuiescenceCapability);
restart_effect_capability!(WorkloadRestartPreparationCapability);
restart_effect_capability!(NetworkRestartAttachmentCapability);
restart_inspection_capability!(WorkloadRestartActivationPrerequisiteCapability);
restart_effect_capability!(WorkloadRestartActivationCapability);
restart_inspection_capability!(WorkloadRestartReadinessCapability);
restart_effect_capability!(RestartPublicationCapability);
restart_inspection_capability!(RestartPublicationObservationCapability);

#[derive(Default)]
struct TeardownProcessProvider {
    observations: Mutex<Vec<WorkloadTerminalObservation>>,
}

impl TeardownProcessProvider {
    fn observe(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownProviderObservation {
        let (success, terminal) = teardown_success(command);
        self.observations
            .lock()
            .expect("teardown observation lock should not be poisoned")
            .push(terminal);
        let outcome = match command.mode() {
            WorkloadTeardownCommandMode::Execute => WorkloadTeardownProviderOutcome::Execute(
                WorkloadTeardownExecuteOutcome::Succeeded(Box::new(success)),
            ),
            WorkloadTeardownCommandMode::Inspect => WorkloadTeardownProviderOutcome::Inspect(
                WorkloadTeardownInspectOutcome::Satisfied(Box::new(success)),
            ),
        };
        WorkloadTeardownProviderObservation::for_command(command, outcome)
    }

    fn observations(&self) -> Vec<WorkloadTerminalObservation> {
        self.observations
            .lock()
            .expect("teardown observation lock should not be poisoned")
            .clone()
    }
}

macro_rules! teardown_capability {
    ($trait_name:ident) => {
        impl $trait_name for TeardownProcessProvider {
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

teardown_capability!(FinalIngressWithdrawalCapability);
teardown_capability!(WorkloadExecutionDrainCapability);
teardown_capability!(WorkloadExecutionStopCapability);
teardown_capability!(NetworkDetachmentCapability);
teardown_capability!(NetworkReleaseCapability);

struct ExactProcessSource {
    key: WorkloadSagaKey,
    identity: WorkloadProvisionSourceIdentity,
    evidence: WorkloadProvisionSourceEvidence,
}

impl ExactProcessSource {
    fn for_record(record: &WorkloadSagaRecord) -> Self {
        Self {
            key: record.key().clone(),
            identity: record.active_intent().source().source_identity().clone(),
            evidence: record.active_intent().source().clone(),
        }
    }
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
            .map_or(WorkloadSagaExpected::Missing, |prior| {
                WorkloadSagaExpected::Revision(history[prior].revision())
            });
        let commit = store
            .compare_and_swap(expected, record.clone())
            .await
            .map_err(|error| format!("restart settlement seed failed: {error}"))?;
        if commit != WorkloadSagaCommit::Applied {
            return Err(format!(
                "restart settlement seed was not applied: {commit:?}"
            ));
        }
    }
    Ok(())
}

fn teardown_capabilities(
    provider: Arc<TeardownProcessProvider>,
    selection: &nimbus_network::NetworkCapabilitySelection,
) -> Result<WorkloadTeardownCapabilityRegistry, String> {
    WorkloadTeardownCapabilityRegistry::new(
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
    .map_err(|error| format!("teardown capability registry failed: {error}"))
}

fn teardown_success(
    command: &ConfirmedWorkloadTeardownCommand,
) -> (WorkloadTeardownSuccessEvidence, WorkloadTerminalObservation) {
    let evidence = WorkloadOwnerEvidenceDigest::sha256(format!(
        "nnc65g:{:?}:{}",
        command.step(),
        command.attempt_id().as_str()
    ));
    match (command.step(), command.subjects()) {
        (
            WorkloadTeardownStep::WithdrawPublication,
            WorkloadTeardownSubjects::Publication(reference),
        ) => (
            WorkloadTeardownSuccessEvidence::PublicationAbsent {
                reference: reference.clone(),
                evidence,
            },
            WorkloadTerminalObservation::PublicationAbsent {
                reference: reference.clone(),
                evidence,
            },
        ),
        (WorkloadTeardownStep::DrainExecution, WorkloadTeardownSubjects::Execution(reference)) => (
            WorkloadTeardownSuccessEvidence::ExecutionDrained {
                reference: reference.clone(),
                evidence,
            },
            WorkloadTerminalObservation::ExecutionDrained {
                reference: reference.clone(),
                evidence,
            },
        ),
        (WorkloadTeardownStep::StopExecution, WorkloadTeardownSubjects::Execution(reference)) => (
            WorkloadTeardownSuccessEvidence::ExecutionStopped {
                reference: reference.clone(),
                evidence,
            },
            WorkloadTerminalObservation::ExecutionStopped {
                reference: reference.clone(),
                evidence,
            },
        ),
        (WorkloadTeardownStep::DetachNetwork, WorkloadTeardownSubjects::Network(reference)) => (
            WorkloadTeardownSuccessEvidence::NetworkDetached {
                reference: reference.clone(),
                evidence,
            },
            WorkloadTerminalObservation::NetworkDetached {
                reference: reference.clone(),
                evidence,
            },
        ),
        (WorkloadTeardownStep::ReleaseNetwork, WorkloadTeardownSubjects::Network(reference)) => (
            WorkloadTeardownSuccessEvidence::NetworkReleased {
                reference: reference.clone(),
                evidence,
            },
            WorkloadTerminalObservation::NetworkReleased {
                reference: reference.clone(),
                evidence,
            },
        ),
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
        .expect("restart settlement provider reports should validate")
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

fn create_marker(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn effect_marker(root: &Path) -> PathBuf {
    root.join("restart-execute-effect")
}

fn unexpected_inspect_marker(root: &Path) -> PathBuf {
    root.join("restart-unexpected-inspect-effect")
}

fn marker_count(root: &Path) -> Result<usize, String> {
    let count = [effect_marker(root), unexpected_inspect_marker(root)]
        .into_iter()
        .filter(|path| path.exists())
        .count();
    Ok(count)
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|error| format!("restart settlement process runtime failed: {error}"))
}

fn child(role: &str, mode: &str) -> ProcessRoleSpec {
    ProcessRoleSpec::new(
        role,
        std::env::current_exe().expect("current test executable should resolve"),
    )
    .arg("--exact")
    .arg(CHILD_TEST)
    .arg("--ignored")
    .arg("--nocapture")
    .env(MODE_ENV, mode)
}

fn process_id(stderr: &str, role: &str) -> u32 {
    stderr
        .lines()
        .find_map(|line| {
            line.strip_prefix(&format!("{PID_PREFIX} {role} "))
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or_else(|| panic!("missing {role} child process id in stderr:\n{stderr}"))
}
