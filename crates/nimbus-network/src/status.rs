use std::collections::BTreeSet;
use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::{
    NetworkLeaseEpoch, NetworkProviderId, NetworkResourceGeneration, NetworkResourcePhase,
    NetworkResourceVersion,
};

/// Provider-neutral condition carried by observed network status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkConditionKind {
    /// Required provider realization is ready for use.
    Ready,
    /// Reachability publication is visible.
    Published,
    /// The resource is serving with reduced capability.
    Degraded,
    /// Cleanup remains fenced and incomplete.
    CleanupPending,
}

/// Tri-state value for an observed condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkConditionState {
    /// Evidence confirms the condition.
    True,
    /// Evidence disproves the condition.
    False,
    /// The provider cannot currently establish the condition.
    Unknown,
}

/// One bounded, provider-neutral observed condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NetworkCondition {
    kind: NetworkConditionKind,
    state: NetworkConditionState,
}

impl NetworkCondition {
    /// Construct one condition value.
    pub const fn new(kind: NetworkConditionKind, state: NetworkConditionState) -> Self {
        Self { kind, state }
    }

    /// Condition category.
    pub const fn kind(self) -> NetworkConditionKind {
        self.kind
    }

    /// Tri-state evidence.
    pub const fn state(self) -> NetworkConditionState {
        self.state
    }
}

/// Generation-scoped provider observation.
///
/// Observations carry the same identity/digest/fencing token as durable state,
/// but are evidence only. They do not authorize allocation, publication,
/// cleanup, or reuse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "NetworkObservationWire")]
pub struct NetworkObservation {
    version: NetworkResourceVersion,
    observed_phase: NetworkResourcePhase,
    provider_id: Option<NetworkProviderId>,
    conditions: Vec<NetworkCondition>,
}

impl NetworkObservation {
    /// Build a canonical observation with at most one value per condition kind.
    pub fn new(
        version: NetworkResourceVersion,
        observed_phase: NetworkResourcePhase,
        provider_id: Option<NetworkProviderId>,
        mut conditions: Vec<NetworkCondition>,
    ) -> Result<Self, NetworkObservationError> {
        conditions.sort_by_key(|condition| condition.kind);
        let distinct: BTreeSet<_> = conditions.iter().map(|condition| condition.kind).collect();
        if distinct.len() != conditions.len() {
            return Err(NetworkObservationError::DuplicateCondition);
        }
        Ok(Self {
            version,
            observed_phase,
            provider_id,
            conditions,
        })
    }

    /// Resource version this provider evidence describes.
    pub fn version(&self) -> &NetworkResourceVersion {
        &self.version
    }

    /// Provider-reported phase; never durable authority by itself.
    pub fn observed_phase(&self) -> NetworkResourcePhase {
        self.observed_phase
    }

    /// Provider registration that produced the evidence, when applicable.
    pub fn provider_id(&self) -> Option<&NetworkProviderId> {
        self.provider_id.as_ref()
    }

    /// Canonically ordered condition values.
    pub fn conditions(&self) -> &[NetworkCondition] {
        &self.conditions
    }
}

/// Validation failure while constructing one provider observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkObservationError {
    /// Each condition kind may occur at most once.
    DuplicateCondition,
}

impl Display for NetworkObservationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateCondition => {
                formatter.write_str("network observation contains a duplicate condition kind")
            }
        }
    }
}

impl StdError for NetworkObservationError {}

/// Rebuildable observed status for one desired resource version.
///
/// `desired_version` is only the comparison target copied from authority; this
/// type cannot change durable state. Projection deletion or loss merely removes
/// `latest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "NetworkStatusWire")]
pub struct NetworkStatus {
    desired_version: NetworkResourceVersion,
    latest: Option<NetworkObservation>,
}

impl NetworkStatus {
    /// Start an empty observed status projection for a desired resource.
    pub fn for_desired(desired_version: NetworkResourceVersion) -> Self {
        Self {
            desired_version,
            latest: None,
        }
    }

    /// Desired resource token against which observations are fenced.
    pub fn desired_version(&self) -> &NetworkResourceVersion {
        &self.desired_version
    }

    /// Current-generation provider evidence suitable for projection.
    ///
    /// Evidence retained from an older desired generation is mechanically
    /// hidden here so a caller cannot accidentally project it as current.
    pub fn current(&self) -> Option<&NetworkObservation> {
        self.latest.as_ref().filter(|observation| {
            observation.version().generation() == self.desired_version.generation()
                && observation.version().plan_digest() == self.desired_version.plan_digest()
                && observation.version().lease_epoch() == self.desired_version.lease_epoch()
        })
    }

    /// Latest accepted evidence retained for reconciliation diagnostics,
    /// including evidence from the immediately prior desired generation.
    pub fn latest_evidence(&self) -> Option<&NetworkObservation> {
        self.latest.as_ref()
    }

    /// Advance the desired comparison token without treating a prior
    /// observation as authority.
    pub fn advance_desired(
        &mut self,
        candidate: NetworkResourceVersion,
    ) -> Result<NetworkStatusUpdate, NetworkStatusError> {
        validate_identity(&self.desired_version, &candidate)?;
        match candidate
            .generation()
            .cmp(&self.desired_version.generation())
        {
            std::cmp::Ordering::Less => Err(NetworkStatusError::StaleDesiredGeneration {
                current: self.desired_version.generation(),
                candidate: candidate.generation(),
            }),
            std::cmp::Ordering::Equal
                if candidate.plan_digest() != self.desired_version.plan_digest() =>
            {
                Err(NetworkStatusError::PlanDigestConflict {
                    generation: candidate.generation(),
                })
            }
            std::cmp::Ordering::Equal
                if candidate.lease_epoch() != self.desired_version.lease_epoch() =>
            {
                Err(NetworkStatusError::LeaseEpochConflict {
                    expected: self.desired_version.lease_epoch(),
                    candidate: candidate.lease_epoch(),
                })
            }
            std::cmp::Ordering::Equal => Ok(NetworkStatusUpdate::Idempotent),
            std::cmp::Ordering::Greater => {
                if candidate.lease_epoch() < self.desired_version.lease_epoch() {
                    return Err(NetworkStatusError::StaleDesiredLeaseEpoch {
                        current: self.desired_version.lease_epoch(),
                        candidate: candidate.lease_epoch(),
                    });
                }
                self.desired_version = candidate;
                Ok(NetworkStatusUpdate::Updated)
            }
        }
    }

    /// Accept current-generation provider evidence or reject it without
    /// changing the latest projection.
    pub fn apply_observation(
        &mut self,
        observation: NetworkObservation,
    ) -> Result<NetworkStatusUpdate, NetworkStatusError> {
        validate_identity(&self.desired_version, observation.version())?;
        match observation
            .version()
            .generation()
            .cmp(&self.desired_version.generation())
        {
            std::cmp::Ordering::Less => {
                return Err(NetworkStatusError::StaleObservation {
                    desired: self.desired_version.generation(),
                    observed: observation.version().generation(),
                });
            }
            std::cmp::Ordering::Greater => {
                return Err(NetworkStatusError::FutureObservation {
                    desired: self.desired_version.generation(),
                    observed: observation.version().generation(),
                });
            }
            std::cmp::Ordering::Equal => {}
        }
        if observation.version().plan_digest() != self.desired_version.plan_digest() {
            return Err(NetworkStatusError::PlanDigestConflict {
                generation: self.desired_version.generation(),
            });
        }
        if observation.version().lease_epoch() != self.desired_version.lease_epoch() {
            return Err(NetworkStatusError::LeaseEpochConflict {
                expected: self.desired_version.lease_epoch(),
                candidate: observation.version().lease_epoch(),
            });
        }
        if self.latest.as_ref() == Some(&observation) {
            return Ok(NetworkStatusUpdate::Idempotent);
        }
        self.latest = Some(observation);
        Ok(NetworkStatusUpdate::Updated)
    }
}

fn validate_identity(
    expected: &NetworkResourceVersion,
    candidate: &NetworkResourceVersion,
) -> Result<(), NetworkStatusError> {
    if candidate.plan_id() != expected.plan_id() {
        return Err(NetworkStatusError::PlanIdentityMismatch);
    }
    if candidate.resource_id() != expected.resource_id() {
        return Err(NetworkStatusError::ResourceIdentityMismatch);
    }
    Ok(())
}

/// Whether observed state changed or was an exact replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkStatusUpdate {
    /// Latest observed status or desired comparison token changed.
    Updated,
    /// Exact desired or observed state was already present.
    Idempotent,
}

/// Rejection from desired/observed generation fencing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkStatusError {
    /// Observation or desired update belongs to another plan.
    PlanIdentityMismatch,
    /// Observation or desired update belongs to another resource.
    ResourceIdentityMismatch,
    /// Desired comparison state cannot move backward.
    StaleDesiredGeneration {
        current: NetworkResourceGeneration,
        candidate: NetworkResourceGeneration,
    },
    /// Provider evidence is older than desired state.
    StaleObservation {
        desired: NetworkResourceGeneration,
        observed: NetworkResourceGeneration,
    },
    /// Provider evidence is newer than the desired state known here.
    FutureObservation {
        desired: NetworkResourceGeneration,
        observed: NetworkResourceGeneration,
    },
    /// Equal generation carries different desired content.
    PlanDigestConflict {
        generation: NetworkResourceGeneration,
    },
    /// Observation or equal-generation desired state carries another lease
    /// epoch.
    LeaseEpochConflict {
        expected: NetworkLeaseEpoch,
        candidate: NetworkLeaseEpoch,
    },
    /// A newer desired generation cannot move the fencing epoch backward.
    StaleDesiredLeaseEpoch {
        current: NetworkLeaseEpoch,
        candidate: NetworkLeaseEpoch,
    },
    /// Retained older evidence cannot carry an epoch newer than desired state.
    RetainedLeaseEpochAhead {
        desired: NetworkLeaseEpoch,
        retained: NetworkLeaseEpoch,
    },
}

impl Display for NetworkStatusError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlanIdentityMismatch => {
                formatter.write_str("network status belongs to a different plan")
            }
            Self::ResourceIdentityMismatch => {
                formatter.write_str("network status belongs to a different resource")
            }
            Self::StaleDesiredGeneration { current, candidate } => write!(
                formatter,
                "desired status generation {} is older than current generation {}",
                candidate.as_u64(),
                current.as_u64()
            ),
            Self::StaleObservation { desired, observed } => write!(
                formatter,
                "observed network generation {} is older than desired generation {}",
                observed.as_u64(),
                desired.as_u64()
            ),
            Self::FutureObservation { desired, observed } => write!(
                formatter,
                "observed network generation {} is newer than desired generation {}",
                observed.as_u64(),
                desired.as_u64()
            ),
            Self::PlanDigestConflict { generation } => write!(
                formatter,
                "network observation generation {} has a conflicting plan digest",
                generation.as_u64()
            ),
            Self::LeaseEpochConflict {
                expected,
                candidate,
            } => write!(
                formatter,
                "network observation lease epoch {} does not match expected epoch {}",
                candidate.as_u64(),
                expected.as_u64()
            ),
            Self::StaleDesiredLeaseEpoch { current, candidate } => write!(
                formatter,
                "desired network lease epoch {} is older than current epoch {}",
                candidate.as_u64(),
                current.as_u64()
            ),
            Self::RetainedLeaseEpochAhead { desired, retained } => write!(
                formatter,
                "retained network lease epoch {} is newer than desired epoch {}",
                retained.as_u64(),
                desired.as_u64()
            ),
        }
    }
}

impl StdError for NetworkStatusError {}

#[derive(Deserialize)]
struct NetworkObservationWire {
    version: NetworkResourceVersion,
    observed_phase: NetworkResourcePhase,
    provider_id: Option<NetworkProviderId>,
    conditions: Vec<NetworkCondition>,
}

impl TryFrom<NetworkObservationWire> for NetworkObservation {
    type Error = NetworkObservationError;

    fn try_from(value: NetworkObservationWire) -> Result<Self, Self::Error> {
        Self::new(
            value.version,
            value.observed_phase,
            value.provider_id,
            value.conditions,
        )
    }
}

#[derive(Deserialize)]
struct NetworkStatusWire {
    desired_version: NetworkResourceVersion,
    latest: Option<NetworkObservation>,
}

impl TryFrom<NetworkStatusWire> for NetworkStatus {
    type Error = NetworkStatusError;

    fn try_from(value: NetworkStatusWire) -> Result<Self, Self::Error> {
        if let Some(latest) = value.latest.as_ref() {
            validate_identity(&value.desired_version, latest.version())?;
            match latest
                .version()
                .generation()
                .cmp(&value.desired_version.generation())
            {
                std::cmp::Ordering::Greater => {
                    return Err(NetworkStatusError::FutureObservation {
                        desired: value.desired_version.generation(),
                        observed: latest.version().generation(),
                    });
                }
                std::cmp::Ordering::Equal => {
                    if latest.version().plan_digest() != value.desired_version.plan_digest() {
                        return Err(NetworkStatusError::PlanDigestConflict {
                            generation: value.desired_version.generation(),
                        });
                    }
                    if latest.version().lease_epoch() != value.desired_version.lease_epoch() {
                        return Err(NetworkStatusError::LeaseEpochConflict {
                            expected: value.desired_version.lease_epoch(),
                            candidate: latest.version().lease_epoch(),
                        });
                    }
                }
                std::cmp::Ordering::Less => {
                    if latest.version().lease_epoch() > value.desired_version.lease_epoch() {
                        return Err(NetworkStatusError::RetainedLeaseEpochAhead {
                            desired: value.desired_version.lease_epoch(),
                            retained: latest.version().lease_epoch(),
                        });
                    }
                }
            }
        }
        Ok(Self {
            desired_version: value.desired_version,
            latest: value.latest,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        NetworkAttachmentId, NetworkPlan, NetworkPlanDigest, NetworkPlanId, NetworkResourceId,
    };

    fn plan_id(value: &str) -> NetworkPlanId {
        value.parse().expect("fixture plan id should parse")
    }

    fn resource(value: &str) -> NetworkResourceId {
        value
            .parse::<NetworkAttachmentId>()
            .expect("fixture attachment id should parse")
            .into()
    }

    fn plan(generation: u64, content: &[u8]) -> NetworkPlan {
        NetworkPlan::new(
            plan_id("netplan_01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            NetworkResourceGeneration::new(generation),
            NetworkPlanDigest::sha256(content),
        )
    }

    fn version(generation: u64, content: &[u8], epoch: u64) -> NetworkResourceVersion {
        NetworkResourceVersion::for_plan(
            &plan(generation, content),
            resource("netattachment_01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            NetworkLeaseEpoch::new(epoch),
        )
    }

    fn observation(
        version: NetworkResourceVersion,
        phase: NetworkResourcePhase,
        conditions: Vec<NetworkCondition>,
    ) -> NetworkObservation {
        NetworkObservation::new(version, phase, None, conditions)
            .expect("observation should validate")
    }

    #[test]
    fn stale_future_conflicting_and_wrong_identity_observations_fail_without_mutation() {
        let desired = version(7, b"desired", 11);
        let mut status = NetworkStatus::for_desired(desired.clone());
        let cases = [
            (
                observation(
                    version(6, b"desired", 11),
                    NetworkResourcePhase::Active,
                    vec![],
                ),
                NetworkStatusError::StaleObservation {
                    desired: NetworkResourceGeneration::new(7),
                    observed: NetworkResourceGeneration::new(6),
                },
            ),
            (
                observation(
                    version(8, b"desired", 11),
                    NetworkResourcePhase::Active,
                    vec![],
                ),
                NetworkStatusError::FutureObservation {
                    desired: NetworkResourceGeneration::new(7),
                    observed: NetworkResourceGeneration::new(8),
                },
            ),
            (
                observation(
                    version(7, b"different", 11),
                    NetworkResourcePhase::Active,
                    vec![],
                ),
                NetworkStatusError::PlanDigestConflict {
                    generation: NetworkResourceGeneration::new(7),
                },
            ),
            (
                observation(
                    version(7, b"desired", 12),
                    NetworkResourcePhase::Active,
                    vec![],
                ),
                NetworkStatusError::LeaseEpochConflict {
                    expected: NetworkLeaseEpoch::new(11),
                    candidate: NetworkLeaseEpoch::new(12),
                },
            ),
            (
                observation(
                    NetworkResourceVersion::for_plan(
                        &NetworkPlan::new(
                            plan_id("netplan_01ARZ3NDEKTSV4RRFFQ69G5FAW"),
                            NetworkResourceGeneration::new(7),
                            NetworkPlanDigest::sha256(b"desired"),
                        ),
                        desired.resource_id().clone(),
                        NetworkLeaseEpoch::new(11),
                    ),
                    NetworkResourcePhase::Active,
                    vec![],
                ),
                NetworkStatusError::PlanIdentityMismatch,
            ),
            (
                observation(
                    NetworkResourceVersion::for_plan(
                        &plan(7, b"desired"),
                        resource("netattachment_01ARZ3NDEKTSV4RRFFQ69G5FAW"),
                        NetworkLeaseEpoch::new(11),
                    ),
                    NetworkResourcePhase::Active,
                    vec![],
                ),
                NetworkStatusError::ResourceIdentityMismatch,
            ),
        ];

        for (candidate, expected) in cases {
            let before = status.clone();
            assert_eq!(status.apply_observation(candidate), Err(expected));
            assert_eq!(status, before);
        }
    }

    #[test]
    fn current_observation_is_idempotent_but_conditions_may_refresh() {
        let desired = version(7, b"desired", 11);
        let mut status = NetworkStatus::for_desired(desired.clone());
        let ready = observation(
            desired.clone(),
            NetworkResourcePhase::Active,
            vec![NetworkCondition::new(
                NetworkConditionKind::Ready,
                NetworkConditionState::True,
            )],
        );

        assert_eq!(
            status.apply_observation(ready.clone()),
            Ok(NetworkStatusUpdate::Updated)
        );
        assert_eq!(
            status.apply_observation(ready),
            Ok(NetworkStatusUpdate::Idempotent)
        );

        let degraded = observation(
            desired,
            NetworkResourcePhase::Active,
            vec![
                NetworkCondition::new(NetworkConditionKind::Degraded, NetworkConditionState::True),
                NetworkCondition::new(NetworkConditionKind::Ready, NetworkConditionState::False),
            ],
        );
        assert_eq!(
            status.apply_observation(degraded),
            Ok(NetworkStatusUpdate::Updated)
        );
        assert_eq!(
            status.current().expect("current observation").conditions(),
            [
                NetworkCondition::new(NetworkConditionKind::Ready, NetworkConditionState::False,),
                NetworkCondition::new(NetworkConditionKind::Degraded, NetworkConditionState::True,),
            ]
        );
    }

    #[test]
    fn desired_advance_retains_but_fences_old_observation() {
        let generation_7 = version(7, b"desired-7", 11);
        let generation_8 = version(8, b"desired-8", 12);
        let mut status = NetworkStatus::for_desired(generation_7.clone());
        status
            .apply_observation(observation(
                generation_7.clone(),
                NetworkResourcePhase::Active,
                vec![],
            ))
            .expect("generation 7 observation");

        assert_eq!(
            status.advance_desired(generation_8.clone()),
            Ok(NetworkStatusUpdate::Updated)
        );
        assert_eq!(
            status
                .latest_evidence()
                .expect("old evidence remains inspectable")
                .version()
                .generation(),
            NetworkResourceGeneration::new(7)
        );
        assert!(
            status.current().is_none(),
            "an older observation must not be exposed as current after desired advance"
        );
        assert!(matches!(
            status.apply_observation(observation(
                generation_7,
                NetworkResourcePhase::Active,
                vec![],
            )),
            Err(NetworkStatusError::StaleObservation { .. })
        ));
        assert_eq!(
            status.apply_observation(observation(
                generation_8,
                NetworkResourcePhase::Provisioning,
                vec![],
            )),
            Ok(NetworkStatusUpdate::Updated)
        );
    }

    #[test]
    fn desired_advance_rejects_epoch_regression_without_mutation() {
        let mut status = NetworkStatus::for_desired(version(7, b"desired-7", 11));
        let before = status.clone();

        assert_eq!(
            status.advance_desired(version(8, b"desired-8", 10)),
            Err(NetworkStatusError::StaleDesiredLeaseEpoch {
                current: NetworkLeaseEpoch::new(11),
                candidate: NetworkLeaseEpoch::new(10),
            })
        );
        assert_eq!(status, before);
    }

    #[test]
    fn duplicate_conditions_fail_and_wire_order_is_canonical() {
        let duplicate = NetworkObservation::new(
            version(7, b"desired", 11),
            NetworkResourcePhase::Active,
            None,
            vec![
                NetworkCondition::new(NetworkConditionKind::Ready, NetworkConditionState::True),
                NetworkCondition::new(NetworkConditionKind::Ready, NetworkConditionState::False),
            ],
        );
        assert_eq!(duplicate, Err(NetworkObservationError::DuplicateCondition));

        let canonical = observation(
            version(7, b"desired", 11),
            NetworkResourcePhase::Active,
            vec![
                NetworkCondition::new(NetworkConditionKind::Degraded, NetworkConditionState::False),
                NetworkCondition::new(NetworkConditionKind::Ready, NetworkConditionState::True),
            ],
        );
        let json = serde_json::to_string(&canonical).expect("observation should serialize");
        let ready_offset = json.find(r#""kind":"ready""#).expect("ready condition");
        let degraded_offset = json
            .find(r#""kind":"degraded""#)
            .expect("degraded condition");
        assert!(
            ready_offset < degraded_offset,
            "condition wire order must be canonical: {json}"
        );
    }

    #[test]
    fn desired_durable_and_observed_types_remain_structurally_distinct() {
        assert_ne!(
            std::any::TypeId::of::<NetworkPlan>(),
            std::any::TypeId::of::<crate::DurableNetworkResourceState>()
        );
        assert_ne!(
            std::any::TypeId::of::<crate::DurableNetworkResourceState>(),
            std::any::TypeId::of::<NetworkStatus>()
        );
    }

    #[test]
    fn observed_status_round_trip_cannot_promote_evidence_to_authority() {
        let desired = version(7, b"desired", 11);
        let observation = observation(
            desired.clone(),
            NetworkResourcePhase::Active,
            vec![NetworkCondition::new(
                NetworkConditionKind::Ready,
                NetworkConditionState::True,
            )],
        );
        let mut status = NetworkStatus::for_desired(desired);
        status
            .apply_observation(observation)
            .expect("current observation should be accepted");

        let json = serde_json::to_string(&status).expect("status should serialize");
        let decoded: NetworkStatus =
            serde_json::from_str(&json).expect("status should deserialize");

        assert_eq!(decoded, status);
        assert_eq!(
            decoded
                .current()
                .expect("current observation")
                .observed_phase(),
            NetworkResourcePhase::Active
        );
        assert!(
            !json.contains("provider_handle"),
            "observed status wire must not grow durable provider authority"
        );
    }

    #[test]
    fn observation_wire_rejects_duplicates_and_canonicalizes_order() {
        let desired = version(7, b"desired", 11);
        let duplicate = serde_json::json!({
            "version": desired,
            "observed_phase": "active",
            "provider_id": null,
            "conditions": [
                {"kind": "ready", "state": "true"},
                {"kind": "ready", "state": "false"}
            ]
        });
        assert!(
            serde_json::from_value::<NetworkObservation>(duplicate)
                .expect_err("duplicate conditions must fail on the wire")
                .to_string()
                .contains("duplicate condition kind")
        );

        let unsorted = serde_json::json!({
            "version": version(7, b"desired", 11),
            "observed_phase": "active",
            "provider_id": null,
            "conditions": [
                {"kind": "degraded", "state": "false"},
                {"kind": "ready", "state": "true"}
            ]
        });
        let decoded: NetworkObservation =
            serde_json::from_value(unsorted).expect("valid conditions should canonicalize");
        assert_eq!(
            decoded.conditions(),
            [
                NetworkCondition::new(NetworkConditionKind::Ready, NetworkConditionState::True),
                NetworkCondition::new(NetworkConditionKind::Degraded, NetworkConditionState::False),
            ]
        );
    }

    #[test]
    fn status_wire_rejects_cross_identity_future_and_epoch_inconsistent_evidence() {
        let desired = version(7, b"desired-7", 11);
        let cases = [
            (
                NetworkStatusWire {
                    desired_version: desired.clone(),
                    latest: Some(observation(
                        NetworkResourceVersion::for_plan(
                            &plan(7, b"desired-7"),
                            resource("netattachment_01ARZ3NDEKTSV4RRFFQ69G5FAW"),
                            NetworkLeaseEpoch::new(11),
                        ),
                        NetworkResourcePhase::Active,
                        vec![],
                    )),
                },
                NetworkStatusError::ResourceIdentityMismatch,
            ),
            (
                NetworkStatusWire {
                    desired_version: desired.clone(),
                    latest: Some(observation(
                        version(8, b"desired-8", 12),
                        NetworkResourcePhase::Provisioning,
                        vec![],
                    )),
                },
                NetworkStatusError::FutureObservation {
                    desired: NetworkResourceGeneration::new(7),
                    observed: NetworkResourceGeneration::new(8),
                },
            ),
            (
                NetworkStatusWire {
                    desired_version: desired,
                    latest: Some(observation(
                        version(6, b"desired-6", 12),
                        NetworkResourcePhase::Active,
                        vec![],
                    )),
                },
                NetworkStatusError::RetainedLeaseEpochAhead {
                    desired: NetworkLeaseEpoch::new(11),
                    retained: NetworkLeaseEpoch::new(12),
                },
            ),
        ];

        for (wire, expected) in cases {
            let json = serde_json::to_value(&NetworkStatus {
                desired_version: wire.desired_version.clone(),
                latest: wire.latest.clone(),
            })
            .expect("wire fixture should serialize");
            let error = serde_json::from_value::<NetworkStatus>(json)
                .expect_err("unreachable status wire must fail");
            assert!(
                error.to_string().contains(&expected.to_string()),
                "expected {expected}, got {error}"
            );
        }
    }
}
