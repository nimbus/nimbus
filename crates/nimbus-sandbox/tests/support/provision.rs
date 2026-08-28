//! Shared Linux live-provider fixtures for the phased sandbox provision seam.
//!
//! These helpers are test-only stand-ins for compute's orchestration and the
//! external ingress owner. They deliberately call every provider phase and do
//! not restore a sandbox-owned coarse lifecycle entry point.

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use nimbus_network::{
    ListenerId, LocalPortLeaseAuthority, NetworkAttachmentId, NetworkCapabilitySourceDigest,
    NetworkLeaseEpoch, NetworkPlan, NetworkPlanContentDigest, NetworkPlanId,
    NetworkReservationClaim, NetworkResourceGeneration, PortBindRealm, PortBindTarget,
    PortBindingSpec, PortExposure, PortIpv6Overlap, PortLeaseAccounting, PortLeaseFence,
    PortLeaseId, PortLeaseRequest, PortProtocol, PortPublicationIntent, PortRequestMode,
};
use nimbus_sandbox::backends::container::{
    CONTAINER_EXECUTION_TEARDOWN_PROVIDER_KEY, ContainerSandboxBackend,
};
use nimbus_sandbox::backends::krun::KrunSandboxBackend;
use nimbus_sandbox::backends::{
    CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY, KRUN_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
};
use nimbus_sandbox::{
    ProviderCommandClaim, ProviderCommandClaimDecision, ProviderCommandClaimInput,
    ProviderCommandObservationKind, SandboxExecutionAttemptId, SandboxExecutionTeardownCommand,
    SandboxExecutionTeardownObservation, SandboxExecutionTeardownOperation, SandboxHandle,
    SandboxId, SandboxNetworkTeardownCommand, SandboxNetworkTeardownCommandInput,
    SandboxNetworkTeardownIdentity, SandboxNetworkTeardownIdentityInput,
    SandboxNetworkTeardownObservation, SandboxNetworkTeardownOperation,
    SandboxProvisionDependencyListener, SandboxProvisionEndpointIdentity, SandboxProvisionListener,
    SandboxProvisionNetworkPlan, SandboxProvisionPhaseObservation, SandboxSpec,
    sandbox_network_plan_requirements,
};
use sha2::{Digest, Sha256};

static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(1);
const EXACT_TEARDOWN_INSPECTION_TIMEOUT: Duration = Duration::from_secs(35);
const EXACT_TEARDOWN_RETRY_EPOCHS: u64 = 8;

struct ExactTeardownProviderKeys<'a> {
    execution: &'a str,
    attachment: &'a str,
}

enum ExactTeardownAttempt {
    Observation(Box<nimbus_sandbox::ProviderCommandObservation>),
    RetryAtEpoch(u64),
}

pub(crate) struct ProvisionedSandbox {
    pub(crate) handle: SandboxHandle,
    pub(crate) ingress: TestIngressSet,
    pub(crate) teardown: ExactTeardownFixture,
}

#[derive(Clone)]
pub(crate) struct ExactTeardownFixture {
    tenant_id: nimbus_core::TenantId,
    sandbox_id: SandboxId,
    execution_attempt_id: SandboxExecutionAttemptId,
    network_plan: SandboxProvisionNetworkPlan,
    network_state_root: PathBuf,
    reservation_claim: NetworkReservationClaim,
    egress_lease: Option<PortLeaseRequest>,
}

#[allow(dead_code)] // Included separately by Container-only and krun-only integration targets.
pub(crate) fn provision_container(
    backend: &ContainerSandboxBackend,
    workload_state_root: &Path,
    spec: SandboxSpec,
    install_ingress: bool,
) -> nimbus_sandbox::Result<ProvisionedSandbox> {
    let id = fixture_id("container", spec.display_name());
    let plan = compiled_network_plan(&spec, &id);
    let attempt = fixture_attempt_id(&id);
    backend.reserve_provision_network(spec, id.clone(), attempt.clone(), plan)?;
    let teardown = exact_teardown_fixture(read_manifest(workload_state_root, &id)?, &id);
    let provision = (|| {
        backend.prepare_provision_workload(&id, &attempt)?;
        require_succeeded(
            "container attachment",
            backend.attach_provision_network(&id, &attempt)?,
        )?;
        require_succeeded(
            "container activation prerequisite",
            backend.inspect_provision_activation_prerequisites(&id, &attempt)?,
        )?;
        require_succeeded(
            "container activation",
            backend.activate_provision_workload(&id, &attempt)?,
        )?;
        require_readiness_observation(
            "container readiness",
            backend.inspect_provision_workload_readiness(&id, &attempt)?,
        )?;
        finish_fixture(
            read_manifest(workload_state_root, &id)?,
            install_ingress,
            TestIngressTarget::Container,
        )
    })();
    finish_or_compensate(provision, teardown, |fixture| {
        retire_container(backend, fixture)
    })
}

#[allow(dead_code)] // Included separately by Container-only and krun-only integration targets.
pub(crate) fn provision_krun(
    backend: &KrunSandboxBackend,
    workload_state_root: &Path,
    spec: SandboxSpec,
    install_ingress: bool,
) -> nimbus_sandbox::Result<ProvisionedSandbox> {
    let id = fixture_id("krun", spec.display_name());
    let plan = compiled_network_plan(&spec, &id);
    let attempt = fixture_attempt_id(&id);
    backend.reserve_provision_network(spec, id.clone(), attempt.clone(), plan)?;
    let teardown = exact_teardown_fixture(read_manifest(workload_state_root, &id)?, &id);
    let provision = (|| {
        backend.prepare_provision_workload(&id, &attempt)?;
        require_succeeded(
            "krun attachment",
            backend.attach_provision_network(&id, &attempt)?,
        )?;
        require_succeeded(
            "krun activation prerequisite",
            backend.inspect_provision_activation_prerequisites(&id, &attempt)?,
        )?;
        require_succeeded(
            "krun activation",
            backend.activate_provision_workload(&id, &attempt)?,
        )?;
        require_readiness_observation(
            "krun readiness",
            backend.inspect_provision_workload_readiness(&id, &attempt)?,
        )?;
        finish_fixture(
            read_manifest(workload_state_root, &id)?,
            install_ingress,
            TestIngressTarget::KrunTsi,
        )
    })();
    finish_or_compensate(provision, teardown, |fixture| retire_krun(backend, fixture))
}

fn finish_or_compensate(
    provision: nimbus_sandbox::Result<(SandboxHandle, TestIngressSet)>,
    teardown: ExactTeardownFixture,
    retire: impl FnOnce(&ExactTeardownFixture) -> nimbus_sandbox::Result<()>,
) -> nimbus_sandbox::Result<ProvisionedSandbox> {
    match provision {
        Ok((handle, ingress)) => Ok(ProvisionedSandbox {
            handle,
            ingress,
            teardown,
        }),
        Err(primary) => match retire(&teardown) {
            Ok(()) => Err(primary),
            Err(cleanup) => Err(nimbus_sandbox::SandboxError::OperationFailed {
                message: format!(
                    "Linux live-provider fixture failed after reservation: {primary}; exact teardown also failed: {cleanup}"
                ),
            }),
        },
    }
}

/// Drive the same four exact provider streams as compute for a live Container
/// fixture. The caller must retain the immutable provision identity; an ID-only
/// cleanup path would recreate the coarse authority that these tests protect.
#[allow(dead_code)] // Included separately by Container-only and krun-only integration targets.
pub(crate) fn retire_container(
    backend: &ContainerSandboxBackend,
    fixture: &ExactTeardownFixture,
) -> nimbus_sandbox::Result<()> {
    retire_exact(
        fixture,
        ExactTeardownProviderKeys {
            execution: CONTAINER_EXECUTION_TEARDOWN_PROVIDER_KEY,
            attachment: CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
        },
        |command, execution| {
            backend
                .execute_execution_teardown_with_claim(command, execution)
                .map_err(provider_journal_error)
        },
        |command, observation| {
            backend.inspect_execution_teardown_with_observation(command, observation)
        },
        |command, execution| {
            backend
                .execute_network_teardown_with_claim(command, execution)
                .map_err(provider_journal_error)
        },
        |command, observation| {
            backend.inspect_network_teardown_with_observation(command, observation)
        },
        || {
            backend
                .attempt_idempotency_journal()
                .map_err(provider_journal_error)
        },
    )
}

/// Drive the same four exact provider streams as compute for a live Krun
/// fixture. This is test composition and does not add a backend lifecycle API.
#[allow(dead_code)] // Included separately by Container-only and krun-only integration targets.
pub(crate) fn retire_krun(
    backend: &KrunSandboxBackend,
    fixture: &ExactTeardownFixture,
) -> nimbus_sandbox::Result<()> {
    retire_exact(
        fixture,
        ExactTeardownProviderKeys {
            execution: "nimbus-sandbox.krun-execution",
            attachment: KRUN_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
        },
        |command, execution| {
            backend
                .execute_execution_teardown_with_claim(command, execution)
                .map_err(provider_journal_error)
        },
        |command, observation| {
            backend.inspect_execution_teardown_with_observation(command, observation)
        },
        |command, execution| {
            backend
                .execute_network_teardown_with_claim(command, execution)
                .map_err(provider_journal_error)
        },
        |command, observation| {
            backend.inspect_network_teardown_with_observation(command, observation)
        },
        || {
            backend
                .attempt_idempotency_journal()
                .map_err(provider_journal_error)
        },
    )
}

fn exact_teardown_fixture(
    manifest: ProviderManifestProjection,
    sandbox_id: &SandboxId,
) -> ExactTeardownFixture {
    ExactTeardownFixture {
        tenant_id: manifest.handle.tenant_id,
        sandbox_id: sandbox_id.clone(),
        execution_attempt_id: manifest.execution_attempt_id,
        network_plan: manifest.provision_network_plan,
        network_state_root: manifest.network_layout.network_state_root,
        reservation_claim: manifest.network_config.reservation_claim,
        egress_lease: manifest.egress_proxy.map(|proxy| proxy.port_lease),
    }
}

fn retire_exact(
    fixture: &ExactTeardownFixture,
    provider_keys: ExactTeardownProviderKeys<'_>,
    mut execute_execution: impl FnMut(
        &SandboxExecutionTeardownCommand,
        nimbus_sandbox::ProviderCommandExecutionClaim,
    ) -> nimbus_sandbox::Result<
        nimbus_sandbox::ProviderCommandObservation,
    >,
    mut inspect_execution: impl FnMut(
        &SandboxExecutionTeardownCommand,
        &nimbus_sandbox::ProviderCommandObservation,
    ) -> SandboxExecutionTeardownObservation,
    mut execute_network: impl FnMut(
        &SandboxNetworkTeardownCommand,
        nimbus_sandbox::ProviderCommandExecutionClaim,
    ) -> nimbus_sandbox::Result<
        nimbus_sandbox::ProviderCommandObservation,
    >,
    mut inspect_network: impl FnMut(
        &SandboxNetworkTeardownCommand,
        &nimbus_sandbox::ProviderCommandObservation,
    ) -> SandboxNetworkTeardownObservation,
    mut journal: impl FnMut() -> nimbus_sandbox::Result<nimbus_sandbox::ProviderCommandAttemptJournal>,
) -> nimbus_sandbox::Result<()> {
    settle_test_ingress_without_effect(fixture)?;

    for operation in [
        SandboxExecutionTeardownOperation::Drain,
        SandboxExecutionTeardownOperation::Stop,
    ] {
        let provider_target_digest = format!(
            "{:x}",
            Sha256::digest(format!("linux-smoke-execution:{}", provider_keys.execution))
        );
        let effect_subject = format!("{{\"sandbox\":\"{}\"}}", fixture.sandbox_id);
        let mut completed = false;
        let mut dispatch_epoch = 1;
        for _ in 0..EXACT_TEARDOWN_RETRY_EPOCHS {
            let claim = fixture.claim(
                operation.provider_operation(),
                &effect_subject,
                &provider_target_digest,
                dispatch_epoch,
            )?;
            let command = SandboxExecutionTeardownCommand::new(
                fixture.tenant_id.clone(),
                fixture.sandbox_id.clone(),
                fixture.execution_attempt_id.clone(),
                provider_keys.execution,
                operation,
                claim,
            )
            .map_err(exact_teardown_error)?;
            let current = match execute_claimed(
                &journal()?,
                command.provider_claim(),
                |execution| execute_execution(&command, execution),
                |observation| exact_execution_inspection(inspect_execution(&command, observation)),
            )? {
                ExactTeardownAttempt::Observation(current) => current,
                ExactTeardownAttempt::RetryAtEpoch(current) => {
                    dispatch_epoch = current;
                    continue;
                }
            };
            match current.kind() {
                ProviderCommandObservationKind::Succeeded
                | ProviderCommandObservationKind::Absent => {
                    completed = true;
                    break;
                }
                ProviderCommandObservationKind::RetryAuthorized => {
                    dispatch_epoch = next_dispatch_epoch(&current)?;
                }
                _ => return Err(nonterminal_teardown_error(operation, &current)),
            }
        }
        if !completed {
            return Err(retry_exhausted_teardown_error(operation));
        }
    }

    for operation in [
        SandboxNetworkTeardownOperation::Detach,
        SandboxNetworkTeardownOperation::Release,
    ] {
        let identity = SandboxNetworkTeardownIdentity::new(SandboxNetworkTeardownIdentityInput {
            tenant_id: fixture.tenant_id.clone(),
            sandbox_id: fixture.sandbox_id.clone(),
            execution_attempt_id: fixture.execution_attempt_id.clone(),
            attachment_id: fixture.network_plan.attachment_id().clone(),
            network_plan: fixture.network_plan.network_plan().clone(),
            provider_registration_key: provider_keys.attachment.to_owned(),
            provider_source_digest: NetworkCapabilitySourceDigest::from_bytes([9; 32]),
        })
        .map_err(exact_teardown_error)?;
        let effect_subject = identity.provider_effect_subject();
        let provider_target_digest = identity.provider_target_digest();
        let mut completed = false;
        let mut dispatch_epoch = 1;
        for _ in 0..EXACT_TEARDOWN_RETRY_EPOCHS {
            let claim = fixture.claim(
                operation.provider_operation(),
                &effect_subject,
                &provider_target_digest,
                dispatch_epoch,
            )?;
            let command = SandboxNetworkTeardownCommand::new(SandboxNetworkTeardownCommandInput {
                identity: identity.clone(),
                operation,
                provider_claim: claim,
            })
            .map_err(exact_teardown_error)?;
            let current = match execute_claimed(
                &journal()?,
                command.provider_claim(),
                |execution| execute_network(&command, execution),
                |observation| exact_network_inspection(inspect_network(&command, observation)),
            )? {
                ExactTeardownAttempt::Observation(current) => current,
                ExactTeardownAttempt::RetryAtEpoch(current) => {
                    dispatch_epoch = current;
                    continue;
                }
            };
            match current.kind() {
                ProviderCommandObservationKind::Succeeded
                | ProviderCommandObservationKind::Absent => {
                    completed = true;
                    break;
                }
                ProviderCommandObservationKind::RetryAuthorized => {
                    dispatch_epoch = next_dispatch_epoch(&current)?;
                }
                _ => return Err(nonterminal_teardown_error(operation, &current)),
            }
        }
        if !completed {
            return Err(retry_exhausted_teardown_error(operation));
        }
    }
    Ok(())
}

fn settle_test_ingress_without_effect(
    fixture: &ExactTeardownFixture,
) -> nimbus_sandbox::Result<()> {
    let published = fixture.network_plan.port_leases();
    if published.is_empty() {
        return Ok(());
    }
    let mut plan_members = published.clone();
    if let Some(egress_lease) = &fixture.egress_lease {
        plan_members.push(egress_lease.clone());
    }
    LocalPortLeaseAuthority::open(&fixture.network_state_root)
        .map_err(exact_teardown_error)?
        .release_reserved_plan_members_without_effect(
            &plan_members,
            &published,
            &fixture.reservation_claim,
        )
        .map(|_| ())
        .map_err(exact_teardown_error)
}

fn next_dispatch_epoch(
    observation: &nimbus_sandbox::ProviderCommandObservation,
) -> nimbus_sandbox::Result<u64> {
    observation
        .claim()
        .dispatch_epoch()
        .checked_add(1)
        .ok_or_else(|| nimbus_sandbox::SandboxError::OperationFailed {
            message: "Linux smoke exact teardown exhausted the provider dispatch epoch space"
                .to_owned(),
        })
}

fn execute_claimed(
    journal: &nimbus_sandbox::ProviderCommandAttemptJournal,
    claim: &ProviderCommandClaim,
    execute: impl FnOnce(
        nimbus_sandbox::ProviderCommandExecutionClaim,
    ) -> nimbus_sandbox::Result<nimbus_sandbox::ProviderCommandObservation>,
    mut inspect: impl FnMut(&nimbus_sandbox::ProviderCommandObservation) -> ExactTeardownInspection,
) -> nimbus_sandbox::Result<ExactTeardownAttempt> {
    let decision = match journal.claim_dispatch_epoch(claim) {
        Ok(decision) => decision,
        Err(nimbus_sandbox::ProviderCommandJournalError::StaleDispatchEpoch {
            current, ..
        }) => {
            return Ok(ExactTeardownAttempt::RetryAtEpoch(current));
        }
        Err(error) => return Err(provider_journal_error(error)),
    };
    let mut current = match decision {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execute(execution)?,
        ProviderCommandClaimDecision::AdoptExactAttempt(observation)
            if observation.kind() == ProviderCommandObservationKind::Claimed =>
        {
            let execution = journal
                .resume_current_claim(&observation)
                .map_err(provider_journal_error)?;
            execute(execution)?
        }
        ProviderCommandClaimDecision::AdoptExactAttempt(observation) => observation,
    };
    let deadline = Instant::now() + EXACT_TEARDOWN_INSPECTION_TIMEOUT;
    loop {
        match current.kind() {
            ProviderCommandObservationKind::Succeeded
            | ProviderCommandObservationKind::DefiniteFailure
            | ProviderCommandObservationKind::Absent
            | ProviderCommandObservationKind::RetryAuthorized => {
                return Ok(ExactTeardownAttempt::Observation(Box::new(current)));
            }
            ProviderCommandObservationKind::Claimed => {
                return Err(nimbus_sandbox::SandboxError::OperationFailed {
                    message:
                        "Linux smoke exact teardown retained an unowned claimed provider state"
                            .to_owned(),
                });
            }
            ProviderCommandObservationKind::InProgress
            | ProviderCommandObservationKind::Ambiguous => {}
        }
        if Instant::now() >= deadline {
            return Err(nimbus_sandbox::SandboxError::OperationFailed {
                message: format!(
                    "Linux smoke exact teardown inspection timed out in provider state {:?}",
                    current.kind()
                ),
            });
        }
        let inspected = inspect(&current);
        current = journal
            .record_observation_with_failure_code(
                claim,
                inspected.kind,
                inspected.failure_code.as_deref(),
                &inspected.evidence,
            )
            .map_err(provider_journal_error)?;
        if matches!(
            current.kind(),
            ProviderCommandObservationKind::InProgress | ProviderCommandObservationKind::Ambiguous
        ) {
            thread::sleep(Duration::from_millis(50));
        }
    }
}

struct ExactTeardownInspection {
    kind: ProviderCommandObservationKind,
    failure_code: Option<String>,
    evidence: Vec<u8>,
}

fn exact_execution_inspection(
    observation: SandboxExecutionTeardownObservation,
) -> ExactTeardownInspection {
    let kind = match &observation {
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
    };
    ExactTeardownInspection {
        kind,
        failure_code: observation.failure_code().map(str::to_owned),
        evidence: observation.evidence().to_vec(),
    }
}

fn exact_network_inspection(
    observation: SandboxNetworkTeardownObservation,
) -> ExactTeardownInspection {
    let kind = match &observation {
        SandboxNetworkTeardownObservation::Succeeded { .. } => {
            ProviderCommandObservationKind::Succeeded
        }
        SandboxNetworkTeardownObservation::DefiniteFailure { .. } => {
            ProviderCommandObservationKind::DefiniteFailure
        }
        SandboxNetworkTeardownObservation::Absent { .. } => ProviderCommandObservationKind::Absent,
        SandboxNetworkTeardownObservation::RetryAuthorized { .. } => {
            ProviderCommandObservationKind::RetryAuthorized
        }
        SandboxNetworkTeardownObservation::InProgress { .. } => {
            ProviderCommandObservationKind::InProgress
        }
        SandboxNetworkTeardownObservation::Ambiguous { .. } => {
            ProviderCommandObservationKind::Ambiguous
        }
    };
    ExactTeardownInspection {
        kind,
        failure_code: observation.failure_code().map(str::to_owned),
        evidence: observation.evidence().to_vec(),
    }
}

fn nonterminal_teardown_error(
    operation: impl std::fmt::Debug,
    observation: &nimbus_sandbox::ProviderCommandObservation,
) -> nimbus_sandbox::SandboxError {
    nimbus_sandbox::SandboxError::OperationFailed {
        message: format!(
            "Linux smoke exact teardown operation {operation:?} returned provider state {:?} \
             (failure_code={:?}, evidence_sha256={:?})",
            observation.kind(),
            observation.failure_code(),
            observation.evidence_sha256()
        ),
    }
}

fn retry_exhausted_teardown_error(operation: impl std::fmt::Debug) -> nimbus_sandbox::SandboxError {
    nimbus_sandbox::SandboxError::OperationFailed {
        message: format!(
            "Linux smoke exact teardown operation {operation:?} exhausted \
             {EXACT_TEARDOWN_RETRY_EPOCHS} retry epochs"
        ),
    }
}

impl ExactTeardownFixture {
    #[allow(dead_code)] // Used only by the krun force-teardown integration target.
    pub(crate) fn sandbox_id(&self) -> &SandboxId {
        &self.sandbox_id
    }

    fn claim(
        &self,
        operation: nimbus_sandbox::ProviderCommandOperation,
        effect_subject: &str,
        provider_target_digest: &str,
        dispatch_epoch: u64,
    ) -> nimbus_sandbox::Result<ProviderCommandClaim> {
        ProviderCommandClaim::new(ProviderCommandClaimInput {
            authority_id: format!("linux-smoke-authority:{}", self.sandbox_id),
            effect_subject: effect_subject.to_owned(),
            source_attempt_id: None,
            attempt_id: format!("linux-smoke-retirement:{}", self.sandbox_id),
            dispatch_epoch,
            workload_generation: self.network_plan.generation().as_u64(),
            restart_ordinal: 0,
            desired_digest: "1".repeat(64),
            source_digest: "2".repeat(64),
            network_plan_digest: self.network_plan.network_plan().digest().to_string(),
            provider_target_digest: provider_target_digest.to_owned(),
            operation,
        })
        .map_err(exact_teardown_error)
    }
}

fn provider_journal_error(
    error: nimbus_sandbox::ProviderCommandJournalError,
) -> nimbus_sandbox::SandboxError {
    exact_teardown_error(error)
}

fn exact_teardown_error(error: impl std::fmt::Display) -> nimbus_sandbox::SandboxError {
    nimbus_sandbox::SandboxError::OperationFailed {
        message: format!("Linux smoke exact teardown failed: {error}"),
    }
}

fn fixture_id(provider: &str, display_name: &str) -> SandboxId {
    let sequence = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
    let label = display_name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    SandboxId::new(format!(
        "phase-{provider}-{label}-{}-{sequence}",
        std::process::id()
    ))
}

fn fixture_attempt_id(sandbox_id: &SandboxId) -> SandboxExecutionAttemptId {
    SandboxExecutionAttemptId::new(format!("linux-smoke:{sandbox_id}"))
        .expect("Linux smoke execution attempt should validate")
}

fn compiled_network_plan(spec: &SandboxSpec, id: &SandboxId) -> SandboxProvisionNetworkPlan {
    let incarnation = format!("linux-smoke:{}", id.as_str());
    let generation = NetworkResourceGeneration::new(1);
    let requirements = sandbox_network_plan_requirements(spec.backend);
    let plan = NetworkPlan::new(
        NetworkPlanId::for_tenant_workload_plan(&spec.tenant_id, &incarnation),
        generation,
        NetworkPlanContentDigest::sha256(format!("linux-smoke:{incarnation}")),
        requirements.capability_requirements().clone(),
    );
    let plan_id = plan.plan_id().clone();
    let endpoint_identities = spec.port_bindings.iter().map(|binding| {
        SandboxProvisionEndpointIdentity::new(
            ListenerId::for_tenant_workload_listener(&spec.tenant_id, &incarnation, &binding.name),
            nimbus_network::PublishedEndpointId::for_workload_endpoint(&incarnation, &binding.name),
        )
    });
    let listeners = spec.port_bindings.iter().map(|binding| {
        let listener_id =
            ListenerId::for_tenant_workload_listener(&spec.tenant_id, &incarnation, &binding.name);
        let request = PortLeaseRequest::new(
            PortLeaseId::for_listener(&listener_id),
            listener_id.clone().into(),
            Some(spec.tenant_id.clone()),
            PortLeaseFence::new(generation, NetworkLeaseEpoch::new(1)),
            PortLeaseAccounting::TenantPublished,
            PortPublicationIntent::host(binding.host_address),
            PortBindingSpec::new(
                PortProtocol::Tcp,
                PortBindRealm::Host,
                bind_target(binding.host_address),
                exposure(binding.host_address),
                NonZeroU16::new(binding.host_port)
                    .map_or(PortRequestMode::ProviderAssigned, PortRequestMode::Exact),
            ),
        )
        .with_plan_id(plan_id.clone());
        SandboxProvisionListener::new(
            nimbus_network::PublishedEndpointId::for_workload_endpoint(&incarnation, &binding.name),
            listener_id,
            binding.clone(),
            request,
        )
    });
    SandboxProvisionNetworkPlan::new(
        plan,
        spec.tenant_id.clone(),
        generation,
        NetworkAttachmentId::for_workload_attachment(&incarnation, "primary"),
        endpoint_identities,
        listeners,
        [SandboxProvisionDependencyListener::new(
            ListenerId::for_tenant_workload_listener(&spec.tenant_id, &incarnation, "egress-pep"),
            "egress-pep",
            requirements.pep_provider_id().clone(),
        )],
    )
    .expect("Linux smoke compiled network plan should validate")
}

fn bind_target(address: IpAddr) -> PortBindTarget {
    match address {
        IpAddr::V4(address) if address == Ipv4Addr::UNSPECIFIED => PortBindTarget::ipv4_wildcard(),
        IpAddr::V4(address) => PortBindTarget::ipv4_specific(address),
        IpAddr::V6(address) if address == Ipv6Addr::UNSPECIFIED => {
            PortBindTarget::ipv6_wildcard(PortIpv6Overlap::Unknown)
        }
        IpAddr::V6(address) => PortBindTarget::ipv6_specific(address, PortIpv6Overlap::Unknown)
            .expect("Linux smoke fixture never uses IPv4-mapped IPv6"),
    }
}

fn exposure(address: IpAddr) -> PortExposure {
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

fn require_succeeded(
    phase: &str,
    observation: SandboxProvisionPhaseObservation,
) -> nimbus_sandbox::Result<()> {
    if matches!(
        observation,
        SandboxProvisionPhaseObservation::Succeeded { .. }
    ) {
        return Ok(());
    }
    Err(nimbus_sandbox::SandboxError::OperationFailed {
        message: format!("{phase} did not publish exact success: {observation:?}"),
    })
}

fn require_readiness_observation(
    phase: &str,
    observation: SandboxProvisionPhaseObservation,
) -> nimbus_sandbox::Result<()> {
    if matches!(
        observation,
        SandboxProvisionPhaseObservation::Succeeded { .. }
            | SandboxProvisionPhaseObservation::InProgress { .. }
    ) {
        return Ok(());
    }
    Err(nimbus_sandbox::SandboxError::OperationFailed {
        message: format!("{phase} returned non-progress evidence: {observation:?}"),
    })
}

#[derive(serde::Deserialize)]
struct ProviderManifestProjection {
    handle: SandboxHandle,
    spec: SandboxSpec,
    network_layout: ProviderNetworkLayoutProjection,
    network_config: ProviderNetworkConfigProjection,
    execution_attempt_id: SandboxExecutionAttemptId,
    provision_network_plan: SandboxProvisionNetworkPlan,
    egress_proxy: Option<ProviderEgressProjection>,
}

#[derive(serde::Deserialize)]
struct ProviderNetworkLayoutProjection {
    network_state_root: PathBuf,
    status_path: std::path::PathBuf,
}

#[derive(serde::Deserialize)]
struct ProviderNetworkConfigProjection {
    reservation_claim: NetworkReservationClaim,
}

#[derive(serde::Deserialize)]
struct ProviderEgressProjection {
    port_lease: PortLeaseRequest,
}

#[derive(serde::Deserialize)]
struct ProviderStatusProjection {
    assigned_ips: Vec<Ipv4Addr>,
}

fn read_manifest(
    workload_state_root: &Path,
    id: &SandboxId,
) -> nimbus_sandbox::Result<ProviderManifestProjection> {
    let mut matches = std::fs::read_dir(workload_state_root.join("tenants"))
        .map_err(|error| nimbus_sandbox::SandboxError::OperationFailed {
            message: format!(
                "failed to enumerate Linux smoke tenants under {}: {error}",
                workload_state_root.display()
            ),
        })?
        .filter_map(Result::ok)
        .map(|tenant| {
            tenant
                .path()
                .join("sandboxes")
                .join(id.as_str())
                .join("state")
                .join("containers")
                .join(id.as_str())
                .join("manifest.json")
        })
        .filter(|path| path.is_file());
    let path = matches
        .next()
        .ok_or_else(|| nimbus_sandbox::SandboxError::NotFound {
            sandbox_id: id.as_str().to_owned(),
        })?;
    if matches.next().is_some() {
        return Err(nimbus_sandbox::SandboxError::OperationFailed {
            message: format!(
                "Linux smoke sandbox {} is not tenant-qualified uniquely under {}",
                id,
                workload_state_root.display()
            ),
        });
    }
    serde_json::from_slice(&std::fs::read(&path).map_err(|error| {
        nimbus_sandbox::SandboxError::OperationFailed {
            message: format!(
                "failed to read Linux smoke manifest {}: {error}",
                path.display()
            ),
        }
    })?)
    .map_err(|error| nimbus_sandbox::SandboxError::OperationFailed {
        message: format!(
            "failed to parse Linux smoke manifest {}: {error}",
            path.display()
        ),
    })
}

fn finish_fixture(
    manifest: ProviderManifestProjection,
    install_ingress: bool,
    ingress_target: TestIngressTarget,
) -> nimbus_sandbox::Result<(SandboxHandle, TestIngressSet)> {
    let ingress = if install_ingress {
        let status: ProviderStatusProjection = serde_json::from_slice(
            &std::fs::read(&manifest.network_layout.status_path).map_err(|error| {
                nimbus_sandbox::SandboxError::OperationFailed {
                    message: format!(
                        "failed to read Linux smoke provider status {}: {error}",
                        manifest.network_layout.status_path.display()
                    ),
                }
            })?,
        )
        .map_err(|error| nimbus_sandbox::SandboxError::OperationFailed {
            message: format!("failed to parse Linux smoke provider status: {error}"),
        })?;
        let assigned_ip = status.assigned_ips.first().copied().ok_or_else(|| {
            nimbus_sandbox::SandboxError::OperationFailed {
                message: "Linux smoke provider status has no assigned private address".to_owned(),
            }
        })?;
        TestIngressSet::bind(&manifest.spec, assigned_ip, ingress_target)?
    } else {
        TestIngressSet::default()
    };
    Ok((manifest.handle, ingress))
}

#[derive(Default)]
pub(crate) struct TestIngressSet {
    listeners: Vec<TestIngress>,
}

#[derive(Clone, Copy)]
enum TestIngressTarget {
    Container,
    KrunTsi,
}

impl TestIngressSet {
    fn bind(
        spec: &SandboxSpec,
        assigned_ip: Ipv4Addr,
        target: TestIngressTarget,
    ) -> nimbus_sandbox::Result<Self> {
        let mut listeners = Vec::with_capacity(spec.port_bindings.len());
        for binding in &spec.port_bindings {
            let private_port = match target {
                TestIngressTarget::Container => binding.guest_port,
                TestIngressTarget::KrunTsi => {
                    if binding.host_port == 0 {
                        binding.guest_port
                    } else {
                        binding.host_port
                    }
                }
            };
            listeners.push(TestIngress::bind(
                SocketAddr::new(binding.host_address, binding.host_port),
                SocketAddr::new(assigned_ip.into(), private_port),
            )?);
        }
        Ok(Self { listeners })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.listeners.is_empty()
    }

    #[allow(
        dead_code,
        reason = "used by KVM smoke cases; shared support also compiles for container-only targets"
    )]
    pub(crate) fn addresses(&self) -> Vec<SocketAddr> {
        self.listeners
            .iter()
            .map(|listener| listener.wake_address)
            .collect()
    }
}

struct TestIngress {
    stop: Arc<AtomicBool>,
    wake_address: SocketAddr,
    worker: Option<thread::JoinHandle<()>>,
}

impl TestIngress {
    fn bind(listen_address: SocketAddr, target: SocketAddr) -> nimbus_sandbox::Result<Self> {
        let listener = TcpListener::bind(listen_address).map_err(|error| {
            nimbus_sandbox::SandboxError::OperationFailed {
                message: format!("test ingress failed to bind {listen_address}: {error}"),
            }
        })?;
        let wake_address = listener.local_addr().map_err(|error| {
            nimbus_sandbox::SandboxError::OperationFailed {
                message: format!("test ingress failed to inspect {listen_address}: {error}"),
            }
        })?;
        listener.set_nonblocking(true).map_err(|error| {
            nimbus_sandbox::SandboxError::OperationFailed {
                message: format!("test ingress failed to make {wake_address} nonblocking: {error}"),
            }
        })?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = thread::Builder::new()
            .name(format!("linux-smoke-ingress-{}", wake_address.port()))
            .spawn(move || {
                while !worker_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((client, _)) => {
                            thread::spawn(move || forward_connection(client, target));
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(_) => break,
                    }
                }
            })
            .map_err(|error| nimbus_sandbox::SandboxError::OperationFailed {
                message: format!("test ingress failed to spawn for {wake_address}: {error}"),
            })?;
        Ok(Self {
            stop,
            wake_address,
            worker: Some(worker),
        })
    }
}

impl Drop for TestIngress {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect_timeout(&self.wake_address, Duration::from_millis(100));
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn forward_connection(client: TcpStream, target: SocketAddr) {
    let Ok(upstream) = TcpStream::connect_timeout(&target, Duration::from_secs(5)) else {
        return;
    };
    let _ = client.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = upstream.set_read_timeout(Some(Duration::from_secs(30)));
    let (Ok(mut client_read), Ok(mut upstream_write)) = (client.try_clone(), upstream.try_clone())
    else {
        return;
    };
    let one_direction = thread::spawn(move || io::copy(&mut client_read, &mut upstream_write));
    let mut upstream_read = upstream;
    let mut client_write = client;
    let _ = io::copy(&mut upstream_read, &mut client_write);
    let _ = one_direction.join();
}
