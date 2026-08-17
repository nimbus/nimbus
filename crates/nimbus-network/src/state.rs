use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::{
    IngressRouteId, ListenerId, NetworkAttachmentId, NetworkLeaseEpoch, NetworkPlan,
    NetworkPlanDigest, NetworkPlanId, NetworkProviderHandle, NetworkResourceGeneration,
    NetworkSegmentId, PortLeaseId, PublishedEndpointId,
};

/// Stable identity of any provider-neutral network resource governed by a
/// plan.
///
/// The enum preserves each resource domain instead of flattening IDs into an
/// untyped string that could be confused across stores or projections.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum NetworkResourceId {
    /// Workload-to-network attachment.
    Attachment(NetworkAttachmentId),
    /// Portable address segment allocation.
    Segment(NetworkSegmentId),
    /// Published reachable endpoint.
    PublishedEndpoint(PublishedEndpointId),
    /// Host or provider listener.
    Listener(ListenerId),
    /// Admitted ingress route.
    IngressRoute(IngressRouteId),
    /// Host-global port reservation.
    PortLease(PortLeaseId),
}

impl From<NetworkAttachmentId> for NetworkResourceId {
    fn from(value: NetworkAttachmentId) -> Self {
        Self::Attachment(value)
    }
}

impl From<NetworkSegmentId> for NetworkResourceId {
    fn from(value: NetworkSegmentId) -> Self {
        Self::Segment(value)
    }
}

impl From<PublishedEndpointId> for NetworkResourceId {
    fn from(value: PublishedEndpointId) -> Self {
        Self::PublishedEndpoint(value)
    }
}

impl From<ListenerId> for NetworkResourceId {
    fn from(value: ListenerId) -> Self {
        Self::Listener(value)
    }
}

impl From<IngressRouteId> for NetworkResourceId {
    fn from(value: IngressRouteId) -> Self {
        Self::IngressRoute(value)
    }
}

impl From<PortLeaseId> for NetworkResourceId {
    fn from(value: PortLeaseId) -> Self {
        Self::PortLease(value)
    }
}

/// Durable network-owned lifecycle phase for one resource generation.
///
/// This is authority state, not a provider observation or a cross-domain
/// workload saga phase. Ambiguous effects move to `CleanupPending`; terminal
/// release requires confirmed deletion evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum NetworkResourcePhase {
    /// Stable identities and leases exist; no provider effect is assumed.
    Reserved,
    /// Provider realization is in progress.
    Provisioning,
    /// Provider inspection proves required realization preconditions.
    Ready,
    /// Reachability publication is in progress.
    Publishing,
    /// The resource is active for this generation.
    Active,
    /// New use is fenced and publication withdrawal is in progress.
    Withdrawing,
    /// Existing use is draining under a bounded owner policy.
    Draining,
    /// Provider deletion/detach is in progress.
    Deleting,
    /// An effect or deletion outcome is ambiguous; identity remains fenced.
    CleanupPending,
    /// Confirmed safe terminal release.
    Released,
    /// Confirmed terminal failure before any reusable effect remains.
    Failed,
}

impl NetworkResourcePhase {
    /// True only for states that never accept a later phase in the same
    /// generation.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Released | Self::Failed)
    }

    /// Decide whether evidence permits a requested durable phase transition.
    ///
    /// Replaying the current phase is always idempotent. Every other legal edge
    /// is enumerated explicitly; there is no ordinal "greater than" shortcut
    /// that could accidentally skip cleanup.
    pub const fn allows_transition(
        self,
        target: Self,
        evidence: NetworkTransitionEvidence,
    ) -> bool {
        if self as u8 == target as u8 {
            return true;
        }
        matches!(
            (self, target, evidence),
            (
                Self::Reserved,
                Self::Provisioning,
                NetworkTransitionEvidence::Progress
            ) | (
                Self::Provisioning,
                Self::Ready,
                NetworkTransitionEvidence::Progress
            ) | (
                Self::Ready,
                Self::Publishing,
                NetworkTransitionEvidence::Progress
            ) | (
                Self::Publishing,
                Self::Active,
                NetworkTransitionEvidence::Progress
            ) | (
                Self::Provisioning | Self::Ready | Self::Publishing | Self::Active,
                Self::Withdrawing,
                NetworkTransitionEvidence::Progress
            ) | (
                Self::Withdrawing,
                Self::Draining | Self::Deleting,
                NetworkTransitionEvidence::Progress
            ) | (
                Self::Draining | Self::CleanupPending,
                Self::Deleting,
                NetworkTransitionEvidence::Progress
            ) | (
                Self::Reserved,
                Self::Released | Self::Failed,
                NetworkTransitionEvidence::ConfirmedNoEffect
            ) | (
                Self::Provisioning,
                Self::Failed,
                NetworkTransitionEvidence::ConfirmedNoEffect
            ) | (
                Self::Provisioning
                    | Self::Ready
                    | Self::Publishing
                    | Self::Active
                    | Self::Withdrawing
                    | Self::Draining
                    | Self::Deleting,
                Self::CleanupPending,
                NetworkTransitionEvidence::AmbiguousEffect
            ) | (
                Self::Deleting | Self::CleanupPending,
                Self::Released,
                NetworkTransitionEvidence::DeletionConfirmed
            ) | (
                Self::Deleting | Self::CleanupPending,
                Self::Provisioning,
                NetworkTransitionEvidence::DeletionConfirmedForReprovision
            )
        )
    }
}

/// Evidence class required to make a durable lifecycle transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkTransitionEvidence {
    /// Normal forward lifecycle progress or the beginning of compensating
    /// teardown.
    Progress,
    /// Inspection proves no provider effect exists for this resource.
    ConfirmedNoEffect,
    /// A provider effect or deletion may exist but its exact outcome is unknown.
    AmbiguousEffect,
    /// Inspection confirms the provider effect is absent and reuse is safe.
    DeletionConfirmed,
    /// Inspection confirms the provider effect is absent while the same desired
    /// resource generation remains admitted and must be realized again.
    ///
    /// This never grants resurrection from `Released` or `Failed`; it only
    /// returns an in-progress delete/cleanup cycle to `Provisioning`.
    DeletionConfirmedForReprovision,
}

/// Immutable identity and fencing token for one durable resource generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkResourceVersion {
    plan_id: NetworkPlanId,
    resource_id: NetworkResourceId,
    generation: NetworkResourceGeneration,
    plan_digest: NetworkPlanDigest,
    lease_epoch: NetworkLeaseEpoch,
}

impl NetworkResourceVersion {
    /// Bind a resource identity and lease epoch to one desired plan
    /// generation.
    pub fn for_plan(
        plan: &NetworkPlan,
        resource_id: NetworkResourceId,
        lease_epoch: NetworkLeaseEpoch,
    ) -> Self {
        Self {
            plan_id: plan.plan_id().clone(),
            resource_id,
            generation: plan.generation(),
            plan_digest: plan.digest(),
            lease_epoch,
        }
    }

    /// Parent desired-plan identity.
    pub fn plan_id(&self) -> &NetworkPlanId {
        &self.plan_id
    }

    /// Stable resource identity.
    pub fn resource_id(&self) -> &NetworkResourceId {
        &self.resource_id
    }

    /// Desired generation this resource realizes.
    pub fn generation(&self) -> NetworkResourceGeneration {
        self.generation
    }

    /// Digest preventing equal-generation desired divergence.
    pub fn plan_digest(&self) -> NetworkPlanDigest {
        self.plan_digest
    }

    /// Allocation/lease fencing epoch.
    pub fn lease_epoch(&self) -> NetworkLeaseEpoch {
        self.lease_epoch
    }
}

/// Generation-scoped request to change one durable resource phase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkStateTransition {
    version: NetworkResourceVersion,
    target: NetworkResourcePhase,
    evidence: NetworkTransitionEvidence,
}

impl NetworkStateTransition {
    /// Construct an explicit transition request.
    pub fn new(
        version: NetworkResourceVersion,
        target: NetworkResourcePhase,
        evidence: NetworkTransitionEvidence,
    ) -> Self {
        Self {
            version,
            target,
            evidence,
        }
    }

    /// Identity and fencing token the caller expects to mutate.
    pub fn version(&self) -> &NetworkResourceVersion {
        &self.version
    }

    /// Requested durable phase.
    pub fn target(&self) -> NetworkResourcePhase {
        self.target
    }

    /// Evidence authorizing the requested edge.
    pub fn evidence(&self) -> NetworkTransitionEvidence {
        self.evidence
    }
}

/// Network-owned durable state for one resource generation.
///
/// This type contains authority and an optional redacted provider handle. It
/// contains no observed readiness/status fields and performs no provider
/// effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "DurableNetworkResourceStateWire")]
pub struct DurableNetworkResourceState {
    version: NetworkResourceVersion,
    phase: NetworkResourcePhase,
    provider_handle: Option<NetworkProviderHandle>,
}

impl DurableNetworkResourceState {
    /// Reserve a resource identity for one desired plan generation.
    pub fn reserve(
        plan: &NetworkPlan,
        resource_id: NetworkResourceId,
        lease_epoch: NetworkLeaseEpoch,
    ) -> Self {
        Self {
            version: NetworkResourceVersion::for_plan(plan, resource_id, lease_epoch),
            phase: NetworkResourcePhase::Reserved,
            provider_handle: None,
        }
    }

    /// Immutable resource version/fencing token.
    pub fn version(&self) -> &NetworkResourceVersion {
        &self.version
    }

    /// Current authoritative phase.
    pub fn phase(&self) -> NetworkResourcePhase {
        self.phase
    }

    /// Opaque provider handle when one has been durably adopted.
    pub fn provider_handle(&self) -> Option<&NetworkProviderHandle> {
        self.provider_handle.as_ref()
    }

    /// Authenticate an exact resource version without changing phase or handle.
    ///
    /// Concrete durable authorities use this before returning an idempotent
    /// replay. Keeping the comparison here prevents concept stores from
    /// reimplementing generation, digest, and lease-epoch fencing.
    pub fn authenticate_version(
        &self,
        candidate: &NetworkResourceVersion,
    ) -> Result<(), NetworkStateError> {
        self.validate_version(candidate)
    }

    /// Apply a generation-, digest-, resource-, and epoch-scoped phase change.
    pub fn apply_transition(
        &mut self,
        transition: &NetworkStateTransition,
    ) -> Result<NetworkStateMutation, NetworkStateError> {
        self.validate_version(transition.version())?;
        if transition.target == self.phase {
            return Ok(NetworkStateMutation::Idempotent);
        }
        if !self
            .phase
            .allows_transition(transition.target, transition.evidence)
        {
            return Err(NetworkStateError::IllegalTransition {
                from: self.phase,
                to: transition.target,
                evidence: transition.evidence,
            });
        }
        if transition.target == NetworkResourcePhase::Released
            && self.provider_handle.is_some()
            && transition.evidence != NetworkTransitionEvidence::DeletionConfirmed
        {
            return Err(NetworkStateError::ProviderHandleRequiresCleanup);
        }
        if transition.target == NetworkResourcePhase::Failed && self.provider_handle.is_some() {
            return Err(NetworkStateError::ProviderHandleRequiresCleanup);
        }
        self.phase = transition.target;
        Ok(NetworkStateMutation::Applied)
    }

    /// Durably adopt the opaque handle returned by a provider effect.
    ///
    /// Replaying the same handle is idempotent; changing a handle in the same
    /// resource generation fails closed.
    pub fn record_provider_handle(
        &mut self,
        expected: &NetworkResourceVersion,
        provider_handle: NetworkProviderHandle,
    ) -> Result<NetworkStateMutation, NetworkStateError> {
        self.validate_version(expected)?;
        if matches!(
            self.phase,
            NetworkResourcePhase::Reserved
                | NetworkResourcePhase::Released
                | NetworkResourcePhase::Failed
        ) {
            return Err(NetworkStateError::ProviderHandleNotAllowed { phase: self.phase });
        }
        match self.provider_handle.as_ref() {
            Some(current) if current == &provider_handle => Ok(NetworkStateMutation::Idempotent),
            Some(_) => Err(NetworkStateError::ProviderHandleConflict),
            None => {
                self.provider_handle = Some(provider_handle);
                Ok(NetworkStateMutation::Applied)
            }
        }
    }

    fn validate_version(
        &self,
        candidate: &NetworkResourceVersion,
    ) -> Result<(), NetworkStateError> {
        if candidate.plan_id != self.version.plan_id {
            return Err(NetworkStateError::PlanIdentityMismatch);
        }
        if candidate.resource_id != self.version.resource_id {
            return Err(NetworkStateError::ResourceIdentityMismatch);
        }
        match candidate.generation.cmp(&self.version.generation) {
            std::cmp::Ordering::Less => {
                return Err(NetworkStateError::StaleGeneration {
                    current: self.version.generation,
                    candidate: candidate.generation,
                });
            }
            std::cmp::Ordering::Greater => {
                return Err(NetworkStateError::FutureGeneration {
                    current: self.version.generation,
                    candidate: candidate.generation,
                });
            }
            std::cmp::Ordering::Equal => {}
        }
        if candidate.plan_digest != self.version.plan_digest {
            return Err(NetworkStateError::PlanDigestConflict {
                generation: self.version.generation,
            });
        }
        match candidate.lease_epoch.cmp(&self.version.lease_epoch) {
            std::cmp::Ordering::Less => Err(NetworkStateError::StaleLeaseEpoch {
                current: self.version.lease_epoch,
                candidate: candidate.lease_epoch,
            }),
            std::cmp::Ordering::Greater => Err(NetworkStateError::FutureLeaseEpoch {
                current: self.version.lease_epoch,
                candidate: candidate.lease_epoch,
            }),
            std::cmp::Ordering::Equal => Ok(()),
        }
    }
}

/// Whether a durable state operation changed bytes or was an exact replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkStateMutation {
    /// The requested durable mutation changed state.
    Applied,
    /// The same generation/digest operation had already completed.
    Idempotent,
}

/// Pure validation failure from a durable network resource operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkStateError {
    /// The operation addressed another stable plan.
    PlanIdentityMismatch,
    /// The operation addressed another stable resource.
    ResourceIdentityMismatch,
    /// The operation belongs to an older generation.
    StaleGeneration {
        current: NetworkResourceGeneration,
        candidate: NetworkResourceGeneration,
    },
    /// A future generation cannot mutate an older generation's record.
    FutureGeneration {
        current: NetworkResourceGeneration,
        candidate: NetworkResourceGeneration,
    },
    /// Equal generation carries different desired content.
    PlanDigestConflict {
        generation: NetworkResourceGeneration,
    },
    /// The operation carries a fenced older lease epoch.
    StaleLeaseEpoch {
        current: NetworkLeaseEpoch,
        candidate: NetworkLeaseEpoch,
    },
    /// A future epoch cannot silently rewrite the issuing allocation epoch.
    FutureLeaseEpoch {
        current: NetworkLeaseEpoch,
        candidate: NetworkLeaseEpoch,
    },
    /// The requested phase/evidence edge is not in the legal state machine.
    IllegalTransition {
        from: NetworkResourcePhase,
        to: NetworkResourcePhase,
        evidence: NetworkTransitionEvidence,
    },
    /// A provider handle cannot first appear before provisioning or after a
    /// terminal phase.
    ProviderHandleNotAllowed { phase: NetworkResourcePhase },
    /// A different provider handle was presented for the same resource
    /// generation.
    ProviderHandleConflict,
    /// A known provider handle prevents terminal failure until provider
    /// deletion is confirmed.
    ProviderHandleRequiresCleanup,
}

impl Display for NetworkStateError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlanIdentityMismatch => {
                formatter.write_str("network state operation addressed a different plan")
            }
            Self::ResourceIdentityMismatch => {
                formatter.write_str("network state operation addressed a different resource")
            }
            Self::StaleGeneration { current, candidate } => write!(
                formatter,
                "stale network resource generation {}; current generation is {}",
                candidate.as_u64(),
                current.as_u64()
            ),
            Self::FutureGeneration { current, candidate } => write!(
                formatter,
                "network resource generation {} cannot mutate generation {}",
                candidate.as_u64(),
                current.as_u64()
            ),
            Self::PlanDigestConflict { generation } => write!(
                formatter,
                "network resource generation {} has a conflicting plan digest",
                generation.as_u64()
            ),
            Self::StaleLeaseEpoch { current, candidate } => write!(
                formatter,
                "stale network lease epoch {}; current epoch is {}",
                candidate.as_u64(),
                current.as_u64()
            ),
            Self::FutureLeaseEpoch { current, candidate } => write!(
                formatter,
                "network lease epoch {} cannot rewrite allocation epoch {}",
                candidate.as_u64(),
                current.as_u64()
            ),
            Self::IllegalTransition { from, to, evidence } => write!(
                formatter,
                "illegal network resource transition {from:?} -> {to:?} with {evidence:?}"
            ),
            Self::ProviderHandleNotAllowed { phase } => write!(
                formatter,
                "provider handle cannot first appear in network phase {phase:?}"
            ),
            Self::ProviderHandleConflict => formatter
                .write_str("network resource generation already has a different provider handle"),
            Self::ProviderHandleRequiresCleanup => formatter.write_str(
                "network resource has a provider handle and must clean up before terminal failure",
            ),
        }
    }
}

impl StdError for NetworkStateError {}

#[derive(Deserialize)]
struct DurableNetworkResourceStateWire {
    version: NetworkResourceVersion,
    phase: NetworkResourcePhase,
    provider_handle: Option<NetworkProviderHandle>,
}

impl TryFrom<DurableNetworkResourceStateWire> for DurableNetworkResourceState {
    type Error = NetworkStateError;

    fn try_from(value: DurableNetworkResourceStateWire) -> Result<Self, Self::Error> {
        if value.provider_handle.is_some()
            && matches!(
                value.phase,
                NetworkResourcePhase::Reserved | NetworkResourcePhase::Failed
            )
        {
            return Err(NetworkStateError::ProviderHandleNotAllowed { phase: value.phase });
        }
        Ok(Self {
            version: value.version,
            phase: value.phase,
            provider_handle: value.provider_handle,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NetworkPlanContentDigest;
    use crate::capability::test_requirements;
    use crate::{NetworkProviderHandle, NetworkProviderId};

    const PHASES: [NetworkResourcePhase; 11] = [
        NetworkResourcePhase::Reserved,
        NetworkResourcePhase::Provisioning,
        NetworkResourcePhase::Ready,
        NetworkResourcePhase::Publishing,
        NetworkResourcePhase::Active,
        NetworkResourcePhase::Withdrawing,
        NetworkResourcePhase::Draining,
        NetworkResourcePhase::Deleting,
        NetworkResourcePhase::CleanupPending,
        NetworkResourcePhase::Released,
        NetworkResourcePhase::Failed,
    ];
    const EVIDENCE: [NetworkTransitionEvidence; 5] = [
        NetworkTransitionEvidence::Progress,
        NetworkTransitionEvidence::ConfirmedNoEffect,
        NetworkTransitionEvidence::AmbiguousEffect,
        NetworkTransitionEvidence::DeletionConfirmed,
        NetworkTransitionEvidence::DeletionConfirmedForReprovision,
    ];

    fn plan_id(value: &str) -> NetworkPlanId {
        value.parse().expect("fixture plan id should parse")
    }

    fn attachment_id(value: &str) -> NetworkAttachmentId {
        value.parse().expect("fixture attachment id should parse")
    }

    fn provider_id(value: &str) -> NetworkProviderId {
        value.parse().expect("fixture provider id should parse")
    }

    fn plan(generation: u64, content: &[u8]) -> NetworkPlan {
        NetworkPlan::new(
            plan_id("netplan_01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            NetworkResourceGeneration::new(generation),
            NetworkPlanContentDigest::sha256(content),
            test_requirements(),
        )
    }

    fn resource() -> NetworkResourceId {
        attachment_id("netattachment_01ARZ3NDEKTSV4RRFFQ69G5FAV").into()
    }

    fn state() -> DurableNetworkResourceState {
        DurableNetworkResourceState::reserve(
            &plan(7, b"desired"),
            resource(),
            NetworkLeaseEpoch::new(11),
        )
    }

    fn transition(
        state: &DurableNetworkResourceState,
        target: NetworkResourcePhase,
        evidence: NetworkTransitionEvidence,
    ) -> NetworkStateTransition {
        NetworkStateTransition::new(state.version().clone(), target, evidence)
    }

    fn expected_legal(
        from: NetworkResourcePhase,
        to: NetworkResourcePhase,
        evidence: NetworkTransitionEvidence,
    ) -> bool {
        if from == to {
            return true;
        }
        const EDGES: [(
            NetworkResourcePhase,
            NetworkResourcePhase,
            NetworkTransitionEvidence,
        ); 26] = [
            (
                NetworkResourcePhase::Reserved,
                NetworkResourcePhase::Provisioning,
                NetworkTransitionEvidence::Progress,
            ),
            (
                NetworkResourcePhase::Provisioning,
                NetworkResourcePhase::Ready,
                NetworkTransitionEvidence::Progress,
            ),
            (
                NetworkResourcePhase::Ready,
                NetworkResourcePhase::Publishing,
                NetworkTransitionEvidence::Progress,
            ),
            (
                NetworkResourcePhase::Publishing,
                NetworkResourcePhase::Active,
                NetworkTransitionEvidence::Progress,
            ),
            (
                NetworkResourcePhase::Provisioning,
                NetworkResourcePhase::Withdrawing,
                NetworkTransitionEvidence::Progress,
            ),
            (
                NetworkResourcePhase::Ready,
                NetworkResourcePhase::Withdrawing,
                NetworkTransitionEvidence::Progress,
            ),
            (
                NetworkResourcePhase::Publishing,
                NetworkResourcePhase::Withdrawing,
                NetworkTransitionEvidence::Progress,
            ),
            (
                NetworkResourcePhase::Active,
                NetworkResourcePhase::Withdrawing,
                NetworkTransitionEvidence::Progress,
            ),
            (
                NetworkResourcePhase::Withdrawing,
                NetworkResourcePhase::Draining,
                NetworkTransitionEvidence::Progress,
            ),
            (
                NetworkResourcePhase::Withdrawing,
                NetworkResourcePhase::Deleting,
                NetworkTransitionEvidence::Progress,
            ),
            (
                NetworkResourcePhase::Draining,
                NetworkResourcePhase::Deleting,
                NetworkTransitionEvidence::Progress,
            ),
            (
                NetworkResourcePhase::CleanupPending,
                NetworkResourcePhase::Deleting,
                NetworkTransitionEvidence::Progress,
            ),
            (
                NetworkResourcePhase::Reserved,
                NetworkResourcePhase::Released,
                NetworkTransitionEvidence::ConfirmedNoEffect,
            ),
            (
                NetworkResourcePhase::Reserved,
                NetworkResourcePhase::Failed,
                NetworkTransitionEvidence::ConfirmedNoEffect,
            ),
            (
                NetworkResourcePhase::Provisioning,
                NetworkResourcePhase::Failed,
                NetworkTransitionEvidence::ConfirmedNoEffect,
            ),
            (
                NetworkResourcePhase::Provisioning,
                NetworkResourcePhase::CleanupPending,
                NetworkTransitionEvidence::AmbiguousEffect,
            ),
            (
                NetworkResourcePhase::Ready,
                NetworkResourcePhase::CleanupPending,
                NetworkTransitionEvidence::AmbiguousEffect,
            ),
            (
                NetworkResourcePhase::Publishing,
                NetworkResourcePhase::CleanupPending,
                NetworkTransitionEvidence::AmbiguousEffect,
            ),
            (
                NetworkResourcePhase::Active,
                NetworkResourcePhase::CleanupPending,
                NetworkTransitionEvidence::AmbiguousEffect,
            ),
            (
                NetworkResourcePhase::Withdrawing,
                NetworkResourcePhase::CleanupPending,
                NetworkTransitionEvidence::AmbiguousEffect,
            ),
            (
                NetworkResourcePhase::Draining,
                NetworkResourcePhase::CleanupPending,
                NetworkTransitionEvidence::AmbiguousEffect,
            ),
            (
                NetworkResourcePhase::Deleting,
                NetworkResourcePhase::CleanupPending,
                NetworkTransitionEvidence::AmbiguousEffect,
            ),
            (
                NetworkResourcePhase::Deleting,
                NetworkResourcePhase::Released,
                NetworkTransitionEvidence::DeletionConfirmed,
            ),
            (
                NetworkResourcePhase::CleanupPending,
                NetworkResourcePhase::Released,
                NetworkTransitionEvidence::DeletionConfirmed,
            ),
            (
                NetworkResourcePhase::Deleting,
                NetworkResourcePhase::Provisioning,
                NetworkTransitionEvidence::DeletionConfirmedForReprovision,
            ),
            (
                NetworkResourcePhase::CleanupPending,
                NetworkResourcePhase::Provisioning,
                NetworkTransitionEvidence::DeletionConfirmedForReprovision,
            ),
        ];
        EDGES.contains(&(from, to, evidence))
    }

    #[test]
    fn exhaustive_phase_evidence_matrix_accepts_only_named_edges() {
        for from in PHASES {
            for to in PHASES {
                for evidence in EVIDENCE {
                    assert_eq!(
                        from.allows_transition(to, evidence),
                        expected_legal(from, to, evidence),
                        "phase matrix mismatch for {from:?} -> {to:?} with {evidence:?}"
                    );

                    let mut candidate = state();
                    candidate.phase = from;
                    let operation = transition(&candidate, to, evidence);
                    let before = candidate.clone();
                    let result = candidate.apply_transition(&operation);
                    if from == to {
                        assert_eq!(result, Ok(NetworkStateMutation::Idempotent));
                        assert_eq!(candidate, before);
                    } else if expected_legal(from, to, evidence) {
                        assert_eq!(result, Ok(NetworkStateMutation::Applied));
                        assert_eq!(candidate.phase(), to);
                    } else {
                        assert_eq!(
                            result,
                            Err(NetworkStateError::IllegalTransition { from, to, evidence })
                        );
                        assert_eq!(
                            candidate, before,
                            "an illegal transition must not mutate authority"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn terminal_phases_cannot_move_or_reactivate() {
        for terminal in [NetworkResourcePhase::Released, NetworkResourcePhase::Failed] {
            assert!(terminal.is_terminal());
            for target in PHASES {
                if target == terminal {
                    continue;
                }
                for evidence in EVIDENCE {
                    assert!(
                        !terminal.allows_transition(target, evidence),
                        "{terminal:?} must not transition to {target:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn deletion_confirmed_reprovision_retains_the_exact_resource_version_and_handle() {
        let mut state = state();
        let version = state.version().clone();
        state.phase = NetworkResourcePhase::Deleting;
        state.provider_handle = Some(
            NetworkProviderHandle::new(
                provider_id("netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"),
                "stable-attachment-handle",
            )
            .expect("provider handle should validate"),
        );
        let before_version = state.version().clone();
        let before_handle = state.provider_handle().cloned();

        assert_eq!(
            state.apply_transition(&NetworkStateTransition::new(
                version,
                NetworkResourcePhase::Provisioning,
                NetworkTransitionEvidence::DeletionConfirmedForReprovision,
            )),
            Ok(NetworkStateMutation::Applied)
        );
        assert_eq!(state.phase(), NetworkResourcePhase::Provisioning);
        assert_eq!(state.version(), &before_version);
        assert_eq!(state.provider_handle(), before_handle.as_ref());
    }

    #[test]
    fn version_validation_rejects_wrong_identity_generation_digest_and_epoch() {
        let mut state = state();
        let base = state.version().clone();

        let cases = [
            (
                NetworkResourceVersion {
                    plan_id: plan_id("netplan_01ARZ3NDEKTSV4RRFFQ69G5FAW"),
                    ..base.clone()
                },
                NetworkStateError::PlanIdentityMismatch,
            ),
            (
                NetworkResourceVersion {
                    resource_id: attachment_id("netattachment_01ARZ3NDEKTSV4RRFFQ69G5FAW").into(),
                    ..base.clone()
                },
                NetworkStateError::ResourceIdentityMismatch,
            ),
            (
                NetworkResourceVersion {
                    generation: NetworkResourceGeneration::new(6),
                    ..base.clone()
                },
                NetworkStateError::StaleGeneration {
                    current: NetworkResourceGeneration::new(7),
                    candidate: NetworkResourceGeneration::new(6),
                },
            ),
            (
                NetworkResourceVersion {
                    generation: NetworkResourceGeneration::new(8),
                    ..base.clone()
                },
                NetworkStateError::FutureGeneration {
                    current: NetworkResourceGeneration::new(7),
                    candidate: NetworkResourceGeneration::new(8),
                },
            ),
            (
                NetworkResourceVersion {
                    plan_digest: NetworkPlanDigest::from_bytes([0x42; 32]),
                    ..base.clone()
                },
                NetworkStateError::PlanDigestConflict {
                    generation: NetworkResourceGeneration::new(7),
                },
            ),
            (
                NetworkResourceVersion {
                    lease_epoch: NetworkLeaseEpoch::new(10),
                    ..base.clone()
                },
                NetworkStateError::StaleLeaseEpoch {
                    current: NetworkLeaseEpoch::new(11),
                    candidate: NetworkLeaseEpoch::new(10),
                },
            ),
            (
                NetworkResourceVersion {
                    lease_epoch: NetworkLeaseEpoch::new(12),
                    ..base
                },
                NetworkStateError::FutureLeaseEpoch {
                    current: NetworkLeaseEpoch::new(11),
                    candidate: NetworkLeaseEpoch::new(12),
                },
            ),
        ];

        for (version, expected) in cases {
            let before = state.clone();
            let result = state.apply_transition(&NetworkStateTransition::new(
                version,
                NetworkResourcePhase::Provisioning,
                NetworkTransitionEvidence::Progress,
            ));
            assert_eq!(result, Err(expected));
            assert_eq!(state, before);
        }
    }

    #[test]
    fn provider_handle_is_generation_scoped_idempotent_and_conflict_safe() {
        let mut state = state();
        let version = state.version().clone();
        let first = NetworkProviderHandle::new(
            provider_id("netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            "provider-handle-a",
        )
        .expect("provider handle should validate");

        assert_eq!(
            state.record_provider_handle(&version, first.clone()),
            Err(NetworkStateError::ProviderHandleNotAllowed {
                phase: NetworkResourcePhase::Reserved,
            })
        );
        assert_eq!(
            state.apply_transition(&transition(
                &state,
                NetworkResourcePhase::Provisioning,
                NetworkTransitionEvidence::Progress,
            )),
            Ok(NetworkStateMutation::Applied)
        );
        assert_eq!(
            state.record_provider_handle(&version, first.clone()),
            Ok(NetworkStateMutation::Applied)
        );
        assert_eq!(
            state.record_provider_handle(&version, first.clone()),
            Ok(NetworkStateMutation::Idempotent)
        );

        let conflicting = NetworkProviderHandle::new(
            provider_id("netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAW"),
            "provider-handle-b",
        )
        .expect("provider handle should validate");
        assert_eq!(
            state.record_provider_handle(&version, conflicting),
            Err(NetworkStateError::ProviderHandleConflict)
        );
        assert_eq!(state.provider_handle(), Some(&first));
        assert_eq!(
            state.apply_transition(&transition(
                &state,
                NetworkResourcePhase::Failed,
                NetworkTransitionEvidence::ConfirmedNoEffect,
            )),
            Err(NetworkStateError::ProviderHandleRequiresCleanup)
        );
        assert_eq!(state.phase(), NetworkResourcePhase::Provisioning);
    }

    #[test]
    fn resource_id_wire_preserves_domain() {
        let resource = resource();
        let json = serde_json::to_string(&resource).expect("resource should serialize");

        assert_eq!(
            json,
            r#"{"kind":"attachment","id":"netattachment_01ARZ3NDEKTSV4RRFFQ69G5FAV"}"#
        );
        assert_eq!(
            serde_json::from_str::<NetworkResourceId>(&json).expect("resource should deserialize"),
            resource
        );
        assert!(
            serde_json::from_str::<NetworkResourceId>(
                r#"{"kind":"attachment","id":"netsegment_01ARZ3NDEKTSV4RRFFQ69G5FAV"}"#
            )
            .expect_err("cross-domain resource id must fail")
            .to_string()
            .contains("expected `netattachment_<ULID>`")
        );
    }

    #[test]
    fn durable_state_round_trip_preserves_authority_and_redacts_debug_output() {
        let mut state = state();
        let version = state.version().clone();
        state
            .apply_transition(&NetworkStateTransition::new(
                version.clone(),
                NetworkResourcePhase::Provisioning,
                NetworkTransitionEvidence::Progress,
            ))
            .expect("provisioning transition should succeed");
        state
            .record_provider_handle(
                &version,
                NetworkProviderHandle::new(
                    provider_id("netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"),
                    "secret-provider-reference",
                )
                .expect("provider handle should validate"),
            )
            .expect("provider handle should be recorded");

        let json = serde_json::to_string(&state).expect("durable state should serialize");
        let decoded: DurableNetworkResourceState =
            serde_json::from_str(&json).expect("durable state should deserialize");

        assert_eq!(decoded, state);
        assert!(
            !format!("{state:?}").contains("secret-provider-reference"),
            "provider handles must remain redacted from aggregate debug output"
        );
    }

    #[test]
    fn durable_state_wire_rejects_api_unreachable_handle_phases() {
        let handle = NetworkProviderHandle::new(
            provider_id("netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            "provider-reference",
        )
        .expect("provider handle should validate");
        let mut wire = serde_json::to_value(state()).expect("state should serialize");
        wire["provider_handle"] =
            serde_json::to_value(handle).expect("provider handle should serialize");

        for phase in ["reserved", "failed"] {
            wire["phase"] = serde_json::Value::String(phase.to_owned());
            assert!(
                serde_json::from_value::<DurableNetworkResourceState>(wire.clone())
                    .expect_err("unreachable phase/handle pair must fail")
                    .to_string()
                    .contains("provider handle cannot first appear"),
                "phase {phase} must fail for the named invariant"
            );
        }
    }

    #[test]
    fn deletion_proof_may_retain_a_historical_provider_handle() {
        let mut state = state();
        let version = state.version().clone();
        state
            .apply_transition(&NetworkStateTransition::new(
                version.clone(),
                NetworkResourcePhase::Provisioning,
                NetworkTransitionEvidence::Progress,
            ))
            .expect("provisioning should begin");
        state
            .record_provider_handle(
                &version,
                NetworkProviderHandle::new(
                    provider_id("netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"),
                    "deleted-provider-reference",
                )
                .expect("provider handle should validate"),
            )
            .expect("provider handle should be durable");
        state
            .apply_transition(&NetworkStateTransition::new(
                version.clone(),
                NetworkResourcePhase::CleanupPending,
                NetworkTransitionEvidence::AmbiguousEffect,
            ))
            .expect("ambiguous effect should quarantine");
        state
            .apply_transition(&NetworkStateTransition::new(
                version,
                NetworkResourcePhase::Released,
                NetworkTransitionEvidence::DeletionConfirmed,
            ))
            .expect("deletion proof should release");

        let json = serde_json::to_string(&state).expect("released state should serialize");
        let decoded: DurableNetworkResourceState =
            serde_json::from_str(&json).expect("released state should deserialize");
        assert_eq!(decoded, state);
        assert_eq!(decoded.phase(), NetworkResourcePhase::Released);
        assert!(
            decoded.provider_handle().is_some(),
            "a deleted provider handle remains audit and reconciliation history"
        );
    }
}
