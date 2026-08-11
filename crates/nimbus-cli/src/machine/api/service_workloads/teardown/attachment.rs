//! Guest-owned composition of exact forwarded Container attachment teardown.
//!
//! Compute owns phase order and retry policy. This sink authenticates the
//! current portable claim, the exact preceding receipt, and the corresponding
//! durable guest journal result before it creates any current provider state.
//! The Container-rooted journal remains the only generic result authority.

use nimbus_machine::{
    MachineForwarderAuthority, MachineForwarderAuthorityMismatch,
    api::{
        MachineApiNetworkReleaseAbsenceEvidence, MachineApiWorkloadTeardownCommandEnvelope,
        MachineApiWorkloadTeardownExecuteObservation, MachineApiWorkloadTeardownInspectObservation,
        MachineApiWorkloadTeardownObservation, MachineApiWorkloadTeardownPhaseResult,
        MachineApiWorkloadTeardownProviderTranslation,
    },
};
use nimbus_network::NetworkCapabilitySourceDigest;
use nimbus_sandbox::{
    ProviderCommandAttemptJournal, ProviderCommandClaim, ProviderCommandClaimInput,
    ProviderCommandCurrentInspection, ProviderCommandObservation, ProviderCommandObservationKind,
    ProviderCommandStartedClaimDecision, SandboxExecutionAttemptId, SandboxId,
    SandboxNetworkTeardownCommand, SandboxNetworkTeardownCommandInput,
    SandboxNetworkTeardownIdentity, SandboxNetworkTeardownIdentityInput,
    SandboxNetworkTeardownObservation, SandboxNetworkTeardownOperation,
    backends::{
        CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY, container::OciMachinePortForwarderConfig,
    },
};
use nimbus_workloads::{
    NodeIdentity, WorkloadOwnerEvidenceDigest, WorkloadTeardownClaim, WorkloadTeardownCommandMode,
    WorkloadTeardownDispatchAuthorization, WorkloadTeardownProviderTarget, WorkloadTeardownReceipt,
    WorkloadTeardownStep, WorkloadTeardownSubjects,
};

use super::{
    GuestNodeWorkloadService, MachineApiHttpError, ambiguous, definite_failure,
    exact_provider_evidence, exact_race_observation, execution_provider_claim_for, journal_error,
    journal_observation, provider_evidence, provider_failure, success_evidence_for,
    terminal_observation,
};

struct ValidatedGuestAttachmentCommand {
    provider_claim: ProviderCommandClaim,
    sandbox_command: SandboxNetworkTeardownCommand,
    prior_observation: ProviderCommandObservation,
    expected_forwarder: OciMachinePortForwarderConfig,
}

pub(super) async fn dispatch(
    service: &GuestNodeWorkloadService,
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    installed_forwarder: &MachineForwarderAuthority,
) -> Result<MachineApiWorkloadTeardownPhaseResult, MachineApiHttpError> {
    let expected_forwarder = match validate_forwarder(command, installed_forwarder) {
        Ok(forwarder) => forwarder,
        Err(observation) => return phase_result(command, observation, None),
    };
    let current = match lower_attachment_command(command, command.claim(), &service.node_id) {
        Ok(current) => current,
        Err(observation) => return phase_result(command, observation, None),
    };
    let journal = match service.bundle_materializer.attempt_idempotency_journal() {
        Ok(journal) => journal,
        Err(error) => return phase_result(command, journal_error(command.mode(), &error), None),
    };
    let prior_observation = match authenticate_prior_success(
        command,
        installed_forwarder,
        &service.node_id,
        &journal,
    ) {
        Ok(observation) => observation,
        Err(observation) => return phase_result(command, observation, None),
    };
    if let Err(observation) = service
        .bundle_materializer
        .preflight_forwarded_network_teardown_substep(
            &current,
            &prior_observation,
            &expected_forwarder,
        )
    {
        return phase_result(
            command,
            preflight_failure(command.mode(), observation),
            None,
        );
    }

    let validated = ValidatedGuestAttachmentCommand {
        provider_claim: current.provider_claim().clone(),
        sandbox_command: current,
        prior_observation,
        expected_forwarder,
    };
    let observation = match command.mode() {
        WorkloadTeardownCommandMode::Execute => {
            execute(service, command, &validated, journal).await
        }
        WorkloadTeardownCommandMode::Inspect => {
            inspect(service, command, &validated, journal).await
        }
    };
    let release_absence = release_absence_evidence(service, command, &validated, &observation);
    match release_absence {
        Ok(evidence) => phase_result(command, observation, evidence),
        Err(()) => phase_result(command, ambiguous(command.mode()), None),
    }
}

fn validate_forwarder(
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    installed_forwarder: &MachineForwarderAuthority,
) -> Result<OciMachinePortForwarderConfig, MachineApiWorkloadTeardownObservation> {
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
    if command.machine_provider_generation() != installed_forwarder.generation() {
        return Err(definite_failure(
            command.mode(),
            "machine_teardown_forwarder_stale",
            "forwarded attachment command has a stale machine provider generation",
        ));
    }
    let forwarder = OciMachinePortForwarderConfig::gvproxy_for_provider_instance(
        installed_forwarder.provider_instance().expose_to_provider(),
        installed_forwarder.generation(),
    )
    .map_err(|error| {
        definite_failure(
            command.mode(),
            "machine_teardown_provider_crossed",
            error.to_string(),
        )
    })?;
    if forwarder.provider_instance() != installed_forwarder.provider_instance() {
        return Err(definite_failure(
            command.mode(),
            "machine_teardown_provider_crossed",
            "installed machine forwarder does not belong to the gvproxy provider",
        ));
    }
    Ok(forwarder)
}

fn authenticate_prior_success(
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    installed_forwarder: &MachineForwarderAuthority,
    local_node: &NodeIdentity,
    journal: &ProviderCommandAttemptJournal,
) -> Result<ProviderCommandObservation, MachineApiWorkloadTeardownObservation> {
    let prior_step = match command.step() {
        WorkloadTeardownStep::DetachNetwork => WorkloadTeardownStep::StopExecution,
        WorkloadTeardownStep::ReleaseNetwork => WorkloadTeardownStep::DetachNetwork,
        _ => {
            return Err(definite_failure(
                command.mode(),
                "sandbox_teardown_command_invalid",
                "guest attachment teardown supports only detach and release",
            ));
        }
    };
    let receipt = command
        .prior_receipt_prefix()
        .receipt_for(prior_step)
        .ok_or_else(|| {
            definite_failure(
                command.mode(),
                "machine_teardown_order_invalid",
                "forwarded attachment teardown lacks its exact preceding receipt",
            )
        })?;
    if !receipt
        .evidence()
        .matches_step_and_subjects(prior_step, receipt.claim().attempt().subjects())
    {
        return Err(definite_failure(
            command.mode(),
            "machine_teardown_order_invalid",
            "forwarded attachment receipt is crossed with its portable claim",
        ));
    }

    let prior_claim = match prior_step {
        WorkloadTeardownStep::StopExecution => {
            validate_prior_execution_claim(command, receipt.claim(), local_node)?;
            execution_provider_claim_for(command, receipt.claim(), installed_forwarder, local_node)
                .map_err(|error| journal_error(command.mode(), &error))?
        }
        WorkloadTeardownStep::DetachNetwork => {
            lower_attachment_command(command, receipt.claim(), local_node)?
                .provider_claim()
                .clone()
        }
        _ => unreachable!("the preceding guest attachment step is closed"),
    };
    require_prior_journal_success(command, receipt, &prior_claim, journal)
}

fn validate_prior_execution_claim(
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    claim: &WorkloadTeardownClaim,
    local_node: &NodeIdentity,
) -> Result<(), MachineApiWorkloadTeardownObservation> {
    validate_same_lifecycle(command, claim, local_node)?;
    let WorkloadTeardownSubjects::Execution(reference) = claim.attempt().subjects() else {
        return Err(definite_failure(
            command.mode(),
            "machine_teardown_order_invalid",
            "prior stop receipt lacks the exact execution subject",
        ));
    };
    let expected_provider =
        crate::machine::backend::provision::forwarded_machine_execution_provider_id();
    let WorkloadTeardownProviderTarget::Execution {
        provider_id,
        provider_source_digest,
    } = claim.provider_target()
    else {
        return Err(definite_failure(
            command.mode(),
            "machine_teardown_provider_crossed",
            "prior stop receipt lacks the guest execution provider target",
        ));
    };
    if claim.attempt().step() != WorkloadTeardownStep::StopExecution
        || reference != command.execution_locator()
        || provider_id != &expected_provider
        || claim.attempt().execution_provider_id() != &expected_provider
        || *provider_source_digest != claim.attempt().source_digest()
    {
        return Err(definite_failure(
            command.mode(),
            "machine_teardown_provider_crossed",
            "prior stop receipt is crossed with the guest execution owner",
        ));
    }
    Ok(())
}

fn lower_attachment_command(
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    claim: &WorkloadTeardownClaim,
    local_node: &NodeIdentity,
) -> Result<SandboxNetworkTeardownCommand, MachineApiWorkloadTeardownObservation> {
    validate_same_lifecycle(command, claim, local_node)?;
    let operation = match claim.attempt().step() {
        WorkloadTeardownStep::DetachNetwork => SandboxNetworkTeardownOperation::Detach,
        WorkloadTeardownStep::ReleaseNetwork => SandboxNetworkTeardownOperation::Release,
        _ => {
            return Err(definite_failure(
                command.mode(),
                "sandbox_teardown_command_invalid",
                "guest attachment teardown supports only detach and release",
            ));
        }
    };
    let WorkloadTeardownSubjects::Network(reference) = claim.attempt().subjects() else {
        return Err(definite_failure(
            command.mode(),
            "sandbox_teardown_command_invalid",
            "guest attachment teardown requires a network subject",
        ));
    };
    let plan = command.compiled_network_plan().plan();
    if reference.plan_id() != plan.plan_id()
        || reference.generation() != plan.generation()
        || reference.digest() != plan.digest()
    {
        return Err(definite_failure(
            command.mode(),
            "sandbox_teardown_command_crossed",
            "guest attachment subject is crossed with the compiled network plan",
        ));
    }
    let provider_source_digest = attachment_provider_source_digest(command, claim)?;
    let attachment_id = command
        .compiled_network_plan()
        .content()
        .attachment()
        .ok_or_else(|| {
            definite_failure(
                command.mode(),
                "sandbox_teardown_command_invalid",
                "guest attachment teardown requires a compiled attachment",
            )
        })?
        .attachment_id()
        .clone();
    let execution_attempt_id =
        SandboxExecutionAttemptId::new(command.execution_locator().attempt_id().to_string())
            .map_err(|error| {
                definite_failure(
                    command.mode(),
                    "sandbox_teardown_identity_invalid",
                    error.to_string(),
                )
            })?;
    let identity = SandboxNetworkTeardownIdentity::new(SandboxNetworkTeardownIdentityInput {
        tenant_id: claim.attempt().key().tenant_id().clone(),
        sandbox_id: SandboxId::new(command.execution_locator().execution_id().as_str()),
        execution_attempt_id,
        attachment_id,
        network_plan: plan.clone(),
        provider_registration_key: CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY.to_owned(),
        provider_source_digest,
    })
    .map_err(|error| {
        definite_failure(
            command.mode(),
            "sandbox_teardown_command_invalid",
            error.to_string(),
        )
    })?;
    let provider_claim = ProviderCommandClaim::new(ProviderCommandClaimInput {
        authority_id: claim.attempt().saga_id().as_str().to_owned(),
        effect_subject: identity.provider_effect_subject(),
        source_attempt_id: None,
        attempt_id: claim.attempt().attempt_id().as_str().to_owned(),
        dispatch_epoch: claim.dispatch_epoch().as_u64(),
        workload_generation: claim.attempt().generation().as_u64(),
        restart_ordinal: 0,
        desired_digest: claim.attempt().desired_digest().to_string(),
        source_digest: claim.attempt().source_digest().to_string(),
        network_plan_digest: claim.attempt().network_plan_digest().to_string(),
        provider_target_digest: identity.provider_target_digest(),
        operation: operation.provider_operation(),
    })
    .map_err(|error| journal_error(command.mode(), &error))?;
    SandboxNetworkTeardownCommand::new(SandboxNetworkTeardownCommandInput {
        identity,
        operation,
        provider_claim,
    })
    .map_err(|error| {
        definite_failure(
            command.mode(),
            "sandbox_teardown_command_crossed",
            error.to_string(),
        )
    })
}

fn validate_same_lifecycle(
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    claim: &WorkloadTeardownClaim,
    local_node: &NodeIdentity,
) -> Result<(), MachineApiWorkloadTeardownObservation> {
    let current = command.claim().attempt();
    let candidate = claim.attempt();
    if command.provider_translation()
        != MachineApiWorkloadTeardownProviderTranslation::GuestContainerAttachment
        || command.execution_locator().node_identity() != local_node
        || current.required_node() != local_node
        || candidate.required_node() != local_node
    {
        return Err(definite_failure(
            command.mode(),
            "machine_teardown_provider_crossed",
            "forwarded attachment teardown targets another guest composition or node",
        ));
    }
    if candidate.key() != current.key()
        || candidate.saga_id() != current.saga_id()
        || candidate.generation() != current.generation()
        || candidate.desired_digest() != current.desired_digest()
        || candidate.source_digest() != current.source_digest()
        || candidate.execution_provider_id() != current.execution_provider_id()
        || candidate.network_plan_digest() != current.network_plan_digest()
        || candidate.selection_evidence() != current.selection_evidence()
    {
        return Err(definite_failure(
            command.mode(),
            "sandbox_teardown_command_crossed",
            "forwarded attachment teardown crosses the retained workload lifecycle",
        ));
    }
    Ok(())
}

fn attachment_provider_source_digest(
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    claim: &WorkloadTeardownClaim,
) -> Result<NetworkCapabilitySourceDigest, MachineApiWorkloadTeardownObservation> {
    let expected_provider =
        crate::machine::backend::provision::forwarded_machine_attachment_provider_id();
    let WorkloadTeardownProviderTarget::Attachment {
        provider_id,
        provider_source_digest,
    } = claim.provider_target()
    else {
        return Err(definite_failure(
            command.mode(),
            "machine_teardown_provider_crossed",
            "guest attachment teardown requires the parent attachment provider target",
        ));
    };
    let selection = claim.attempt().selection_evidence().ok_or_else(|| {
        definite_failure(
            command.mode(),
            "sandbox_teardown_command_invalid",
            "guest attachment teardown lacks capability selection evidence",
        )
    })?;
    if provider_id != &expected_provider
        || command.source().attachment_provider_id() != &expected_provider
        || selection.selection().attachment_provider_id() != &expected_provider
        || selection.source_digest() != *provider_source_digest
    {
        return Err(definite_failure(
            command.mode(),
            "machine_teardown_provider_crossed",
            "forwarded attachment provider or source evidence is crossed",
        ));
    }
    Ok(*provider_source_digest)
}

fn require_prior_journal_success(
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    receipt: &WorkloadTeardownReceipt,
    expected_claim: &ProviderCommandClaim,
    journal: &ProviderCommandAttemptJournal,
) -> Result<ProviderCommandObservation, MachineApiWorkloadTeardownObservation> {
    let observation = match journal.adopt_exact_attempt(expected_claim) {
        Ok(Some(observation)) => observation,
        Ok(None) => return Err(ambiguous(command.mode())),
        Err(error) => return Err(journal_error(command.mode(), &error)),
    };
    match observation.kind() {
        ProviderCommandObservationKind::Succeeded => {}
        ProviderCommandObservationKind::Claimed
        | ProviderCommandObservationKind::InProgress
        | ProviderCommandObservationKind::Ambiguous => return Err(ambiguous(command.mode())),
        ProviderCommandObservationKind::DefiniteFailure
        | ProviderCommandObservationKind::Absent
        | ProviderCommandObservationKind::RetryAuthorized => {
            return Err(definite_failure(
                command.mode(),
                "sandbox_teardown_order_invalid",
                "preceding guest teardown phase has no exact durable success",
            ));
        }
    }
    let evidence =
        exact_provider_evidence(&observation).ok_or_else(|| ambiguous(command.mode()))?;
    let expected = success_evidence_for(
        receipt.claim().attempt().step(),
        receipt.claim().attempt().subjects(),
        evidence,
    )
    .ok_or_else(|| {
        definite_failure(
            command.mode(),
            "machine_teardown_order_invalid",
            "preceding receipt has an unsupported success-evidence shape",
        )
    })?;
    if &expected != receipt.evidence() {
        return Err(definite_failure(
            command.mode(),
            "sandbox_teardown_order_invalid",
            "preceding receipt is crossed with the exact durable guest result",
        ));
    }
    Ok(observation)
}

async fn execute(
    service: &GuestNodeWorkloadService,
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    validated: &ValidatedGuestAttachmentCommand,
    journal: ProviderCommandAttemptJournal,
) -> MachineApiWorkloadTeardownObservation {
    let provider_claim = validated.provider_claim.clone();
    let prepared = match serde_json::to_vec(command) {
        Ok(prepared) => prepared,
        Err(_) => return ambiguous(command.mode()),
    };
    let decision = match command.claim().authorization() {
        WorkloadTeardownDispatchAuthorization::Initial => {
            journal.claim_dispatch_epoch_started(&validated.provider_claim, &prepared)
        }
        WorkloadTeardownDispatchAuthorization::RetryAfterNotCompleted(evidence) => {
            let encoded = match serde_json::to_vec(evidence) {
                Ok(encoded) => encoded,
                Err(_) => return ambiguous(command.mode()),
            };
            journal.claim_dispatch_epoch_after_inspected_absence_started(
                &validated.provider_claim,
                evidence.dispatch_epoch().as_u64(),
                &encoded,
                &prepared,
            )
        }
    };
    let execution = match decision {
        Ok(ProviderCommandStartedClaimDecision::ExecuteStarted(execution)) => execution,
        Ok(ProviderCommandStartedClaimDecision::AdoptExactAttempt(observation)) => {
            return journal_observation(command, &observation);
        }
        Err(error) => return journal_error(command.mode(), &error),
    };
    let container = std::sync::Arc::clone(&service.bundle_materializer);
    let sandbox_command = validated.sandbox_command.clone();
    let prior_observation = validated.prior_observation.clone();
    let expected_forwarder = validated.expected_forwarder.clone();
    let result = journal
        .execute_started_claim_async(execution, move |current| {
            Box::pin(async move {
                let observation = container.execute_forwarded_network_teardown_substep(
                    &sandbox_command,
                    current,
                    &prior_observation,
                    &expected_forwarder,
                );
                let kind = execute_observation_kind(&observation);
                let failure_code = observation.failure_code().map(str::to_owned);
                let evidence = observation.evidence().to_vec();
                ((), kind, failure_code, evidence)
            })
        })
        .await;
    match result {
        Ok(((), observation)) => journal_observation(command, &observation),
        Err(error) => exact_race_observation(command, &journal, &provider_claim, &error),
    }
}

async fn inspect(
    service: &GuestNodeWorkloadService,
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    validated: &ValidatedGuestAttachmentCommand,
    journal: ProviderCommandAttemptJournal,
) -> MachineApiWorkloadTeardownObservation {
    let provider_claim = validated.provider_claim.clone();
    let current = match journal.adopt_exact_attempt(&validated.provider_claim) {
        Ok(None) => return ambiguous(command.mode()),
        Ok(Some(observation)) if terminal_observation(observation.kind()) => {
            return journal_observation(command, &observation);
        }
        Ok(Some(observation)) => observation,
        Err(error) => {
            return exact_race_observation(command, &journal, &provider_claim, &error);
        }
    };
    let container = std::sync::Arc::clone(&service.bundle_materializer);
    let sandbox_command = validated.sandbox_command.clone();
    let prior_observation = validated.prior_observation.clone();
    let expected_forwarder = validated.expected_forwarder.clone();
    let inspected = journal
        .inspect_current_claim_async(&current, move |locked| {
            Box::pin(async move {
                container.inspect_forwarded_network_teardown_substep(
                    &sandbox_command,
                    locked,
                    &prior_observation,
                    &expected_forwarder,
                )
            })
        })
        .await;
    match inspected {
        Ok(ProviderCommandCurrentInspection::EffectCanStillStart(observation)) => {
            effect_can_still_start_observation(&observation)
        }
        Ok(ProviderCommandCurrentInspection::Inspected(observation)) => {
            inspect_observation(command, observation)
        }
        Err(error) => exact_race_observation(command, &journal, &provider_claim, &error),
    }
}

fn effect_can_still_start_observation(
    observation: &ProviderCommandObservation,
) -> MachineApiWorkloadTeardownObservation {
    MachineApiWorkloadTeardownObservation::Inspect(
        MachineApiWorkloadTeardownInspectObservation::InProgress {
            evidence: provider_evidence(observation),
        },
    )
}

fn execute_observation_kind(
    observation: &SandboxNetworkTeardownObservation,
) -> ProviderCommandObservationKind {
    match observation {
        SandboxNetworkTeardownObservation::Succeeded { .. } => {
            ProviderCommandObservationKind::Succeeded
        }
        SandboxNetworkTeardownObservation::DefiniteFailure { .. } => {
            ProviderCommandObservationKind::DefiniteFailure
        }
        SandboxNetworkTeardownObservation::Absent { .. }
        | SandboxNetworkTeardownObservation::RetryAuthorized { .. }
        | SandboxNetworkTeardownObservation::InProgress { .. } => {
            ProviderCommandObservationKind::InProgress
        }
        SandboxNetworkTeardownObservation::Ambiguous { .. } => {
            ProviderCommandObservationKind::Ambiguous
        }
    }
}

fn inspect_observation(
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    observation: SandboxNetworkTeardownObservation,
) -> MachineApiWorkloadTeardownObservation {
    let evidence = WorkloadOwnerEvidenceDigest::sha256(observation.evidence());
    MachineApiWorkloadTeardownObservation::Inspect(match observation {
        SandboxNetworkTeardownObservation::Succeeded { .. } => {
            let Some(success) = success_evidence_for(command.step(), command.subjects(), evidence)
            else {
                return definite_failure(
                    command.mode(),
                    "sandbox_teardown_command_invalid",
                    "guest attachment success has crossed typed subjects",
                );
            };
            MachineApiWorkloadTeardownInspectObservation::Satisfied {
                evidence: Box::new(success),
            }
        }
        SandboxNetworkTeardownObservation::DefiniteFailure { code, .. } => {
            MachineApiWorkloadTeardownInspectObservation::DefiniteFailure {
                failure: provider_failure(code, evidence),
            }
        }
        SandboxNetworkTeardownObservation::Absent { .. }
        | SandboxNetworkTeardownObservation::RetryAuthorized { .. } => {
            MachineApiWorkloadTeardownInspectObservation::NotCompleted { evidence }
        }
        SandboxNetworkTeardownObservation::InProgress { .. } => {
            MachineApiWorkloadTeardownInspectObservation::InProgress { evidence }
        }
        SandboxNetworkTeardownObservation::Ambiguous { .. } => {
            MachineApiWorkloadTeardownInspectObservation::Ambiguous
        }
    })
}

fn release_absence_evidence(
    service: &GuestNodeWorkloadService,
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    validated: &ValidatedGuestAttachmentCommand,
    observation: &MachineApiWorkloadTeardownObservation,
) -> Result<Option<MachineApiNetworkReleaseAbsenceEvidence>, ()> {
    let successful_release = command.step() == WorkloadTeardownStep::ReleaseNetwork
        && matches!(
            observation,
            MachineApiWorkloadTeardownObservation::Execute(
                MachineApiWorkloadTeardownExecuteObservation::Succeeded { .. }
            ) | MachineApiWorkloadTeardownObservation::Inspect(
                MachineApiWorkloadTeardownInspectObservation::Satisfied { .. }
            )
        );
    if !successful_release {
        return Ok(None);
    }
    let evidence = service
        .bundle_materializer
        .inspect_forwarded_network_release_absence_evidence(
            &validated.sandbox_command,
            &validated.prior_observation,
            &validated.expected_forwarder,
        )
        .map_err(|_| ())?;
    let provider_absence = evidence.provider_absence_sha256().parse().map_err(|_| ())?;
    let publication_absence = evidence
        .publication_absence_sha256()
        .parse()
        .map_err(|_| ())?;
    Ok(Some(MachineApiNetworkReleaseAbsenceEvidence::new(
        provider_absence,
        publication_absence,
    )))
}

fn phase_result(
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    observation: MachineApiWorkloadTeardownObservation,
    release_absence: Option<MachineApiNetworkReleaseAbsenceEvidence>,
) -> Result<MachineApiWorkloadTeardownPhaseResult, MachineApiHttpError> {
    MachineApiWorkloadTeardownPhaseResult::new(command, observation, release_absence).map_err(
        |error| MachineApiHttpError {
            status: axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            message: format!(
                "guest attachment teardown result violated its exact contract: {error}"
            ),
        },
    )
}

fn preflight_failure(
    mode: WorkloadTeardownCommandMode,
    observation: SandboxNetworkTeardownObservation,
) -> MachineApiWorkloadTeardownObservation {
    match observation {
        SandboxNetworkTeardownObservation::DefiniteFailure { code, evidence } => {
            let failure = provider_failure(code, WorkloadOwnerEvidenceDigest::sha256(evidence));
            match mode {
                WorkloadTeardownCommandMode::Execute => {
                    MachineApiWorkloadTeardownObservation::Execute(
                        MachineApiWorkloadTeardownExecuteObservation::DefiniteFailure { failure },
                    )
                }
                WorkloadTeardownCommandMode::Inspect => {
                    MachineApiWorkloadTeardownObservation::Inspect(
                        MachineApiWorkloadTeardownInspectObservation::DefiniteFailure { failure },
                    )
                }
            }
        }
        SandboxNetworkTeardownObservation::Succeeded { .. }
        | SandboxNetworkTeardownObservation::Absent { .. }
        | SandboxNetworkTeardownObservation::RetryAuthorized { .. }
        | SandboxNetworkTeardownObservation::InProgress { .. }
        | SandboxNetworkTeardownObservation::Ambiguous { .. } => ambiguous(mode),
    }
}

#[cfg(test)]
pub(super) fn lower_attachment_command_for_test(
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    claim: &WorkloadTeardownClaim,
    local_node: &NodeIdentity,
) -> Result<SandboxNetworkTeardownCommand, MachineApiWorkloadTeardownObservation> {
    lower_attachment_command(command, claim, local_node)
}

#[cfg(test)]
pub(super) fn require_prior_journal_success_for_test(
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    receipt: &WorkloadTeardownReceipt,
    expected_claim: &ProviderCommandClaim,
    journal: &ProviderCommandAttemptJournal,
) -> Result<ProviderCommandObservation, MachineApiWorkloadTeardownObservation> {
    require_prior_journal_success(command, receipt, expected_claim, journal)
}

#[cfg(test)]
pub(super) fn expected_forwarder_for_test(
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    installed: &MachineForwarderAuthority,
) -> Result<OciMachinePortForwarderConfig, MachineApiWorkloadTeardownObservation> {
    validate_forwarder(command, installed)
}

#[cfg(test)]
pub(super) fn prior_claim_for_test(
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    installed_forwarder: &MachineForwarderAuthority,
    local_node: &NodeIdentity,
) -> Result<ProviderCommandClaim, MachineApiWorkloadTeardownObservation> {
    let prior_step = match command.step() {
        WorkloadTeardownStep::DetachNetwork => WorkloadTeardownStep::StopExecution,
        WorkloadTeardownStep::ReleaseNetwork => WorkloadTeardownStep::DetachNetwork,
        _ => {
            return Err(definite_failure(
                command.mode(),
                "sandbox_teardown_command_invalid",
                "test claim requires detach or release",
            ));
        }
    };
    let receipt = command
        .prior_receipt_prefix()
        .receipt_for(prior_step)
        .ok_or_else(|| {
            definite_failure(
                command.mode(),
                "machine_teardown_order_invalid",
                "test command lacks prior receipt",
            )
        })?;
    match prior_step {
        WorkloadTeardownStep::StopExecution => {
            validate_prior_execution_claim(command, receipt.claim(), local_node)?;
            execution_provider_claim_for(command, receipt.claim(), installed_forwarder, local_node)
                .map_err(|error| journal_error(command.mode(), &error))
        }
        WorkloadTeardownStep::DetachNetwork => {
            Ok(
                lower_attachment_command(command, receipt.claim(), local_node)?
                    .provider_claim()
                    .clone(),
            )
        }
        _ => unreachable!("the preceding test step is closed"),
    }
}

#[cfg(test)]
pub(super) fn effect_can_still_start_observation_for_test(
    observation: &ProviderCommandObservation,
) -> MachineApiWorkloadTeardownObservation {
    effect_can_still_start_observation(observation)
}
