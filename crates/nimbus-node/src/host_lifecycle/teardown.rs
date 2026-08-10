//! Exact node-local drain and stop authority for confirmed workload teardown.
//!
//! Compute owns the saga and lowers a durable command into these claims. The
//! node validates the complete portable fence before a provider can inspect or
//! mutate a process mechanism. No scheduling, retry, or tenant policy lives
//! here.

use std::future::Future;
use std::pin::Pin;

use nimbus_core::{Error, Result};
use nimbus_workloads::{
    WorkloadExecutionReference, WorkloadFailureEvidence, WorkloadOwnerEvidenceDigest,
    WorkloadProvisionSourceEvidence, WorkloadSagaRevision, WorkloadSagaTransitionId,
    WorkloadTeardownClaim, WorkloadTeardownCommandId, WorkloadTeardownCommandMode,
    WorkloadTeardownDispatchAuthorization, WorkloadTeardownProviderTarget, WorkloadTeardownReceipt,
    WorkloadTeardownReceiptPrefix, WorkloadTeardownStep, WorkloadTeardownSubjects,
    WorkloadTeardownSuccessEvidence,
};
use serde::{Deserialize, Serialize};

use super::HostProvisionActivationFence;

/// Complete portable input for one node-owned provider claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostTeardownProviderClaimInput {
    pub claim: WorkloadTeardownClaim,
    pub command_id: WorkloadTeardownCommandId,
    pub confirmed_revision: WorkloadSagaRevision,
    pub confirmed_transition_id: WorkloadSagaTransitionId,
    pub source: WorkloadProvisionSourceEvidence,
    pub execution: WorkloadExecutionReference,
    pub provider_target: WorkloadTeardownProviderTarget,
    pub prior_receipt_prefix: WorkloadTeardownReceiptPrefix,
}

/// Effect authority for one exact confirmed teardown command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostTeardownExecuteClaim(HostTeardownClaim);

/// Read-only authority for one exact confirmed teardown command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostTeardownInspectClaim(HostTeardownClaim);

/// Exact provider-operation identity derived from one execute authority.
///
/// Providers retain this closed value beside an effect. It accepts only the
/// same execute claim or its one corresponding read-only inspect claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HostTeardownOperationFence {
    execute: HostTeardownClaim,
    inspect: Option<HostTeardownClaim>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostTeardownClaim {
    portable: WorkloadTeardownClaim,
    command_id: WorkloadTeardownCommandId,
    confirmed_revision: WorkloadSagaRevision,
    confirmed_transition_id: WorkloadSagaTransitionId,
    source: WorkloadProvisionSourceEvidence,
    execution: WorkloadExecutionReference,
    provider_target: WorkloadTeardownProviderTarget,
    prior_receipt_prefix: WorkloadTeardownReceiptPrefix,
}

impl HostTeardownExecuteClaim {
    pub fn new(input: HostTeardownProviderClaimInput) -> Result<Self> {
        HostTeardownClaim::new(input, WorkloadTeardownCommandMode::Execute).map(Self)
    }

    pub(crate) fn operation_fence(&self) -> HostTeardownOperationFence {
        HostTeardownOperationFence {
            execute: self.0.clone(),
            inspect: None,
        }
    }
}

impl HostTeardownInspectClaim {
    pub fn new(input: HostTeardownProviderClaimInput) -> Result<Self> {
        HostTeardownClaim::new(input, WorkloadTeardownCommandMode::Inspect).map(Self)
    }
}

macro_rules! claim_accessors {
    ($claim:ty) => {
        impl $claim {
            pub fn command_id(&self) -> WorkloadTeardownCommandId {
                self.0.command_id
            }

            pub fn confirmed_transition_id(&self) -> &WorkloadSagaTransitionId {
                &self.0.confirmed_transition_id
            }

            pub fn portable_claim(&self) -> &WorkloadTeardownClaim {
                &self.0.portable
            }

            pub fn source(&self) -> &WorkloadProvisionSourceEvidence {
                &self.0.source
            }

            pub fn execution(&self) -> &WorkloadExecutionReference {
                &self.0.execution
            }

            pub fn provider_target(&self) -> &WorkloadTeardownProviderTarget {
                &self.0.provider_target
            }

            pub fn prior_receipt_prefix(&self) -> &WorkloadTeardownReceiptPrefix {
                &self.0.prior_receipt_prefix
            }

            pub fn step(&self) -> WorkloadTeardownStep {
                self.0.portable.attempt().step()
            }

            pub fn require_step(&self, expected: WorkloadTeardownStep) -> Result<()> {
                self.0.require_step(expected)
            }

            pub fn canonical_evidence(&self, domain: &str) -> WorkloadOwnerEvidenceDigest {
                self.0.canonical_evidence(domain)
            }
        }
    };
}

claim_accessors!(HostTeardownExecuteClaim);
claim_accessors!(HostTeardownInspectClaim);

impl HostTeardownOperationFence {
    pub(crate) fn matches_execute(&self, claim: &HostTeardownExecuteClaim) -> bool {
        self.execute == claim.0
    }

    pub(crate) fn bind_or_matches_inspect(&mut self, claim: &HostTeardownInspectClaim) -> bool {
        let command_matches = WorkloadTeardownCommandId::for_confirmed_dispatch(
            &self.execute.portable,
            claim.0.confirmed_revision,
            &claim.0.confirmed_transition_id,
            WorkloadTeardownCommandMode::Inspect,
        )
        .is_ok_and(|expected| expected == claim.0.command_id);
        if self.execute.portable != claim.0.portable
            || self.execute.source != claim.0.source
            || self.execute.execution != claim.0.execution
            || self.execute.provider_target != claim.0.provider_target
            || self.execute.prior_receipt_prefix != claim.0.prior_receipt_prefix
            || self.execute.confirmed_revision.checked_next() != Some(claim.0.confirmed_revision)
            || !command_matches
        {
            return false;
        }
        match &self.inspect {
            Some(bound) => bound == &claim.0,
            None => {
                self.inspect = Some(claim.0.clone());
                true
            }
        }
    }

    pub(crate) fn matches_inspect(&self, claim: &HostTeardownInspectClaim) -> bool {
        let mut candidate = self.clone();
        candidate.bind_or_matches_inspect(claim)
    }

    pub(crate) fn advance_after_not_completed(
        &mut self,
        claim: &HostTeardownExecuteClaim,
        evidence_domain: &str,
    ) -> bool {
        let Some(inspect) = &self.inspect else {
            return false;
        };
        let WorkloadTeardownDispatchAuthorization::RetryAfterNotCompleted(evidence) =
            claim.0.portable.authorization()
        else {
            return false;
        };
        let matches = self.execute.portable.attempt() == claim.0.portable.attempt()
            && self.execute.portable.dispatch_epoch().checked_next()
                == Some(claim.0.portable.dispatch_epoch())
            && self.execute.source == claim.0.source
            && self.execute.execution == claim.0.execution
            && self.execute.provider_target == claim.0.provider_target
            && self.execute.prior_receipt_prefix == claim.0.prior_receipt_prefix
            && evidence.attempt_id() == self.execute.portable.attempt().attempt_id()
            && evidence.dispatch_epoch() == self.execute.portable.dispatch_epoch()
            && evidence.inspected_revision() == inspect.confirmed_revision
            && evidence.inspected_transition_id() == &inspect.confirmed_transition_id
            && evidence.inspection_command_id() == inspect.command_id
            && evidence.provider_target() == &self.execute.provider_target
            && evidence.step() == self.execute.portable.attempt().step()
            && evidence.evidence() == inspect.canonical_evidence(evidence_domain);
        if matches {
            self.execute = claim.0.clone();
            self.inspect = None;
        }
        matches
    }

    pub(crate) fn matches_prior_receipt(&self, receipt: &WorkloadTeardownReceipt) -> bool {
        self.execute.portable == *receipt.claim()
    }
}

impl HostTeardownClaim {
    fn new(
        input: HostTeardownProviderClaimInput,
        mode: WorkloadTeardownCommandMode,
    ) -> Result<Self> {
        let expected_command_id = WorkloadTeardownCommandId::for_confirmed_dispatch(
            &input.claim,
            input.confirmed_revision,
            &input.confirmed_transition_id,
            mode,
        )
        .map_err(|error| permission_denied(error.to_string()))?;
        if input.command_id != expected_command_id {
            return Err(permission_denied(
                "host teardown command ID is crossed with its confirmed command fence",
            ));
        }
        let expected_revision = match mode {
            WorkloadTeardownCommandMode::Execute => input.claim.claimed_revision(),
            WorkloadTeardownCommandMode::Inspect => input
                .claim
                .claimed_revision()
                .checked_next()
                .ok_or_else(|| {
                permission_denied("host teardown inspection revision overflow")
            })?,
        };
        if input.confirmed_revision != expected_revision {
            return Err(permission_denied(
                "host teardown confirmation revision is not exact for its authority",
            ));
        }
        input
            .prior_receipt_prefix
            .validate_for_claim(&input.claim)
            .map_err(|error| permission_denied(error.to_string()))?;
        let attempt = input.claim.attempt();
        if input.provider_target != *input.claim.provider_target() {
            return Err(permission_denied(
                "host teardown provider target is crossed with its durable claim",
            ));
        }
        let WorkloadTeardownProviderTarget::Execution {
            provider_id,
            provider_source_digest,
        } = &input.provider_target
        else {
            return Err(permission_denied(
                "host teardown node claim requires an execution provider target",
            ));
        };
        if provider_id != attempt.execution_provider_id()
            || *provider_source_digest != attempt.source_digest()
            || input.source.source_digest() != attempt.source_digest()
            || input.source.execution_provider_id() != attempt.execution_provider_id()
        {
            return Err(permission_denied(
                "host teardown source or provider is crossed with its admitted attempt",
            ));
        }
        let WorkloadTeardownSubjects::Execution(subject) = attempt.subjects() else {
            return Err(permission_denied(
                "host teardown node claim requires one exact execution subject",
            ));
        };
        if subject != &input.execution
            || subject.generation() != attempt.generation()
            || subject.desired_digest() != attempt.desired_digest()
            || subject.node_identity() != attempt.required_node()
        {
            return Err(permission_denied(
                "host teardown execution is crossed with its durable subject",
            ));
        }
        Ok(Self {
            portable: input.claim,
            command_id: input.command_id,
            confirmed_revision: input.confirmed_revision,
            confirmed_transition_id: input.confirmed_transition_id,
            source: input.source,
            execution: input.execution,
            provider_target: input.provider_target,
            prior_receipt_prefix: input.prior_receipt_prefix,
        })
    }

    fn require_step(&self, expected: WorkloadTeardownStep) -> Result<()> {
        if self.portable.attempt().step() != expected
            || self.portable.attempt().step().phases() != expected.phases()
        {
            return Err(permission_denied(format!(
                "host teardown provider expected {expected:?}, got {:?}",
                self.portable.attempt().step()
            )));
        }
        Ok(())
    }

    fn canonical_evidence(&self, domain: &str) -> WorkloadOwnerEvidenceDigest {
        let prior_receipt_prefix = serde_json::to_vec(&self.prior_receipt_prefix)
            .expect("validated teardown receipt prefix should serialize");
        let prior_receipt_prefix_digest = WorkloadOwnerEvidenceDigest::sha256(prior_receipt_prefix);
        WorkloadOwnerEvidenceDigest::sha256(format!(
            "{domain}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
            self.command_id,
            self.confirmed_revision,
            self.confirmed_transition_id,
            self.portable.attempt().attempt_id().as_str(),
            self.portable.dispatch_epoch(),
            self.execution.workload_uid().as_str(),
            self.execution.node_identity().as_str(),
            self.execution.execution_id().as_str(),
            self.execution.attempt_id().as_str(),
            self.portable.attempt().execution_provider_id().as_str(),
            prior_receipt_prefix_digest,
        ))
    }
}

impl HostProvisionActivationFence {
    pub(super) fn matches_teardown_execution(
        &self,
        execution: &WorkloadExecutionReference,
        source: &WorkloadProvisionSourceEvidence,
        provider_target: &WorkloadTeardownProviderTarget,
        claim: &WorkloadTeardownClaim,
    ) -> bool {
        let WorkloadTeardownProviderTarget::Execution {
            provider_id,
            provider_source_digest,
        } = provider_target
        else {
            return false;
        };
        self.workload_uid == execution.workload_uid().as_str()
            && self.node_identity == execution.node_identity().as_str()
            && self.execution_id == *execution.execution_id()
            && self.execution_attempt_id == *execution.attempt_id()
            && self.execution_provider_id == provider_id.as_str()
            && self.execution_provider_id == source.execution_provider_id().as_str()
            && self.generation == execution.generation().as_u64()
            && self.desired_digest == execution.desired_digest().to_string()
            && self.source_digest == provider_source_digest.to_string()
            && self.source_digest == source.source_digest().to_string()
            && self.network_plan_digest == claim.attempt().network_plan_digest().to_string()
    }
}

fn permission_denied(message: impl Into<String>) -> Error {
    Error::PermissionDenied(message.into())
}

/// Closed results from an effect-authorized node operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostTeardownExecuteObservation {
    Succeeded(Box<WorkloadTeardownSuccessEvidence>),
    DefiniteFailure(WorkloadFailureEvidence),
    Ambiguous,
}

/// Closed results from a read-only node inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostTeardownInspectObservation {
    Satisfied(Box<WorkloadTeardownSuccessEvidence>),
    NotCompleted(WorkloadOwnerEvidenceDigest),
    DefiniteFailure(WorkloadFailureEvidence),
    InProgress(WorkloadOwnerEvidenceDigest),
    Ambiguous,
}

pub type HostTeardownFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Drain the exact Nimbus-admitted request barrier without stopping execution.
pub trait HostExecutionDrainProvider: Send + Sync {
    fn execute_drain<'a>(
        &'a self,
        claim: HostTeardownExecuteClaim,
    ) -> HostTeardownFuture<'a, HostTeardownExecuteObservation>;

    fn inspect_drain<'a>(
        &'a self,
        claim: HostTeardownInspectClaim,
    ) -> HostTeardownFuture<'a, HostTeardownInspectObservation>;
}

/// Stop one exact execution after its admitted request barrier is drained.
pub trait HostExecutionStopProvider: Send + Sync {
    fn execute_stop<'a>(
        &'a self,
        claim: HostTeardownExecuteClaim,
    ) -> HostTeardownFuture<'a, HostTeardownExecuteObservation>;

    fn inspect_stop<'a>(
        &'a self,
        claim: HostTeardownInspectClaim,
    ) -> HostTeardownFuture<'a, HostTeardownInspectObservation>;
}
