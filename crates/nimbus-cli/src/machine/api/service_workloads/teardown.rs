//! Guest-owned composition of one exact execution teardown phase.
//!
//! Compute owns lifecycle order and retry policy. This adapter authenticates
//! one complete forwarded command, claims the existing Container-rooted
//! provider journal, and composes the exact Systemd and Container child owners.
//! It never detaches or releases network authority and never creates another
//! journal or workload store.

use std::sync::Arc;

use nimbus::SandboxId;
use nimbus_machine::{
    MachineForwarderAuthority, MachineForwarderAuthorityMismatch,
    api::{
        MachineApiWorkloadTeardownCommandEnvelope, MachineApiWorkloadTeardownExecuteObservation,
        MachineApiWorkloadTeardownInspectObservation, MachineApiWorkloadTeardownObservation,
        MachineApiWorkloadTeardownPhaseResult, MachineApiWorkloadTeardownProviderTranslation,
    },
};
use nimbus_node::{
    HostExecutionDrainProvider, HostExecutionStopProvider, HostTeardownExecuteClaim,
    HostTeardownExecuteObservation, HostTeardownInspectClaim, HostTeardownInspectObservation,
    HostTeardownProviderClaimInput,
};
use nimbus_sandbox::{
    ProviderCommandAttemptJournal, ProviderCommandClaim, ProviderCommandClaimInput,
    ProviderCommandCurrentExecution, ProviderCommandCurrentInspection, ProviderCommandJournalError,
    ProviderCommandObservation, ProviderCommandObservationKind, ProviderCommandOperation,
    ProviderCommandStartedClaimDecision, SandboxExecutionAttemptId,
    SandboxExecutionTeardownCommand, SandboxExecutionTeardownObservation,
    SandboxExecutionTeardownOperation,
    backends::container::{
        CONTAINER_EXECUTION_TEARDOWN_PROVIDER_KEY, ContainerHostTerminalEvidence,
        ContainerSandboxBackend,
    },
};
use nimbus_workloads::{
    WorkloadFailureEvidence, WorkloadOwnerEvidenceDigest, WorkloadTeardownCommandMode,
    WorkloadTeardownDispatchAuthorization, WorkloadTeardownProviderTarget, WorkloadTeardownStep,
    WorkloadTeardownSubjects, WorkloadTeardownSuccessEvidence,
};
use serde::Serialize;

use super::{GuestNodeWorkloadService, MachineApiHttpError};

mod attachment;

const GUEST_TEARDOWN_OBSERVATION_DOMAIN: &[u8] = b"nimbus.machine.guest-teardown.observation.v1\0";
const GUEST_TEARDOWN_COMPOSITE_DOMAIN: &str = "nimbus.machine.guest-teardown.composite.v1";

struct ValidatedGuestTeardownCommand {
    provider_claim: ProviderCommandClaim,
    sandbox_command: SandboxExecutionTeardownCommand,
    host_claim: ValidatedHostClaim,
}

enum ValidatedHostClaim {
    Execute(HostTeardownExecuteClaim),
    Inspect(HostTeardownInspectClaim),
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum ChildObservationKind {
    Succeeded,
    DefiniteFailure,
    Absent,
    RetryAuthorized,
    InProgress,
    Ambiguous,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChildObservationEvidence {
    owner: &'static str,
    kind: ChildObservationKind,
    failure_code: Option<String>,
    evidence_sha256: WorkloadOwnerEvidenceDigest,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompositeObservationEvidence<'a> {
    domain: &'static str,
    command_id: String,
    step: WorkloadTeardownStep,
    provider_claim: &'a ProviderCommandClaim,
    systemd: &'a ChildObservationEvidence,
    container: Option<&'a ChildObservationEvidence>,
}

struct CompositeExecuteResult {
    kind: ProviderCommandObservationKind,
    failure_code: Option<String>,
    evidence: Vec<u8>,
}

enum CompositeInspectResult {
    Satisfied(WorkloadOwnerEvidenceDigest),
    NotCompleted(WorkloadOwnerEvidenceDigest),
    DefiniteFailure(WorkloadFailureEvidence),
    InProgress(WorkloadOwnerEvidenceDigest),
    Ambiguous,
}

pub(super) async fn dispatch(
    service: &GuestNodeWorkloadService,
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    installed_forwarder: &MachineForwarderAuthority,
) -> Result<MachineApiWorkloadTeardownPhaseResult, MachineApiHttpError> {
    if command.provider_translation()
        == MachineApiWorkloadTeardownProviderTranslation::GuestContainerAttachment
    {
        return attachment::dispatch(service, command, installed_forwarder).await;
    }
    let validated = match validate_command(service, command, installed_forwarder) {
        Ok(validated) => validated,
        Err(observation) => return phase_result(command, observation),
    };
    let journal = match service.bundle_materializer.attempt_idempotency_journal() {
        Ok(journal) => journal,
        Err(error) => return phase_result(command, journal_error(command.mode(), &error)),
    };

    let observation = match command.mode() {
        WorkloadTeardownCommandMode::Execute => execute(service, command, validated, journal).await,
        WorkloadTeardownCommandMode::Inspect => inspect(service, command, validated, journal).await,
    }?;
    phase_result(command, observation)
}

fn phase_result(
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    observation: MachineApiWorkloadTeardownObservation,
) -> Result<MachineApiWorkloadTeardownPhaseResult, MachineApiHttpError> {
    MachineApiWorkloadTeardownPhaseResult::new(command, observation, None).map_err(|error| {
        MachineApiHttpError {
            status: axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("guest workload teardown result violated its exact contract: {error}"),
        }
    })
}

fn validate_command(
    service: &GuestNodeWorkloadService,
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    installed_forwarder: &MachineForwarderAuthority,
) -> Result<ValidatedGuestTeardownCommand, MachineApiWorkloadTeardownObservation> {
    if let Err(error) = installed_forwarder.authenticate(command.machine_forwarder_authority()) {
        let code = match error {
            MachineForwarderAuthorityMismatch::ProviderInstance => {
                "machine_teardown_provider_crossed"
            }
            MachineForwarderAuthorityMismatch::Generation { .. } => {
                "machine_teardown_forwarder_stale"
            }
        };
        return Err(definite_failure(command.mode(), code, error.to_string()));
    }
    if command.provider_translation()
        != MachineApiWorkloadTeardownProviderTranslation::GuestExecutionComposition
    {
        return Err(definite_failure(
            command.mode(),
            "machine_teardown_provider_crossed",
            "forwarded teardown command does not select guest execution composition",
        ));
    }

    let attempt = command.claim().attempt();
    if command.execution_locator().node_identity() != &service.node_id
        || attempt.required_node() != &service.node_id
    {
        return Err(definite_failure(
            command.mode(),
            "machine_teardown_provider_crossed",
            "forwarded teardown command targets another guest node",
        ));
    }
    if !matches!(
        command.step(),
        WorkloadTeardownStep::DrainExecution | WorkloadTeardownStep::StopExecution
    ) {
        return Err(definite_failure(
            command.mode(),
            "sandbox_teardown_command_invalid",
            "guest execution composition supports only drain and stop",
        ));
    }
    let expected_provider =
        crate::machine::backend::provision::forwarded_machine_execution_provider_id();
    let WorkloadTeardownProviderTarget::Execution {
        provider_id,
        provider_source_digest,
    } = command.provider_target()
    else {
        return Err(definite_failure(
            command.mode(),
            "machine_teardown_provider_crossed",
            "guest execution composition requires an execution provider target",
        ));
    };
    if provider_id != &expected_provider
        || command.source().execution_provider_id() != &expected_provider
        || *provider_source_digest != command.source_digest()
    {
        return Err(definite_failure(
            command.mode(),
            "machine_teardown_provider_crossed",
            "forwarded teardown provider is crossed with the guest execution owner",
        ));
    }

    let sandbox_id = SandboxId::new(command.execution_locator().execution_id().as_str());
    let details = match service.state_view.inspect(&sandbox_id) {
        Ok(Some(details)) => details,
        Ok(None) => return Err(ambiguous(command.mode())),
        Err(_) => return Err(ambiguous(command.mode())),
    };
    if details.summary.tenant_id != *attempt.key().tenant_id()
        || details.summary.sandbox_id != sandbox_id
    {
        return Err(definite_failure(
            command.mode(),
            "sandbox_teardown_command_crossed",
            "guest Container manifest is crossed with the confirmed tenant or execution",
        ));
    }

    let provider_claim = match provider_claim(command, installed_forwarder, &service.node_id) {
        Ok(claim) => claim,
        Err(error) => return Err(journal_error(command.mode(), &error)),
    };
    let operation = match command.step() {
        WorkloadTeardownStep::DrainExecution => SandboxExecutionTeardownOperation::Drain,
        WorkloadTeardownStep::StopExecution => SandboxExecutionTeardownOperation::Stop,
        _ => unreachable!("validated guest teardown step is drain or stop"),
    };
    let execution_attempt_id = match SandboxExecutionAttemptId::new(
        command.execution_locator().attempt_id().to_string(),
    ) {
        Ok(attempt_id) => attempt_id,
        Err(error) => {
            return Err(definite_failure(
                command.mode(),
                "sandbox_teardown_command_invalid",
                error.to_string(),
            ));
        }
    };
    let sandbox_command = match SandboxExecutionTeardownCommand::new(
        attempt.key().tenant_id().clone(),
        sandbox_id,
        execution_attempt_id,
        CONTAINER_EXECUTION_TEARDOWN_PROVIDER_KEY,
        operation,
        provider_claim.clone(),
    ) {
        Ok(command) => command,
        Err(error) => {
            return Err(definite_failure(
                command.mode(),
                "sandbox_teardown_command_invalid",
                error.to_string(),
            ));
        }
    };
    let host_input = HostTeardownProviderClaimInput {
        claim: command.claim().clone(),
        command_id: command.command_id(),
        confirmed_revision: command.confirmed_revision(),
        confirmed_transition_id: command.confirmed_transition_id().clone(),
        source: command.source().clone(),
        execution: command.execution_locator().clone(),
        provider_target: command.provider_target().clone(),
        prior_receipt_prefix: command.prior_receipt_prefix().clone(),
    };
    let host_claim = match command.mode() {
        WorkloadTeardownCommandMode::Execute => {
            HostTeardownExecuteClaim::new(host_input).map(ValidatedHostClaim::Execute)
        }
        WorkloadTeardownCommandMode::Inspect => {
            HostTeardownInspectClaim::new(host_input).map(ValidatedHostClaim::Inspect)
        }
    }
    .map_err(|error| {
        definite_failure(
            command.mode(),
            "machine_teardown_order_invalid",
            error.to_string(),
        )
    })?;

    Ok(ValidatedGuestTeardownCommand {
        provider_claim,
        sandbox_command,
        host_claim,
    })
}

fn provider_claim(
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    installed_forwarder: &MachineForwarderAuthority,
    local_node: &nimbus_workloads::NodeIdentity,
) -> Result<ProviderCommandClaim, ProviderCommandJournalError> {
    execution_provider_claim_for(command, command.claim(), installed_forwarder, local_node)
}

#[cfg(test)]
pub(crate) fn provider_claim_for_test(
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    installed_forwarder: &MachineForwarderAuthority,
    local_node: &nimbus_workloads::NodeIdentity,
) -> Result<ProviderCommandClaim, ProviderCommandJournalError> {
    provider_claim(command, installed_forwarder, local_node)
}

fn execution_provider_claim_for(
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    claim: &nimbus_workloads::WorkloadTeardownClaim,
    installed_forwarder: &MachineForwarderAuthority,
    local_node: &nimbus_workloads::NodeIdentity,
) -> Result<ProviderCommandClaim, ProviderCommandJournalError> {
    let effect_subject =
        serde_json::to_string(&(command.execution_locator(), claim.attempt().subjects())).map_err(
            |error| ProviderCommandJournalError::InvalidClaim {
                message: format!("guest teardown subject cannot be encoded: {error}"),
            },
        )?;
    let provider_realm = serde_json::to_vec(&(
        claim.provider_target(),
        MachineApiWorkloadTeardownProviderTranslation::GuestExecutionComposition,
        installed_forwarder,
        command.machine_provider_generation(),
        local_node,
    ))
    .map_err(|error| ProviderCommandJournalError::InvalidClaim {
        message: format!("guest teardown provider realm cannot be encoded: {error}"),
    })?;
    ProviderCommandClaim::new(ProviderCommandClaimInput {
        authority_id: claim.attempt().saga_id().as_str().to_owned(),
        effect_subject,
        source_attempt_id: None,
        attempt_id: claim.attempt().attempt_id().as_str().to_owned(),
        dispatch_epoch: claim.dispatch_epoch().as_u64(),
        workload_generation: claim.attempt().generation().as_u64(),
        restart_ordinal: 0,
        desired_digest: claim.attempt().desired_digest().to_string(),
        source_digest: claim.attempt().source_digest().to_string(),
        network_plan_digest: claim.attempt().network_plan_digest().to_string(),
        provider_target_digest: WorkloadOwnerEvidenceDigest::sha256(provider_realm).to_string(),
        operation: match claim.attempt().step() {
            WorkloadTeardownStep::DrainExecution => ProviderCommandOperation::DrainExecution,
            WorkloadTeardownStep::StopExecution => ProviderCommandOperation::StopExecution,
            _ => {
                return Err(ProviderCommandJournalError::InvalidClaim {
                    message: "guest execution teardown claim must be drain or stop".to_owned(),
                });
            }
        },
    })
}

async fn execute(
    service: &GuestNodeWorkloadService,
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    validated: ValidatedGuestTeardownCommand,
    journal: ProviderCommandAttemptJournal,
) -> Result<MachineApiWorkloadTeardownObservation, MachineApiHttpError> {
    let provider_claim = validated.provider_claim.clone();
    let prepared = match serde_json::to_vec(command) {
        Ok(prepared) => prepared,
        Err(_) => return Ok(ambiguous(command.mode())),
    };
    let claim_decision = match command.claim().authorization() {
        WorkloadTeardownDispatchAuthorization::Initial => {
            journal.claim_dispatch_epoch_started(&validated.provider_claim, &prepared)
        }
        WorkloadTeardownDispatchAuthorization::RetryAfterNotCompleted(evidence) => {
            let encoded = match serde_json::to_vec(evidence) {
                Ok(encoded) => encoded,
                Err(_) => return Ok(ambiguous(command.mode())),
            };
            journal.claim_dispatch_epoch_after_inspected_absence_started(
                &validated.provider_claim,
                evidence.dispatch_epoch().as_u64(),
                &encoded,
                &prepared,
            )
        }
    };
    let execution = match claim_decision {
        Ok(ProviderCommandStartedClaimDecision::ExecuteStarted(execution)) => execution,
        Ok(ProviderCommandStartedClaimDecision::AdoptExactAttempt(observation)) => {
            return Ok(journal_observation(command, &observation));
        }
        Err(error) => return Ok(journal_error(command.mode(), &error)),
    };

    let host_claim = match validated.host_claim {
        ValidatedHostClaim::Execute(claim) => claim,
        ValidatedHostClaim::Inspect(_) => unreachable!("Execute mode has an execute host claim"),
    };
    let systemd_drain = Arc::clone(&service.execution_drain_provider);
    let systemd_stop = Arc::clone(&service.execution_stop_provider);
    let container = Arc::clone(&service.bundle_materializer);
    let sandbox_command = validated.sandbox_command;
    let command_id = command.command_id().to_string();
    let step = command.step();

    let result = journal
        .execute_started_claim_async(execution, move |current| {
            Box::pin(async move {
                let composite = execute_children(
                    systemd_drain,
                    systemd_stop,
                    container,
                    host_claim,
                    sandbox_command,
                    command_id,
                    step,
                    current,
                )
                .await;
                (
                    (),
                    composite.kind,
                    composite.failure_code,
                    composite.evidence,
                )
            })
        })
        .await;
    match result {
        Ok(((), observation)) => Ok(journal_observation(command, &observation)),
        Err(error) => Ok(exact_race_observation(
            command,
            &journal,
            &provider_claim,
            &error,
        )),
    }
}

#[allow(clippy::too_many_arguments)]
async fn execute_children(
    systemd_drain: Arc<dyn HostExecutionDrainProvider>,
    systemd_stop: Arc<dyn HostExecutionStopProvider>,
    container: Arc<ContainerSandboxBackend>,
    host_claim: HostTeardownExecuteClaim,
    sandbox_command: SandboxExecutionTeardownCommand,
    command_id: String,
    step: WorkloadTeardownStep,
    current: &ProviderCommandCurrentExecution,
) -> CompositeExecuteResult {
    let expected_execution = host_claim.execution().clone();
    let host = match step {
        WorkloadTeardownStep::DrainExecution => systemd_drain.execute_drain(host_claim).await,
        WorkloadTeardownStep::StopExecution => systemd_stop.execute_stop(host_claim).await,
        _ => unreachable!("validated guest teardown step is drain or stop"),
    };
    let systemd = match host_execute_evidence(&host) {
        Ok(evidence) => evidence,
        Err(error) => return encoding_ambiguous_result(error),
    };
    match host {
        HostTeardownExecuteObservation::DefiniteFailure(failure) => composite_execute_result(
            ProviderCommandObservationKind::DefiniteFailure,
            Some(failure.code().to_owned()),
            composite_evidence(&command_id, step, current.claim(), &systemd, None),
        ),
        HostTeardownExecuteObservation::Ambiguous => composite_execute_result(
            ProviderCommandObservationKind::Ambiguous,
            None,
            composite_evidence(&command_id, step, current.claim(), &systemd, None),
        ),
        HostTeardownExecuteObservation::Succeeded(success)
            if success.matches_step_and_subjects(
                step,
                &WorkloadTeardownSubjects::Execution(expected_execution),
            ) =>
        {
            let child = match step {
                WorkloadTeardownStep::DrainExecution => {
                    container.execute_execution_teardown_substep(&sandbox_command, current)
                }
                WorkloadTeardownStep::StopExecution => {
                    let encoded = match serde_json::to_vec(success.as_ref()) {
                        Ok(encoded) => encoded,
                        Err(error) => return encoding_ambiguous_result(error),
                    };
                    let host_terminal = match ContainerHostTerminalEvidence::new(
                        sandbox_command.tenant_id().clone(),
                        sandbox_command.sandbox_id().clone(),
                        sandbox_command.execution_attempt_id().clone(),
                        current.claim().clone(),
                        encoded,
                    ) {
                        Ok(evidence) => evidence,
                        Err(error) => {
                            let crossed = match child_evidence(
                                "systemd",
                                ChildObservationKind::DefiniteFailure,
                                Some("sandbox_teardown_command_crossed".to_owned()),
                                &error.to_string(),
                            ) {
                                Ok(evidence) => evidence,
                                Err(error) => return encoding_ambiguous_result(error),
                            };
                            return composite_execute_result(
                                ProviderCommandObservationKind::DefiniteFailure,
                                Some("sandbox_teardown_command_crossed".to_owned()),
                                composite_evidence(
                                    &command_id,
                                    step,
                                    current.claim(),
                                    &crossed,
                                    None,
                                ),
                            );
                        }
                    };
                    container.record_externally_stopped_execution_substep(
                        &sandbox_command,
                        current,
                        &host_terminal,
                    )
                }
                _ => unreachable!("validated guest teardown step is drain or stop"),
            };
            let container = sandbox_evidence(&child);
            let (kind, failure_code) = match &child {
                SandboxExecutionTeardownObservation::Succeeded { .. } => {
                    (ProviderCommandObservationKind::Succeeded, None)
                }
                SandboxExecutionTeardownObservation::DefiniteFailure { code, .. } => (
                    ProviderCommandObservationKind::DefiniteFailure,
                    Some(code.clone()),
                ),
                SandboxExecutionTeardownObservation::Absent { .. }
                | SandboxExecutionTeardownObservation::RetryAuthorized { .. }
                | SandboxExecutionTeardownObservation::InProgress { .. } => {
                    (ProviderCommandObservationKind::InProgress, None)
                }
                SandboxExecutionTeardownObservation::Ambiguous { .. } => {
                    (ProviderCommandObservationKind::Ambiguous, None)
                }
            };
            composite_execute_result(
                kind,
                failure_code,
                composite_evidence(
                    &command_id,
                    step,
                    current.claim(),
                    &systemd,
                    Some(&container),
                ),
            )
        }
        HostTeardownExecuteObservation::Succeeded(success) => {
            let crossed = match child_evidence(
                "systemd",
                ChildObservationKind::DefiniteFailure,
                Some("machine_teardown_provider_crossed".to_owned()),
                success.as_ref(),
            ) {
                Ok(evidence) => evidence,
                Err(error) => return encoding_ambiguous_result(error),
            };
            composite_execute_result(
                ProviderCommandObservationKind::DefiniteFailure,
                Some("machine_teardown_provider_crossed".to_owned()),
                composite_evidence(&command_id, step, current.claim(), &crossed, None),
            )
        }
    }
}

async fn inspect(
    service: &GuestNodeWorkloadService,
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    validated: ValidatedGuestTeardownCommand,
    journal: ProviderCommandAttemptJournal,
) -> Result<MachineApiWorkloadTeardownObservation, MachineApiHttpError> {
    let provider_claim = validated.provider_claim.clone();
    let current = match journal.adopt_exact_attempt(&validated.provider_claim) {
        Ok(None) => return Ok(ambiguous(command.mode())),
        Ok(Some(observation)) if terminal_observation(observation.kind()) => {
            return Ok(journal_observation(command, &observation));
        }
        Ok(Some(observation)) => observation,
        Err(error) => {
            return Ok(exact_race_observation(
                command,
                &journal,
                &provider_claim,
                &error,
            ));
        }
    };

    let host_claim = match validated.host_claim {
        ValidatedHostClaim::Inspect(claim) => claim,
        ValidatedHostClaim::Execute(_) => unreachable!("Inspect mode has an inspect host claim"),
    };
    let systemd_drain = Arc::clone(&service.execution_drain_provider);
    let systemd_stop = Arc::clone(&service.execution_stop_provider);
    let container = Arc::clone(&service.bundle_materializer);
    let sandbox_command = validated.sandbox_command;
    let command_id = command.command_id().to_string();
    let step = command.step();
    let inspected = journal
        .inspect_current_claim_async(&current, move |locked| {
            Box::pin(async move {
                inspect_children(
                    systemd_drain,
                    systemd_stop,
                    container,
                    host_claim,
                    sandbox_command,
                    command_id,
                    step,
                    locked,
                )
                .await
            })
        })
        .await;

    let result = match inspected {
        Ok(ProviderCommandCurrentInspection::EffectCanStillStart(observation)) => {
            return Ok(MachineApiWorkloadTeardownObservation::Inspect(
                MachineApiWorkloadTeardownInspectObservation::InProgress {
                    evidence: provider_evidence(&observation),
                },
            ));
        }
        Ok(ProviderCommandCurrentInspection::Inspected(result)) => result,
        Err(error) => {
            return Ok(exact_race_observation(
                command,
                &journal,
                &provider_claim,
                &error,
            ));
        }
    };
    Ok(inspect_observation(command, result))
}

#[allow(clippy::too_many_arguments)]
async fn inspect_children(
    systemd_drain: Arc<dyn HostExecutionDrainProvider>,
    systemd_stop: Arc<dyn HostExecutionStopProvider>,
    container: Arc<ContainerSandboxBackend>,
    host_claim: HostTeardownInspectClaim,
    sandbox_command: SandboxExecutionTeardownCommand,
    command_id: String,
    step: WorkloadTeardownStep,
    current: &ProviderCommandObservation,
) -> CompositeInspectResult {
    let expected_execution = host_claim.execution().clone();
    let host = match step {
        WorkloadTeardownStep::DrainExecution => systemd_drain.inspect_drain(host_claim).await,
        WorkloadTeardownStep::StopExecution => systemd_stop.inspect_stop(host_claim).await,
        _ => unreachable!("validated guest teardown step is drain or stop"),
    };
    let systemd = match host_inspect_evidence(&host) {
        Ok(evidence) => evidence,
        Err(_) => return CompositeInspectResult::Ambiguous,
    };
    let success = match host {
        HostTeardownInspectObservation::Satisfied(success)
            if success.matches_step_and_subjects(
                step,
                &WorkloadTeardownSubjects::Execution(expected_execution),
            ) =>
        {
            success
        }
        HostTeardownInspectObservation::Satisfied(success) => {
            let encoded = match serde_json::to_vec(success.as_ref()) {
                Ok(encoded) => encoded,
                Err(_) => return CompositeInspectResult::Ambiguous,
            };
            return CompositeInspectResult::DefiniteFailure(provider_failure(
                "machine_teardown_provider_crossed",
                WorkloadOwnerEvidenceDigest::sha256(encoded),
            ));
        }
        HostTeardownInspectObservation::NotCompleted(_) => {
            let evidence =
                match composite_evidence(&command_id, step, current.claim(), &systemd, None) {
                    Ok(evidence) => evidence,
                    Err(_) => return CompositeInspectResult::Ambiguous,
                };
            return CompositeInspectResult::NotCompleted(WorkloadOwnerEvidenceDigest::sha256(
                evidence,
            ));
        }
        HostTeardownInspectObservation::DefiniteFailure(failure) => {
            return CompositeInspectResult::DefiniteFailure(failure);
        }
        HostTeardownInspectObservation::InProgress(_) => {
            let evidence =
                match composite_evidence(&command_id, step, current.claim(), &systemd, None) {
                    Ok(evidence) => evidence,
                    Err(_) => return CompositeInspectResult::Ambiguous,
                };
            return CompositeInspectResult::InProgress(WorkloadOwnerEvidenceDigest::sha256(
                evidence,
            ));
        }
        HostTeardownInspectObservation::Ambiguous => {
            return CompositeInspectResult::Ambiguous;
        }
    };

    let child = match step {
        WorkloadTeardownStep::DrainExecution => {
            container.inspect_execution_teardown_substep(&sandbox_command, current)
        }
        WorkloadTeardownStep::StopExecution => {
            let encoded = match serde_json::to_vec(success.as_ref()) {
                Ok(encoded) => encoded,
                Err(_) => return CompositeInspectResult::Ambiguous,
            };
            let host_terminal = match ContainerHostTerminalEvidence::new(
                sandbox_command.tenant_id().clone(),
                sandbox_command.sandbox_id().clone(),
                sandbox_command.execution_attempt_id().clone(),
                current.claim().clone(),
                encoded,
            ) {
                Ok(evidence) => evidence,
                Err(error) => {
                    return CompositeInspectResult::DefiniteFailure(provider_failure(
                        "sandbox_teardown_command_crossed",
                        WorkloadOwnerEvidenceDigest::sha256(error.to_string()),
                    ));
                }
            };
            container.inspect_externally_stopped_execution_substep(
                &sandbox_command,
                current,
                &host_terminal,
            )
        }
        _ => unreachable!("validated guest teardown step is drain or stop"),
    };
    let container = sandbox_evidence(&child);
    let composite = match composite_evidence(
        &command_id,
        step,
        current.claim(),
        &systemd,
        Some(&container),
    ) {
        Ok(evidence) => evidence,
        Err(_) => return CompositeInspectResult::Ambiguous,
    };
    let evidence = WorkloadOwnerEvidenceDigest::sha256(composite);
    if let SandboxExecutionTeardownObservation::DefiniteFailure { code, .. } = child {
        return CompositeInspectResult::DefiniteFailure(provider_failure(code, evidence));
    }
    if matches!(child, SandboxExecutionTeardownObservation::Ambiguous { .. }) {
        return CompositeInspectResult::Ambiguous;
    }
    if matches!(
        child,
        SandboxExecutionTeardownObservation::InProgress { .. }
    ) {
        return CompositeInspectResult::InProgress(evidence);
    }
    if matches!(
        child,
        SandboxExecutionTeardownObservation::Absent { .. }
            | SandboxExecutionTeardownObservation::RetryAuthorized { .. }
    ) {
        return CompositeInspectResult::NotCompleted(evidence);
    }
    match child {
        SandboxExecutionTeardownObservation::Succeeded { .. } => {
            CompositeInspectResult::Satisfied(evidence)
        }
        _ => CompositeInspectResult::Ambiguous,
    }
}

fn terminal_observation(kind: ProviderCommandObservationKind) -> bool {
    matches!(
        kind,
        ProviderCommandObservationKind::Succeeded
            | ProviderCommandObservationKind::DefiniteFailure
            | ProviderCommandObservationKind::Absent
            | ProviderCommandObservationKind::RetryAuthorized
    )
}

fn journal_observation(
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    observation: &ProviderCommandObservation,
) -> MachineApiWorkloadTeardownObservation {
    let evidence = provider_evidence(observation);
    match command.mode() {
        WorkloadTeardownCommandMode::Execute => {
            MachineApiWorkloadTeardownObservation::Execute(match observation.kind() {
                ProviderCommandObservationKind::Succeeded => {
                    MachineApiWorkloadTeardownExecuteObservation::Succeeded {
                        evidence: Box::new(success_evidence(command, evidence)),
                    }
                }
                ProviderCommandObservationKind::DefiniteFailure => {
                    MachineApiWorkloadTeardownExecuteObservation::DefiniteFailure {
                        failure: provider_failure(
                            observation
                                .failure_code()
                                .expect("validated teardown failure has a durable code"),
                            evidence,
                        ),
                    }
                }
                ProviderCommandObservationKind::Claimed
                | ProviderCommandObservationKind::Absent
                | ProviderCommandObservationKind::RetryAuthorized
                | ProviderCommandObservationKind::InProgress
                | ProviderCommandObservationKind::Ambiguous => {
                    MachineApiWorkloadTeardownExecuteObservation::Ambiguous
                }
            })
        }
        WorkloadTeardownCommandMode::Inspect => {
            MachineApiWorkloadTeardownObservation::Inspect(match observation.kind() {
                ProviderCommandObservationKind::Succeeded => {
                    MachineApiWorkloadTeardownInspectObservation::Satisfied {
                        evidence: Box::new(success_evidence(command, evidence)),
                    }
                }
                ProviderCommandObservationKind::DefiniteFailure => {
                    MachineApiWorkloadTeardownInspectObservation::DefiniteFailure {
                        failure: provider_failure(
                            observation
                                .failure_code()
                                .expect("validated teardown failure has a durable code"),
                            evidence,
                        ),
                    }
                }
                ProviderCommandObservationKind::Absent
                | ProviderCommandObservationKind::RetryAuthorized => {
                    MachineApiWorkloadTeardownInspectObservation::NotCompleted { evidence }
                }
                ProviderCommandObservationKind::Claimed
                | ProviderCommandObservationKind::InProgress => {
                    MachineApiWorkloadTeardownInspectObservation::InProgress { evidence }
                }
                ProviderCommandObservationKind::Ambiguous => {
                    MachineApiWorkloadTeardownInspectObservation::Ambiguous
                }
            })
        }
    }
}

fn inspect_observation(
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    result: CompositeInspectResult,
) -> MachineApiWorkloadTeardownObservation {
    MachineApiWorkloadTeardownObservation::Inspect(match result {
        CompositeInspectResult::Satisfied(evidence) => {
            MachineApiWorkloadTeardownInspectObservation::Satisfied {
                evidence: Box::new(success_evidence(command, evidence)),
            }
        }
        CompositeInspectResult::NotCompleted(evidence) => {
            MachineApiWorkloadTeardownInspectObservation::NotCompleted { evidence }
        }
        CompositeInspectResult::DefiniteFailure(failure) => {
            MachineApiWorkloadTeardownInspectObservation::DefiniteFailure { failure }
        }
        CompositeInspectResult::InProgress(evidence) => {
            MachineApiWorkloadTeardownInspectObservation::InProgress { evidence }
        }
        CompositeInspectResult::Ambiguous => {
            MachineApiWorkloadTeardownInspectObservation::Ambiguous
        }
    })
}

fn success_evidence(
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    evidence: WorkloadOwnerEvidenceDigest,
) -> WorkloadTeardownSuccessEvidence {
    success_evidence_for(command.step(), command.subjects(), evidence)
        .expect("validated guest teardown command has matching typed subjects")
}

fn success_evidence_for(
    step: WorkloadTeardownStep,
    subjects: &WorkloadTeardownSubjects,
    evidence: WorkloadOwnerEvidenceDigest,
) -> Option<WorkloadTeardownSuccessEvidence> {
    match (step, subjects) {
        (WorkloadTeardownStep::DrainExecution, WorkloadTeardownSubjects::Execution(reference)) => {
            Some(WorkloadTeardownSuccessEvidence::ExecutionDrained {
                reference: reference.clone(),
                evidence,
            })
        }
        (WorkloadTeardownStep::StopExecution, WorkloadTeardownSubjects::Execution(reference)) => {
            Some(WorkloadTeardownSuccessEvidence::ExecutionStopped {
                reference: reference.clone(),
                evidence,
            })
        }
        (WorkloadTeardownStep::DetachNetwork, WorkloadTeardownSubjects::Network(reference)) => {
            Some(WorkloadTeardownSuccessEvidence::NetworkDetached {
                reference: reference.clone(),
                evidence,
            })
        }
        (WorkloadTeardownStep::ReleaseNetwork, WorkloadTeardownSubjects::Network(reference)) => {
            Some(WorkloadTeardownSuccessEvidence::NetworkReleased {
                reference: reference.clone(),
                evidence,
            })
        }
        _ => None,
    }
}

fn host_execute_evidence(
    observation: &HostTeardownExecuteObservation,
) -> serde_json::Result<ChildObservationEvidence> {
    match observation {
        HostTeardownExecuteObservation::Succeeded(evidence) => child_evidence(
            "systemd",
            ChildObservationKind::Succeeded,
            None,
            evidence.as_ref(),
        ),
        HostTeardownExecuteObservation::DefiniteFailure(failure) => child_evidence(
            "systemd",
            ChildObservationKind::DefiniteFailure,
            Some(failure.code().to_owned()),
            failure,
        ),
        HostTeardownExecuteObservation::Ambiguous => child_evidence(
            "systemd",
            ChildObservationKind::Ambiguous,
            None,
            &"systemd execution state is ambiguous",
        ),
    }
}

fn host_inspect_evidence(
    observation: &HostTeardownInspectObservation,
) -> serde_json::Result<ChildObservationEvidence> {
    match observation {
        HostTeardownInspectObservation::Satisfied(evidence) => child_evidence(
            "systemd",
            ChildObservationKind::Succeeded,
            None,
            evidence.as_ref(),
        ),
        HostTeardownInspectObservation::NotCompleted(evidence) => {
            child_evidence("systemd", ChildObservationKind::Absent, None, evidence)
        }
        HostTeardownInspectObservation::DefiniteFailure(failure) => child_evidence(
            "systemd",
            ChildObservationKind::DefiniteFailure,
            Some(failure.code().to_owned()),
            failure,
        ),
        HostTeardownInspectObservation::InProgress(evidence) => {
            child_evidence("systemd", ChildObservationKind::InProgress, None, evidence)
        }
        HostTeardownInspectObservation::Ambiguous => child_evidence(
            "systemd",
            ChildObservationKind::Ambiguous,
            None,
            &"systemd inspection state is ambiguous",
        ),
    }
}

fn sandbox_evidence(observation: &SandboxExecutionTeardownObservation) -> ChildObservationEvidence {
    let (kind, failure_code) = match observation {
        SandboxExecutionTeardownObservation::Succeeded { .. } => {
            (ChildObservationKind::Succeeded, None)
        }
        SandboxExecutionTeardownObservation::DefiniteFailure { code, .. } => {
            (ChildObservationKind::DefiniteFailure, Some(code.clone()))
        }
        SandboxExecutionTeardownObservation::Absent { .. } => (ChildObservationKind::Absent, None),
        SandboxExecutionTeardownObservation::RetryAuthorized { .. } => {
            (ChildObservationKind::RetryAuthorized, None)
        }
        SandboxExecutionTeardownObservation::InProgress { .. } => {
            (ChildObservationKind::InProgress, None)
        }
        SandboxExecutionTeardownObservation::Ambiguous { .. } => {
            (ChildObservationKind::Ambiguous, None)
        }
    };
    ChildObservationEvidence {
        owner: "container",
        kind,
        failure_code,
        evidence_sha256: WorkloadOwnerEvidenceDigest::sha256(observation.evidence()),
    }
}

fn child_evidence(
    owner: &'static str,
    kind: ChildObservationKind,
    failure_code: Option<String>,
    evidence: &impl Serialize,
) -> serde_json::Result<ChildObservationEvidence> {
    let encoded = serde_json::to_vec(evidence)?;
    Ok(ChildObservationEvidence {
        owner,
        kind,
        failure_code,
        evidence_sha256: WorkloadOwnerEvidenceDigest::sha256(encoded),
    })
}

fn composite_evidence(
    command_id: &str,
    step: WorkloadTeardownStep,
    provider_claim: &ProviderCommandClaim,
    systemd: &ChildObservationEvidence,
    container: Option<&ChildObservationEvidence>,
) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec(&CompositeObservationEvidence {
        domain: GUEST_TEARDOWN_COMPOSITE_DOMAIN,
        command_id: command_id.to_owned(),
        step,
        provider_claim,
        systemd,
        container,
    })
}

fn composite_execute_result(
    kind: ProviderCommandObservationKind,
    failure_code: Option<String>,
    evidence: serde_json::Result<Vec<u8>>,
) -> CompositeExecuteResult {
    match evidence {
        Ok(evidence) => CompositeExecuteResult {
            kind,
            failure_code,
            evidence,
        },
        Err(error) => encoding_ambiguous_result(error),
    }
}

fn encoding_ambiguous_result(error: serde_json::Error) -> CompositeExecuteResult {
    CompositeExecuteResult {
        kind: ProviderCommandObservationKind::Ambiguous,
        failure_code: None,
        evidence: format!("guest teardown evidence encoding is ambiguous: {error}").into_bytes(),
    }
}

fn provider_evidence(observation: &ProviderCommandObservation) -> WorkloadOwnerEvidenceDigest {
    exact_provider_evidence(observation).unwrap_or_else(|| {
        WorkloadOwnerEvidenceDigest::sha256(
            [
                GUEST_TEARDOWN_OBSERVATION_DOMAIN,
                b"provider_claimed_without_outcome_evidence",
            ]
            .concat(),
        )
    })
}

fn exact_provider_evidence(
    observation: &ProviderCommandObservation,
) -> Option<WorkloadOwnerEvidenceDigest> {
    let durable = observation.evidence_sha256()?;
    Some(WorkloadOwnerEvidenceDigest::sha256(
        [GUEST_TEARDOWN_OBSERVATION_DOMAIN, durable.as_bytes()].concat(),
    ))
}

fn provider_failure(
    code: impl Into<String>,
    evidence: WorkloadOwnerEvidenceDigest,
) -> WorkloadFailureEvidence {
    WorkloadFailureEvidence::new(code, evidence)
        .expect("guest teardown failure codes are fixed valid identifiers")
}

fn definite_failure(
    mode: WorkloadTeardownCommandMode,
    code: impl Into<String>,
    message: impl AsRef<str>,
) -> MachineApiWorkloadTeardownObservation {
    let failure = provider_failure(code, WorkloadOwnerEvidenceDigest::sha256(message.as_ref()));
    match mode {
        WorkloadTeardownCommandMode::Execute => MachineApiWorkloadTeardownObservation::Execute(
            MachineApiWorkloadTeardownExecuteObservation::DefiniteFailure { failure },
        ),
        WorkloadTeardownCommandMode::Inspect => MachineApiWorkloadTeardownObservation::Inspect(
            MachineApiWorkloadTeardownInspectObservation::DefiniteFailure { failure },
        ),
    }
}

fn ambiguous(mode: WorkloadTeardownCommandMode) -> MachineApiWorkloadTeardownObservation {
    match mode {
        WorkloadTeardownCommandMode::Execute => MachineApiWorkloadTeardownObservation::Execute(
            MachineApiWorkloadTeardownExecuteObservation::Ambiguous,
        ),
        WorkloadTeardownCommandMode::Inspect => MachineApiWorkloadTeardownObservation::Inspect(
            MachineApiWorkloadTeardownInspectObservation::Ambiguous,
        ),
    }
}

fn journal_error(
    mode: WorkloadTeardownCommandMode,
    error: &ProviderCommandJournalError,
) -> MachineApiWorkloadTeardownObservation {
    let code = match error {
        ProviderCommandJournalError::InvalidClaim { .. } => {
            Some("sandbox_teardown_command_invalid")
        }
        ProviderCommandJournalError::StaleWorkloadGeneration { .. }
        | ProviderCommandJournalError::StaleRestartOrdinal { .. }
        | ProviderCommandJournalError::StaleDispatchEpoch { .. } => {
            Some("sandbox_teardown_command_stale")
        }
        ProviderCommandJournalError::SkippedRestartOrdinal { .. }
        | ProviderCommandJournalError::SkippedDispatchEpoch { .. }
        | ProviderCommandJournalError::CrossedClaim
        | ProviderCommandJournalError::RetryWithoutAuthority
        | ProviderCommandJournalError::PriorEffectUnresolved => {
            Some("sandbox_teardown_epoch_invalid")
        }
        ProviderCommandJournalError::Corrupt { .. } | ProviderCommandJournalError::Store { .. } => {
            None
        }
    };
    code.map_or_else(
        || ambiguous(mode),
        |code| definite_failure(mode, code, error.to_string()),
    )
}

fn exact_race_observation(
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    journal: &ProviderCommandAttemptJournal,
    claim: &ProviderCommandClaim,
    error: &ProviderCommandJournalError,
) -> MachineApiWorkloadTeardownObservation {
    if error != &ProviderCommandJournalError::PriorEffectUnresolved {
        return journal_error(command.mode(), error);
    }
    match journal.adopt_exact_attempt(claim) {
        Ok(Some(observation)) if observation.kind() != ProviderCommandObservationKind::Claimed => {
            journal_observation(command, &observation)
        }
        Ok(Some(_)) | Ok(None) | Err(_) => ambiguous(command.mode()),
    }
}

#[cfg(test)]
#[path = "teardown/tests.rs"]
pub(in crate::machine::api) mod tests;
