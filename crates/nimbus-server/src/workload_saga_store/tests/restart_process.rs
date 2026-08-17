use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use nimbus_compute::workload_saga::{
    WorkloadRestartCommandMode, WorkloadRestartDecision, WorkloadRestartSymbolicAction,
    WorkloadSagaConfirmation, WorkloadSagaCoordinator, decide_restart_progress,
};
use nimbus_core::{TenantId, WorkloadId};
use nimbus_engine::Engine;
use nimbus_process_harness::{
    ProcessRoleSpec, SubprocessCrashCutHarness, run_crash_cut_child, run_crash_recovery_child,
};
use nimbus_workloads::{
    WorkloadActivationIntent, WorkloadPublicationIntent, WorkloadRestartCommandClaim,
    WorkloadRestartDispatchAuthorization, WorkloadRestartDispatchEpoch, WorkloadRestartDisposition,
    WorkloadRestartEffectResult, WorkloadRestartEvidenceDigest, WorkloadRestartNotBeforeUnixMillis,
    WorkloadRestartPhase, WorkloadRestartStep, WorkloadSagaCommit, WorkloadSagaExpected,
    WorkloadSagaKey, WorkloadSagaPhase, WorkloadSagaRecord, WorkloadSagaStore,
};
use sha2::{Digest, Sha256};

use super::super::EngineWorkloadSagaStore;
use super::recovery::provision_history;
use super::restart::{admit, explicit_input};

const CHILD_TEST: &str =
    "workload_saga_store::tests::restart_process::workload_restart_recovery_child";
const MODE_ENV: &str = "NIMBUS_NNC64A_RESTART_PROCESS_MODE";
const WRITE_MODE: &str = "write";
const RECOVER_MODE: &str = "recover";
const BOUNDARY: &str = "workload-restart.phase-matrix-durable";
const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_CHILD_DIAGNOSTIC_BYTES: usize = 4 * 1024;
const PID_PREFIX: &str = "NIMBUS_NNC64A_RESTART_PROCESS_ID";
const RESTART_NOT_BEFORE: u64 = 500;
const EXPECTED_OBSERVATION: &str =
    "restart-matrix-v1:16:aae7a68b1f831a90e6e8a64d06c9cb9a7ac2e44b95386f0f43b4a4ac8238feb5";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryCase {
    Requested,
    PublicationWithdrawal,
    ExecutionQuiescence,
    ScheduledNotDue,
    ScheduledDue,
    Preparation,
    Attachment,
    ActivationPrerequisite,
    Activation,
    Readiness,
    Publication,
    Observation,
    DispatchPending,
    InspectionRequired,
    DefiniteFailure,
    Completed,
}

impl RecoveryCase {
    const ALL: [Self; 16] = [
        Self::Requested,
        Self::PublicationWithdrawal,
        Self::ExecutionQuiescence,
        Self::ScheduledNotDue,
        Self::ScheduledDue,
        Self::Preparation,
        Self::Attachment,
        Self::ActivationPrerequisite,
        Self::Activation,
        Self::Readiness,
        Self::Publication,
        Self::Observation,
        Self::DispatchPending,
        Self::InspectionRequired,
        Self::DefiniteFailure,
        Self::Completed,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Requested => "restart-process-requested",
            Self::PublicationWithdrawal => "restart-process-withdrawal",
            Self::ExecutionQuiescence => "restart-process-quiescence",
            Self::ScheduledNotDue => "restart-process-scheduled-not-due",
            Self::ScheduledDue => "restart-process-scheduled-due",
            Self::Preparation => "restart-process-preparation",
            Self::Attachment => "restart-process-attachment",
            Self::ActivationPrerequisite => "restart-process-activation-prerequisite",
            Self::Activation => "restart-process-activation",
            Self::Readiness => "restart-process-readiness",
            Self::Publication => "restart-process-publication",
            Self::Observation => "restart-process-observation",
            Self::DispatchPending => "restart-process-dispatch-pending",
            Self::InspectionRequired => "restart-process-inspection-required",
            Self::DefiniteFailure => "restart-process-definite-failure",
            Self::Completed => "restart-process-completed",
        }
    }

    const fn phase(self) -> WorkloadRestartPhase {
        match self {
            Self::Requested => WorkloadRestartPhase::Requested,
            Self::PublicationWithdrawal
            | Self::DispatchPending
            | Self::InspectionRequired
            | Self::DefiniteFailure => WorkloadRestartPhase::PublicationWithdrawalPending,
            Self::ExecutionQuiescence => WorkloadRestartPhase::ExecutionQuiescencePending,
            Self::ScheduledNotDue | Self::ScheduledDue => WorkloadRestartPhase::Scheduled,
            Self::Preparation => WorkloadRestartPhase::PreparationPending,
            Self::Attachment => WorkloadRestartPhase::AttachmentPending,
            Self::ActivationPrerequisite => WorkloadRestartPhase::ActivationPrerequisitePending,
            Self::Activation => WorkloadRestartPhase::ActivationPending,
            Self::Readiness => WorkloadRestartPhase::ReadinessPending,
            Self::Publication => WorkloadRestartPhase::PublicationPending,
            Self::Observation => WorkloadRestartPhase::ObservationPending,
            Self::Completed => WorkloadRestartPhase::Idle,
        }
    }

    const fn step(self) -> Option<WorkloadRestartStep> {
        match self {
            Self::PublicationWithdrawal
            | Self::DispatchPending
            | Self::InspectionRequired
            | Self::DefiniteFailure => Some(WorkloadRestartStep::WithdrawPublication),
            Self::ExecutionQuiescence => Some(WorkloadRestartStep::QuiesceExecution),
            Self::Preparation => Some(WorkloadRestartStep::PrepareExecution),
            Self::Attachment => Some(WorkloadRestartStep::AttachNetwork),
            Self::ActivationPrerequisite => {
                Some(WorkloadRestartStep::InspectActivationPrerequisites)
            }
            Self::Activation => Some(WorkloadRestartStep::ActivateExecution),
            Self::Readiness => Some(WorkloadRestartStep::InspectReadiness),
            Self::Publication => Some(WorkloadRestartStep::Publish),
            Self::Observation => Some(WorkloadRestartStep::ObservePublication),
            Self::Requested | Self::ScheduledNotDue | Self::ScheduledDue | Self::Completed => None,
        }
    }

    const fn now(self) -> WorkloadRestartNotBeforeUnixMillis {
        match self {
            Self::ScheduledNotDue => {
                WorkloadRestartNotBeforeUnixMillis::new(RESTART_NOT_BEFORE - 1)
            }
            _ => WorkloadRestartNotBeforeUnixMillis::new(RESTART_NOT_BEFORE),
        }
    }
}

#[test]
fn fresh_process_restart_reopens_engine() {
    let root = tempfile::tempdir().expect("restart process root should build");
    let result = SubprocessCrashCutHarness::new(TIMEOUT)
        .run(
            root.path(),
            BOUNDARY,
            EXPECTED_OBSERVATION,
            child("restart-matrix-writer", WRITE_MODE),
            child("restart-matrix-recovery", RECOVER_MODE),
        )
        .unwrap_or_else(|error| panic!("restart fresh-process recovery failed: {error}"));

    assert_eq!(result.boundary(), BOUNDARY);
    assert_eq!(result.observation(), EXPECTED_OBSERVATION);
    assert_eq!(
        result.crash_diagnostic().cleanup(),
        "killed-at-boundary-and-reaped"
    );
    assert_eq!(result.crash_diagnostic().successful(), Some(false));
    assert_eq!(result.crash_diagnostic().role(), "restart-matrix-writer");
    assert_eq!(result.recovery_diagnostic().successful(), Some(true));
    assert_eq!(result.recovery_diagnostic().cleanup(), "exited-and-reaped");
    assert_eq!(
        result.recovery_diagnostic().role(),
        "restart-matrix-recovery"
    );

    let writer_pid = process_id(result.crash_diagnostic().stderr(), "writer");
    let recovery_pid = process_id(result.recovery_diagnostic().stderr(), "recovery");
    assert_ne!(
        writer_pid, recovery_pid,
        "recovery must reopen Engine durability in a distinct process"
    );
    assert_eq!(
        result.crash_diagnostic().stderr(),
        format!("{PID_PREFIX} writer {writer_pid}\n")
    );
    assert_eq!(
        result.recovery_diagnostic().stderr(),
        format!("{PID_PREFIX} recovery {recovery_pid}\n")
    );
    for diagnostic in [result.crash_diagnostic(), result.recovery_diagnostic()] {
        assert!(
            diagnostic.stdout().len() <= MAX_CHILD_DIAGNOSTIC_BYTES,
            "{} stdout exceeded the bounded diagnostic contract",
            diagnostic.role()
        );
        assert!(
            diagnostic.stderr().len() <= MAX_CHILD_DIAGNOSTIC_BYTES,
            "{} stderr exceeded the bounded diagnostic contract",
            diagnostic.role()
        );
    }
}

#[test]
#[ignore = "spawned only by the restart fresh-process proof parent"]
fn workload_restart_recovery_child() {
    let mode = std::env::var(MODE_ENV).expect("restart process child mode should be set");
    match mode.as_str() {
        WRITE_MODE => run_crash_cut_child(|context| {
            eprintln!("{PID_PREFIX} writer {}", std::process::id());
            let runtime = runtime()?;
            let engine = Arc::new(
                Engine::new(context.state_root())
                    .map_err(|error| format!("writer Engine open failed: {error}"))?,
            );
            let store = EngineWorkloadSagaStore::new(engine);
            runtime.block_on(persist_matrix(&store))?;
            context.reach_boundary(BOUNDARY)
        })
        .unwrap_or_else(|error| panic!("restart matrix writer failed: {error}")),
        RECOVER_MODE => run_crash_recovery_child(|context| {
            eprintln!("{PID_PREFIX} recovery {}", std::process::id());
            let runtime = runtime()?;
            runtime.block_on(recover_matrix(context.state_root()))
        })
        .unwrap_or_else(|error| panic!("restart matrix recovery failed: {error}")),
        unknown => panic!("unknown restart process child mode {unknown:?}"),
    }
}

async fn persist_matrix(store: &EngineWorkloadSagaStore) -> Result<(), String> {
    for case in RecoveryCase::ALL {
        let history = history_for(case);
        let latest = history
            .last()
            .ok_or_else(|| format!("{} produced an empty history", case.label()))?;
        if latest.key() != &key(case)
            || latest.phase() != WorkloadSagaPhase::Observed
            || latest.restart_state().phase() != case.phase()
        {
            return Err(format!(
                "{} writer fixture mismatch: outer={:?} restart={:?}",
                case.label(),
                latest.phase(),
                latest.restart_state().phase()
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
                .map_err(|error| format!("{} persistence failed: {error}", case.label()))?;
            if commit != WorkloadSagaCommit::Applied {
                return Err(format!(
                    "{} persistence did not apply: {commit:?}",
                    case.label()
                ));
            }
        }
    }
    Ok(())
}

async fn recover_matrix(root: &Path) -> Result<String, String> {
    let engine = Arc::new(
        Engine::new(root).map_err(|error| format!("recovery Engine open failed: {error}"))?,
    );
    let store: Arc<dyn WorkloadSagaStore> = Arc::new(EngineWorkloadSagaStore::new(engine));
    let coordinator = WorkloadSagaCoordinator::new(store);
    let mut digest = Sha256::new();

    for case in RecoveryCase::ALL {
        let record = coordinator
            .load(&key(case))
            .await
            .map_err(|error| format!("{} recovery load failed: {error}", case.label()))?
            .ok_or_else(|| format!("{} recovery record is missing", case.label()))?;
        assert_recovered_identity(case, &record)?;
        let semantic = recover_case(&coordinator, case, &record).await?;
        digest.update(
            format!(
                "{}|{}|{}|{}|{}|{}\n",
                case.label(),
                record.saga_id().as_str(),
                record.revision().as_u64(),
                record.active_intent().desired_digest(),
                record.active_intent().network().digest(),
                semantic,
            )
            .as_bytes(),
        );
    }

    Ok(format!(
        "restart-matrix-v1:{}:{:x}",
        RecoveryCase::ALL.len(),
        digest.finalize()
    ))
}

async fn recover_case(
    coordinator: &WorkloadSagaCoordinator,
    case: RecoveryCase,
    record: &WorkloadSagaRecord,
) -> Result<String, String> {
    let decision = decide_restart_progress(record, case.now())
        .map_err(|error| format!("{} restart decision failed: {error}", case.label()))?;
    match case {
        RecoveryCase::Requested => {
            let WorkloadRestartDecision::Proposed(proposed) = decision else {
                return Err(format!("{} did not propose an advance", case.label()));
            };
            require(
                proposed.action_after_confirmation().is_none()
                    && proposed.candidate().restart_state().phase()
                        == WorkloadRestartPhase::PublicationWithdrawalPending,
                format!("{} proposed the wrong no-effect advance", case.label()),
            )?;
            assert_candidate_identity(record, proposed.candidate())?;
            Ok("advance:publication-withdrawal".to_owned())
        }
        RecoveryCase::ScheduledNotDue => {
            if decision
                != WorkloadRestartDecision::WaitUntil(WorkloadRestartNotBeforeUnixMillis::new(
                    RESTART_NOT_BEFORE,
                ))
            {
                return Err(format!(
                    "{} did not preserve its exact deadline",
                    case.label()
                ));
            }
            Ok(format!("wait-until:{RESTART_NOT_BEFORE}"))
        }
        RecoveryCase::ScheduledDue => {
            let WorkloadRestartDecision::Proposed(proposed) = decision else {
                return Err(format!("{} did not propose its due advance", case.label()));
            };
            require(
                proposed.action_after_confirmation().is_none()
                    && proposed.candidate().restart_state().phase()
                        == WorkloadRestartPhase::PreparationPending,
                format!("{} proposed the wrong due advance", case.label()),
            )?;
            assert_candidate_identity(record, proposed.candidate())?;
            Ok("advance:preparation".to_owned())
        }
        RecoveryCase::DispatchPending | RecoveryCase::InspectionRequired => {
            let WorkloadRestartDecision::InspectExact(decided_claim) = decision else {
                return Err(format!("{} did not require exact inspection", case.label()));
            };
            let retained_claim = active_claim(record)?;
            require(
                decided_claim.as_ref() == retained_claim,
                format!("{} crossed the durable inspection claim", case.label()),
            )?;
            let recovered = coordinator
                .inspect_confirmed_restart(record.key())
                .await
                .map_err(|error| format!("{} inspection recovery failed: {error}", case.label()))?;
            let expected_confirmation = if case == RecoveryCase::DispatchPending {
                WorkloadSagaConfirmation::AppliedByThisCall
            } else {
                WorkloadSagaConfirmation::ConfirmedReplay
            };
            require(
                recovered.confirmation() == expected_confirmation,
                format!("{} used the wrong recovery confirmation", case.label()),
            )?;
            let confirmed = recovered
                .confirmed_record()
                .ok_or_else(|| format!("{} inspection omitted durable truth", case.label()))?;
            require(
                matches!(
                    confirmed
                        .restart_state()
                        .active()
                        .map(|active| active.disposition()),
                    Some(WorkloadRestartDisposition::InspectionRequired { claim })
                        if claim == retained_claim
                ),
                format!("{} did not retain exact inspection state", case.label()),
            )?;
            let command = recovered
                .command()
                .ok_or_else(|| format!("{} inspection omitted its command", case.label()))?;
            assert_inspection_command(record, confirmed, retained_claim, command)?;
            Ok(format!(
                "inspect:{}:{}:{}:{}",
                retained_claim.command_id().as_str(),
                retained_claim.attempt_id().as_str(),
                retained_claim.restart_epoch().as_u64(),
                retained_claim.dispatch_epoch().as_u64(),
            ))
        }
        RecoveryCase::DefiniteFailure => {
            require(
                decision == WorkloadRestartDecision::DefiniteFailure,
                format!("{} did not remain fenced", case.label()),
            )?;
            let claim = active_claim(record)?;
            Ok(format!(
                "fenced:{}:{}:{}",
                claim.command_id().as_str(),
                claim.attempt_id().as_str(),
                claim.dispatch_epoch().as_u64(),
            ))
        }
        RecoveryCase::Completed => {
            require(
                decision == WorkloadRestartDecision::Wait,
                format!("{} did not remain complete", case.label()),
            )?;
            let state = record.restart_state();
            let completed = state
                .last_completed()
                .ok_or_else(|| format!("{} omitted completed history", case.label()))?;
            require(
                state.active().is_none()
                    && state.completed_restart_epoch() == completed.restart_epoch()
                    && state.current_execution_attempt_id() == completed.attempt_id(),
                format!("{} crossed completed attempt history", case.label()),
            )?;
            Ok(format!(
                "complete:{}:{}",
                completed.attempt_id().as_str(),
                completed.restart_epoch().as_u64(),
            ))
        }
        RecoveryCase::PublicationWithdrawal
        | RecoveryCase::ExecutionQuiescence
        | RecoveryCase::Preparation
        | RecoveryCase::Attachment
        | RecoveryCase::ActivationPrerequisite
        | RecoveryCase::Activation
        | RecoveryCase::Readiness
        | RecoveryCase::Publication
        | RecoveryCase::Observation => {
            let WorkloadRestartDecision::Proposed(proposed) = decision else {
                return Err(format!("{} did not propose exact dispatch", case.label()));
            };
            let expected_step = case
                .step()
                .ok_or_else(|| format!("{} has no expected command", case.label()))?;
            let expected_action = if expected_step.is_inspection() {
                WorkloadRestartSymbolicAction::InspectExactAttempt
            } else {
                WorkloadRestartSymbolicAction::StartExactAttempt
            };
            require(
                proposed.action_after_confirmation() == Some(expected_action),
                format!("{} omitted exact dispatch authority", case.label()),
            )?;
            assert_candidate_identity(record, proposed.candidate())?;
            let claim = active_claim(proposed.candidate())?;
            assert_claim(record, claim, expected_step)?;
            Ok(format!(
                "dispatch:{expected_step:?}:{}:{}:{}:{}",
                claim.command_id().as_str(),
                claim.attempt_id().as_str(),
                claim.restart_epoch().as_u64(),
                claim.dispatch_epoch().as_u64(),
            ))
        }
    }
}

fn assert_recovered_identity(
    case: RecoveryCase,
    record: &WorkloadSagaRecord,
) -> Result<(), String> {
    record
        .validate()
        .map_err(|error| format!("{} recovered invalid durable truth: {error}", case.label()))?;
    require(
        record.key() == &key(case)
            && record.phase() == WorkloadSagaPhase::Observed
            && record.active_intent().generation().as_u64() == 1
            && record
                .active_intent()
                .network()
                .compiled_plan()
                .plan()
                .generation()
                .as_u64()
                == 1
            && record.active_intent().publication() == WorkloadPublicationIntent::PublishWhenReady,
        format!("{} crossed desired or network identity", case.label()),
    )?;
    require(
        record.restart_state().phase() == case.phase(),
        format!("{} recovered the wrong restart phase", case.label()),
    )?;

    if let Some(active) = record.restart_state().active() {
        let admission = active.admission();
        let expected_request = nimbus_workloads::WorkloadRestartRequestId::for_explicit(
            record.saga_id(),
            record.active_intent().source().source_generation(),
            case.label(),
        )
        .map_err(|error| format!("{} request derivation failed: {error}", case.label()))?;
        require(
            admission.saga_id() == record.saga_id()
                && admission.source() == record.active_intent().source()
                && admission.generation() == record.active_intent().generation()
                && admission.desired_digest() == record.active_intent().desired_digest()
                && admission.provider_selection()
                    == record.active_intent().source().execution_provider_id()
                && admission.request_id() == &expected_request
                && admission.restart_epoch().as_u64() == 1
                && admission.policy_attempt_count() == 0
                && admission.not_before_unix_millis().as_u64() == RESTART_NOT_BEFORE
                && admission.source_attempt_id()
                    == record.restart_state().current_execution_attempt_id()
                && admission.attempt_id() != admission.source_attempt_id(),
            format!("{} crossed restart admission fences", case.label()),
        )?;
    }
    Ok(())
}

fn assert_candidate_identity(
    loaded: &WorkloadSagaRecord,
    candidate: &WorkloadSagaRecord,
) -> Result<(), String> {
    require(
        candidate.key() == loaded.key()
            && candidate.saga_id() == loaded.saga_id()
            && candidate.active_intent() == loaded.active_intent()
            && candidate.restart_state().completed_restart_epoch()
                == loaded.restart_state().completed_restart_epoch()
            && candidate.restart_state().current_execution_attempt_id()
                == loaded.restart_state().current_execution_attempt_id()
            && loaded.revision().checked_next() == Some(candidate.revision()),
        "restart proposal crossed desired, network, attempt, or revision truth",
    )
}

fn assert_claim(
    loaded: &WorkloadSagaRecord,
    claim: &WorkloadRestartCommandClaim,
    expected_step: WorkloadRestartStep,
) -> Result<(), String> {
    let active = loaded
        .restart_state()
        .active()
        .ok_or_else(|| "restart dispatch lost its active admission".to_owned())?;
    require(
        claim.request_id() == active.admission().request_id()
            && claim.restart_epoch() == active.admission().restart_epoch()
            && claim.attempt_id() == active.admission().attempt_id()
            && claim.step() == expected_step
            && claim.dispatch_epoch() == WorkloadRestartDispatchEpoch::new(0)
            && claim.issuing_revision() == loaded.revision()
            && matches!(
                claim.authorization(),
                WorkloadRestartDispatchAuthorization::Initial
            ),
        "restart proposal crossed command, attempt, epoch, or revision fences",
    )
}

fn assert_inspection_command(
    loaded: &WorkloadSagaRecord,
    confirmed: &WorkloadSagaRecord,
    claim: &WorkloadRestartCommandClaim,
    command: &nimbus_compute::workload_saga::ConfirmedWorkloadRestartCommand,
) -> Result<(), String> {
    let admission = loaded
        .restart_state()
        .active()
        .ok_or_else(|| "restart inspection lost its active admission".to_owned())?
        .admission();
    require(
        command.mode() == WorkloadRestartCommandMode::Inspect
            && command.command_id() == claim.command_id()
            && command.key() == loaded.key()
            && command.saga_id() == loaded.saga_id()
            && command.transition_id() == confirmed.last_transition().transition_id()
            && command.generation() == loaded.active_intent().generation()
            && command.desired_digest() == loaded.active_intent().desired_digest()
            && command.source() == loaded.active_intent().source()
            && command.source_attempt_id() == admission.source_attempt_id()
            && command.attempt_id() == admission.attempt_id()
            && command.restart_epoch() == admission.restart_epoch()
            && command.dispatch_epoch() == claim.dispatch_epoch()
            && command.request_id() == admission.request_id()
            && command.issuing_revision() == claim.issuing_revision()
            && command.confirmed_revision() == confirmed.revision()
            && command.provider_selection() == admission.provider_selection()
            && command.step() == claim.step()
            && command.claim() == claim
            && command.executable() == loaded.active_intent().executable()
            && command.compiled_network_plan() == loaded.active_intent().network().compiled_plan()
            && command.network_plan_digest() == loaded.active_intent().network().digest(),
        "fresh-process inspection crossed a desired, provider, attempt, command, or revision fence",
    )
}

fn history_for(case: RecoveryCase) -> Vec<WorkloadSagaRecord> {
    let mut history = provision_history(
        case.label(),
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let observed = history
        .last()
        .expect("published fixture must reach Observed")
        .clone();
    let admitted = admit(
        &observed,
        explicit_input(&observed, case.label(), RESTART_NOT_BEFORE),
    );
    history.push(admitted);
    if case == RecoveryCase::Requested {
        return history;
    }

    push_no_effect_advance(&mut history);
    if case == RecoveryCase::PublicationWithdrawal {
        return history;
    }
    match case {
        RecoveryCase::DispatchPending => {
            push_claim(&mut history);
            return history;
        }
        RecoveryCase::InspectionRequired => {
            let claim = push_claim(&mut history);
            let inspection = history
                .last()
                .expect("claimed restart exists")
                .restart_dispatch_to_inspection(&claim)
                .expect("exact pending restart should enter inspection");
            history.push(inspection);
            return history;
        }
        RecoveryCase::DefiniteFailure => {
            let claim = push_claim(&mut history);
            let failed = history
                .last()
                .expect("claimed restart exists")
                .apply_restart_effect_result(
                    &claim,
                    WorkloadRestartEffectResult::Failed {
                        evidence: WorkloadRestartEvidenceDigest::sha256(case.label()),
                    },
                )
                .expect("exact restart failure should persist");
            history.push(failed);
            return history;
        }
        _ => {}
    }

    push_success(&mut history, case.label());
    if case == RecoveryCase::ExecutionQuiescence {
        return history;
    }
    push_success(&mut history, case.label());
    if matches!(
        case,
        RecoveryCase::ScheduledNotDue | RecoveryCase::ScheduledDue
    ) {
        return history;
    }

    let current = history.last().expect("scheduled restart exists");
    let request_id = active_request_id(current);
    history.push(
        current
            .advance_scheduled_restart(
                &request_id,
                WorkloadRestartNotBeforeUnixMillis::new(RESTART_NOT_BEFORE),
            )
            .expect("due restart should enter preparation"),
    );
    if case == RecoveryCase::Preparation {
        return history;
    }

    for target in [
        RecoveryCase::Attachment,
        RecoveryCase::ActivationPrerequisite,
        RecoveryCase::Activation,
        RecoveryCase::Readiness,
        RecoveryCase::Publication,
        RecoveryCase::Observation,
    ] {
        push_success(&mut history, case.label());
        if case == target {
            return history;
        }
    }
    push_success(&mut history, case.label());
    debug_assert_eq!(case, RecoveryCase::Completed);
    history
}

pub(super) fn publication_withdrawal_history(label: &str) -> Vec<WorkloadSagaRecord> {
    let mut history = provision_history(
        label,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let observed = history
        .last()
        .expect("published fixture must reach Observed")
        .clone();
    history.push(admit(
        &observed,
        explicit_input(&observed, label, RESTART_NOT_BEFORE),
    ));
    push_no_effect_advance(&mut history);
    history
}

fn push_no_effect_advance(history: &mut Vec<WorkloadSagaRecord>) {
    let current = history.last().expect("active restart exists");
    let request_id = active_request_id(current);
    history.push(
        current
            .advance_restart_without_effect(&request_id)
            .expect("requested restart should advance without an effect"),
    );
}

fn push_claim(history: &mut Vec<WorkloadSagaRecord>) -> WorkloadRestartCommandClaim {
    let current = history.last().expect("active restart exists");
    let request_id = active_request_id(current);
    let claimed = current
        .claim_restart_command(&request_id)
        .expect("restart command should claim");
    let claim = active_claim(&claimed)
        .expect("claimed restart must retain its command")
        .clone();
    history.push(claimed);
    claim
}

fn push_success(history: &mut Vec<WorkloadSagaRecord>, label: &str) {
    let claim = push_claim(history);
    let succeeded = history
        .last()
        .expect("claimed restart exists")
        .apply_restart_effect_result(
            &claim,
            WorkloadRestartEffectResult::Succeeded {
                evidence: WorkloadRestartEvidenceDigest::sha256(format!(
                    "{label}:{:?}",
                    claim.step()
                )),
            },
        )
        .expect("exact restart command should succeed");
    history.push(succeeded);
}

fn active_request_id(record: &WorkloadSagaRecord) -> nimbus_workloads::WorkloadRestartRequestId {
    record
        .restart_state()
        .active()
        .expect("restart should remain active")
        .admission()
        .request_id()
        .clone()
}

fn active_claim(record: &WorkloadSagaRecord) -> Result<&WorkloadRestartCommandClaim, String> {
    record
        .restart_state()
        .active()
        .and_then(|active| active.disposition().claim())
        .ok_or_else(|| "active restart omitted its exact command claim".to_owned())
}

fn key(case: RecoveryCase) -> WorkloadSagaKey {
    WorkloadSagaKey::new(
        TenantId::new(format!("tenant-{}", case.label())).expect("fixture tenant should validate"),
        WorkloadId::new(format!("workload-{}", case.label()))
            .expect("fixture workload should validate"),
    )
}

fn require(condition: bool, message: impl Into<String>) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|error| format!("restart process runtime failed: {error}"))
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
