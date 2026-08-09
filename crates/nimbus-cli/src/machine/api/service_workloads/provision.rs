//! Authenticated guest-side execution of one compute-confirmed provision phase.
//!
//! This module is an adapter, not a coordinator. It validates the complete
//! transport envelope against current guest/provider facts, claims one
//! provider-local idempotency fence, and invokes exactly one requested phase.
//! It never admits desired state, advances saga state, retries, or changes
//! phase order.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use nimbus::{SandboxBackendKind, SandboxError, SandboxHandle, SandboxId, SandboxSpec};
use nimbus_compute::workload_executable::decode_sandbox_spec;
use nimbus_machine::{
    MachineForwarderAuthority,
    api::{
        MAX_MACHINE_API_PROVISION_EVIDENCE_BYTES, MachineApiWorkloadProvisionCommandEnvelope,
        MachineApiWorkloadProvisionObservation,
    },
};
use nimbus_network::{
    NetworkCapabilityRole, NetworkLeaseEpoch, PortBindRealm, PortBindTarget, PortBindingSpec,
    PortExposure, PortIpv6Overlap, PortLeaseAccounting, PortLeaseFence, PortLeaseRequest,
    PortProtocol, PortPublicationIntent, PortRequestMode,
};
use nimbus_node::{HostLifecycleRequest, HostLifecycleStatus, TenantWorkloadPhase};
use nimbus_sandbox::{
    ProviderCommandAttemptJournal, ProviderCommandClaim, ProviderCommandClaimDecision,
    ProviderCommandClaimInput, ProviderCommandObservation, ProviderCommandObservationKind,
    ProviderCommandOperation, SandboxExecutionAttemptId, SandboxProvisionDependencyListener,
    SandboxProvisionListener, SandboxProvisionNetworkPlan, SandboxProvisionPhaseObservation,
};
use nimbus_workloads::{
    WorkloadNetworkPortRequestMode, WorkloadOwnerEvidenceDigest, WorkloadProvisionCommandMode,
    WorkloadProvisionProviderTarget, WorkloadProvisionStep,
};

use super::{GuestNodeWorkloadService, MachineApiHttpError};

pub(super) struct ValidatedGuestProvisionCommand {
    spec: SandboxSpec,
    sandbox_id: SandboxId,
    execution_attempt_id: SandboxExecutionAttemptId,
    network_plan: SandboxProvisionNetworkPlan,
}

impl ValidatedGuestProvisionCommand {
    pub(super) fn spec(&self) -> &SandboxSpec {
        &self.spec
    }

    pub(super) fn sandbox_id(&self) -> &SandboxId {
        &self.sandbox_id
    }

    pub(super) fn execution_attempt_id(&self) -> &SandboxExecutionAttemptId {
        &self.execution_attempt_id
    }
}

impl GuestNodeWorkloadService {
    async fn activate_exact_provision(
        &self,
        command: &MachineApiWorkloadProvisionCommandEnvelope,
        validated: &ValidatedGuestProvisionCommand,
    ) -> MachineApiWorkloadProvisionObservation {
        match self
            .bundle_materializer
            .inspect_provision_activation_prerequisites(
                validated.sandbox_id(),
                validated.execution_attempt_id(),
            ) {
            Ok(SandboxProvisionPhaseObservation::Succeeded { .. }) => {}
            other => return phase_result(other),
        }
        let request = match self.exact_runner_request(validated) {
            Ok(request) => request,
            Err(observation) => return observation,
        };
        host_status_result(
            self.lifecycle_backend
                .activate_exact(
                    command.execution().clone(),
                    command.claim().clone(),
                    request,
                )
                .await,
            HostStatusSuccess::Activated,
        )
    }

    async fn inspect_exact_activation(
        &self,
        command: &MachineApiWorkloadProvisionCommandEnvelope,
        validated: &ValidatedGuestProvisionCommand,
    ) -> MachineApiWorkloadProvisionObservation {
        let request = match self.exact_runner_request(validated) {
            Ok(request) => request,
            Err(observation) => return observation,
        };
        host_status_result(
            self.lifecycle_backend
                .inspect_activation(
                    command.execution().clone(),
                    command.claim().clone(),
                    request,
                )
                .await,
            HostStatusSuccess::Activated,
        )
    }

    async fn inspect_exact_readiness(
        &self,
        command: &MachineApiWorkloadProvisionCommandEnvelope,
        validated: &ValidatedGuestProvisionCommand,
    ) -> MachineApiWorkloadProvisionObservation {
        let request = match self.exact_runner_request(validated) {
            Ok(request) => request,
            Err(observation) => return observation,
        };
        host_status_result(
            self.lifecycle_backend
                .inspect_activation(
                    command.execution().clone(),
                    command.claim().clone(),
                    request,
                )
                .await,
            HostStatusSuccess::Ready,
        )
    }

    fn exact_runner_request(
        &self,
        validated: &ValidatedGuestProvisionCommand,
    ) -> Result<HostLifecycleRequest, MachineApiWorkloadProvisionObservation> {
        let details = self
            .state_view
            .inspect(validated.sandbox_id())
            .map_err(|error| ambiguous(error.to_string()))?
            .ok_or_else(|| MachineApiWorkloadProvisionObservation::Absent {
                evidence: b"prepared guest workload is absent".to_vec(),
            })?;
        if details.summary.tenant_id != validated.spec().tenant_id
            || details.resources != validated.spec().resources
        {
            return Err(definite_failure(
                "prepared guest workload is crossed with the confirmed executable",
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
    command: &MachineApiWorkloadProvisionCommandEnvelope,
    forwarder_authority: &MachineForwarderAuthority,
) -> Result<MachineApiWorkloadProvisionObservation, MachineApiHttpError> {
    let validated = match validate_command(&service.node_id, command, forwarder_authority) {
        Ok(validated) => validated,
        Err(observation) => return Ok(observation),
    };
    let journal = match service.bundle_materializer.attempt_idempotency_journal() {
        Ok(journal) => journal,
        Err(error) => return Ok(ambiguous(error.to_string())),
    };
    let claim = match provider_claim(command) {
        Ok(claim) => claim,
        Err(error) => return Ok(ambiguous(error.to_string())),
    };
    let decision = match journal.claim_dispatch_epoch(&claim) {
        Ok(decision) => decision,
        Err(error) => return Ok(ambiguous(error.to_string())),
    };

    match command.mode() {
        WorkloadProvisionCommandMode::Execute => match decision {
            ProviderCommandClaimDecision::AdoptExactAttempt(observation) => {
                Ok(journal_observation(&observation))
            }
            ProviderCommandClaimDecision::ExecuteClaimed(_) => {
                let effect = execute_phase(service, command, &validated, forwarder_authority).await;
                Ok(record_effect(&journal, &claim, effect, false))
            }
        },
        WorkloadProvisionCommandMode::Inspect => {
            if let ProviderCommandClaimDecision::AdoptExactAttempt(observation) = &decision
                && terminal_without_live_reconciliation(
                    command.claim().attempt().step(),
                    observation.kind(),
                )
            {
                return Ok(journal_observation(observation));
            }
            let effect = inspect_phase(service, command, &validated, forwarder_authority).await;
            Ok(record_effect(
                &journal,
                &claim,
                effect,
                matches!(
                    command.claim().attempt().step(),
                    WorkloadProvisionStep::Publish | WorkloadProvisionStep::ObservePublication
                ),
            ))
        }
    }
}

fn validate_command(
    current_node: &nimbus_workloads::NodeIdentity,
    command: &MachineApiWorkloadProvisionCommandEnvelope,
    forwarder_authority: &MachineForwarderAuthority,
) -> Result<ValidatedGuestProvisionCommand, MachineApiWorkloadProvisionObservation> {
    let attempt = command.claim().attempt();
    if attempt.required_node() != current_node {
        return Err(definite_failure(format!(
            "confirmed provision command targets node {}, not current guest node {}",
            attempt.required_node().as_str(),
            current_node.as_str()
        )));
    }
    validate_provider_target(command, forwarder_authority)?;

    // Decoding the already-admitted executable is pure. The guest does not
    // call tenant admission or read desired state to synthesize a replacement.
    let spec = decode_sandbox_spec(command.executable())
        .map_err(|error| definite_failure(format!("invalid executable: {error}")))?;
    if spec.backend != SandboxBackendKind::Container {
        return Err(definite_failure(format!(
            "guest container provider cannot execute {:?}",
            spec.backend
        )));
    }
    if &spec.tenant_id != attempt.key().tenant_id() {
        return Err(definite_failure(
            "executable tenant does not match the confirmed workload key",
        ));
    }
    let network_plan = sandbox_network_plan(command, &spec)?;
    Ok(ValidatedGuestProvisionCommand {
        spec,
        sandbox_id: SandboxId::new(command.execution().execution_id().as_str()),
        execution_attempt_id: SandboxExecutionAttemptId::new(
            command.execution().attempt_id().to_string(),
        )
        .map_err(|error| definite_failure(format!("invalid execution attempt: {error}")))?,
        network_plan,
    })
}

fn validate_provider_target(
    command: &MachineApiWorkloadProvisionCommandEnvelope,
    forwarder_authority: &MachineForwarderAuthority,
) -> Result<(), MachineApiWorkloadProvisionObservation> {
    let expected_attachment =
        crate::machine::backend::provision::forwarded_machine_attachment_provider_id();
    let expected_execution =
        crate::machine::backend::provision::forwarded_machine_execution_provider_id();
    let matches = match (command.claim().attempt().step(), command.provider_target()) {
        (
            WorkloadProvisionStep::ReserveNetwork | WorkloadProvisionStep::AttachNetwork,
            WorkloadProvisionProviderTarget::Network {
                role: NetworkCapabilityRole::Attachment,
                provider_id,
                ..
            },
        ) => provider_id == &expected_attachment,
        (
            WorkloadProvisionStep::Publish | WorkloadProvisionStep::ObservePublication,
            WorkloadProvisionProviderTarget::Network {
                role: NetworkCapabilityRole::Ingress,
                provider_id,
                ..
            },
        ) => provider_id == forwarder_authority.provider_instance().provider_id(),
        (
            WorkloadProvisionStep::PrepareWorkload
            | WorkloadProvisionStep::InspectActivationPrerequisites
            | WorkloadProvisionStep::ActivateWorkload
            | WorkloadProvisionStep::InspectWorkloadReadiness,
            WorkloadProvisionProviderTarget::Execution { provider_id, .. },
        ) => provider_id == &expected_execution,
        _ => false,
    };
    if matches {
        Ok(())
    } else {
        Err(definite_failure(
            "confirmed provision command targets a provider not owned by this guest adapter",
        ))
    }
}

async fn execute_phase(
    service: &GuestNodeWorkloadService,
    command: &MachineApiWorkloadProvisionCommandEnvelope,
    validated: &ValidatedGuestProvisionCommand,
    authority: &MachineForwarderAuthority,
) -> MachineApiWorkloadProvisionObservation {
    match command.claim().attempt().step() {
        WorkloadProvisionStep::ReserveNetwork => {
            handle_result(service.bundle_materializer.reserve_provision_network(
                validated.spec.clone(),
                validated.sandbox_id.clone(),
                validated.execution_attempt_id.clone(),
                validated.network_plan.clone(),
            ))
        }
        WorkloadProvisionStep::PrepareWorkload => handle_result(
            service
                .bundle_materializer
                .prepare_provision_workload(&validated.sandbox_id, &validated.execution_attempt_id),
        ),
        WorkloadProvisionStep::AttachNetwork => phase_result(
            service
                .bundle_materializer
                .attach_provision_network(&validated.sandbox_id, &validated.execution_attempt_id),
        ),
        WorkloadProvisionStep::ActivateWorkload => {
            service.activate_exact_provision(command, validated).await
        }
        WorkloadProvisionStep::Publish => phase_result(
            service
                .bundle_materializer
                .publish_provision_machine_ingress(
                    &validated.sandbox_id,
                    &validated.execution_attempt_id,
                    &validated.network_plan,
                    authority.provider_instance(),
                    authority.generation(),
                ),
        ),
        WorkloadProvisionStep::InspectActivationPrerequisites
        | WorkloadProvisionStep::InspectWorkloadReadiness
        | WorkloadProvisionStep::ObservePublication => {
            definite_failure("inspection-only provision phase cannot execute a provider effect")
        }
    }
}

async fn inspect_phase(
    service: &GuestNodeWorkloadService,
    command: &MachineApiWorkloadProvisionCommandEnvelope,
    validated: &ValidatedGuestProvisionCommand,
    authority: &MachineForwarderAuthority,
) -> MachineApiWorkloadProvisionObservation {
    match command.claim().attempt().step() {
        WorkloadProvisionStep::ReserveNetwork => optional_handle_result(
            service
                .bundle_materializer
                .inspect_provision_network_reservation(
                    &validated.sandbox_id,
                    &validated.execution_attempt_id,
                    &validated.network_plan,
                ),
        ),
        WorkloadProvisionStep::PrepareWorkload => {
            optional_handle_result(service.bundle_materializer.inspect_provision_preparation(
                &validated.sandbox_id,
                &validated.execution_attempt_id,
            ))
        }
        WorkloadProvisionStep::AttachNetwork
        | WorkloadProvisionStep::InspectActivationPrerequisites => phase_result(
            service
                .bundle_materializer
                .inspect_provision_activation_prerequisites(
                    &validated.sandbox_id,
                    &validated.execution_attempt_id,
                ),
        ),
        WorkloadProvisionStep::ActivateWorkload => {
            service.inspect_exact_activation(command, validated).await
        }
        WorkloadProvisionStep::InspectWorkloadReadiness => {
            service.inspect_exact_readiness(command, validated).await
        }
        WorkloadProvisionStep::Publish | WorkloadProvisionStep::ObservePublication => phase_result(
            service
                .bundle_materializer
                .inspect_provision_machine_ingress(
                    &validated.sandbox_id,
                    &validated.execution_attempt_id,
                    &validated.network_plan,
                    authority.provider_instance(),
                    authority.generation(),
                ),
        ),
    }
}

fn provider_claim(
    command: &MachineApiWorkloadProvisionCommandEnvelope,
) -> Result<ProviderCommandClaim, nimbus_sandbox::ProviderCommandJournalError> {
    let attempt = command.claim().attempt();
    let effect_subject = serde_json::to_string(attempt.subjects()).map_err(|error| {
        nimbus_sandbox::ProviderCommandJournalError::InvalidClaim {
            message: format!("confirmed provider subject cannot be encoded: {error}"),
        }
    })?;
    let target = serde_json::to_vec(command.provider_target()).map_err(|error| {
        nimbus_sandbox::ProviderCommandJournalError::InvalidClaim {
            message: format!("confirmed provider target cannot be encoded: {error}"),
        }
    })?;
    ProviderCommandClaim::new(ProviderCommandClaimInput {
        authority_id: attempt.saga_id().as_str().to_owned(),
        effect_subject,
        source_attempt_id: None,
        attempt_id: command.attempt_id().as_str().to_owned(),
        dispatch_epoch: command.dispatch_epoch().as_u64(),
        workload_generation: command.generation().as_u64(),
        restart_ordinal: 0,
        desired_digest: command.desired_digest().to_string(),
        source_digest: command.source_digest().to_string(),
        network_plan_digest: command.network_plan_digest().to_string(),
        provider_target_digest: WorkloadOwnerEvidenceDigest::sha256(target).to_string(),
        operation: operation(attempt.step()),
    })
}

const fn operation(step: WorkloadProvisionStep) -> ProviderCommandOperation {
    match step {
        WorkloadProvisionStep::ReserveNetwork => ProviderCommandOperation::ReserveNetwork,
        WorkloadProvisionStep::PrepareWorkload => ProviderCommandOperation::PrepareWorkload,
        WorkloadProvisionStep::AttachNetwork => ProviderCommandOperation::AttachNetwork,
        WorkloadProvisionStep::InspectActivationPrerequisites => {
            ProviderCommandOperation::InspectActivationPrerequisites
        }
        WorkloadProvisionStep::ActivateWorkload => ProviderCommandOperation::ActivateWorkload,
        WorkloadProvisionStep::InspectWorkloadReadiness => {
            ProviderCommandOperation::InspectWorkloadReadiness
        }
        WorkloadProvisionStep::Publish => ProviderCommandOperation::PublishIngress,
        WorkloadProvisionStep::ObservePublication => ProviderCommandOperation::ObserveIngress,
    }
}

fn record_effect(
    journal: &ProviderCommandAttemptJournal,
    claim: &ProviderCommandClaim,
    effect: MachineApiWorkloadProvisionObservation,
    reconcile_live_absence: bool,
) -> MachineApiWorkloadProvisionObservation {
    let kind = match &effect {
        MachineApiWorkloadProvisionObservation::Succeeded { .. } => {
            ProviderCommandObservationKind::Succeeded
        }
        MachineApiWorkloadProvisionObservation::DefiniteFailure { .. } => {
            ProviderCommandObservationKind::DefiniteFailure
        }
        MachineApiWorkloadProvisionObservation::Absent { .. } => {
            ProviderCommandObservationKind::Absent
        }
        MachineApiWorkloadProvisionObservation::InProgress { .. } => {
            ProviderCommandObservationKind::InProgress
        }
        MachineApiWorkloadProvisionObservation::Ambiguous { .. } => {
            ProviderCommandObservationKind::Ambiguous
        }
    };
    let result = if reconcile_live_absence && kind == ProviderCommandObservationKind::Absent {
        journal.record_reconciled_absence(claim, effect.evidence())
    } else {
        journal.record_observation(claim, kind, effect.evidence())
    };
    match result {
        Ok(observation) => journal_observation(&observation),
        Err(error) => ambiguous(error.to_string()),
    }
}

fn terminal_without_live_reconciliation(
    step: WorkloadProvisionStep,
    kind: ProviderCommandObservationKind,
) -> bool {
    !matches!(
        step,
        WorkloadProvisionStep::Publish | WorkloadProvisionStep::ObservePublication
    ) && matches!(
        kind,
        ProviderCommandObservationKind::Succeeded
            | ProviderCommandObservationKind::DefiniteFailure
            | ProviderCommandObservationKind::Absent
    )
}

fn journal_observation(
    observation: &ProviderCommandObservation,
) -> MachineApiWorkloadProvisionObservation {
    let evidence = bounded_evidence(
        observation
            .evidence_sha256()
            .unwrap_or("provider_attempt_claimed"),
    );
    match observation.kind() {
        ProviderCommandObservationKind::Succeeded => {
            MachineApiWorkloadProvisionObservation::Succeeded { evidence }
        }
        ProviderCommandObservationKind::DefiniteFailure => {
            MachineApiWorkloadProvisionObservation::DefiniteFailure { evidence }
        }
        ProviderCommandObservationKind::Absent => {
            MachineApiWorkloadProvisionObservation::Absent { evidence }
        }
        ProviderCommandObservationKind::Claimed | ProviderCommandObservationKind::InProgress => {
            MachineApiWorkloadProvisionObservation::InProgress { evidence }
        }
        ProviderCommandObservationKind::Ambiguous => {
            MachineApiWorkloadProvisionObservation::Ambiguous { evidence }
        }
    }
}

fn handle_result(
    result: Result<SandboxHandle, SandboxError>,
) -> MachineApiWorkloadProvisionObservation {
    match result {
        Ok(handle) => match serde_json::to_vec(&handle) {
            Ok(evidence) => MachineApiWorkloadProvisionObservation::Succeeded {
                evidence: bounded_bytes(evidence),
            },
            Err(error) => ambiguous(error.to_string()),
        },
        Err(error @ (SandboxError::InvalidSpec { .. } | SandboxError::NotFound { .. })) => {
            definite_failure(error.to_string())
        }
        Err(error) => ambiguous(error.to_string()),
    }
}

fn optional_handle_result(
    result: Result<Option<SandboxHandle>, SandboxError>,
) -> MachineApiWorkloadProvisionObservation {
    match result {
        Ok(Some(handle)) => handle_result(Ok(handle)),
        Ok(None) => MachineApiWorkloadProvisionObservation::Absent {
            evidence: b"sandbox phase is absent".to_vec(),
        },
        Err(error) => ambiguous(error.to_string()),
    }
}

fn phase_result(
    result: Result<SandboxProvisionPhaseObservation, SandboxError>,
) -> MachineApiWorkloadProvisionObservation {
    match result {
        Ok(SandboxProvisionPhaseObservation::Succeeded { evidence }) => {
            MachineApiWorkloadProvisionObservation::Succeeded {
                evidence: bounded_bytes(evidence),
            }
        }
        Ok(SandboxProvisionPhaseObservation::Absent { evidence }) => {
            MachineApiWorkloadProvisionObservation::Absent {
                evidence: bounded_bytes(evidence),
            }
        }
        Ok(SandboxProvisionPhaseObservation::InProgress { evidence }) => {
            MachineApiWorkloadProvisionObservation::InProgress {
                evidence: bounded_bytes(evidence),
            }
        }
        Ok(SandboxProvisionPhaseObservation::Ambiguous { evidence }) => {
            MachineApiWorkloadProvisionObservation::Ambiguous {
                evidence: bounded_bytes(evidence),
            }
        }
        Err(error @ (SandboxError::InvalidSpec { .. } | SandboxError::NotFound { .. })) => {
            definite_failure(error.to_string())
        }
        Err(error) => ambiguous(error.to_string()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostStatusSuccess {
    Activated,
    Ready,
}

fn host_status_result(
    result: nimbus::Result<HostLifecycleStatus>,
    success: HostStatusSuccess,
) -> MachineApiWorkloadProvisionObservation {
    match result {
        Ok(status) => {
            let phase = status.phase();
            let evidence = match serde_json::to_vec(&status) {
                Ok(evidence) => bounded_bytes(evidence),
                Err(error) => return ambiguous(error.to_string()),
            };
            host_phase_result(phase, success, evidence)
        }
        Err(error @ (nimbus::Error::InvalidInput(_) | nimbus::Error::PermissionDenied(_))) => {
            definite_failure(error.to_string())
        }
        Err(nimbus::Error::NotFound(message)) => MachineApiWorkloadProvisionObservation::Absent {
            evidence: bounded_evidence(message),
        },
        Err(error) => ambiguous(error.to_string()),
    }
}

fn host_phase_result(
    phase: TenantWorkloadPhase,
    success: HostStatusSuccess,
    evidence: Vec<u8>,
) -> MachineApiWorkloadProvisionObservation {
    match (success, phase) {
        (
            HostStatusSuccess::Activated,
            TenantWorkloadPhase::Running | TenantWorkloadPhase::Ready,
        )
        | (HostStatusSuccess::Ready, TenantWorkloadPhase::Ready) => {
            MachineApiWorkloadProvisionObservation::Succeeded { evidence }
        }
        (
            HostStatusSuccess::Activated | HostStatusSuccess::Ready,
            TenantWorkloadPhase::Pending
            | TenantWorkloadPhase::Bound
            | TenantWorkloadPhase::Running,
        ) => MachineApiWorkloadProvisionObservation::InProgress { evidence },
        (
            HostStatusSuccess::Activated | HostStatusSuccess::Ready,
            TenantWorkloadPhase::Deleting | TenantWorkloadPhase::Denied,
        ) => MachineApiWorkloadProvisionObservation::DefiniteFailure { evidence },
        (
            HostStatusSuccess::Activated | HostStatusSuccess::Ready,
            TenantWorkloadPhase::Degraded,
        ) => MachineApiWorkloadProvisionObservation::Ambiguous { evidence },
    }
}

fn definite_failure(evidence: impl AsRef<[u8]>) -> MachineApiWorkloadProvisionObservation {
    MachineApiWorkloadProvisionObservation::DefiniteFailure {
        evidence: bounded_evidence(evidence),
    }
}

fn ambiguous(evidence: impl AsRef<[u8]>) -> MachineApiWorkloadProvisionObservation {
    MachineApiWorkloadProvisionObservation::Ambiguous {
        evidence: bounded_evidence(evidence),
    }
}

fn bounded_evidence(evidence: impl AsRef<[u8]>) -> Vec<u8> {
    bounded_bytes(evidence.as_ref().to_vec())
}

fn bounded_bytes(mut evidence: Vec<u8>) -> Vec<u8> {
    evidence.truncate(MAX_MACHINE_API_PROVISION_EVIDENCE_BYTES);
    evidence
}

pub(super) fn sandbox_network_plan(
    command: &MachineApiWorkloadProvisionCommandEnvelope,
    spec: &SandboxSpec,
) -> Result<SandboxProvisionNetworkPlan, MachineApiWorkloadProvisionObservation> {
    sandbox_network_plan_for(command.compiled_network_plan(), command.generation(), spec)
}

pub(super) fn sandbox_network_plan_for(
    compiled_network_plan: &nimbus_workloads::CompiledWorkloadNetworkPlan,
    generation: nimbus_workloads::WorkloadGeneration,
    spec: &SandboxSpec,
) -> Result<SandboxProvisionNetworkPlan, MachineApiWorkloadProvisionObservation> {
    let content = compiled_network_plan.content();
    if content.identity().tenant_id() != &spec.tenant_id
        || content.identity().generation().as_u64() != generation.as_u64()
    {
        return Err(definite_failure(
            "compiled network-plan tenant or generation is crossed",
        ));
    }
    let attachment = content
        .attachment()
        .ok_or_else(|| definite_failure("compiled network plan lacks an attachment"))?;
    let plan_id = content.identity().plan_id();
    let mut listeners = Vec::with_capacity(content.listeners().len());
    for blueprint in content.listeners() {
        let binding = spec
            .port_bindings
            .iter()
            .find(|binding| binding.name == blueprint.name())
            .ok_or_else(|| definite_failure("compiled listener is absent from executable"))?
            .clone();
        if binding.protocol != blueprint.protocol()
            || binding.host_address != blueprint.desired_host_address()
            || Some(binding.guest_port) != blueprint.guest_port()
            || !port_request_matches(binding.host_port, blueprint.port_request())
        {
            return Err(definite_failure(
                "compiled listener diverges from executable binding",
            ));
        }
        let request = PortLeaseRequest::new(
            blueprint.port_lease_id().clone(),
            blueprint.listener_id().clone().into(),
            Some(spec.tenant_id.clone()),
            PortLeaseFence::new(content.identity().generation(), NetworkLeaseEpoch::new(1)),
            PortLeaseAccounting::TenantPublished,
            PortPublicationIntent::host(blueprint.desired_host_address()),
            PortBindingSpec::new(
                PortProtocol::Tcp,
                PortBindRealm::Host,
                port_bind_target(blueprint.desired_host_address())?,
                port_exposure(blueprint.desired_host_address()),
                match blueprint.port_request() {
                    WorkloadNetworkPortRequestMode::Exact { port } => PortRequestMode::Exact(port),
                    WorkloadNetworkPortRequestMode::ProviderAssigned => {
                        PortRequestMode::ProviderAssigned
                    }
                },
            ),
        )
        .with_plan_id(plan_id.clone());
        listeners.push(SandboxProvisionListener::new(
            blueprint.listener_id().clone(),
            binding,
            request,
        ));
    }
    if spec.port_bindings.len() != listeners.len() {
        return Err(definite_failure(
            "executable bindings contain a listener absent from the compiled plan",
        ));
    }
    let dependency_listeners = content.dependency_listeners().iter().map(|dependency| {
        SandboxProvisionDependencyListener::new(
            dependency.listener_id().clone(),
            dependency.name(),
            dependency.provider_id().clone(),
        )
    });
    SandboxProvisionNetworkPlan::new(
        compiled_network_plan.plan().clone(),
        spec.tenant_id.clone(),
        content.identity().generation(),
        attachment.attachment_id().clone(),
        listeners,
        dependency_listeners,
    )
    .map_err(|error| definite_failure(error.to_string()))
}

pub(super) fn port_request_matches(
    host_port: u16,
    request: WorkloadNetworkPortRequestMode,
) -> bool {
    match request {
        WorkloadNetworkPortRequestMode::Exact { port } => host_port == port.get(),
        WorkloadNetworkPortRequestMode::ProviderAssigned => host_port == 0,
    }
}

pub(super) fn port_bind_target(
    address: IpAddr,
) -> Result<PortBindTarget, MachineApiWorkloadProvisionObservation> {
    match address {
        IpAddr::V4(address) if address == Ipv4Addr::UNSPECIFIED => {
            Ok(PortBindTarget::ipv4_wildcard())
        }
        IpAddr::V4(address) => Ok(PortBindTarget::ipv4_specific(address)),
        IpAddr::V6(address) if address == Ipv6Addr::UNSPECIFIED => {
            Ok(PortBindTarget::ipv6_wildcard(PortIpv6Overlap::Unknown))
        }
        IpAddr::V6(address) => PortBindTarget::ipv6_specific(address, PortIpv6Overlap::Unknown)
            .map_err(|error| definite_failure(error.to_string())),
    }
}

pub(super) fn port_exposure(address: IpAddr) -> PortExposure {
    match address {
        address if address.is_loopback() => PortExposure::Loopback,
        IpAddr::V4(address) if address.is_private() || address.is_link_local() => {
            PortExposure::Private
        }
        IpAddr::V6(address) if address.is_unique_local() || address.is_unicast_link_local() => {
            PortExposure::Private
        }
        _ => PortExposure::Public,
    }
}

#[cfg(test)]
pub(crate) mod tests;
