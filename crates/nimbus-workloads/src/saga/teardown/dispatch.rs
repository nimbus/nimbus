//! Exact provider target and dispatch fencing for portable teardown state.

use std::fmt::{self, Display, Formatter};

use nimbus_network::{NetworkCapabilityRole, NetworkCapabilitySourceDigest, NetworkProviderId};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::*;

/// Whether a confirmed teardown dispatch may execute or only inspect an effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadTeardownCommandMode {
    Execute,
    Inspect,
}

/// Cross-process stable identity for one confirmed teardown dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkloadTeardownCommandId(WorkloadOwnerEvidenceDigest);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkloadTeardownCommandIdentity<'a> {
    domain: &'static str,
    claim: &'a WorkloadTeardownClaim,
    confirmed_revision: WorkloadSagaRevision,
    confirmed_transition_id: &'a WorkloadSagaTransitionId,
    mode: WorkloadTeardownCommandMode,
}

impl WorkloadTeardownCommandId {
    pub fn for_confirmed_dispatch(
        claim: &WorkloadTeardownClaim,
        confirmed_revision: WorkloadSagaRevision,
        confirmed_transition_id: &WorkloadSagaTransitionId,
        mode: WorkloadTeardownCommandMode,
    ) -> Result<Self, WorkloadSagaError> {
        let encoded = serde_json::to_vec(&WorkloadTeardownCommandIdentity {
            domain: "nimbus.compute.workload.teardown.command.id.v1",
            claim,
            confirmed_revision,
            confirmed_transition_id,
            mode,
        })
        .map_err(|_| {
            WorkloadSagaError::InvalidEvidence(
                "confirmed teardown command identity cannot be encoded",
            )
        })?;
        Ok(Self(WorkloadOwnerEvidenceDigest::sha256(encoded)))
    }
}

impl Display for WorkloadTeardownCommandId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Monotonic execution fence for one stable teardown attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkloadTeardownDispatchEpoch(u64);

impl WorkloadTeardownDispatchEpoch {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }

    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Some(Self(next)),
            None => None,
        }
    }
}

impl Display for WorkloadTeardownDispatchEpoch {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Serialize for WorkloadTeardownDispatchEpoch {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for WorkloadTeardownDispatchEpoch {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        parse_decimal(
            &value,
            "workload teardown dispatch epoch must be canonical unsigned decimal text",
        )
        .map(Self)
        .map_err(serde::de::Error::custom)
    }
}

/// Exact admitted provider target for one teardown operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkloadTeardownProviderTarget {
    Ingress {
        provider_id: NetworkProviderId,
        provider_source_digest: NetworkCapabilitySourceDigest,
    },
    Execution {
        provider_id: WorkloadExecutionProviderId,
        provider_source_digest: WorkloadProvisionSourceDigest,
    },
    Attachment {
        provider_id: NetworkProviderId,
        provider_source_digest: NetworkCapabilitySourceDigest,
    },
}

impl WorkloadTeardownProviderTarget {
    pub fn for_attempt(
        attempt: &WorkloadTeardownAttempt,
    ) -> Result<Option<Self>, WorkloadSagaError> {
        match attempt.step() {
            WorkloadTeardownStep::WithdrawPublication => {
                let selection =
                    attempt
                        .selection_evidence()
                        .ok_or(WorkloadSagaError::InvalidEvidence(
                            "publication withdrawal requires exact ingress provider selection",
                        ))?;
                Ok(Some(Self::Ingress {
                    provider_id: selection.selection().ingress_provider_id().clone(),
                    provider_source_digest: selection.source_digest(),
                }))
            }
            WorkloadTeardownStep::DrainExecution | WorkloadTeardownStep::StopExecution => {
                Ok(Some(Self::Execution {
                    provider_id: attempt.execution_provider_id().clone(),
                    provider_source_digest: attempt.source_digest(),
                }))
            }
            WorkloadTeardownStep::DetachNetwork | WorkloadTeardownStep::ReleaseNetwork => {
                Ok(attempt
                    .selection_evidence()
                    .map(|selection| Self::Attachment {
                        provider_id: selection.selection().attachment_provider_id().clone(),
                        provider_source_digest: selection.source_digest(),
                    }))
            }
        }
    }

    fn validate_for_attempt(
        &self,
        attempt: &WorkloadTeardownAttempt,
    ) -> Result<(), WorkloadSagaError> {
        if Self::for_attempt(attempt)?.as_ref() == Some(self) {
            Ok(())
        } else {
            Err(WorkloadSagaError::InvalidEvidence(
                "teardown provider target is crossed with its admitted attempt",
            ))
        }
    }

    pub const fn network_role(&self) -> Option<NetworkCapabilityRole> {
        match self {
            Self::Ingress { .. } => Some(NetworkCapabilityRole::Ingress),
            Self::Attachment { .. } => Some(NetworkCapabilityRole::Attachment),
            Self::Execution { .. } => None,
        }
    }
}

/// Proof that exact inspection did not find the provider effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadTeardownRetryEvidence {
    attempt_id: WorkloadTeardownAttemptId,
    dispatch_epoch: WorkloadTeardownDispatchEpoch,
    inspected_revision: WorkloadSagaRevision,
    inspected_transition_id: WorkloadSagaTransitionId,
    inspection_command_id: WorkloadTeardownCommandId,
    provider_target: WorkloadTeardownProviderTarget,
    step: WorkloadTeardownStep,
    evidence: WorkloadOwnerEvidenceDigest,
}

impl WorkloadTeardownRetryEvidence {
    pub fn for_inspection(
        record: &WorkloadSagaRecord,
        claim: &WorkloadTeardownClaim,
        evidence: WorkloadOwnerEvidenceDigest,
    ) -> Result<Self, WorkloadSagaError> {
        if !matches!(
            record.teardown_disposition(),
            Some(WorkloadTeardownDisposition::InspectionRequired { claim: retained, .. })
                if retained == claim
        ) {
            return Err(WorkloadSagaError::InvalidEvidence(
                "teardown retry evidence requires exact durable inspection state",
            ));
        }
        let inspection_command_id = WorkloadTeardownCommandId::for_confirmed_dispatch(
            claim,
            record.revision(),
            record.last_transition().transition_id(),
            WorkloadTeardownCommandMode::Inspect,
        )?;
        Ok(Self {
            attempt_id: claim.attempt().attempt_id().clone(),
            dispatch_epoch: claim.dispatch_epoch(),
            inspected_revision: record.revision(),
            inspected_transition_id: record.last_transition().transition_id().clone(),
            inspection_command_id,
            provider_target: claim.provider_target().clone(),
            step: claim.attempt().step(),
            evidence,
        })
    }

    pub fn attempt_id(&self) -> &WorkloadTeardownAttemptId {
        &self.attempt_id
    }

    pub const fn dispatch_epoch(&self) -> WorkloadTeardownDispatchEpoch {
        self.dispatch_epoch
    }

    pub const fn inspected_revision(&self) -> WorkloadSagaRevision {
        self.inspected_revision
    }

    pub fn inspected_transition_id(&self) -> &WorkloadSagaTransitionId {
        &self.inspected_transition_id
    }

    pub const fn inspection_command_id(&self) -> WorkloadTeardownCommandId {
        self.inspection_command_id
    }

    pub fn provider_target(&self) -> &WorkloadTeardownProviderTarget {
        &self.provider_target
    }

    pub const fn step(&self) -> WorkloadTeardownStep {
        self.step
    }

    pub const fn evidence(&self) -> WorkloadOwnerEvidenceDigest {
        self.evidence
    }

    pub(crate) fn matches_claim(&self, claim: &WorkloadTeardownClaim) -> bool {
        self.attempt_id == *claim.attempt().attempt_id()
            && self.dispatch_epoch == claim.dispatch_epoch()
            && self.provider_target == *claim.provider_target()
            && self.step == claim.attempt().step()
    }

    pub(crate) fn matches_inspection(
        &self,
        record: &WorkloadSagaRecord,
        claim: &WorkloadTeardownClaim,
    ) -> bool {
        self.matches_claim(claim)
            && self.inspected_revision == record.revision()
            && self.inspected_transition_id == *record.last_transition().transition_id()
            && WorkloadTeardownCommandId::for_confirmed_dispatch(
                claim,
                record.revision(),
                record.last_transition().transition_id(),
                WorkloadTeardownCommandMode::Inspect,
            )
            .is_ok_and(|expected| self.inspection_command_id == expected)
    }
}

/// Why a durable teardown claim may execute at its epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "evidence",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum WorkloadTeardownDispatchAuthorization {
    Initial,
    RetryAfterNotCompleted(WorkloadTeardownRetryEvidence),
}

/// Durable inert claim for one exact effect or inspection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadTeardownClaim {
    attempt: WorkloadTeardownAttempt,
    claimed_revision: WorkloadSagaRevision,
    dispatch_epoch: WorkloadTeardownDispatchEpoch,
    provider_target: WorkloadTeardownProviderTarget,
    authorization: WorkloadTeardownDispatchAuthorization,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WorkloadTeardownClaimWire {
    attempt: WorkloadTeardownAttempt,
    claimed_revision: WorkloadSagaRevision,
    dispatch_epoch: WorkloadTeardownDispatchEpoch,
    provider_target: WorkloadTeardownProviderTarget,
    authorization: WorkloadTeardownDispatchAuthorization,
}

impl<'de> Deserialize<'de> for WorkloadTeardownClaim {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WorkloadTeardownClaimWire::deserialize(deserializer)?;
        let claim = Self {
            attempt: wire.attempt,
            claimed_revision: wire.claimed_revision,
            dispatch_epoch: wire.dispatch_epoch,
            provider_target: wire.provider_target,
            authorization: wire.authorization,
        };
        claim.validate().map_err(serde::de::Error::custom)?;
        Ok(claim)
    }
}

impl WorkloadTeardownClaim {
    pub(crate) fn initial(
        attempt: WorkloadTeardownAttempt,
        provider_target: WorkloadTeardownProviderTarget,
    ) -> Result<Self, WorkloadSagaError> {
        let claimed_revision = attempt
            .issuing_revision()
            .checked_next()
            .ok_or(WorkloadSagaError::RevisionOverflow)?;
        let claim = Self {
            attempt,
            claimed_revision,
            dispatch_epoch: WorkloadTeardownDispatchEpoch::new(0),
            provider_target,
            authorization: WorkloadTeardownDispatchAuthorization::Initial,
        };
        claim.validate()?;
        Ok(claim)
    }

    pub(crate) fn retry_after_not_completed(
        previous: &Self,
        claimed_revision: WorkloadSagaRevision,
        evidence: WorkloadTeardownRetryEvidence,
    ) -> Result<Self, WorkloadSagaError> {
        if !evidence.matches_claim(previous)
            || evidence.inspected_revision().checked_next() != Some(claimed_revision)
        {
            return Err(WorkloadSagaError::InvalidEvidence(
                "teardown retry evidence is crossed with the inspected claim transition",
            ));
        }
        let dispatch_epoch =
            previous
                .dispatch_epoch
                .checked_next()
                .ok_or(WorkloadSagaError::InvalidCounter(
                    "workload teardown dispatch epoch overflow",
                ))?;
        let claim = Self {
            attempt: previous.attempt.clone(),
            claimed_revision,
            dispatch_epoch,
            provider_target: previous.provider_target.clone(),
            authorization: WorkloadTeardownDispatchAuthorization::RetryAfterNotCompleted(evidence),
        };
        claim.validate()?;
        Ok(claim)
    }

    pub fn attempt(&self) -> &WorkloadTeardownAttempt {
        &self.attempt
    }

    pub const fn claimed_revision(&self) -> WorkloadSagaRevision {
        self.claimed_revision
    }

    pub const fn dispatch_epoch(&self) -> WorkloadTeardownDispatchEpoch {
        self.dispatch_epoch
    }

    pub fn provider_target(&self) -> &WorkloadTeardownProviderTarget {
        &self.provider_target
    }

    pub fn authorization(&self) -> &WorkloadTeardownDispatchAuthorization {
        &self.authorization
    }

    pub(crate) fn validate(&self) -> Result<(), WorkloadSagaError> {
        self.provider_target.validate_for_attempt(&self.attempt)?;
        if self.claimed_revision <= self.attempt.issuing_revision() {
            return Err(WorkloadSagaError::InvalidEvidence(
                "teardown claim revision must follow its attempt revision",
            ));
        }
        match &self.authorization {
            WorkloadTeardownDispatchAuthorization::Initial => {
                if self.dispatch_epoch != WorkloadTeardownDispatchEpoch::new(0)
                    || self.attempt.issuing_revision().checked_next() != Some(self.claimed_revision)
                {
                    return Err(WorkloadSagaError::InvalidEvidence(
                        "initial teardown claim must use epoch zero at the next revision",
                    ));
                }
            }
            WorkloadTeardownDispatchAuthorization::RetryAfterNotCompleted(evidence) => {
                if evidence.attempt_id() != self.attempt.attempt_id()
                    || evidence.provider_target() != &self.provider_target
                    || evidence.step() != self.attempt.step()
                    || evidence.dispatch_epoch().checked_next() != Some(self.dispatch_epoch)
                    || evidence.inspected_revision().checked_next() != Some(self.claimed_revision)
                {
                    return Err(WorkloadSagaError::InvalidEvidence(
                        "teardown retry claim is not authorized by exact inspection evidence",
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Closed side-effect-free inspection of one durable teardown claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkloadTeardownInspectionResult {
    NotCompleted {
        evidence: WorkloadTeardownRetryEvidence,
    },
    Ambiguous {
        attempt_id: WorkloadTeardownAttemptId,
        dispatch_epoch: WorkloadTeardownDispatchEpoch,
        provider_target: WorkloadTeardownProviderTarget,
        inspection_command_id: WorkloadTeardownCommandId,
    },
    DefiniteFailure {
        attempt_id: WorkloadTeardownAttemptId,
        dispatch_epoch: WorkloadTeardownDispatchEpoch,
        provider_target: WorkloadTeardownProviderTarget,
        inspection_command_id: WorkloadTeardownCommandId,
        failure: WorkloadFailureEvidence,
    },
    InProgress {
        attempt_id: WorkloadTeardownAttemptId,
        dispatch_epoch: WorkloadTeardownDispatchEpoch,
        provider_target: WorkloadTeardownProviderTarget,
        inspection_command_id: WorkloadTeardownCommandId,
        evidence: WorkloadOwnerEvidenceDigest,
    },
    Satisfied {
        attempt_id: WorkloadTeardownAttemptId,
        dispatch_epoch: WorkloadTeardownDispatchEpoch,
        provider_target: WorkloadTeardownProviderTarget,
        inspection_command_id: WorkloadTeardownCommandId,
        evidence: WorkloadTeardownSuccessEvidence,
    },
}

impl WorkloadTeardownInspectionResult {
    pub(crate) fn validate_for_claim(
        &self,
        record: &WorkloadSagaRecord,
        claim: &WorkloadTeardownClaim,
    ) -> Result<(), WorkloadSagaError> {
        if !matches!(
            record.teardown_disposition(),
            Some(WorkloadTeardownDisposition::InspectionRequired { claim: retained, .. })
                if retained == claim
        ) {
            return Err(WorkloadSagaError::InvalidEvidence(
                "teardown inspection result requires the exact current inspection state",
            ));
        }
        let expected_command_id = WorkloadTeardownCommandId::for_confirmed_dispatch(
            claim,
            record.revision(),
            record.last_transition().transition_id(),
            WorkloadTeardownCommandMode::Inspect,
        )?;
        let matches = match self {
            Self::NotCompleted { evidence } => evidence.matches_inspection(record, claim),
            Self::Ambiguous {
                attempt_id,
                dispatch_epoch,
                provider_target,
                inspection_command_id,
            }
            | Self::DefiniteFailure {
                attempt_id,
                dispatch_epoch,
                provider_target,
                inspection_command_id,
                ..
            }
            | Self::InProgress {
                attempt_id,
                dispatch_epoch,
                provider_target,
                inspection_command_id,
                ..
            } => {
                attempt_id == claim.attempt().attempt_id()
                    && *dispatch_epoch == claim.dispatch_epoch()
                    && provider_target == claim.provider_target()
                    && *inspection_command_id == expected_command_id
            }
            Self::Satisfied {
                attempt_id,
                dispatch_epoch,
                provider_target,
                inspection_command_id,
                evidence,
            } => {
                attempt_id == claim.attempt().attempt_id()
                    && *dispatch_epoch == claim.dispatch_epoch()
                    && provider_target == claim.provider_target()
                    && *inspection_command_id == expected_command_id
                    && evidence.matches_step_and_subjects(
                        claim.attempt().step(),
                        claim.attempt().subjects(),
                    )
            }
        };
        if matches {
            Ok(())
        } else {
            Err(WorkloadSagaError::InvalidEvidence(
                "teardown inspection is crossed with the durable claim",
            ))
        }
    }
}
