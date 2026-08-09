//! Authenticated guest-side execution of one compute-confirmed restart phase.
//!
//! This adapter is an effect sink, not a restart coordinator. It validates the
//! complete portable command against current guest/provider facts, claims one
//! provider-local idempotency fence, and executes or inspects only the named
//! phase. Compute remains the sole owner of policy, order, retries, counts,
//! scheduling, cancellation, and durable saga state.

use nimbus::{SandboxBackendKind, SandboxError, SandboxId, SandboxSpec};
use nimbus_compute::workload_executable::decode_sandbox_spec;
use nimbus_machine::api::{
    MachineApiWorkloadRestartCommandEnvelope, MachineApiWorkloadRestartCommandMode,
    MachineApiWorkloadRestartObservation,
};
use nimbus_node::{
    HostLifecycleRequest, HostLifecycleStatus, HostRestartProviderClaim,
    HostRestartProviderClaimInput, TenantWorkloadPhase,
};
use nimbus_sandbox::{
    ProviderCommandAttemptJournal, ProviderCommandClaim, ProviderCommandClaimDecision,
    ProviderCommandClaimInput, ProviderCommandJournalError, ProviderCommandObservation,
    ProviderCommandObservationKind, ProviderCommandOperation, SandboxExecutionAttemptId,
    SandboxProvisionNetworkPlan, SandboxProvisionPhaseObservation, SandboxRestartAttemptFence,
};
use nimbus_workloads::{
    WorkloadOwnerEvidenceDigest, WorkloadRestartEvidenceDigest, WorkloadRestartStep,
};

use super::{GuestNodeWorkloadService, MachineApiHttpError};

const GUEST_RESTART_OBSERVATION_DOMAIN: &[u8] = b"nimbus.machine.provider-restart.observation.v1\0";

struct ValidatedGuestRestartCommand {
    spec: SandboxSpec,
    sandbox_id: SandboxId,
    attempt_fence: SandboxRestartAttemptFence,
    network_plan: SandboxProvisionNetworkPlan,
    host_claim: HostRestartProviderClaim,
}

impl GuestNodeWorkloadService {
    async fn execute_restart_quiescence(
        &self,
        validated: &ValidatedGuestRestartCommand,
    ) -> MachineApiWorkloadRestartObservation {
        let host = self
            .lifecycle_backend
            .quiesce_restart_exact(validated.host_claim.clone())
            .await;
        if let Err(observation) = require_host_phase(host, HostPhaseSuccess::Quiesced) {
            return observation;
        }
        phase_result(
            self.bundle_materializer
                .quiesce_restart_source(&validated.sandbox_id, &validated.attempt_fence),
        )
    }

    async fn inspect_restart_quiescence(
        &self,
        validated: &ValidatedGuestRestartCommand,
    ) -> MachineApiWorkloadRestartObservation {
        let host = self
            .lifecycle_backend
            .inspect_restart_quiescence(validated.host_claim.clone())
            .await;
        if let Err(observation) = require_host_phase(host, HostPhaseSuccess::Quiesced) {
            return observation;
        }
        phase_result(
            self.bundle_materializer
                .inspect_restart_source_quiescence(&validated.sandbox_id, &validated.attempt_fence),
        )
    }

    async fn execute_restart_activation(
        &self,
        validated: &ValidatedGuestRestartCommand,
    ) -> MachineApiWorkloadRestartObservation {
        match self
            .bundle_materializer
            .inspect_provision_activation_prerequisites(
                &validated.sandbox_id,
                validated.attempt_fence.attempt_id(),
            ) {
            Ok(SandboxProvisionPhaseObservation::Succeeded { .. }) => {}
            other => return phase_result(other),
        }
        let request = match self.restart_runner_request(validated) {
            Ok(request) => request,
            Err(observation) => return observation,
        };
        host_result(
            self.lifecycle_backend
                .activate_restart_exact(validated.host_claim.clone(), request)
                .await,
            HostPhaseSuccess::Activated,
        )
    }

    async fn inspect_restart_activation(
        &self,
        validated: &ValidatedGuestRestartCommand,
    ) -> MachineApiWorkloadRestartObservation {
        let request = match self.restart_runner_request(validated) {
            Ok(request) => request,
            Err(observation) => return observation,
        };
        host_result(
            self.lifecycle_backend
                .inspect_restart_activation(validated.host_claim.clone(), request)
                .await,
            HostPhaseSuccess::Activated,
        )
    }

    fn restart_runner_request(
        &self,
        validated: &ValidatedGuestRestartCommand,
    ) -> Result<HostLifecycleRequest, MachineApiWorkloadRestartObservation> {
        let details = self
            .state_view
            .inspect(&validated.sandbox_id)
            .map_err(|error| ambiguous(error.to_string()))?
            .ok_or_else(|| authenticated_absent("prepared restart target is absent"))?;
        if details.summary.tenant_id != validated.spec.tenant_id
            || details.resources != validated.spec.resources
        {
            return Err(definite_failure(
                "prepared restart target is crossed with the confirmed executable",
            ));
        }
        let bundle_dir = super::bundle_dir_from_manifest_path(&details.manifest_path)
            .map_err(|error| ambiguous(error.message))?;
        super::service_container_runner_request(&bundle_dir, &details.resources)
            .map_err(|error| definite_failure(error.message))
    }
}

pub(super) async fn dispatch(
    service: &GuestNodeWorkloadService,
    command: &MachineApiWorkloadRestartCommandEnvelope,
) -> Result<MachineApiWorkloadRestartObservation, MachineApiHttpError> {
    let validated = match validate_command(&service.node_id, command) {
        Ok(validated) => validated,
        Err(observation) => return Ok(observation),
    };
    let journal = match service.bundle_materializer.attempt_idempotency_journal() {
        Ok(journal) => journal,
        Err(error) => return Ok(ambiguous(error.to_string())),
    };
    let claim = match provider_claim(command) {
        Ok(claim) => claim,
        Err(error) => return Ok(journal_error(&error)),
    };
    let decision = match journal.claim_dispatch_epoch(&claim) {
        Ok(decision) => decision,
        Err(error) => return Ok(journal_error(&error)),
    };

    match command.mode() {
        MachineApiWorkloadRestartCommandMode::Execute => match decision {
            ProviderCommandClaimDecision::AdoptExactAttempt(observation) => {
                Ok(journal_observation(&observation))
            }
            ProviderCommandClaimDecision::ExecuteClaimed(_) => {
                let effect = execute_phase(service, command, &validated).await;
                Ok(record_effect(
                    &journal,
                    &claim,
                    effect,
                    MachineApiWorkloadRestartCommandMode::Execute,
                    false,
                ))
            }
        },
        MachineApiWorkloadRestartCommandMode::Inspect => {
            let live = requires_live_reconciliation(command.step());
            if !live
                && let ProviderCommandClaimDecision::AdoptExactAttempt(observation) = &decision
                && terminal_observation(observation.kind())
            {
                return Ok(journal_observation(observation));
            }
            let effect = inspect_phase(service, command, &validated).await;
            Ok(record_effect(
                &journal,
                &claim,
                effect,
                MachineApiWorkloadRestartCommandMode::Inspect,
                live,
            ))
        }
    }
}

fn validate_command(
    current_node: &nimbus_workloads::NodeIdentity,
    command: &MachineApiWorkloadRestartCommandEnvelope,
) -> Result<ValidatedGuestRestartCommand, MachineApiWorkloadRestartObservation> {
    if command.execution().node_identity() != current_node
        || command.source_execution().node_identity() != current_node
    {
        return Err(definite_failure(format!(
            "confirmed restart command targets node {}, not current guest node {}",
            command.execution().node_identity().as_str(),
            current_node.as_str()
        )));
    }
    let expected_provider =
        crate::machine::backend::provision::forwarded_machine_execution_provider_id();
    if command.provider_selection() != &expected_provider
        || command.source().execution_provider_id() != &expected_provider
    {
        return Err(definite_failure(
            "confirmed restart command targets an execution provider not owned by this guest",
        ));
    }

    let spec = decode_sandbox_spec(command.executable())
        .map_err(|error| definite_failure(format!("invalid executable: {error}")))?;
    if spec.backend != SandboxBackendKind::Container {
        return Err(definite_failure(format!(
            "guest container provider cannot execute {:?}",
            spec.backend
        )));
    }
    if &spec.tenant_id != command.key().tenant_id() {
        return Err(definite_failure(
            "executable tenant does not match the confirmed workload key",
        ));
    }

    let source_attempt_id = SandboxExecutionAttemptId::new(command.source_attempt_id().to_string())
        .map_err(|error| definite_failure(format!("invalid source execution attempt: {error}")))?;
    let attempt_id = SandboxExecutionAttemptId::new(command.attempt_id().to_string())
        .map_err(|error| definite_failure(format!("invalid target execution attempt: {error}")))?;
    let attempt_fence = SandboxRestartAttemptFence::new(
        source_attempt_id,
        attempt_id,
        command.restart_epoch().as_u64(),
    )
    .map_err(|error| definite_failure(format!("invalid restart attempt fence: {error}")))?;
    let network_plan = super::provision::sandbox_network_plan_for(
        command.compiled_network_plan(),
        command.generation(),
        &spec,
    )
    .map_err(provision_validation_error)?;
    let host_claim = host_claim(command)?;

    Ok(ValidatedGuestRestartCommand {
        spec,
        sandbox_id: SandboxId::new(command.execution().execution_id().as_str()),
        attempt_fence,
        network_plan,
        host_claim,
    })
}

fn provision_validation_error(
    observation: nimbus_machine::api::MachineApiWorkloadProvisionObservation,
) -> MachineApiWorkloadRestartObservation {
    match observation {
        nimbus_machine::api::MachineApiWorkloadProvisionObservation::Succeeded { evidence } => {
            succeeded(evidence)
        }
        nimbus_machine::api::MachineApiWorkloadProvisionObservation::DefiniteFailure {
            evidence,
        } => definite_failure(evidence),
        nimbus_machine::api::MachineApiWorkloadProvisionObservation::Absent { evidence } => {
            authenticated_absent(evidence)
        }
        nimbus_machine::api::MachineApiWorkloadProvisionObservation::InProgress { evidence } => {
            in_progress(evidence)
        }
        nimbus_machine::api::MachineApiWorkloadProvisionObservation::Ambiguous { .. } => {
            MachineApiWorkloadRestartObservation::Ambiguous
        }
    }
}

fn host_claim(
    command: &MachineApiWorkloadRestartCommandEnvelope,
) -> Result<HostRestartProviderClaim, MachineApiWorkloadRestartObservation> {
    let mode = match (command.mode(), command.successor_veto_generation()) {
        (MachineApiWorkloadRestartCommandMode::Execute, None) => {
            HostRestartProviderClaimInput::execute_mode()
        }
        (MachineApiWorkloadRestartCommandMode::Inspect, None) => {
            HostRestartProviderClaimInput::inspect_mode()
        }
        (MachineApiWorkloadRestartCommandMode::Inspect, Some(successor_generation)) => {
            HostRestartProviderClaimInput::inspect_mode_after_successor_veto(successor_generation)
        }
        (MachineApiWorkloadRestartCommandMode::Execute, Some(_)) => {
            return Err(definite_failure(
                "machine restart execute command cannot carry a successor veto",
            ));
        }
    };
    HostRestartProviderClaim::new(HostRestartProviderClaimInput {
        saga_id: command.saga_id().clone(),
        transition_id: command.transition_id().clone(),
        command_id: command.command_id().clone(),
        request_id: command.request_id().clone(),
        source_execution: command.source_execution().clone(),
        execution: command.execution().clone(),
        restart_epoch: command.restart_epoch(),
        dispatch_epoch: command.dispatch_epoch(),
        issuing_revision: command.issuing_revision(),
        confirmed_revision: command.confirmed_revision(),
        source_generation: command.source().source_generation(),
        source_digest: command.source().source_digest(),
        network_plan_digest: command.network_plan_digest().to_string(),
        provider_selection: command.provider_selection().clone(),
        step: command.step(),
        mode,
    })
    .map_err(|error| definite_failure(error.to_string()))
}

async fn execute_phase(
    service: &GuestNodeWorkloadService,
    command: &MachineApiWorkloadRestartCommandEnvelope,
    validated: &ValidatedGuestRestartCommand,
) -> MachineApiWorkloadRestartObservation {
    match command.step() {
        WorkloadRestartStep::WithdrawPublication => phase_result(
            service
                .bundle_materializer
                .withdraw_restart_machine_ingress(
                    &validated.sandbox_id,
                    &validated.attempt_fence,
                    &validated.network_plan,
                    command.machine_forwarder_authority().provider_instance(),
                    command.machine_provider_generation(),
                ),
        ),
        WorkloadRestartStep::QuiesceExecution => {
            service.execute_restart_quiescence(validated).await
        }
        WorkloadRestartStep::PrepareExecution => phase_result(
            service
                .bundle_materializer
                .prepare_restart_target_attempt(&validated.sandbox_id, &validated.attempt_fence),
        ),
        WorkloadRestartStep::AttachNetwork => phase_result(
            service
                .bundle_materializer
                .attach_restart_retained_network(&validated.sandbox_id, &validated.attempt_fence),
        ),
        WorkloadRestartStep::ActivateExecution => {
            service.execute_restart_activation(validated).await
        }
        WorkloadRestartStep::Publish => {
            phase_result(service.bundle_materializer.publish_restart_machine_ingress(
                &validated.sandbox_id,
                &validated.attempt_fence,
                &validated.network_plan,
                command.machine_forwarder_authority().provider_instance(),
                command.machine_provider_generation(),
            ))
        }
        WorkloadRestartStep::InspectActivationPrerequisites
        | WorkloadRestartStep::InspectReadiness
        | WorkloadRestartStep::ObservePublication => {
            definite_failure("inspection-only restart phase cannot execute a provider effect")
        }
    }
}

async fn inspect_phase(
    service: &GuestNodeWorkloadService,
    command: &MachineApiWorkloadRestartCommandEnvelope,
    validated: &ValidatedGuestRestartCommand,
) -> MachineApiWorkloadRestartObservation {
    match command.step() {
        WorkloadRestartStep::WithdrawPublication => phase_result(
            service
                .bundle_materializer
                .inspect_restart_machine_ingress_withdrawal(
                    &validated.sandbox_id,
                    &validated.attempt_fence,
                    &validated.network_plan,
                    command.machine_forwarder_authority().provider_instance(),
                    command.machine_provider_generation(),
                ),
        ),
        WorkloadRestartStep::QuiesceExecution => {
            service.inspect_restart_quiescence(validated).await
        }
        WorkloadRestartStep::PrepareExecution => phase_result(
            service
                .bundle_materializer
                .inspect_restart_target_preparation(
                    &validated.sandbox_id,
                    &validated.attempt_fence,
                ),
        ),
        WorkloadRestartStep::AttachNetwork => phase_result(
            service
                .bundle_materializer
                .inspect_restart_retained_network(&validated.sandbox_id, &validated.attempt_fence),
        ),
        WorkloadRestartStep::InspectActivationPrerequisites => phase_result(
            service
                .bundle_materializer
                .inspect_provision_activation_prerequisites(
                    &validated.sandbox_id,
                    validated.attempt_fence.attempt_id(),
                ),
        ),
        WorkloadRestartStep::ActivateExecution => {
            service.inspect_restart_activation(validated).await
        }
        WorkloadRestartStep::InspectReadiness => phase_result(
            service
                .bundle_materializer
                .inspect_provision_workload_readiness(
                    &validated.sandbox_id,
                    validated.attempt_fence.attempt_id(),
                ),
        ),
        WorkloadRestartStep::Publish | WorkloadRestartStep::ObservePublication => phase_result(
            service
                .bundle_materializer
                .inspect_restart_machine_ingress_publication(
                    &validated.sandbox_id,
                    &validated.attempt_fence,
                    &validated.network_plan,
                    command.machine_forwarder_authority().provider_instance(),
                    command.machine_provider_generation(),
                ),
        ),
    }
}

fn provider_claim(
    command: &MachineApiWorkloadRestartCommandEnvelope,
) -> Result<ProviderCommandClaim, ProviderCommandJournalError> {
    let effect_subject = serde_json::to_string(&(
        command.step(),
        command.source_execution(),
        command.execution(),
        command.compiled_network_plan().plan().plan_id(),
    ))
    .map_err(|error| ProviderCommandJournalError::InvalidClaim {
        message: format!("confirmed restart subject cannot be encoded: {error}"),
    })?;
    let provider_realm = serde_json::to_vec(&(
        command.provider_selection(),
        command
            .compiled_network_plan()
            .content()
            .capability_selection(),
        command.machine_forwarder_authority(),
        command.machine_provider_generation(),
    ))
    .map_err(|error| ProviderCommandJournalError::InvalidClaim {
        message: format!("confirmed restart provider realm cannot be encoded: {error}"),
    })?;
    ProviderCommandClaim::new(ProviderCommandClaimInput {
        authority_id: command.saga_id().as_str().to_owned(),
        effect_subject,
        source_attempt_id: Some(command.source_attempt_id().to_string()),
        attempt_id: command.attempt_id().to_string(),
        dispatch_epoch: command.dispatch_epoch().as_u64(),
        workload_generation: command.generation().as_u64(),
        restart_ordinal: command.restart_epoch().as_u64(),
        desired_digest: command.desired_digest().to_string(),
        source_digest: command.source().source_digest().to_string(),
        network_plan_digest: command.network_plan_digest().to_string(),
        provider_target_digest: WorkloadOwnerEvidenceDigest::sha256(provider_realm).to_string(),
        operation: operation(command.step()),
    })
}

const fn operation(step: WorkloadRestartStep) -> ProviderCommandOperation {
    match step {
        WorkloadRestartStep::WithdrawPublication => ProviderCommandOperation::WithdrawPublication,
        WorkloadRestartStep::QuiesceExecution => ProviderCommandOperation::ResetWorkloadForRestart,
        WorkloadRestartStep::PrepareExecution => ProviderCommandOperation::PrepareRestartAttempt,
        WorkloadRestartStep::AttachNetwork => ProviderCommandOperation::AttachRetainedNetwork,
        WorkloadRestartStep::InspectActivationPrerequisites => {
            ProviderCommandOperation::InspectRestartActivationPrerequisites
        }
        WorkloadRestartStep::ActivateExecution => {
            ProviderCommandOperation::ActivateRestartedWorkload
        }
        WorkloadRestartStep::InspectReadiness => ProviderCommandOperation::InspectRestartReadiness,
        WorkloadRestartStep::Publish => ProviderCommandOperation::PublishRestartIngress,
        WorkloadRestartStep::ObservePublication => {
            ProviderCommandOperation::ObserveRestartPublication
        }
    }
}

fn record_effect(
    journal: &ProviderCommandAttemptJournal,
    claim: &ProviderCommandClaim,
    effect: MachineApiWorkloadRestartObservation,
    mode: MachineApiWorkloadRestartCommandMode,
    reconcile_live_absence: bool,
) -> MachineApiWorkloadRestartObservation {
    let kind = durable_observation_kind(mode, &effect);
    let evidence = match &effect {
        MachineApiWorkloadRestartObservation::Succeeded { evidence }
        | MachineApiWorkloadRestartObservation::AuthenticatedAbsent { evidence }
        | MachineApiWorkloadRestartObservation::DefiniteFailure { evidence }
        | MachineApiWorkloadRestartObservation::InProgress { evidence } => evidence.to_string(),
        MachineApiWorkloadRestartObservation::Ambiguous => {
            "guest_restart_outcome_ambiguous".to_owned()
        }
    };
    let recorded = if reconcile_live_absence && kind == ProviderCommandObservationKind::Absent {
        journal.record_reconciled_absence(claim, evidence.as_bytes())
    } else {
        journal.record_observation(claim, kind, evidence.as_bytes())
    };
    match recorded {
        Ok(observation) => journal_observation(&observation),
        Err(error) => journal_error(&error),
    }
}

fn durable_observation_kind(
    mode: MachineApiWorkloadRestartCommandMode,
    effect: &MachineApiWorkloadRestartObservation,
) -> ProviderCommandObservationKind {
    match effect {
        MachineApiWorkloadRestartObservation::Succeeded { .. } => {
            ProviderCommandObservationKind::Succeeded
        }
        MachineApiWorkloadRestartObservation::AuthenticatedAbsent { .. }
            if mode == MachineApiWorkloadRestartCommandMode::Execute =>
        {
            ProviderCommandObservationKind::Ambiguous
        }
        MachineApiWorkloadRestartObservation::AuthenticatedAbsent { .. } => {
            ProviderCommandObservationKind::Absent
        }
        MachineApiWorkloadRestartObservation::DefiniteFailure { .. } => {
            ProviderCommandObservationKind::DefiniteFailure
        }
        MachineApiWorkloadRestartObservation::InProgress { .. } => {
            ProviderCommandObservationKind::InProgress
        }
        MachineApiWorkloadRestartObservation::Ambiguous => {
            ProviderCommandObservationKind::Ambiguous
        }
    }
}

fn requires_live_reconciliation(step: WorkloadRestartStep) -> bool {
    matches!(
        step,
        WorkloadRestartStep::AttachNetwork
            | WorkloadRestartStep::ActivateExecution
            | WorkloadRestartStep::Publish
            | WorkloadRestartStep::ObservePublication
    )
}

fn terminal_observation(kind: ProviderCommandObservationKind) -> bool {
    matches!(
        kind,
        ProviderCommandObservationKind::Succeeded
            | ProviderCommandObservationKind::DefiniteFailure
            | ProviderCommandObservationKind::Absent
    )
}

fn journal_observation(
    observation: &ProviderCommandObservation,
) -> MachineApiWorkloadRestartObservation {
    let evidence = durable_observation_digest(observation);
    match observation.kind() {
        ProviderCommandObservationKind::Succeeded => {
            MachineApiWorkloadRestartObservation::Succeeded { evidence }
        }
        ProviderCommandObservationKind::DefiniteFailure => {
            MachineApiWorkloadRestartObservation::DefiniteFailure { evidence }
        }
        ProviderCommandObservationKind::Absent => {
            MachineApiWorkloadRestartObservation::AuthenticatedAbsent { evidence }
        }
        ProviderCommandObservationKind::Claimed | ProviderCommandObservationKind::InProgress => {
            MachineApiWorkloadRestartObservation::InProgress { evidence }
        }
        ProviderCommandObservationKind::RetryAuthorized
        | ProviderCommandObservationKind::Ambiguous => {
            MachineApiWorkloadRestartObservation::Ambiguous
        }
    }
}

fn durable_observation_digest(
    observation: &ProviderCommandObservation,
) -> WorkloadRestartEvidenceDigest {
    let durable = observation
        .evidence_sha256()
        .unwrap_or("provider_claimed_without_outcome_evidence");
    evidence_digest(durable.as_bytes())
}

fn phase_result(
    result: Result<SandboxProvisionPhaseObservation, SandboxError>,
) -> MachineApiWorkloadRestartObservation {
    match result {
        Ok(SandboxProvisionPhaseObservation::Succeeded { evidence }) => succeeded(evidence),
        Ok(SandboxProvisionPhaseObservation::Absent { evidence }) => authenticated_absent(evidence),
        Ok(SandboxProvisionPhaseObservation::InProgress { evidence }) => in_progress(evidence),
        Ok(SandboxProvisionPhaseObservation::Ambiguous { .. }) => {
            MachineApiWorkloadRestartObservation::Ambiguous
        }
        Err(error @ (SandboxError::InvalidSpec { .. } | SandboxError::NotFound { .. })) => {
            definite_failure(error.to_string())
        }
        Err(_) => MachineApiWorkloadRestartObservation::Ambiguous,
    }
}

#[derive(Clone, Copy)]
enum HostPhaseSuccess {
    Quiesced,
    Activated,
}

fn require_host_phase(
    result: nimbus::Result<HostLifecycleStatus>,
    success: HostPhaseSuccess,
) -> Result<(), MachineApiWorkloadRestartObservation> {
    match host_result(result, success) {
        MachineApiWorkloadRestartObservation::Succeeded { .. } => Ok(()),
        observation => Err(observation),
    }
}

fn host_result(
    result: nimbus::Result<HostLifecycleStatus>,
    success: HostPhaseSuccess,
) -> MachineApiWorkloadRestartObservation {
    match result {
        Ok(status) => {
            let evidence = match serde_json::to_vec(&status) {
                Ok(evidence) => evidence,
                Err(_) => return MachineApiWorkloadRestartObservation::Ambiguous,
            };
            match (success, status.phase()) {
                (HostPhaseSuccess::Quiesced, TenantWorkloadPhase::Deleting)
                | (
                    HostPhaseSuccess::Activated,
                    TenantWorkloadPhase::Running | TenantWorkloadPhase::Ready,
                ) => succeeded(evidence),
                (
                    HostPhaseSuccess::Quiesced,
                    TenantWorkloadPhase::Pending
                    | TenantWorkloadPhase::Bound
                    | TenantWorkloadPhase::Running
                    | TenantWorkloadPhase::Ready,
                )
                | (
                    HostPhaseSuccess::Activated,
                    TenantWorkloadPhase::Pending
                    | TenantWorkloadPhase::Bound
                    | TenantWorkloadPhase::Deleting,
                ) => in_progress(evidence),
                (
                    HostPhaseSuccess::Quiesced | HostPhaseSuccess::Activated,
                    TenantWorkloadPhase::Denied,
                ) => definite_failure(evidence),
                (
                    HostPhaseSuccess::Quiesced | HostPhaseSuccess::Activated,
                    TenantWorkloadPhase::Degraded,
                ) => MachineApiWorkloadRestartObservation::Ambiguous,
            }
        }
        Err(nimbus::Error::InvalidInput(message) | nimbus::Error::PermissionDenied(message)) => {
            definite_failure(message)
        }
        Err(_) => MachineApiWorkloadRestartObservation::Ambiguous,
    }
}

fn journal_error(error: &ProviderCommandJournalError) -> MachineApiWorkloadRestartObservation {
    match error {
        ProviderCommandJournalError::InvalidClaim { .. }
        | ProviderCommandJournalError::StaleWorkloadGeneration { .. }
        | ProviderCommandJournalError::StaleRestartOrdinal { .. }
        | ProviderCommandJournalError::SkippedRestartOrdinal { .. }
        | ProviderCommandJournalError::StaleDispatchEpoch { .. }
        | ProviderCommandJournalError::SkippedDispatchEpoch { .. }
        | ProviderCommandJournalError::CrossedClaim
        | ProviderCommandJournalError::RetryWithoutAuthority
        | ProviderCommandJournalError::PriorEffectUnresolved => definite_failure(error.to_string()),
        ProviderCommandJournalError::Corrupt { .. } | ProviderCommandJournalError::Store { .. } => {
            MachineApiWorkloadRestartObservation::Ambiguous
        }
    }
}

fn succeeded(evidence: impl AsRef<[u8]>) -> MachineApiWorkloadRestartObservation {
    MachineApiWorkloadRestartObservation::Succeeded {
        evidence: evidence_digest(evidence.as_ref()),
    }
}

fn authenticated_absent(evidence: impl AsRef<[u8]>) -> MachineApiWorkloadRestartObservation {
    MachineApiWorkloadRestartObservation::AuthenticatedAbsent {
        evidence: evidence_digest(evidence.as_ref()),
    }
}

fn definite_failure(evidence: impl AsRef<[u8]>) -> MachineApiWorkloadRestartObservation {
    MachineApiWorkloadRestartObservation::DefiniteFailure {
        evidence: evidence_digest(evidence.as_ref()),
    }
}

fn in_progress(evidence: impl AsRef<[u8]>) -> MachineApiWorkloadRestartObservation {
    MachineApiWorkloadRestartObservation::InProgress {
        evidence: evidence_digest(evidence.as_ref()),
    }
}

fn ambiguous(_evidence: impl AsRef<[u8]>) -> MachineApiWorkloadRestartObservation {
    MachineApiWorkloadRestartObservation::Ambiguous
}

fn evidence_digest(evidence: &[u8]) -> WorkloadRestartEvidenceDigest {
    WorkloadRestartEvidenceDigest::sha256([GUEST_RESTART_OBSERVATION_DOMAIN, evidence].concat())
}

#[cfg(test)]
mod tests;
