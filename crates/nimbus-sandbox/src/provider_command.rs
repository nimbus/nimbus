//! Provider-local idempotency for compute-issued commands.
//!
//! This module deliberately knows nothing about the workload saga. It stores
//! only the provider's stable authority key and complete opaque fences supplied
//! by its adapter. The upper coordinator remains the sole owner of lifecycle
//! order and durable desired state.

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

const JOURNAL_DIRECTORY: &str = ".nimbus-provider-command-attempts";
const RECORD_SUFFIX: &str = ".json";
const STAGE_SUFFIX: &str = ".stage";
const LOCK_SUFFIX: &str = ".lock";
const CURRENT_ENVELOPE_VERSION: u32 = 4;
const MAX_IDENTITY_LEN: usize = 256;
const MAX_CANONICAL_SUBJECT_LEN: usize = 64 * 1024;
const LOCK_TIMEOUT: Duration = Duration::from_secs(5);
const LOCK_RETRY: Duration = Duration::from_millis(10);

mod current_claim;
mod prepared_request;

use prepared_request::validate_prepared_request;

#[cfg(test)]
std::thread_local! {
    static PROVIDER_COMMAND_LOCK_TEST_PROBE: std::cell::RefCell<Option<ProviderCommandLockTestProbe>> =
        const { std::cell::RefCell::new(None) };
}

/// Thread-scoped proof that a test reached real provider-lock contention.
#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct ProviderCommandLockTestProbe {
    state: std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    timeout: Duration,
}

#[cfg(test)]
impl ProviderCommandLockTestProbe {
    pub(crate) fn new(timeout: Duration) -> Self {
        Self {
            state: std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new())),
            timeout,
        }
    }

    fn record_contention(&self) {
        let (state, changed) = &*self.state;
        *state.lock().expect("provider lock probe should lock") = true;
        changed.notify_all();
    }

    pub(crate) fn wait_until_contended(&self) -> bool {
        let (state, changed) = &*self.state;
        let state = state.lock().expect("provider lock probe should lock");
        let (state, _) = changed
            .wait_timeout_while(state, self.timeout, |contended| !*contended)
            .expect("provider lock probe should wait");
        *state
    }
}

#[cfg(test)]
struct ProviderCommandLockTestProbeScope {
    previous: Option<ProviderCommandLockTestProbe>,
}

#[cfg(test)]
impl Drop for ProviderCommandLockTestProbeScope {
    fn drop(&mut self) {
        PROVIDER_COMMAND_LOCK_TEST_PROBE.with(|slot| {
            *slot.borrow_mut() = self.previous.take();
        });
    }
}

#[cfg(test)]
pub(crate) fn with_provider_command_lock_test_probe<T>(
    probe: ProviderCommandLockTestProbe,
    action: impl FnOnce() -> T,
) -> T {
    let previous = PROVIDER_COMMAND_LOCK_TEST_PROBE.with(|slot| slot.borrow_mut().replace(probe));
    let _scope = ProviderCommandLockTestProbeScope { previous };
    action()
}

#[cfg(test)]
fn record_provider_command_lock_contention() {
    let probe = PROVIDER_COMMAND_LOCK_TEST_PROBE.with(|slot| slot.borrow().clone());
    if let Some(probe) = probe {
        probe.record_contention();
    }
}

/// Provider operation fenced independently within one stable workload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCommandOperation {
    ReserveNetwork,
    PrepareWorkload,
    AttachNetwork,
    InspectActivationPrerequisites,
    ActivateWorkload,
    InspectWorkloadReadiness,
    PublishIngress,
    ObserveIngress,
    WithdrawPublication,
    WithdrawFinalPublication,
    ResetWorkloadForRestart,
    PrepareRestartAttempt,
    AttachRetainedNetwork,
    InspectRestartActivationPrerequisites,
    ActivateRestartedWorkload,
    InspectRestartReadiness,
    PublishRestartIngress,
    ObserveRestartPublication,
    DrainExecution,
    StopExecution,
    DetachNetwork,
    ReleaseNetwork,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderCommandOperationFamily {
    Provision,
    Restart,
    Teardown,
}

impl ProviderCommandOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::ReserveNetwork => "reserve_network",
            Self::PrepareWorkload => "prepare_workload",
            Self::AttachNetwork => "attach_network",
            Self::InspectActivationPrerequisites => "inspect_activation_prerequisites",
            Self::ActivateWorkload => "activate_workload",
            Self::InspectWorkloadReadiness => "inspect_workload_readiness",
            Self::PublishIngress => "publish_ingress",
            Self::ObserveIngress => "observe_ingress",
            Self::WithdrawPublication => "withdraw_publication",
            Self::WithdrawFinalPublication => "withdraw_final_publication",
            Self::ResetWorkloadForRestart => "reset_workload_for_restart",
            Self::PrepareRestartAttempt => "prepare_restart_attempt",
            Self::AttachRetainedNetwork => "attach_retained_network",
            Self::InspectRestartActivationPrerequisites => {
                "inspect_restart_activation_prerequisites"
            }
            Self::ActivateRestartedWorkload => "activate_restarted_workload",
            Self::InspectRestartReadiness => "inspect_restart_readiness",
            Self::PublishRestartIngress => "publish_restart_ingress",
            Self::ObserveRestartPublication => "observe_restart_publication",
            Self::DrainExecution => "drain_execution",
            Self::StopExecution => "stop_execution",
            Self::DetachNetwork => "detach_network",
            Self::ReleaseNetwork => "release_network",
        }
    }

    const fn is_restart(self) -> bool {
        matches!(self.family(), ProviderCommandOperationFamily::Restart)
    }

    const fn family(self) -> ProviderCommandOperationFamily {
        match self {
            Self::WithdrawPublication
            | Self::ResetWorkloadForRestart
            | Self::PrepareRestartAttempt
            | Self::AttachRetainedNetwork
            | Self::InspectRestartActivationPrerequisites
            | Self::ActivateRestartedWorkload
            | Self::InspectRestartReadiness
            | Self::PublishRestartIngress
            | Self::ObserveRestartPublication => ProviderCommandOperationFamily::Restart,
            Self::WithdrawFinalPublication
            | Self::DrainExecution
            | Self::StopExecution
            | Self::DetachNetwork
            | Self::ReleaseNetwork => ProviderCommandOperationFamily::Teardown,
            Self::ReserveNetwork
            | Self::PrepareWorkload
            | Self::AttachNetwork
            | Self::InspectActivationPrerequisites
            | Self::ActivateWorkload
            | Self::InspectWorkloadReadiness
            | Self::PublishIngress
            | Self::ObserveIngress => ProviderCommandOperationFamily::Provision,
        }
    }

    const fn permits_live_absence_reconciliation(self) -> bool {
        matches!(
            self,
            Self::PublishIngress
                | Self::AttachRetainedNetwork
                | Self::ActivateRestartedWorkload
                | Self::PublishRestartIngress
                | Self::ObserveRestartPublication
        )
    }

    const fn permits_retry_authorization(self) -> bool {
        matches!(
            self,
            Self::WithdrawFinalPublication
                | Self::StopExecution
                | Self::DetachNetwork
                | Self::ReleaseNetwork
        )
    }
}

/// Complete opaque fences a provider must authenticate before one effect.
pub struct ProviderCommandClaimInput {
    pub authority_id: String,
    pub effect_subject: String,
    pub source_attempt_id: Option<String>,
    pub attempt_id: String,
    pub dispatch_epoch: u64,
    pub workload_generation: u64,
    pub restart_ordinal: u64,
    pub desired_digest: String,
    pub source_digest: String,
    pub network_plan_digest: String,
    pub provider_target_digest: String,
    pub operation: ProviderCommandOperation,
}

/// Validated provider-local claim. No address or allocated port is identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderCommandClaim {
    authority_id: String,
    effect_subject: String,
    source_attempt_id: Option<String>,
    attempt_id: String,
    dispatch_epoch: u64,
    workload_generation: u64,
    restart_ordinal: u64,
    desired_digest: String,
    source_digest: String,
    network_plan_digest: String,
    provider_target_digest: String,
    operation: ProviderCommandOperation,
}

impl ProviderCommandClaim {
    pub fn new(input: ProviderCommandClaimInput) -> Result<Self, ProviderCommandJournalError> {
        validate_identity("authority ID", &input.authority_id)?;
        validate_identity("attempt ID", &input.attempt_id)?;
        if let Some(source_attempt_id) = input.source_attempt_id.as_deref() {
            validate_identity("source attempt ID", source_attempt_id)?;
        }
        if input.effect_subject.is_empty() || input.effect_subject.len() > MAX_CANONICAL_SUBJECT_LEN
        {
            return Err(ProviderCommandJournalError::InvalidClaim {
                message: "effect subject must be non-empty and bounded".to_owned(),
            });
        }
        for (label, digest) in [
            ("desired", &input.desired_digest),
            ("source", &input.source_digest),
            ("network plan", &input.network_plan_digest),
            ("provider target", &input.provider_target_digest),
        ] {
            validate_sha256(label, digest)?;
        }
        let attempt_domain_error = match input.operation.family() {
            ProviderCommandOperationFamily::Provision
                if input.source_attempt_id.is_some() || input.restart_ordinal != 0 =>
            {
                Some("provision commands require no source attempt and restart ordinal zero")
            }
            ProviderCommandOperationFamily::Restart
                if input.source_attempt_id.is_none() || input.restart_ordinal == 0 =>
            {
                Some("restart commands require a source attempt and nonzero restart ordinal")
            }
            ProviderCommandOperationFamily::Teardown
                if input.source_attempt_id.is_some() || input.restart_ordinal != 0 =>
            {
                Some("teardown commands require no source attempt and restart ordinal zero")
            }
            _ => None,
        };
        if let Some(message) = attempt_domain_error {
            return Err(ProviderCommandJournalError::InvalidClaim {
                message: message.to_owned(),
            });
        }
        if input.source_attempt_id.as_ref() == Some(&input.attempt_id) {
            return Err(ProviderCommandJournalError::InvalidClaim {
                message: "provider command source and target attempts must differ".to_owned(),
            });
        }
        Ok(Self {
            authority_id: input.authority_id,
            effect_subject: input.effect_subject,
            source_attempt_id: input.source_attempt_id,
            attempt_id: input.attempt_id,
            dispatch_epoch: input.dispatch_epoch,
            workload_generation: input.workload_generation,
            restart_ordinal: input.restart_ordinal,
            desired_digest: input.desired_digest,
            source_digest: input.source_digest,
            network_plan_digest: input.network_plan_digest,
            provider_target_digest: input.provider_target_digest,
            operation: input.operation,
        })
    }

    pub fn authority_id(&self) -> &str {
        &self.authority_id
    }

    pub fn effect_subject(&self) -> &str {
        &self.effect_subject
    }

    pub fn attempt_id(&self) -> &str {
        &self.attempt_id
    }

    pub fn source_attempt_id(&self) -> Option<&str> {
        self.source_attempt_id.as_deref()
    }

    pub const fn dispatch_epoch(&self) -> u64 {
        self.dispatch_epoch
    }

    pub const fn workload_generation(&self) -> u64 {
        self.workload_generation
    }

    pub fn desired_digest(&self) -> &str {
        &self.desired_digest
    }

    pub fn source_digest(&self) -> &str {
        &self.source_digest
    }

    pub fn network_plan_digest(&self) -> &str {
        &self.network_plan_digest
    }

    pub fn provider_target_digest(&self) -> &str {
        &self.provider_target_digest
    }

    pub const fn restart_ordinal(&self) -> u64 {
        self.restart_ordinal
    }

    pub const fn operation(&self) -> ProviderCommandOperation {
        self.operation
    }

    /// Compare the stable workload authority shared by adjacent lifecycle steps.
    ///
    /// The attempt, dispatch epoch, effect subject, provider target, and
    /// operation are command-local fences. They must not be equal across, for
    /// example, `StopExecution` and `DetachNetwork`.
    pub(crate) fn same_lifecycle_fence(&self, other: &Self) -> bool {
        self.authority_id == other.authority_id
            && self.source_attempt_id == other.source_attempt_id
            && self.workload_generation == other.workload_generation
            && self.restart_ordinal == other.restart_ordinal
            && self.desired_digest == other.desired_digest
            && self.source_digest == other.source_digest
            && self.network_plan_digest == other.network_plan_digest
    }

    fn same_attempt_fence(&self, other: &Self) -> bool {
        self.same_workload_fence(other)
            && self.source_attempt_id == other.source_attempt_id
            && self.attempt_id == other.attempt_id
            && self.restart_ordinal == other.restart_ordinal
    }

    fn same_workload_fence(&self, other: &Self) -> bool {
        self.authority_id == other.authority_id
            && self.effect_subject == other.effect_subject
            && self.workload_generation == other.workload_generation
            && self.desired_digest == other.desired_digest
            && self.source_digest == other.source_digest
            && self.network_plan_digest == other.network_plan_digest
            && self.provider_target_digest == other.provider_target_digest
            && self.operation == other.operation
    }

    fn validate(&self) -> Result<(), ProviderCommandJournalError> {
        Self::new(ProviderCommandClaimInput {
            authority_id: self.authority_id.clone(),
            effect_subject: self.effect_subject.clone(),
            source_attempt_id: self.source_attempt_id.clone(),
            attempt_id: self.attempt_id.clone(),
            dispatch_epoch: self.dispatch_epoch,
            workload_generation: self.workload_generation,
            restart_ordinal: self.restart_ordinal,
            desired_digest: self.desired_digest.clone(),
            source_digest: self.source_digest.clone(),
            network_plan_digest: self.network_plan_digest.clone(),
            provider_target_digest: self.provider_target_digest.clone(),
            operation: self.operation,
        })
        .map(|_| ())
    }
}

/// Durable provider observation for one exact attempt and epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCommandObservationKind {
    Claimed,
    Succeeded,
    DefiniteFailure,
    Absent,
    RetryAuthorized,
    InProgress,
    Ambiguous,
}

impl ProviderCommandObservationKind {
    fn is_final_for_epoch(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::DefiniteFailure | Self::Absent | Self::RetryAuthorized
        )
    }

    fn resolves_effect(self) -> bool {
        matches!(self, Self::Succeeded | Self::DefiniteFailure | Self::Absent)
    }

    fn authorizes_retry(self, operation: ProviderCommandOperation) -> bool {
        self == Self::Absent
            || (self == Self::RetryAuthorized && operation.permits_retry_authorization())
    }
}

/// Exact durable outcome that authorized one adjacent retry epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ProviderCommandRetryReceipt {
    claim: ProviderCommandClaim,
    kind: ProviderCommandObservationKind,
    evidence_sha256: String,
}

impl ProviderCommandRetryReceipt {
    fn from_observation(
        observation: &ProviderCommandObservation,
    ) -> Result<Self, ProviderCommandJournalError> {
        if !observation
            .kind
            .authorizes_retry(observation.claim.operation)
        {
            return Err(ProviderCommandJournalError::RetryWithoutAuthority);
        }
        Ok(Self {
            claim: observation.claim.clone(),
            kind: observation.kind,
            evidence_sha256: observation.evidence_sha256.clone().ok_or_else(|| {
                ProviderCommandJournalError::Corrupt {
                    message: "retry authority lacks outcome evidence".to_owned(),
                }
            })?,
        })
    }

    fn validate(&self) -> Result<(), ProviderCommandJournalError> {
        self.claim.validate()?;
        if self.kind == ProviderCommandObservationKind::RetryAuthorized
            && !self.claim.operation.permits_retry_authorization()
        {
            return Err(ProviderCommandJournalError::Corrupt {
                message: "provider retry authorization is valid only for execution stop or network detach/release"
                    .to_owned(),
            });
        }
        validate_sha256("provider retry evidence", &self.evidence_sha256)?;
        if !self.kind.authorizes_retry(self.claim.operation) {
            return Err(ProviderCommandJournalError::Corrupt {
                message: "provider retry lineage contains a non-authorizing outcome".to_owned(),
            });
        }
        Ok(())
    }
}

/// Authenticated current provider observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderCommandObservation {
    claim: ProviderCommandClaim,
    kind: ProviderCommandObservationKind,
    evidence_sha256: Option<String>,
    prepared_request: Option<Vec<u8>>,
    prepared_request_sha256: Option<String>,
    #[serde(deserialize_with = "deserialize_present_optional_string")]
    failure_code: Option<String>,
    retry_lineage: Vec<ProviderCommandRetryReceipt>,
}

impl ProviderCommandObservation {
    pub fn claim(&self) -> &ProviderCommandClaim {
        &self.claim
    }

    pub const fn kind(&self) -> ProviderCommandObservationKind {
        self.kind
    }

    pub fn evidence_sha256(&self) -> Option<&str> {
        self.evidence_sha256.as_deref()
    }

    /// Exact request bytes durably prepared before a provider effect may start.
    pub fn prepared_request(&self) -> Option<&[u8]> {
        self.prepared_request.as_deref()
    }

    /// Stable provider failure code retained for exact teardown replay.
    pub fn failure_code(&self) -> Option<&str> {
        self.failure_code.as_deref()
    }

    /// Whether exact journal receipts authorize provider progress at an older
    /// epoch of this same command attempt.
    pub fn authenticates_retry_progress(&self, progress: &ProviderCommandClaim) -> bool {
        self.claim.same_attempt_fence(progress)
            && self.claim.operation == progress.operation
            && self
                .retry_lineage
                .iter()
                .any(|receipt| receipt.claim == *progress)
    }

    fn claimed(
        claim: ProviderCommandClaim,
        retry_lineage: Vec<ProviderCommandRetryReceipt>,
    ) -> Self {
        Self {
            claim,
            kind: ProviderCommandObservationKind::Claimed,
            evidence_sha256: None,
            prepared_request: None,
            prepared_request_sha256: None,
            failure_code: None,
            retry_lineage,
        }
    }

    fn validate(&self) -> Result<(), ProviderCommandJournalError> {
        self.claim.validate()?;
        let mut prior: Option<&ProviderCommandClaim> = None;
        for receipt in &self.retry_lineage {
            receipt.validate()?;
            if !receipt.claim.same_attempt_fence(&self.claim)
                || receipt.claim.operation != self.claim.operation
            {
                return Err(ProviderCommandJournalError::Corrupt {
                    message: "provider retry lineage crosses its current command attempt"
                        .to_owned(),
                });
            }
            if let Some(prior) = prior
                && prior.dispatch_epoch.checked_add(1) != Some(receipt.claim.dispatch_epoch)
            {
                return Err(ProviderCommandJournalError::Corrupt {
                    message: "provider retry lineage skips a dispatch epoch".to_owned(),
                });
            }
            prior = Some(&receipt.claim);
        }
        if let Some(prior) = prior
            && prior.dispatch_epoch.checked_add(1) != Some(self.claim.dispatch_epoch)
        {
            return Err(ProviderCommandJournalError::Corrupt {
                message: "provider retry lineage does not end immediately before the current claim"
                    .to_owned(),
            });
        }
        match (self.kind, self.failure_code.as_deref()) {
            (ProviderCommandObservationKind::DefiniteFailure, Some(code))
                if is_valid_identity(code) => {}
            (ProviderCommandObservationKind::DefiniteFailure, None)
                if self.claim.operation.family() != ProviderCommandOperationFamily::Teardown => {}
            (ProviderCommandObservationKind::DefiniteFailure, _) => {
                return Err(ProviderCommandJournalError::Corrupt {
                    message: "a teardown definite failure requires one bounded portable code"
                        .to_owned(),
                });
            }
            (_, None) => {}
            (_, Some(_)) => {
                return Err(ProviderCommandJournalError::Corrupt {
                    message: "only a definite provider failure can carry a failure code".to_owned(),
                });
            }
        }
        match (self.kind, self.evidence_sha256.as_deref()) {
            (ProviderCommandObservationKind::Claimed, None) => Ok(()),
            (ProviderCommandObservationKind::Claimed, Some(_)) => {
                Err(ProviderCommandJournalError::Corrupt {
                    message: "a claimed provider attempt cannot carry outcome evidence".to_owned(),
                })
            }
            (_, Some(evidence)) => validate_sha256("provider evidence", evidence),
            (_, None) => Err(ProviderCommandJournalError::Corrupt {
                message: "a provider outcome must carry SHA-256 evidence".to_owned(),
            }),
        }?;
        match (
            self.prepared_request.as_deref(),
            self.prepared_request_sha256.as_deref(),
        ) {
            (None, None) => Ok(()),
            (Some(request), Some(digest)) => {
                if self.kind == ProviderCommandObservationKind::Claimed {
                    return Err(ProviderCommandJournalError::Corrupt {
                        message: "a claimed provider attempt cannot carry a prepared request"
                            .to_owned(),
                    });
                }
                validate_prepared_request(request)?;
                if evidence_sha256(request) != digest {
                    return Err(ProviderCommandJournalError::Corrupt {
                        message: "prepared provider request digest is crossed".to_owned(),
                    });
                }
                validate_sha256("prepared provider request", digest)
            }
            _ => Err(ProviderCommandJournalError::Corrupt {
                message:
                    "prepared provider request bytes and digest are missing, crossed, or oversized"
                        .to_owned(),
            }),
        }
    }
}

/// Result of claiming one provider-local dispatch epoch.
#[derive(Debug)]
pub enum ProviderCommandClaimDecision {
    ExecuteClaimed(ProviderCommandExecutionClaim),
    AdoptExactAttempt(ProviderCommandObservation),
}

/// Result of atomically claiming an epoch with an exact prepared request.
#[derive(Debug)]
pub enum ProviderCommandStartedClaimDecision {
    ExecuteStarted(ProviderCommandStartedExecutionClaim),
    AdoptExactAttempt(ProviderCommandObservation),
}

/// Journal-authenticated authorization for one provider execution.
///
/// A retry carries the exact preceding absence that authorized its epoch. A
/// provider can use this receipt to reconcile local progress that did not yet
/// reach the claimed epoch before a crash.
#[derive(Debug)]
pub struct ProviderCommandExecutionClaim {
    observation: ProviderCommandObservation,
}

impl ProviderCommandExecutionClaim {
    pub fn observation(&self) -> &ProviderCommandObservation {
        &self.observation
    }

    pub fn claim(&self) -> &ProviderCommandClaim {
        self.observation.claim()
    }
}

/// Journal-authenticated request-may-exist authority for one provider effect.
#[derive(Debug)]
pub struct ProviderCommandStartedExecutionClaim {
    observation: ProviderCommandObservation,
}

impl ProviderCommandStartedExecutionClaim {
    pub fn observation(&self) -> &ProviderCommandObservation {
        &self.observation
    }

    pub fn claim(&self) -> &ProviderCommandClaim {
        self.observation.claim()
    }
}

/// Borrowed proof that one exact Execute claim is current under its journal lock.
///
/// Only `ProviderCommandAttemptJournal` can construct this capability. A
/// provider substep can borrow it but cannot retain or recreate effect
/// authority after the journal releases the exact stream lock.
#[doc(hidden)]
pub struct ProviderCommandCurrentExecution {
    observation: ProviderCommandObservation,
}

impl ProviderCommandCurrentExecution {
    pub fn observation(&self) -> &ProviderCommandObservation {
        &self.observation
    }

    pub fn claim(&self) -> &ProviderCommandClaim {
        self.observation.claim()
    }
}

/// Read-only result while an exact provider stream stays locked.
#[doc(hidden)]
#[derive(Debug)]
pub enum ProviderCommandCurrentInspection<T> {
    /// A durable Execute token can still start after this inspection returns.
    EffectCanStillStart(Box<ProviderCommandObservation>),
    /// The current durable state already fences every older Execute token.
    Inspected(T),
}

/// Typed fail-before or durable-store error.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProviderCommandJournalError {
    #[error("invalid provider command claim: {message}")]
    InvalidClaim { message: String },
    #[error(
        "provider command workload generation {candidate} is stale relative to durable generation {current}"
    )]
    StaleWorkloadGeneration { current: u64, candidate: u64 },
    #[error(
        "provider command restart ordinal {candidate} is stale relative to durable ordinal {current}"
    )]
    StaleRestartOrdinal { current: u64, candidate: u64 },
    #[error(
        "provider command restart ordinal {candidate} skips durable ordinal {current}; only exact +1 is allowed"
    )]
    SkippedRestartOrdinal { current: u64, candidate: u64 },
    #[error(
        "provider command dispatch epoch {candidate} is stale relative to durable epoch {current}"
    )]
    StaleDispatchEpoch { current: u64, candidate: u64 },
    #[error(
        "provider command dispatch epoch {candidate} skips durable epoch {current}; only exact +1 after absence is allowed"
    )]
    SkippedDispatchEpoch { current: u64, candidate: u64 },
    #[error("provider command claim crosses durable authority at the same command ordinal")]
    CrossedClaim,
    #[error(
        "provider command retry requires exact durable absence or stop-redelivery authority at the preceding epoch"
    )]
    RetryWithoutAuthority,
    #[error("a later provider command cannot replace an in-progress or ambiguous effect")]
    PriorEffectUnresolved,
    #[error("provider command journal is corrupt: {message}")]
    Corrupt { message: String },
    #[error("provider command journal operation failed: {message}")]
    Store { message: String },
}

/// One provider-owned durable attempt journal rooted below its configured state.
#[derive(Debug, Clone)]
pub struct ProviderCommandAttemptJournal {
    state_root: PathBuf,
    namespace: String,
}

impl ProviderCommandAttemptJournal {
    /// Open an idempotency journal. Directory effects occur only on first use.
    pub fn open(
        state_root: impl Into<PathBuf>,
        namespace: impl Into<String>,
    ) -> Result<Self, ProviderCommandJournalError> {
        let namespace = namespace.into();
        validate_identity("provider namespace", &namespace)?;
        let state_root = state_root.into();
        if state_root == Path::new("/") {
            return Err(ProviderCommandJournalError::InvalidClaim {
                message: "provider journal state root cannot be the filesystem root".to_owned(),
            });
        }
        Ok(Self {
            state_root,
            namespace,
        })
    }

    /// Claim one exact epoch before any provider mutation.
    pub fn claim_dispatch_epoch(
        &self,
        claim: &ProviderCommandClaim,
    ) -> Result<ProviderCommandClaimDecision, ProviderCommandJournalError> {
        claim.validate()?;
        let paths = self.paths(claim);
        self.establish_directory(&paths.directory)?;
        let _guard = lock(&paths.lock)?;
        remove_stale_stage(&paths.stage)?;
        let current = read_if_present(&paths.record)?;
        match current {
            None => {
                Self::require_initial_restart_ordinal(claim)?;
                self.publish_new_claim(&paths, claim.clone(), None)
            }
            Some(current) => self.decide_existing(&paths, current, claim),
        }
    }

    /// Claim one adjacent epoch after the coordinator confirms exact absence.
    ///
    /// The caller must authenticate its coordinator-owned inspection receipt
    /// before it invokes this seam. The journal then binds that receipt to the
    /// exact prior provider epoch under the same stream lock that publishes the
    /// successor claim. `Claimed` is never eligible because its Execute token
    /// can still start. Inspect remains read-only, and a crash cannot leave an
    /// unfenced gap between absence authorization and the successor claim.
    pub fn claim_dispatch_epoch_after_inspected_absence(
        &self,
        claim: &ProviderCommandClaim,
        inspected_dispatch_epoch: u64,
        inspection_evidence: &[u8],
    ) -> Result<ProviderCommandClaimDecision, ProviderCommandJournalError> {
        claim.validate()?;
        if inspected_dispatch_epoch.checked_add(1) != Some(claim.dispatch_epoch) {
            return Err(ProviderCommandJournalError::RetryWithoutAuthority);
        }
        let paths = self.paths(claim);
        self.establish_directory(&paths.directory)?;
        let _guard = lock(&paths.lock)?;
        remove_stale_stage(&paths.stage)?;
        let Some(current) = read_if_present(&paths.record)? else {
            return Err(ProviderCommandJournalError::RetryWithoutAuthority);
        };

        if current.claim == *claim {
            return Ok(ProviderCommandClaimDecision::AdoptExactAttempt(current));
        }
        if !claim.same_attempt_fence(&current.claim) {
            return match self.reject_stale_or_crossed(&current.claim, claim) {
                Ok(()) => Err(ProviderCommandJournalError::CrossedClaim),
                Err(error) => Err(error),
            };
        }
        if current.claim.dispatch_epoch != inspected_dispatch_epoch {
            return Err(if claim.dispatch_epoch < current.claim.dispatch_epoch {
                Self::reject_stale_dispatch_epoch(
                    current.claim.dispatch_epoch,
                    claim.dispatch_epoch,
                )
            } else {
                ProviderCommandJournalError::CrossedClaim
            });
        }

        match current.kind {
            ProviderCommandObservationKind::Absent
            | ProviderCommandObservationKind::RetryAuthorized => {
                self.decide_existing(&paths, current, claim)
            }
            ProviderCommandObservationKind::InProgress
            | ProviderCommandObservationKind::Ambiguous => {
                let absence = ProviderCommandObservation {
                    claim: current.claim.clone(),
                    kind: ProviderCommandObservationKind::Absent,
                    evidence_sha256: Some(evidence_sha256(inspection_evidence)),
                    prepared_request: current.prepared_request.clone(),
                    prepared_request_sha256: current.prepared_request_sha256.clone(),
                    failure_code: None,
                    retry_lineage: current.retry_lineage.clone(),
                };
                self.publish_new_claim(&paths, claim.clone(), Some(absence))
            }
            ProviderCommandObservationKind::Claimed => {
                Err(ProviderCommandJournalError::PriorEffectUnresolved)
            }
            ProviderCommandObservationKind::Succeeded
            | ProviderCommandObservationKind::DefiniteFailure => {
                Err(ProviderCommandJournalError::RetryWithoutAuthority)
            }
        }
    }

    /// Inspect only when the durable authority is the exact attempt and epoch.
    ///
    /// This operation never creates a directory, lock, or record. A missing
    /// lock means that the stream has no durable authority. A record without
    /// its persistent stream lock is corrupt.
    pub fn adopt_exact_attempt(
        &self,
        claim: &ProviderCommandClaim,
    ) -> Result<Option<ProviderCommandObservation>, ProviderCommandJournalError> {
        claim.validate()?;
        let paths = self.paths(claim);
        if !self.journal_directory_exists(&paths.directory)? {
            return Ok(None);
        }
        let Some(_guard) = lock_existing(&paths.lock)? else {
            let record = read_if_present(&paths.record)?;
            let stage_exists = regular_file_exists(&paths.stage, "journal stage")?;
            return if record.is_none() && !stage_exists {
                Ok(None)
            } else {
                Err(ProviderCommandJournalError::Corrupt {
                    message: format!(
                        "provider journal stream for {} has a record or stage but no persistent lock",
                        paths.record.display()
                    ),
                })
            };
        };
        if regular_file_exists(&paths.stage, "journal stage")? {
            return Err(ProviderCommandJournalError::Corrupt {
                message: format!(
                    "provider journal stream for {} has an unresolved staged publication",
                    paths.record.display()
                ),
            });
        }
        let Some(current) = read_if_present(&paths.record)? else {
            return Ok(None);
        };
        if current.claim == *claim {
            Ok(Some(current))
        } else {
            self.reject_stale_or_crossed(&current.claim, claim)?;
            Err(ProviderCommandJournalError::CrossedClaim)
        }
    }

    /// Record one exact provider observation after the corresponding effect or inspection.
    pub fn record_observation(
        &self,
        claim: &ProviderCommandClaim,
        kind: ProviderCommandObservationKind,
        evidence: &[u8],
    ) -> Result<ProviderCommandObservation, ProviderCommandJournalError> {
        Self::validate_outcome(claim, kind, None)?;
        claim.validate()?;
        let paths = self.paths(claim);
        self.establish_directory(&paths.directory)?;
        let _guard = lock(&paths.lock)?;
        remove_stale_stage(&paths.stage)?;
        let current =
            read_if_present(&paths.record)?.ok_or_else(|| ProviderCommandJournalError::Store {
                message: "provider outcome has no durable preceding claim".to_owned(),
            })?;
        if current.claim != *claim {
            self.reject_stale_or_crossed(&current.claim, claim)?;
            return Err(ProviderCommandJournalError::CrossedClaim);
        }
        self.record_observation_locked(&paths, current, kind, None, evidence)
    }

    /// Record an exact provider observation with its stable failure code.
    pub fn record_observation_with_failure_code(
        &self,
        claim: &ProviderCommandClaim,
        kind: ProviderCommandObservationKind,
        failure_code: Option<&str>,
        evidence: &[u8],
    ) -> Result<ProviderCommandObservation, ProviderCommandJournalError> {
        Self::validate_outcome(claim, kind, failure_code)?;
        claim.validate()?;
        let paths = self.paths(claim);
        self.establish_directory(&paths.directory)?;
        let _guard = lock(&paths.lock)?;
        remove_stale_stage(&paths.stage)?;
        let current =
            read_if_present(&paths.record)?.ok_or_else(|| ProviderCommandJournalError::Store {
                message: "provider outcome has no durable preceding claim".to_owned(),
            })?;
        if current.claim != *claim {
            self.reject_stale_or_crossed(&current.claim, claim)?;
            return Err(ProviderCommandJournalError::CrossedClaim);
        }
        self.record_observation_locked(&paths, current, kind, failure_code, evidence)
    }

    fn record_observation_locked(
        &self,
        paths: &JournalPaths,
        current: ProviderCommandObservation,
        kind: ProviderCommandObservationKind,
        failure_code: Option<&str>,
        evidence: &[u8],
    ) -> Result<ProviderCommandObservation, ProviderCommandJournalError> {
        Self::validate_outcome(&current.claim, kind, failure_code)?;
        if current.kind.is_final_for_epoch() {
            let expected = evidence_sha256(evidence);
            if current.kind == kind
                && current.failure_code.as_deref() == failure_code
                && current.evidence_sha256.as_deref() == Some(&expected)
            {
                return Ok(current);
            }
            return Err(ProviderCommandJournalError::CrossedClaim);
        }
        let observation = ProviderCommandObservation {
            claim: current.claim.clone(),
            kind,
            evidence_sha256: Some(evidence_sha256(evidence)),
            prepared_request: current.prepared_request.clone(),
            prepared_request_sha256: current.prepared_request_sha256.clone(),
            failure_code: failure_code.map(str::to_owned),
            retry_lineage: current.retry_lineage.clone(),
        };
        publish(paths, &observation)?;
        Ok(observation)
    }

    fn validate_outcome_kind(
        claim: &ProviderCommandClaim,
        kind: ProviderCommandObservationKind,
    ) -> Result<(), ProviderCommandJournalError> {
        if kind == ProviderCommandObservationKind::Claimed {
            return Err(ProviderCommandJournalError::InvalidClaim {
                message: "record_observation requires an outcome kind".to_owned(),
            });
        }
        if kind == ProviderCommandObservationKind::RetryAuthorized
            && !claim.operation.permits_retry_authorization()
        {
            return Err(ProviderCommandJournalError::InvalidClaim {
                message:
                    "retry authorization is valid only for execution stop or network detach/release"
                        .to_owned(),
            });
        }
        Ok(())
    }

    fn validate_outcome(
        claim: &ProviderCommandClaim,
        kind: ProviderCommandObservationKind,
        failure_code: Option<&str>,
    ) -> Result<(), ProviderCommandJournalError> {
        Self::validate_outcome_kind(claim, kind)?;
        match (kind, failure_code) {
            (ProviderCommandObservationKind::DefiniteFailure, Some(code)) => {
                validate_identity("provider failure code", code)
            }
            (ProviderCommandObservationKind::DefiniteFailure, None)
                if claim.operation.family() != ProviderCommandOperationFamily::Teardown =>
            {
                Ok(())
            }
            (ProviderCommandObservationKind::DefiniteFailure, None) => {
                Err(ProviderCommandJournalError::InvalidClaim {
                    message: "teardown definite failure requires a stable failure code".to_owned(),
                })
            }
            (_, None) => Ok(()),
            (_, Some(_)) => Err(ProviderCommandJournalError::InvalidClaim {
                message: "only a definite provider failure can carry a failure code".to_owned(),
            }),
        }
    }

    /// Replace an exact live-resource observation with provider-proven current absence.
    ///
    /// Process-bound ingress can disappear when its owner process dies after
    /// the provider journal recorded success but before compute committed the
    /// result. The provider's lifetime recovery is conclusive no-effect proof;
    /// recording that absence at the same dispatch epoch authorizes the sole
    /// coordinator to retry the same attempt at exactly the next epoch.
    pub fn record_reconciled_absence(
        &self,
        claim: &ProviderCommandClaim,
        evidence: &[u8],
    ) -> Result<ProviderCommandObservation, ProviderCommandJournalError> {
        if !claim.operation.permits_live_absence_reconciliation() {
            return Err(ProviderCommandJournalError::InvalidClaim {
                message: "this provider command cannot reconcile live state to absence".to_owned(),
            });
        }
        claim.validate()?;
        let paths = self.paths(claim);
        self.establish_directory(&paths.directory)?;
        let _guard = lock(&paths.lock)?;
        remove_stale_stage(&paths.stage)?;
        let current =
            read_if_present(&paths.record)?.ok_or_else(|| ProviderCommandJournalError::Store {
                message: "provider absence has no durable preceding claim".to_owned(),
            })?;
        if current.claim != *claim {
            self.reject_stale_or_crossed(&current.claim, claim)?;
            return Err(ProviderCommandJournalError::CrossedClaim);
        }
        if current.kind == ProviderCommandObservationKind::DefiniteFailure {
            return Err(ProviderCommandJournalError::CrossedClaim);
        }
        let observation = ProviderCommandObservation {
            claim: claim.clone(),
            kind: ProviderCommandObservationKind::Absent,
            evidence_sha256: Some(evidence_sha256(evidence)),
            prepared_request: current.prepared_request.clone(),
            prepared_request_sha256: current.prepared_request_sha256.clone(),
            failure_code: None,
            retry_lineage: current.retry_lineage.clone(),
        };
        if current == observation {
            return Ok(current);
        }
        publish(&paths, &observation)?;
        Ok(observation)
    }

    fn publish_new_claim(
        &self,
        paths: &JournalPaths,
        claim: ProviderCommandClaim,
        retry_authority: Option<ProviderCommandObservation>,
    ) -> Result<ProviderCommandClaimDecision, ProviderCommandJournalError> {
        let mut retry_lineage = retry_authority
            .as_ref()
            .map_or_else(Vec::new, |observation| observation.retry_lineage.clone());
        if let Some(observation) = retry_authority.as_ref() {
            retry_lineage.push(ProviderCommandRetryReceipt::from_observation(observation)?);
        }
        let observation = ProviderCommandObservation::claimed(claim, retry_lineage);
        publish(paths, &observation)?;
        Ok(ProviderCommandClaimDecision::ExecuteClaimed(
            ProviderCommandExecutionClaim { observation },
        ))
    }

    fn decide_existing(
        &self,
        paths: &JournalPaths,
        current: ProviderCommandObservation,
        candidate: &ProviderCommandClaim,
    ) -> Result<ProviderCommandClaimDecision, ProviderCommandJournalError> {
        if candidate.workload_generation < current.claim.workload_generation {
            return Err(ProviderCommandJournalError::StaleWorkloadGeneration {
                current: current.claim.workload_generation,
                candidate: candidate.workload_generation,
            });
        }
        if candidate.workload_generation > current.claim.workload_generation {
            if !matches!(
                current.kind,
                ProviderCommandObservationKind::Absent
                    | ProviderCommandObservationKind::DefiniteFailure
            ) {
                return Err(ProviderCommandJournalError::PriorEffectUnresolved);
            }
            Self::require_initial_restart_ordinal(candidate)?;
            return self.publish_new_claim(paths, candidate.clone(), None);
        }
        if candidate.restart_ordinal < current.claim.restart_ordinal {
            return Err(ProviderCommandJournalError::StaleRestartOrdinal {
                current: current.claim.restart_ordinal,
                candidate: candidate.restart_ordinal,
            });
        }
        if candidate.restart_ordinal > current.claim.restart_ordinal {
            let expected = current.claim.restart_ordinal.checked_add(1).ok_or(
                ProviderCommandJournalError::SkippedRestartOrdinal {
                    current: current.claim.restart_ordinal,
                    candidate: candidate.restart_ordinal,
                },
            )?;
            if candidate.restart_ordinal != expected {
                return Err(ProviderCommandJournalError::SkippedRestartOrdinal {
                    current: current.claim.restart_ordinal,
                    candidate: candidate.restart_ordinal,
                });
            }
            if !candidate.same_workload_fence(&current.claim) {
                return Err(ProviderCommandJournalError::CrossedClaim);
            }
            if !current.kind.resolves_effect() {
                return Err(ProviderCommandJournalError::PriorEffectUnresolved);
            }
            if candidate.operation.is_restart()
                && candidate.source_attempt_id.as_deref() != Some(current.claim.attempt_id())
            {
                return Err(ProviderCommandJournalError::CrossedClaim);
            }
            return self.publish_new_claim(paths, candidate.clone(), None);
        }
        if !candidate.same_attempt_fence(&current.claim) {
            return Err(ProviderCommandJournalError::CrossedClaim);
        }
        if candidate.dispatch_epoch < current.claim.dispatch_epoch {
            return Err(Self::reject_stale_dispatch_epoch(
                current.claim.dispatch_epoch,
                candidate.dispatch_epoch,
            ));
        }
        if candidate.dispatch_epoch == current.claim.dispatch_epoch {
            return Ok(ProviderCommandClaimDecision::AdoptExactAttempt(current));
        }
        let expected = current.claim.dispatch_epoch.checked_add(1).ok_or(
            ProviderCommandJournalError::SkippedDispatchEpoch {
                current: current.claim.dispatch_epoch,
                candidate: candidate.dispatch_epoch,
            },
        )?;
        if candidate.dispatch_epoch != expected {
            return Err(ProviderCommandJournalError::SkippedDispatchEpoch {
                current: current.claim.dispatch_epoch,
                candidate: candidate.dispatch_epoch,
            });
        }
        if !current.kind.authorizes_retry(current.claim.operation) {
            return Err(ProviderCommandJournalError::RetryWithoutAuthority);
        }
        self.publish_new_claim(paths, candidate.clone(), Some(current))
    }

    fn reject_stale_or_crossed(
        &self,
        current: &ProviderCommandClaim,
        candidate: &ProviderCommandClaim,
    ) -> Result<(), ProviderCommandJournalError> {
        if candidate.workload_generation < current.workload_generation {
            return Err(ProviderCommandJournalError::StaleWorkloadGeneration {
                current: current.workload_generation,
                candidate: candidate.workload_generation,
            });
        }
        if candidate.workload_generation == current.workload_generation
            && candidate.restart_ordinal < current.restart_ordinal
        {
            return Err(ProviderCommandJournalError::StaleRestartOrdinal {
                current: current.restart_ordinal,
                candidate: candidate.restart_ordinal,
            });
        }
        if candidate.workload_generation == current.workload_generation
            && candidate.restart_ordinal > current.restart_ordinal.saturating_add(1)
        {
            return Err(ProviderCommandJournalError::SkippedRestartOrdinal {
                current: current.restart_ordinal,
                candidate: candidate.restart_ordinal,
            });
        }
        if candidate.workload_generation == current.workload_generation
            && candidate.restart_ordinal == current.restart_ordinal
            && candidate.same_attempt_fence(current)
            && candidate.dispatch_epoch < current.dispatch_epoch
        {
            return Err(Self::reject_stale_dispatch_epoch(
                current.dispatch_epoch,
                candidate.dispatch_epoch,
            ));
        }
        Err(ProviderCommandJournalError::CrossedClaim)
    }

    fn require_initial_restart_ordinal(
        claim: &ProviderCommandClaim,
    ) -> Result<(), ProviderCommandJournalError> {
        if claim.operation.is_restart() && claim.restart_ordinal != 1 {
            return Err(ProviderCommandJournalError::SkippedRestartOrdinal {
                current: 0,
                candidate: claim.restart_ordinal,
            });
        }
        Ok(())
    }

    fn reject_stale_dispatch_epoch(current: u64, candidate: u64) -> ProviderCommandJournalError {
        ProviderCommandJournalError::StaleDispatchEpoch { current, candidate }
    }

    fn establish_directory(&self, directory: &Path) -> Result<(), ProviderCommandJournalError> {
        crate::backends::oci::durable_directory::establish_durable_directory_chain_with(
            &self.state_root,
            directory,
            "provider command attempt journal",
            sync_directory,
        )
        .map_err(|error| ProviderCommandJournalError::Store {
            message: error.to_string(),
        })
    }

    fn journal_directory_exists(
        &self,
        directory: &Path,
    ) -> Result<bool, ProviderCommandJournalError> {
        let journal_directory = self.state_root.join(JOURNAL_DIRECTORY);
        let namespace_directory =
            journal_directory.join(format!("{:x}", Sha256::digest(self.namespace.as_bytes())));
        debug_assert_eq!(directory, namespace_directory);
        for component in [
            self.state_root.as_path(),
            journal_directory.as_path(),
            namespace_directory.as_path(),
        ] {
            match fs::symlink_metadata(component) {
                Ok(metadata) if metadata.file_type().is_dir() => {}
                Ok(_) => {
                    return Err(ProviderCommandJournalError::Corrupt {
                        message: format!(
                            "provider journal directory component {} is not a real directory",
                            component.display()
                        ),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(error) => {
                    return Err(ProviderCommandJournalError::Store {
                        message: format!(
                            "failed to inspect provider journal directory {}: {error}",
                            component.display()
                        ),
                    });
                }
            }
        }
        Ok(true)
    }

    fn paths(&self, claim: &ProviderCommandClaim) -> JournalPaths {
        let directory = self
            .state_root
            .join(JOURNAL_DIRECTORY)
            .join(format!("{:x}", Sha256::digest(self.namespace.as_bytes())));
        let key = stream_key(&self.namespace, claim);
        JournalPaths {
            record: directory.join(format!("{key}{RECORD_SUFFIX}")),
            stage: directory.join(format!("{key}{STAGE_SUFFIX}")),
            lock: directory.join(format!("{key}{LOCK_SUFFIX}")),
            directory,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct JournalEnvelope {
    version: u32,
    observation_sha256: String,
    observation: ProviderCommandObservation,
}

impl JournalEnvelope {
    fn new(observation: ProviderCommandObservation) -> Result<Self, ProviderCommandJournalError> {
        observation.validate()?;
        Ok(Self {
            version: CURRENT_ENVELOPE_VERSION,
            observation_sha256: observation_sha256(&observation)?,
            observation,
        })
    }

    fn authenticate(
        self,
        path: &Path,
    ) -> Result<ProviderCommandObservation, ProviderCommandJournalError> {
        if self.version != CURRENT_ENVELOPE_VERSION
            || self.observation_sha256 != observation_sha256(&self.observation)?
        {
            return Err(ProviderCommandJournalError::Corrupt {
                message: format!(
                    "{} has an unsupported version or failed SHA-256 authentication",
                    path.display()
                ),
            });
        }
        self.observation.validate()?;
        Ok(self.observation)
    }
}

struct JournalPaths {
    directory: PathBuf,
    record: PathBuf,
    stage: PathBuf,
    lock: PathBuf,
}

fn publish(
    paths: &JournalPaths,
    observation: &ProviderCommandObservation,
) -> Result<(), ProviderCommandJournalError> {
    let envelope = JournalEnvelope::new(observation.clone())?;
    let mut bytes = serde_json::to_vec_pretty(&envelope).map_err(|error| {
        ProviderCommandJournalError::Store {
            message: format!("failed to encode provider command observation: {error}"),
        }
    })?;
    bytes.push(b'\n');
    let result = (|| {
        let mut stage = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&paths.stage)
            .map_err(|error| ProviderCommandJournalError::Store {
                message: format!(
                    "failed to create journal stage {}: {error}",
                    paths.stage.display()
                ),
            })?;
        stage
            .write_all(&bytes)
            .and_then(|()| stage.sync_all())
            .map_err(|error| ProviderCommandJournalError::Store {
                message: format!(
                    "failed to durably write journal stage {}: {error}",
                    paths.stage.display()
                ),
            })?;
        fs::rename(&paths.stage, &paths.record).map_err(|error| {
            ProviderCommandJournalError::Store {
                message: format!(
                    "failed to atomically publish journal {}: {error}",
                    paths.record.display()
                ),
            }
        })?;
        sync_directory(&paths.directory).map_err(|error| ProviderCommandJournalError::Store {
            message: format!(
                "journal {} reached its commit point but directory sync failed; outcome is ambiguous: {error}",
                paths.record.display()
            ),
        })
    })();
    match (result, remove_stale_stage(&paths.stage)) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(primary), Ok(())) => Err(primary),
        (Ok(()), Err(cleanup)) => Err(cleanup),
        (Err(primary), Err(cleanup)) => Err(ProviderCommandJournalError::Store {
            message: format!("{primary}; staged journal cleanup also failed: {cleanup}"),
        }),
    }
}

fn read_if_present(
    path: &Path,
) -> Result<Option<ProviderCommandObservation>, ProviderCommandJournalError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            Err(ProviderCommandJournalError::Corrupt {
                message: format!("journal entry {} is not a regular file", path.display()),
            })
        }
        Ok(_) => {
            let bytes = fs::read(path).map_err(|error| ProviderCommandJournalError::Store {
                message: format!("failed to read journal {}: {error}", path.display()),
            })?;
            let envelope: JournalEnvelope = serde_json::from_slice(&bytes).map_err(|error| {
                ProviderCommandJournalError::Corrupt {
                    message: format!("failed to parse strict journal {}: {error}", path.display()),
                }
            })?;
            envelope.authenticate(path).map(Some)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ProviderCommandJournalError::Store {
            message: format!("failed to inspect journal {}: {error}", path.display()),
        }),
    }
}

fn regular_file_exists(path: &Path, label: &str) -> Result<bool, ProviderCommandJournalError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(true),
        Ok(_) => Err(ProviderCommandJournalError::Corrupt {
            message: format!("{label} {} is not a regular file", path.display()),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(ProviderCommandJournalError::Store {
            message: format!("failed to inspect {label} {}: {error}", path.display()),
        }),
    }
}

fn lock(path: &Path) -> Result<JournalGuard, ProviderCommandJournalError> {
    if let Ok(metadata) = fs::symlink_metadata(path)
        && !metadata.file_type().is_file()
    {
        return Err(ProviderCommandJournalError::Corrupt {
            message: format!("journal lock {} is not a regular file", path.display()),
        });
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|error| ProviderCommandJournalError::Store {
            message: format!("failed to open journal lock {}: {error}", path.display()),
        })?;
    acquire_lock(file, path)
}

fn lock_existing(path: &Path) -> Result<Option<JournalGuard>, ProviderCommandJournalError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(ProviderCommandJournalError::Corrupt {
                message: format!("journal lock {} is not a regular file", path.display()),
            });
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(ProviderCommandJournalError::Store {
                message: format!("failed to inspect journal lock {}: {error}", path.display()),
            });
        }
    }
    let file = match OpenOptions::new().read(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(ProviderCommandJournalError::Store {
                message: format!("failed to open journal lock {}: {error}", path.display()),
            });
        }
    };
    acquire_lock(file, path).map(Some)
}

fn acquire_lock(file: File, path: &Path) -> Result<JournalGuard, ProviderCommandJournalError> {
    if !file
        .metadata()
        .map_err(|error| ProviderCommandJournalError::Store {
            message: format!("failed to inspect journal lock {}: {error}", path.display()),
        })?
        .is_file()
    {
        return Err(ProviderCommandJournalError::Corrupt {
            message: format!("journal lock {} is not a regular file", path.display()),
        });
    }
    let deadline = Instant::now() + LOCK_TIMEOUT;
    loop {
        match FileExt::try_lock_exclusive(&file) {
            Ok(()) => return Ok(JournalGuard { _file: file }),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                #[cfg(test)]
                record_provider_command_lock_contention();
                if Instant::now() >= deadline {
                    return Err(ProviderCommandJournalError::Store {
                        message: format!("timed out acquiring journal lock {}", path.display()),
                    });
                }
                thread::sleep(LOCK_RETRY);
            }
            Err(error) => {
                return Err(ProviderCommandJournalError::Store {
                    message: format!("failed to acquire journal lock {}: {error}", path.display()),
                });
            }
        }
    }
}

fn remove_stale_stage(path: &Path) -> Result<(), ProviderCommandJournalError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            Err(ProviderCommandJournalError::Corrupt {
                message: format!("journal stage {} is not a regular file", path.display()),
            })
        }
        Ok(_) => fs::remove_file(path).map_err(|error| ProviderCommandJournalError::Store {
            message: format!(
                "failed to remove stale journal stage {}: {error}",
                path.display()
            ),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ProviderCommandJournalError::Store {
            message: format!(
                "failed to inspect journal stage {}: {error}",
                path.display()
            ),
        }),
    }
}

fn validate_identity(label: &str, value: &str) -> Result<(), ProviderCommandJournalError> {
    if !is_valid_identity(value) {
        return Err(ProviderCommandJournalError::InvalidClaim {
            message: format!("{label} must be a bounded portable identity"),
        });
    }
    Ok(())
}

fn is_valid_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_IDENTITY_LEN
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

fn deserialize_present_optional_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
}

fn validate_sha256(label: &str, value: &str) -> Result<(), ProviderCommandJournalError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ProviderCommandJournalError::InvalidClaim {
            message: format!("{label} digest must be canonical lowercase SHA-256"),
        });
    }
    Ok(())
}

fn stream_key(namespace: &str, claim: &ProviderCommandClaim) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"nimbus.sandbox.provider-command.stream.v2\0");
    for component in [namespace, claim.authority_id(), claim.operation().as_str()] {
        hasher.update(
            u64::try_from(component.len())
                .expect("a Rust string length fits u64 on supported targets")
                .to_be_bytes(),
        );
        hasher.update(component.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn evidence_sha256(evidence: &[u8]) -> String {
    format!("{:x}", Sha256::digest(evidence))
}

fn observation_sha256(
    observation: &ProviderCommandObservation,
) -> Result<String, ProviderCommandJournalError> {
    let bytes =
        serde_json::to_vec(observation).map_err(|error| ProviderCommandJournalError::Store {
            message: format!("failed to authenticate provider observation: {error}"),
        })?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn sync_directory(path: &Path) -> std::io::Result<()> {
    File::open(path)?.sync_all()
}

#[derive(Debug)]
struct JournalGuard {
    _file: File,
}

#[cfg(test)]
#[path = "provider_command/tests.rs"]
mod tests;

#[cfg(test)]
mod teardown_operation_tests {
    use super::*;

    const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const TEARDOWN_OPERATIONS: [(ProviderCommandOperation, &str); 5] = [
        (
            ProviderCommandOperation::WithdrawFinalPublication,
            "withdraw_final_publication",
        ),
        (ProviderCommandOperation::DrainExecution, "drain_execution"),
        (ProviderCommandOperation::StopExecution, "stop_execution"),
        (ProviderCommandOperation::DetachNetwork, "detach_network"),
        (ProviderCommandOperation::ReleaseNetwork, "release_network"),
    ];

    fn teardown_claim_input(operation: ProviderCommandOperation) -> ProviderCommandClaimInput {
        ProviderCommandClaimInput {
            authority_id: "wsg_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .to_owned(),
            effect_subject: r#"{"kind":"execution","id":"wex_alpha"}"#.to_owned(),
            source_attempt_id: None,
            attempt_id: "wpa_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                .to_owned(),
            dispatch_epoch: 0,
            workload_generation: 7,
            restart_ordinal: 0,
            desired_digest: DIGEST_A.to_owned(),
            source_digest: DIGEST_B.to_owned(),
            network_plan_digest: DIGEST_A.to_owned(),
            provider_target_digest: DIGEST_B.to_owned(),
            operation,
        }
    }

    #[test]
    fn teardown_operations_have_stable_names_and_accept_only_teardown_attempt_domains() {
        for (operation, stable_name) in TEARDOWN_OPERATIONS {
            assert_eq!(operation.family(), ProviderCommandOperationFamily::Teardown);
            assert!(!operation.is_restart());
            assert_eq!(operation.as_str(), stable_name);
            assert_eq!(
                serde_json::to_string(&operation).expect("operation should serialize"),
                format!(r#""{stable_name}""#)
            );
            assert_eq!(
                serde_json::from_str::<ProviderCommandOperation>(&format!(r#""{stable_name}""#))
                    .expect("stable operation name should deserialize"),
                operation
            );

            let claim = ProviderCommandClaim::new(teardown_claim_input(operation))
                .expect("a teardown claim without restart lineage should be valid");
            assert_eq!(claim.operation(), operation);
            assert_eq!(claim.source_attempt_id(), None);
            assert_eq!(claim.restart_ordinal(), 0);
        }
    }

    #[test]
    fn teardown_operations_reject_all_restart_lineage_combinations() {
        for (operation, _) in TEARDOWN_OPERATIONS {
            for (source_attempt_id, restart_ordinal) in [
                (Some("wpa_source".to_owned()), 0),
                (None, 1),
                (Some("wpa_source".to_owned()), 1),
            ] {
                let mut input = teardown_claim_input(operation);
                input.source_attempt_id = source_attempt_id;
                input.restart_ordinal = restart_ordinal;
                assert_eq!(
                    ProviderCommandClaim::new(input)
                        .expect_err("teardown commands must reject restart lineage"),
                    ProviderCommandJournalError::InvalidClaim {
                        message:
                            "teardown commands require no source attempt and restart ordinal zero"
                                .to_owned(),
                    }
                );
            }
        }
    }

    #[test]
    fn provision_and_restart_attempt_domain_messages_remain_exact() {
        let mut invalid_provision = teardown_claim_input(ProviderCommandOperation::PrepareWorkload);
        invalid_provision.source_attempt_id = Some("wpa_source".to_owned());
        invalid_provision.restart_ordinal = 1;
        assert_eq!(
            ProviderCommandClaim::new(invalid_provision)
                .expect_err("provision commands must reject restart lineage"),
            ProviderCommandJournalError::InvalidClaim {
                message: "provision commands require no source attempt and restart ordinal zero"
                    .to_owned(),
            }
        );

        let mut invalid_restart =
            teardown_claim_input(ProviderCommandOperation::PrepareRestartAttempt);
        assert_eq!(
            ProviderCommandClaim::new(invalid_restart)
                .expect_err("restart commands must require restart lineage"),
            ProviderCommandJournalError::InvalidClaim {
                message: "restart commands require a source attempt and nonzero restart ordinal"
                    .to_owned(),
            }
        );

        invalid_restart = teardown_claim_input(ProviderCommandOperation::PrepareRestartAttempt);
        invalid_restart.source_attempt_id = Some("wpa_source".to_owned());
        invalid_restart.restart_ordinal = 1;
        ProviderCommandClaim::new(invalid_restart)
            .expect("an exact restart attempt domain must remain valid");
    }

    #[test]
    fn serialized_teardown_semantics_fail_closed_after_corruption() {
        let claim = ProviderCommandClaim::new(teardown_claim_input(
            ProviderCommandOperation::StopExecution,
        ))
        .expect("fixture teardown claim should be valid");
        let mut value = serde_json::to_value(claim).expect("claim should serialize");
        value["sourceAttemptId"] = serde_json::Value::String("wpa_source".to_owned());
        value["restartOrdinal"] = serde_json::Value::from(1_u64);

        let corrupted: ProviderCommandClaim =
            serde_json::from_value(value).expect("the corruption is structurally valid JSON");
        assert_eq!(
            corrupted
                .validate()
                .expect_err("semantic corruption must fail validation"),
            ProviderCommandJournalError::InvalidClaim {
                message: "teardown commands require no source attempt and restart ordinal zero"
                    .to_owned(),
            }
        );
        assert!(
            serde_json::from_str::<ProviderCommandOperation>(r#""destroy_network""#).is_err(),
            "unknown teardown operations must fail closed"
        );
    }
}
