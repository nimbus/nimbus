//! Exact provider-local authority for one compute-confirmed restart command.
//!
//! This value contains no restart policy, schedule, retry loop, or transport.
//! An authenticated upper adapter lowers one already-confirmed command into
//! this claim. The node uses it only to fence one systemd effect or inspection.

use nimbus_core::{Error, Result};
use nimbus_workloads::{
    WorkloadExecutionProviderId, WorkloadExecutionReference, WorkloadGeneration,
    WorkloadProvisionSourceDigest, WorkloadProvisionSourceGeneration, WorkloadRestartCommandId,
    WorkloadRestartDispatchEpoch, WorkloadRestartEpoch, WorkloadRestartRequestId,
    WorkloadRestartStep, WorkloadSagaId, WorkloadSagaRevision, WorkloadSagaTransitionId,
};
use serde::Serialize;

use super::{
    HostActivationFence, HostLifecycleRequest, HostProviderPlan, SystemdUnitKind, SystemdUnitName,
};
#[cfg(any(test, all(target_os = "linux", feature = "systemd-dbus")))]
use super::{invalid_activation_fence, parse_fence_counter};

/// Complete input for one node-owned restart provider claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRestartProviderClaimInput {
    pub saga_id: WorkloadSagaId,
    pub transition_id: WorkloadSagaTransitionId,
    pub command_id: WorkloadRestartCommandId,
    pub request_id: WorkloadRestartRequestId,
    pub source_execution: WorkloadExecutionReference,
    pub execution: WorkloadExecutionReference,
    pub restart_epoch: WorkloadRestartEpoch,
    pub dispatch_epoch: WorkloadRestartDispatchEpoch,
    pub issuing_revision: WorkloadSagaRevision,
    pub confirmed_revision: WorkloadSagaRevision,
    pub source_generation: WorkloadProvisionSourceGeneration,
    pub source_digest: WorkloadProvisionSourceDigest,
    pub network_plan_digest: String,
    pub provider_selection: WorkloadExecutionProviderId,
    pub step: WorkloadRestartStep,
    pub mode: HostRestartProviderMode,
}

/// Durable command authority authenticated by the upper restart coordinator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostRestartProviderMode {
    /// Apply the one exact effect confirmed at `issuing_revision + 1`.
    Execute,
    /// Inspect without effect authority.
    Inspect {
        /// Later desired generation that permanently vetoed further effects.
        successor_veto_generation: Option<WorkloadGeneration>,
    },
}

impl HostRestartProviderMode {
    pub const fn inspect() -> Self {
        Self::Inspect {
            successor_veto_generation: None,
        }
    }

    pub const fn inspect_after_successor_veto(
        successor_veto_generation: WorkloadGeneration,
    ) -> Self {
        Self::Inspect {
            successor_veto_generation: Some(successor_veto_generation),
        }
    }
}

impl HostRestartProviderClaimInput {
    pub const fn execute_mode() -> HostRestartProviderMode {
        HostRestartProviderMode::Execute
    }

    pub const fn inspect_mode() -> HostRestartProviderMode {
        HostRestartProviderMode::inspect()
    }

    pub const fn inspect_mode_after_successor_veto(
        successor_veto_generation: WorkloadGeneration,
    ) -> HostRestartProviderMode {
        HostRestartProviderMode::inspect_after_successor_veto(successor_veto_generation)
    }
}

/// Provider-local projection of one exact compute-confirmed restart command.
///
/// Construction checks the complete source-to-target attempt chain. This value
/// does not grant scheduling or retry authority and is not durable desired
/// state. Systemd retains its exact activation projection beside the effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRestartProviderClaim {
    pub(super) saga_id: WorkloadSagaId,
    pub(super) transition_id: WorkloadSagaTransitionId,
    pub(super) command_id: WorkloadRestartCommandId,
    pub(super) request_id: WorkloadRestartRequestId,
    source_execution: WorkloadExecutionReference,
    execution: WorkloadExecutionReference,
    pub(super) restart_epoch: WorkloadRestartEpoch,
    pub(super) dispatch_epoch: WorkloadRestartDispatchEpoch,
    pub(super) issuing_revision: WorkloadSagaRevision,
    pub(super) confirmed_revision: WorkloadSagaRevision,
    pub(super) source_generation: WorkloadProvisionSourceGeneration,
    pub(super) source_digest: WorkloadProvisionSourceDigest,
    pub(super) network_plan_digest: String,
    pub(super) provider_selection: WorkloadExecutionProviderId,
    step: WorkloadRestartStep,
    mode: HostRestartProviderMode,
}

impl HostRestartProviderClaim {
    pub fn new(input: HostRestartProviderClaimInput) -> Result<Self> {
        let source = &input.source_execution;
        let target = &input.execution;
        if source.execution_id() != target.execution_id()
            || source.workload_uid() != target.workload_uid()
            || source.node_identity() != target.node_identity()
            || source.generation() != target.generation()
            || source.desired_digest() != target.desired_digest()
            || source.restart_epoch().checked_next() != Some(target.restart_epoch())
            || target.restart_epoch() != input.restart_epoch
            || target.attempt_id()
                != &nimbus_workloads::WorkloadExecutionAttemptId::for_execution(
                    target.execution_id(),
                    input.restart_epoch,
                )
        {
            return Err(Error::PermissionDenied(
                "host restart claim has a crossed source-to-target execution chain".to_owned(),
            ));
        }
        let execute_revision = input.issuing_revision.checked_next();
        let inspection_revision = execute_revision.and_then(WorkloadSagaRevision::checked_next);
        let mode = match input.mode {
            HostRestartProviderMode::Execute
                if execute_revision == Some(input.confirmed_revision) =>
            {
                HostRestartProviderMode::Execute
            }
            HostRestartProviderMode::Inspect {
                successor_veto_generation,
            } if inspection_revision == Some(input.confirmed_revision) => {
                validate_successor_veto_generation(successor_veto_generation, target)?;
                input.mode
            }
            HostRestartProviderMode::Inspect {
                successor_veto_generation: Some(successor_veto_generation),
            } if inspection_revision
                .is_some_and(|revision| revision < input.confirmed_revision) =>
            {
                validate_successor_veto_generation(Some(successor_veto_generation), target)?;
                input.mode
            }
            _ => {
                return Err(Error::PermissionDenied(
                    "host restart claim confirmation revision is not exact for its execute or inspection authority"
                        .to_owned(),
                ));
            }
        };
        validate_digest(&input.network_plan_digest, "network plan")?;
        Ok(Self {
            saga_id: input.saga_id,
            transition_id: input.transition_id,
            command_id: input.command_id,
            request_id: input.request_id,
            source_execution: input.source_execution,
            execution: input.execution,
            restart_epoch: input.restart_epoch,
            dispatch_epoch: input.dispatch_epoch,
            issuing_revision: input.issuing_revision,
            confirmed_revision: input.confirmed_revision,
            source_generation: input.source_generation,
            source_digest: input.source_digest,
            network_plan_digest: input.network_plan_digest,
            provider_selection: input.provider_selection,
            step: input.step,
            mode,
        })
    }

    pub fn source_execution(&self) -> &WorkloadExecutionReference {
        &self.source_execution
    }

    pub fn execution(&self) -> &WorkloadExecutionReference {
        &self.execution
    }

    pub const fn step(&self) -> WorkloadRestartStep {
        self.step
    }

    pub(crate) fn require_step(&self, expected: WorkloadRestartStep) -> Result<()> {
        if self.step != expected {
            return Err(Error::PermissionDenied(format!(
                "host restart provider expected {expected:?}, got {:?}",
                self.step
            )));
        }
        Ok(())
    }

    pub(crate) fn require_execute_authority(&self) -> Result<()> {
        if !matches!(self.mode, HostRestartProviderMode::Execute) {
            return Err(Error::PermissionDenied(
                "host restart provider effect requires exact execute authority".to_owned(),
            ));
        }
        Ok(())
    }
}

fn validate_successor_veto_generation(
    successor_veto_generation: Option<WorkloadGeneration>,
    execution: &WorkloadExecutionReference,
) -> Result<()> {
    if successor_veto_generation.is_some_and(|generation| generation <= execution.generation()) {
        return Err(Error::PermissionDenied(
            "host restart inspection successor veto does not name a later workload generation"
                .to_owned(),
        ));
    }
    Ok(())
}

impl HostProviderPlan {
    pub(crate) fn from_restart(
        claim: &HostRestartProviderClaim,
        request: HostLifecycleRequest,
    ) -> Result<Self> {
        request.ensure_external_restart_disabled()?;
        let execution = claim.execution();
        let activation_fence = HostActivationFence::for_restart_target(claim)?;
        let unit_name =
            SystemdUnitName::for_execution(execution.execution_id(), SystemdUnitKind::Service)?;
        Ok(Self {
            execution_id: execution.execution_id().clone(),
            backend: request.backend,
            unit_name,
            executable: request.executable,
            args: request.args,
            properties: request.properties,
            trust_class: request.trust_class,
            activation_fence: Some(activation_fence),
        })
    }
}

/// Restart-specific activation identity persisted in systemd LogExtraFields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct HostRestartActivationFence {
    saga_id: WorkloadSagaId,
    transition_id: WorkloadSagaTransitionId,
    command_id: WorkloadRestartCommandId,
    request_id: WorkloadRestartRequestId,
    workload_uid: String,
    node_identity: String,
    execution_id: nimbus_workloads::WorkloadExecutionId,
    source_attempt_id: nimbus_workloads::WorkloadExecutionAttemptId,
    attempt_id: nimbus_workloads::WorkloadExecutionAttemptId,
    restart_epoch: WorkloadRestartEpoch,
    dispatch_epoch: WorkloadRestartDispatchEpoch,
    issuing_revision: WorkloadSagaRevision,
    confirmed_revision: WorkloadSagaRevision,
    source_generation: WorkloadProvisionSourceGeneration,
    generation: u64,
    desired_digest: String,
    source_digest: WorkloadProvisionSourceDigest,
    network_plan_digest: String,
    provider_selection: WorkloadExecutionProviderId,
}

impl HostRestartActivationFence {
    #[cfg(any(test, all(target_os = "linux", feature = "systemd-dbus")))]
    const JOURNAL_FIELD_NAMES: [&'static str; 19] = [
        "NIMBUS_RESTART_SAGA_ID",
        "NIMBUS_RESTART_TRANSITION_ID",
        "NIMBUS_RESTART_COMMAND_ID",
        "NIMBUS_RESTART_REQUEST_ID",
        "NIMBUS_WORKLOAD_UID",
        "NIMBUS_NODE_IDENTITY",
        "NIMBUS_WORKLOAD_EXECUTION_ID",
        "NIMBUS_RESTART_SOURCE_ATTEMPT_ID",
        "NIMBUS_RESTART_ATTEMPT_ID",
        "NIMBUS_RESTART_EPOCH",
        "NIMBUS_RESTART_DISPATCH_EPOCH",
        "NIMBUS_RESTART_ISSUING_REVISION",
        "NIMBUS_RESTART_CONFIRMED_REVISION",
        "NIMBUS_WORKLOAD_SOURCE_GENERATION",
        "NIMBUS_WORKLOAD_GENERATION",
        "NIMBUS_WORKLOAD_DESIRED_DIGEST",
        "NIMBUS_WORKLOAD_SOURCE_DIGEST",
        "NIMBUS_NETWORK_PLAN_DIGEST",
        "NIMBUS_WORKLOAD_EXECUTION_PROVIDER_ID",
    ];

    pub(super) fn from_claim(claim: &HostRestartProviderClaim) -> Result<Self> {
        claim.require_step(WorkloadRestartStep::ActivateExecution)?;
        let source = claim.source_execution();
        let target = claim.execution();
        Ok(Self {
            saga_id: claim.saga_id.clone(),
            transition_id: claim.transition_id.clone(),
            command_id: claim.command_id.clone(),
            request_id: claim.request_id.clone(),
            workload_uid: target.workload_uid().as_str().to_owned(),
            node_identity: target.node_identity().as_str().to_owned(),
            execution_id: target.execution_id().clone(),
            source_attempt_id: source.attempt_id().clone(),
            attempt_id: target.attempt_id().clone(),
            restart_epoch: claim.restart_epoch,
            dispatch_epoch: claim.dispatch_epoch,
            issuing_revision: claim.issuing_revision,
            confirmed_revision: claim.confirmed_revision,
            source_generation: claim.source_generation,
            generation: target.generation().as_u64(),
            desired_digest: target.desired_digest().to_string(),
            source_digest: claim.source_digest,
            network_plan_digest: claim.network_plan_digest.clone(),
            provider_selection: claim.provider_selection.clone(),
        })
    }

    pub(super) fn matches_source(&self, claim: &HostRestartProviderClaim) -> bool {
        let source = claim.source_execution();
        self.saga_id == claim.saga_id
            && self.workload_uid == source.workload_uid().as_str()
            && self.node_identity == source.node_identity().as_str()
            && self.execution_id == *source.execution_id()
            && self.attempt_id == *source.attempt_id()
            && self.restart_epoch == source.restart_epoch()
            && self.source_generation == claim.source_generation
            && self.generation == source.generation().as_u64()
            && self.desired_digest == source.desired_digest().to_string()
            && self.source_digest == claim.source_digest
            && self.network_plan_digest == claim.network_plan_digest.as_str()
            && self.provider_selection == claim.provider_selection
    }

    fn matches_target(&self, claim: &HostRestartProviderClaim) -> bool {
        let source = claim.source_execution();
        let target = claim.execution();
        self.saga_id == claim.saga_id
            && self.command_id == claim.command_id
            && self.request_id == claim.request_id
            && self.workload_uid == target.workload_uid().as_str()
            && self.node_identity == target.node_identity().as_str()
            && self.execution_id == *target.execution_id()
            && self.source_attempt_id == *source.attempt_id()
            && self.attempt_id == *target.attempt_id()
            && self.restart_epoch == claim.restart_epoch
            && self.dispatch_epoch == claim.dispatch_epoch
            && self.issuing_revision == claim.issuing_revision
            && claim.issuing_revision.checked_next() == Some(self.confirmed_revision)
            && self.source_generation == claim.source_generation
            && self.generation == target.generation().as_u64()
            && self.desired_digest == target.desired_digest().to_string()
            && self.source_digest == claim.source_digest
            && self.network_plan_digest == claim.network_plan_digest.as_str()
            && self.provider_selection == claim.provider_selection
    }

    pub(super) fn journal_fields(&self) -> Vec<String> {
        vec![
            format!("NIMBUS_RESTART_SAGA_ID={}", self.saga_id),
            format!("NIMBUS_RESTART_TRANSITION_ID={}", self.transition_id),
            format!("NIMBUS_RESTART_COMMAND_ID={}", self.command_id),
            format!("NIMBUS_RESTART_REQUEST_ID={}", self.request_id),
            format!("NIMBUS_WORKLOAD_UID={}", self.workload_uid),
            format!("NIMBUS_NODE_IDENTITY={}", self.node_identity),
            format!("NIMBUS_WORKLOAD_EXECUTION_ID={}", self.execution_id),
            format!(
                "NIMBUS_RESTART_SOURCE_ATTEMPT_ID={}",
                self.source_attempt_id
            ),
            format!("NIMBUS_RESTART_ATTEMPT_ID={}", self.attempt_id),
            format!("NIMBUS_RESTART_EPOCH={}", self.restart_epoch),
            format!("NIMBUS_RESTART_DISPATCH_EPOCH={}", self.dispatch_epoch),
            format!("NIMBUS_RESTART_ISSUING_REVISION={}", self.issuing_revision),
            format!(
                "NIMBUS_RESTART_CONFIRMED_REVISION={}",
                self.confirmed_revision
            ),
            format!(
                "NIMBUS_WORKLOAD_SOURCE_GENERATION={}",
                self.source_generation
            ),
            format!("NIMBUS_WORKLOAD_GENERATION={}", self.generation),
            format!("NIMBUS_WORKLOAD_DESIRED_DIGEST={}", self.desired_digest),
            format!("NIMBUS_WORKLOAD_SOURCE_DIGEST={}", self.source_digest),
            format!("NIMBUS_NETWORK_PLAN_DIGEST={}", self.network_plan_digest),
            format!(
                "NIMBUS_WORKLOAD_EXECUTION_PROVIDER_ID={}",
                self.provider_selection
            ),
        ]
    }

    #[cfg(any(test, all(target_os = "linux", feature = "systemd-dbus")))]
    pub(super) fn from_log_extra_fields(fields: &[Vec<u8>]) -> Result<Option<Self>> {
        let retained = retained_restart_fields(fields)?;
        if retained.is_empty() {
            return Err(invalid_activation_fence(
                "systemd restart activation fence is incomplete",
            ));
        }
        if retained.len() != Self::JOURNAL_FIELD_NAMES.len() {
            return Err(invalid_activation_fence(
                "systemd restart activation fence is incomplete",
            ));
        }
        let field = |name: &'static str| -> Result<&str> {
            retained
                .get(name)
                .copied()
                .ok_or_else(|| invalid_activation_fence(format!("missing {name}")))
        };
        let workload_uid = field("NIMBUS_WORKLOAD_UID")?.to_owned();
        nimbus_workloads::TenantWorkloadUid::try_from(workload_uid.clone())
            .map_err(|_| invalid_activation_fence("invalid restart workload UID"))?;
        let node_identity = field("NIMBUS_NODE_IDENTITY")?.to_owned();
        nimbus_workloads::NodeIdentity::new(node_identity.clone())
            .map_err(|_| invalid_activation_fence("invalid restart node identity"))?;
        let network_plan_digest = field("NIMBUS_NETWORK_PLAN_DIGEST")?.to_owned();
        validate_digest(&network_plan_digest, "network plan")?;
        Ok(Some(Self {
            saga_id: parse(field("NIMBUS_RESTART_SAGA_ID")?, "restart saga ID")?,
            transition_id: parse(
                field("NIMBUS_RESTART_TRANSITION_ID")?,
                "restart transition ID",
            )?,
            command_id: parse(field("NIMBUS_RESTART_COMMAND_ID")?, "restart command ID")?,
            request_id: parse(field("NIMBUS_RESTART_REQUEST_ID")?, "restart request ID")?,
            workload_uid,
            node_identity,
            execution_id: parse(
                field("NIMBUS_WORKLOAD_EXECUTION_ID")?,
                "restart execution ID",
            )?,
            source_attempt_id: parse(
                field("NIMBUS_RESTART_SOURCE_ATTEMPT_ID")?,
                "restart source attempt ID",
            )?,
            attempt_id: parse(
                field("NIMBUS_RESTART_ATTEMPT_ID")?,
                "restart target attempt ID",
            )?,
            restart_epoch: WorkloadRestartEpoch::new(parse_fence_counter(
                field("NIMBUS_RESTART_EPOCH")?,
                "restart epoch",
            )?),
            dispatch_epoch: WorkloadRestartDispatchEpoch::new(parse_fence_counter(
                field("NIMBUS_RESTART_DISPATCH_EPOCH")?,
                "restart dispatch epoch",
            )?),
            issuing_revision: WorkloadSagaRevision::new(parse_fence_counter(
                field("NIMBUS_RESTART_ISSUING_REVISION")?,
                "restart issuing revision",
            )?),
            confirmed_revision: WorkloadSagaRevision::new(parse_fence_counter(
                field("NIMBUS_RESTART_CONFIRMED_REVISION")?,
                "restart confirmed revision",
            )?),
            source_generation: WorkloadProvisionSourceGeneration::new(parse_fence_counter(
                field("NIMBUS_WORKLOAD_SOURCE_GENERATION")?,
                "workload source generation",
            )?),
            generation: parse_fence_counter(
                field("NIMBUS_WORKLOAD_GENERATION")?,
                "workload generation",
            )?,
            desired_digest: parse::<nimbus_workloads::WorkloadDesiredDigest>(
                field("NIMBUS_WORKLOAD_DESIRED_DIGEST")?,
                "restart desired digest",
            )?
            .to_string(),
            source_digest: parse(
                field("NIMBUS_WORKLOAD_SOURCE_DIGEST")?,
                "restart source digest",
            )?,
            network_plan_digest,
            provider_selection: parse(
                field("NIMBUS_WORKLOAD_EXECUTION_PROVIDER_ID")?,
                "restart execution provider ID",
            )?,
        }))
    }
}

#[cfg(any(test, all(target_os = "linux", feature = "systemd-dbus")))]
pub(super) fn is_restart_fence(fields: &[Vec<u8>]) -> bool {
    fields
        .iter()
        .any(|field| field.starts_with(b"NIMBUS_RESTART_"))
}

#[cfg(any(test, all(target_os = "linux", feature = "systemd-dbus")))]
fn retained_restart_fields(
    fields: &[Vec<u8>],
) -> Result<std::collections::BTreeMap<&'static str, &str>> {
    let mut retained = std::collections::BTreeMap::new();
    for field in fields {
        if !field.starts_with(b"NIMBUS_") {
            continue;
        }
        let field = std::str::from_utf8(field)
            .map_err(|_| invalid_activation_fence("a restart LogExtraFields value is not UTF-8"))?;
        let (name, value) = field.split_once('=').ok_or_else(|| {
            invalid_activation_fence("a restart LogExtraFields value is not NAME=value")
        })?;
        if name.starts_with("NIMBUS_PROVISION_") || name == "NIMBUS_WORKLOAD_EXECUTION_ATTEMPT_ID" {
            return Err(invalid_activation_fence(
                "systemd restart activation fence is mixed with provision authority",
            ));
        }
        let Some(name) = HostRestartActivationFence::JOURNAL_FIELD_NAMES
            .iter()
            .copied()
            .find(|candidate| *candidate == name)
        else {
            continue;
        };
        if value.is_empty() || retained.insert(name, value).is_some() {
            return Err(invalid_activation_fence(format!(
                "systemd restart activation fence field {name} is empty or duplicated"
            )));
        }
    }
    Ok(retained)
}

#[cfg(any(test, all(target_os = "linux", feature = "systemd-dbus")))]
fn parse<T>(value: &str, label: &str) -> Result<T>
where
    T: std::str::FromStr,
{
    value
        .parse()
        .map_err(|_| invalid_activation_fence(format!("invalid {label}")))
}

fn validate_digest(value: &str, label: &str) -> Result<()> {
    if value.len() != 64
        || value
            .bytes()
            .any(|byte| !byte.is_ascii_digit() && !(b'a'..=b'f').contains(&byte))
    {
        return Err(Error::PermissionDenied(format!(
            "host restart {label} digest is not 64 lowercase hexadecimal characters"
        )));
    }
    Ok(())
}

impl HostActivationFence {
    pub(super) fn for_restart_target(claim: &HostRestartProviderClaim) -> Result<Self> {
        HostRestartActivationFence::from_claim(claim).map(Self::Restart)
    }

    pub(crate) fn authenticate_restart_source(
        &self,
        claim: &HostRestartProviderClaim,
    ) -> Result<()> {
        claim.require_step(WorkloadRestartStep::QuiesceExecution)?;
        let matches = match self {
            Self::Provision(fence) => fence.matches_restart_source(claim),
            Self::Restart(fence) => fence.matches_source(claim),
        };
        if matches {
            Ok(())
        } else {
            Err(Error::PermissionDenied(
                "systemd restart source is crossed with the retained activation fence".to_owned(),
            ))
        }
    }

    pub(crate) fn authenticate_restart_target(
        &self,
        claim: &HostRestartProviderClaim,
    ) -> Result<()> {
        claim.require_step(WorkloadRestartStep::ActivateExecution)?;
        if matches!(self, Self::Restart(fence) if fence.matches_target(claim)) {
            Ok(())
        } else {
            Err(Error::PermissionDenied(
                "systemd restart target is crossed with the retained activation fence".to_owned(),
            ))
        }
    }
}
