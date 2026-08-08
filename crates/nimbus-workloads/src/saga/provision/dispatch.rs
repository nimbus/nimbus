//! Durable provider-target and dispatch-fencing vocabulary.

use nimbus_network::{
    NetworkCapabilityRole, NetworkCapabilitySourceDigest, NetworkPlanDigest, NetworkProviderId,
};
use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::*;

/// Whether a confirmed provision dispatch may invoke or only inspect an effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadProvisionCommandMode {
    Execute,
    Inspect,
}

/// Domain-separated identity of one exact confirmed provider dispatch.
///
/// The value is portable inert vocabulary. It grants no effect authority;
/// only the compute-owned confirmed-command constructor can do that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkloadProvisionCommandId(WorkloadOwnerEvidenceDigest);

impl Display for WorkloadProvisionCommandId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkloadProvisionCommandIdentity<'a> {
    domain: &'static str,
    key: &'a WorkloadSagaKey,
    saga_id: &'a WorkloadSagaId,
    attempt_id: &'a WorkloadProvisionAttemptId,
    issuing_revision: WorkloadSagaRevision,
    confirmed_revision: WorkloadSagaRevision,
    transition_id: &'a WorkloadSagaTransitionId,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    source_digest: WorkloadProvisionSourceDigest,
    network_plan_digest: NetworkPlanDigest,
    provider_target: &'a WorkloadProvisionProviderTarget,
    execution: &'a WorkloadExecutionReference,
    source_phase: WorkloadSagaPhase,
    target_phase: WorkloadSagaPhase,
    step: WorkloadProvisionStep,
    subjects: &'a WorkloadProvisionSubjects,
    prerequisite: &'a Option<WorkloadProvisionPrerequisiteEvidence>,
    dispatch_epoch: WorkloadProvisionDispatchEpoch,
    authorization: &'a WorkloadProvisionDispatchAuthorization,
    mode: WorkloadProvisionCommandMode,
}

impl WorkloadProvisionCommandId {
    /// Derive the one transport-stable identity shared by compute and provider
    /// adapters without duplicating the hashing contract.
    pub fn for_confirmed_dispatch(
        claim: &WorkloadProvisionDispatchClaim,
        confirmed_revision: WorkloadSagaRevision,
        transition_id: &WorkloadSagaTransitionId,
        execution: &WorkloadExecutionReference,
        mode: WorkloadProvisionCommandMode,
    ) -> Result<Self, WorkloadSagaError> {
        let attempt = claim.attempt();
        let prerequisite = attempt.prerequisite().cloned();
        let identity = WorkloadProvisionCommandIdentity {
            domain: "nimbus.compute.workload.provision.command.id.v1",
            key: attempt.key(),
            saga_id: attempt.saga_id(),
            attempt_id: attempt.attempt_id(),
            issuing_revision: attempt.issuing_revision(),
            confirmed_revision,
            transition_id,
            generation: attempt.generation(),
            desired_digest: attempt.desired_digest(),
            source_digest: attempt.source_digest(),
            network_plan_digest: attempt.network_plan_digest(),
            provider_target: claim.provider_target(),
            execution,
            source_phase: attempt.source_phase(),
            target_phase: attempt.target_phase(),
            step: attempt.step(),
            subjects: attempt.subjects(),
            prerequisite: &prerequisite,
            dispatch_epoch: claim.dispatch_epoch(),
            authorization: claim.authorization(),
            mode,
        };
        let encoded = serde_json::to_vec(&identity).map_err(|_| {
            WorkloadSagaError::InvalidEvidence(
                "confirmed provision command identity cannot be encoded",
            )
        })?;
        Ok(Self(WorkloadOwnerEvidenceDigest::sha256(encoded)))
    }
}

/// Monotonic execution fence within one stable provision attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkloadProvisionDispatchEpoch(u64);

impl WorkloadProvisionDispatchEpoch {
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

impl Display for WorkloadProvisionDispatchEpoch {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Serialize for WorkloadProvisionDispatchEpoch {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for WorkloadProvisionDispatchEpoch {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value.is_empty()
            || (value.len() > 1 && value.starts_with('0'))
            || value.bytes().any(|byte| !byte.is_ascii_digit())
        {
            return Err(serde::de::Error::custom(
                "workload provision dispatch epoch must be canonical unsigned decimal text",
            ));
        }
        value.parse::<u64>().map(Self).map_err(|_| {
            serde::de::Error::custom(
                "workload provision dispatch epoch must be canonical unsigned decimal text",
            )
        })
    }
}

/// Exact provider authority for one provision command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkloadProvisionProviderTarget {
    Network {
        role: NetworkCapabilityRole,
        provider_id: NetworkProviderId,
        provider_source_digest: NetworkCapabilitySourceDigest,
    },
    Execution {
        provider_id: WorkloadExecutionProviderId,
        provider_source_digest: WorkloadProvisionSourceDigest,
    },
}

impl WorkloadProvisionProviderTarget {
    pub fn for_attempt(
        attempt: &WorkloadProvisionAttempt,
    ) -> Result<Option<Self>, WorkloadSagaError> {
        let network_target = |role, provider_id: &NetworkProviderId, source_digest| Self::Network {
            role,
            provider_id: provider_id.clone(),
            provider_source_digest: source_digest,
        };
        match attempt.step() {
            WorkloadProvisionStep::ReserveNetwork | WorkloadProvisionStep::AttachNetwork => {
                Ok(attempt.selection_evidence().map(|selection| {
                    network_target(
                        NetworkCapabilityRole::Attachment,
                        selection.selection().attachment_provider_id(),
                        selection.source_digest(),
                    )
                }))
            }
            WorkloadProvisionStep::Publish | WorkloadProvisionStep::ObservePublication => {
                let selection = attempt.selection_evidence().ok_or(
                    WorkloadSagaError::InvalidEvidence(
                        "publication command requires exact ingress provider selection evidence",
                    ),
                )?;
                Ok(Some(network_target(
                    NetworkCapabilityRole::Ingress,
                    selection.selection().ingress_provider_id(),
                    selection.source_digest(),
                )))
            }
            WorkloadProvisionStep::PrepareWorkload
            | WorkloadProvisionStep::InspectActivationPrerequisites
            | WorkloadProvisionStep::ActivateWorkload
            | WorkloadProvisionStep::InspectWorkloadReadiness => Ok(Some(Self::Execution {
                provider_id: attempt.execution_provider_id().clone(),
                provider_source_digest: attempt.source_digest(),
            })),
        }
    }

    fn validate_for_attempt(
        &self,
        attempt: &WorkloadProvisionAttempt,
    ) -> Result<(), WorkloadSagaError> {
        if Self::for_attempt(attempt)?.as_ref() == Some(self) {
            Ok(())
        } else {
            Err(WorkloadSagaError::InvalidEvidence(
                "provision provider target is crossed with the admitted attempt",
            ))
        }
    }
}

/// Proof that an exact inspected dispatch did not create its provider effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadProvisionAbsenceEvidence {
    attempt_id: WorkloadProvisionAttemptId,
    dispatch_epoch: WorkloadProvisionDispatchEpoch,
    confirmed_revision: WorkloadSagaRevision,
    transition_id: WorkloadSagaTransitionId,
    provider_target: WorkloadProvisionProviderTarget,
    step: WorkloadProvisionStep,
    evidence: WorkloadOwnerEvidenceDigest,
}

impl WorkloadProvisionAbsenceEvidence {
    /// Bind an absence observation to the exact durable claim it inspected.
    pub fn for_inspection(
        record: &WorkloadSagaRecord,
        claim: &WorkloadProvisionDispatchClaim,
        evidence: WorkloadOwnerEvidenceDigest,
    ) -> Result<Self, WorkloadSagaError> {
        let retained = match record.provision_disposition() {
            Some(WorkloadProvisionDisposition::InspectionRequired(retained)) => Some(retained),
            _ => None,
        };
        if retained != Some(claim) {
            return Err(WorkloadSagaError::InvalidEvidence(
                "absence observation requires the exact durable inspection state",
            ));
        }
        Ok(Self::for_confirmation(
            claim,
            record.revision(),
            record.last_transition().transition_id().clone(),
            evidence,
        ))
    }

    /// Bind provider-owned absence to the exact confirmed command transition.
    pub fn for_confirmation(
        claim: &WorkloadProvisionDispatchClaim,
        confirmed_revision: WorkloadSagaRevision,
        transition_id: WorkloadSagaTransitionId,
        evidence: WorkloadOwnerEvidenceDigest,
    ) -> Self {
        Self {
            attempt_id: claim.attempt().attempt_id().clone(),
            dispatch_epoch: claim.dispatch_epoch(),
            confirmed_revision,
            transition_id,
            provider_target: claim.provider_target().clone(),
            step: claim.attempt().step(),
            evidence,
        }
    }

    pub fn attempt_id(&self) -> &WorkloadProvisionAttemptId {
        &self.attempt_id
    }

    pub const fn dispatch_epoch(&self) -> WorkloadProvisionDispatchEpoch {
        self.dispatch_epoch
    }

    pub const fn confirmed_revision(&self) -> WorkloadSagaRevision {
        self.confirmed_revision
    }

    pub fn transition_id(&self) -> &WorkloadSagaTransitionId {
        &self.transition_id
    }

    pub fn provider_target(&self) -> &WorkloadProvisionProviderTarget {
        &self.provider_target
    }

    pub const fn step(&self) -> WorkloadProvisionStep {
        self.step
    }

    pub const fn evidence(&self) -> WorkloadOwnerEvidenceDigest {
        self.evidence
    }

    pub(crate) fn matches_claim(&self, claim: &WorkloadProvisionDispatchClaim) -> bool {
        self.attempt_id == *claim.attempt().attempt_id()
            && self.dispatch_epoch == claim.dispatch_epoch()
            && self.provider_target == *claim.provider_target()
            && self.step == claim.attempt().step()
    }

    pub(crate) fn matches_inspection(
        &self,
        record: &WorkloadSagaRecord,
        claim: &WorkloadProvisionDispatchClaim,
    ) -> bool {
        self.matches_claim(claim)
            && self.confirmed_revision == record.revision()
            && self.transition_id == *record.last_transition().transition_id()
            && record
                .provision_disposition()
                .and_then(WorkloadProvisionDisposition::claim)
                == Some(claim)
    }
}

/// Why a durable dispatch claim may execute at its epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "evidence",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum WorkloadProvisionDispatchAuthorization {
    Initial,
    RetryAfterAbsence(WorkloadProvisionAbsenceEvidence),
}

/// Durable claim that authorizes one exact provider effect or inspection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadProvisionDispatchClaim {
    attempt: WorkloadProvisionAttempt,
    claimed_revision: WorkloadSagaRevision,
    dispatch_epoch: WorkloadProvisionDispatchEpoch,
    provider_target: WorkloadProvisionProviderTarget,
    authorization: WorkloadProvisionDispatchAuthorization,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WorkloadProvisionDispatchClaimWire {
    attempt: WorkloadProvisionAttempt,
    claimed_revision: WorkloadSagaRevision,
    dispatch_epoch: WorkloadProvisionDispatchEpoch,
    provider_target: WorkloadProvisionProviderTarget,
    authorization: WorkloadProvisionDispatchAuthorization,
}

impl<'de> Deserialize<'de> for WorkloadProvisionDispatchClaim {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WorkloadProvisionDispatchClaimWire::deserialize(deserializer)?;
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

impl WorkloadProvisionDispatchClaim {
    pub(crate) fn initial(
        attempt: WorkloadProvisionAttempt,
        provider_target: WorkloadProvisionProviderTarget,
    ) -> Result<Self, WorkloadSagaError> {
        let claimed_revision = attempt
            .issuing_revision()
            .checked_next()
            .ok_or(WorkloadSagaError::RevisionOverflow)?;
        let claim = Self {
            attempt,
            claimed_revision,
            dispatch_epoch: WorkloadProvisionDispatchEpoch::new(0),
            provider_target,
            authorization: WorkloadProvisionDispatchAuthorization::Initial,
        };
        claim.validate()?;
        Ok(claim)
    }

    pub(crate) fn retry_after_absence(
        previous: &Self,
        claimed_revision: WorkloadSagaRevision,
        absence: WorkloadProvisionAbsenceEvidence,
    ) -> Result<Self, WorkloadSagaError> {
        if !absence.matches_claim(previous) {
            return Err(WorkloadSagaError::InvalidEvidence(
                "retry absence evidence is crossed with the inspected dispatch claim",
            ));
        }
        if absence.confirmed_revision.checked_next() != Some(claimed_revision) {
            return Err(WorkloadSagaError::InvalidEvidence(
                "retry claim revision must immediately follow its absence observation",
            ));
        }
        let dispatch_epoch =
            previous
                .dispatch_epoch
                .checked_next()
                .ok_or(WorkloadSagaError::InvalidCounter(
                    "workload provision dispatch epoch overflow",
                ))?;
        let claim = Self {
            attempt: previous.attempt.clone(),
            claimed_revision,
            dispatch_epoch,
            provider_target: previous.provider_target.clone(),
            authorization: WorkloadProvisionDispatchAuthorization::RetryAfterAbsence(absence),
        };
        claim.validate()?;
        Ok(claim)
    }

    pub fn attempt(&self) -> &WorkloadProvisionAttempt {
        &self.attempt
    }

    pub const fn claimed_revision(&self) -> WorkloadSagaRevision {
        self.claimed_revision
    }

    pub const fn dispatch_epoch(&self) -> WorkloadProvisionDispatchEpoch {
        self.dispatch_epoch
    }

    pub fn provider_target(&self) -> &WorkloadProvisionProviderTarget {
        &self.provider_target
    }

    pub fn authorization(&self) -> &WorkloadProvisionDispatchAuthorization {
        &self.authorization
    }

    fn validate(&self) -> Result<(), WorkloadSagaError> {
        self.provider_target.validate_for_attempt(&self.attempt)?;
        if self.claimed_revision <= self.attempt.issuing_revision() {
            return Err(WorkloadSagaError::InvalidEvidence(
                "dispatch claim revision must follow its attempt issuing revision",
            ));
        }
        match &self.authorization {
            WorkloadProvisionDispatchAuthorization::Initial => {
                if self.dispatch_epoch != WorkloadProvisionDispatchEpoch::new(0)
                    || self.attempt.issuing_revision().checked_next() != Some(self.claimed_revision)
                {
                    return Err(WorkloadSagaError::InvalidEvidence(
                        "initial dispatch claim must use epoch zero at the next revision",
                    ));
                }
            }
            WorkloadProvisionDispatchAuthorization::RetryAfterAbsence(absence) => {
                if absence.attempt_id != *self.attempt.attempt_id()
                    || absence.provider_target != self.provider_target
                    || absence.step != self.attempt.step()
                    || absence.dispatch_epoch.checked_next() != Some(self.dispatch_epoch)
                    || absence.confirmed_revision.checked_next() != Some(self.claimed_revision)
                {
                    return Err(WorkloadSagaError::InvalidEvidence(
                        "retry dispatch claim is not authorized by exact prior absence",
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Closed side-effect-free observation of one provider dispatch claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkloadProvisionInspectionResult {
    Absent {
        evidence: WorkloadProvisionAbsenceEvidence,
    },
    Ambiguous {
        attempt_id: WorkloadProvisionAttemptId,
        dispatch_epoch: WorkloadProvisionDispatchEpoch,
        provider_target: WorkloadProvisionProviderTarget,
    },
    DefiniteFailure {
        attempt_id: WorkloadProvisionAttemptId,
        dispatch_epoch: WorkloadProvisionDispatchEpoch,
        provider_target: WorkloadProvisionProviderTarget,
        failure: WorkloadFailureEvidence,
    },
    InProgress {
        attempt_id: WorkloadProvisionAttemptId,
        dispatch_epoch: WorkloadProvisionDispatchEpoch,
        provider_target: WorkloadProvisionProviderTarget,
        evidence: WorkloadOwnerEvidenceDigest,
    },
    Succeeded {
        attempt_id: WorkloadProvisionAttemptId,
        dispatch_epoch: WorkloadProvisionDispatchEpoch,
        provider_target: WorkloadProvisionProviderTarget,
        evidence: WorkloadProvisionSuccessEvidence,
    },
}

impl WorkloadProvisionInspectionResult {
    pub(crate) fn validate_for_claim(
        &self,
        claim: &WorkloadProvisionDispatchClaim,
    ) -> Result<(), WorkloadSagaError> {
        let matches = match self {
            Self::Absent { evidence } => evidence.matches_claim(claim),
            Self::Ambiguous {
                attempt_id,
                dispatch_epoch,
                provider_target,
            }
            | Self::DefiniteFailure {
                attempt_id,
                dispatch_epoch,
                provider_target,
                ..
            }
            | Self::InProgress {
                attempt_id,
                dispatch_epoch,
                provider_target,
                ..
            }
            | Self::Succeeded {
                attempt_id,
                dispatch_epoch,
                provider_target,
                ..
            } => {
                attempt_id == claim.attempt().attempt_id()
                    && *dispatch_epoch == claim.dispatch_epoch()
                    && provider_target == claim.provider_target()
            }
        };
        if matches {
            Ok(())
        } else {
            Err(WorkloadSagaError::InvalidEvidence(
                "provision inspection result is crossed with the durable dispatch claim",
            ))
        }
    }

    /// Validate that the observation names the exact durable claim and record
    /// transition inspected by a confirmed command.
    pub fn validate_for_record(
        &self,
        record: &WorkloadSagaRecord,
        claim: &WorkloadProvisionDispatchClaim,
    ) -> Result<(), WorkloadSagaError> {
        self.validate_for_claim(claim)?;
        if let Self::Absent { evidence } = self {
            if !evidence.matches_inspection(record, claim) {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "provision absence is crossed with the inspected durable transition",
                ));
            }
        } else if record
            .provision_disposition()
            .and_then(WorkloadProvisionDisposition::claim)
            != Some(claim)
        {
            return Err(WorkloadSagaError::InvalidEvidence(
                "provision inspection is crossed with durable dispatch state",
            ));
        }
        Ok(())
    }
}
