//! Provider-neutral readiness requirements and exact satisfaction evidence.
//!
//! These values compose desired intent, durable lease identity, and observed
//! conditions without performing provider effects. In particular, neither a
//! socket address nor an opaque provider handle is copied into readiness
//! identity.

use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::{
    NetworkCondition, NetworkConditionKind, NetworkConditionState, NetworkPlan, NetworkProviderId,
    NetworkResourceGeneration, NetworkResourceId, NetworkResourceVersion, PortLeaseId,
    PortLeaseLifetime, PortLeasePhase, PortLeaseRecord,
};

/// One provider-neutral condition required by a desired network plan.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkReadinessRequirement {
    resource_id: NetworkResourceId,
    provider_id: NetworkProviderId,
    condition_kind: NetworkConditionKind,
}

impl NetworkReadinessRequirement {
    /// Construct one exact desired readiness requirement.
    pub fn new(
        resource_id: NetworkResourceId,
        provider_id: NetworkProviderId,
        condition_kind: NetworkConditionKind,
    ) -> Self {
        Self {
            resource_id,
            provider_id,
            condition_kind,
        }
    }

    /// Stable resource whose condition is required.
    pub fn resource_id(&self) -> &NetworkResourceId {
        &self.resource_id
    }

    /// Exact provider registration expected to produce evidence.
    pub fn provider_id(&self) -> &NetworkProviderId {
        &self.provider_id
    }

    /// Condition that must be observed as true.
    pub const fn condition_kind(&self) -> NetworkConditionKind {
        self.condition_kind
    }
}

/// Invalid desired readiness requirement set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkReadinessRequirementError {
    /// The same exact requirement appeared more than once.
    Duplicate {
        requirement: NetworkReadinessRequirement,
    },
}

impl Display for NetworkReadinessRequirementError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Duplicate { requirement } => write!(
                formatter,
                "network plan contains duplicate readiness requirement for {:?} from {}",
                requirement.resource_id, requirement.provider_id
            ),
        }
    }
}

impl StdError for NetworkReadinessRequirementError {}

pub(crate) fn canonicalize_requirements(
    requirements: impl IntoIterator<Item = NetworkReadinessRequirement>,
) -> Result<Vec<NetworkReadinessRequirement>, NetworkReadinessRequirementError> {
    let mut requirements: Vec<_> = requirements.into_iter().collect();
    requirements.sort();
    if let Some(duplicate) = requirements
        .windows(2)
        .find(|pair| pair[0] == pair[1])
        .map(|pair| pair[0].clone())
    {
        return Err(NetworkReadinessRequirementError::Duplicate {
            requirement: duplicate,
        });
    }
    Ok(requirements)
}

/// Exact durable lease fence backing one desired readiness requirement.
///
/// Construction authenticates the current Active port lease and its provider.
/// The resulting value retains only portable identity and fencing data: no IP
/// address and no opaque provider handle can become workload identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "NetworkReadinessDependencyWire")]
pub struct NetworkReadinessDependency {
    requirement: NetworkReadinessRequirement,
    version: NetworkResourceVersion,
    port_lease_id: PortLeaseId,
    lifetime: PortLeaseLifetime,
}

impl NetworkReadinessDependency {
    /// Authenticate an Active port lease as the durable backing dependency.
    pub fn new(
        plan: &NetworkPlan,
        requirement: NetworkReadinessRequirement,
        version: NetworkResourceVersion,
        lease: &PortLeaseRecord,
        lifetime: PortLeaseLifetime,
    ) -> Result<Self, NetworkReadinessDependencyError> {
        if !plan.readiness_requirements().contains(&requirement) {
            return Err(NetworkReadinessDependencyError::RequirementNotInPlan);
        }
        validate_version(plan, &requirement, &version)?;

        let request = lease.request();
        if lease.phase() != PortLeasePhase::Active {
            return Err(NetworkReadinessDependencyError::LeaseNotActive {
                phase: lease.phase(),
            });
        }
        if request.owner_id() != requirement.resource_id() {
            return Err(NetworkReadinessDependencyError::LeaseOwnerMismatch);
        }
        if request.generation() != version.generation() {
            return Err(NetworkReadinessDependencyError::LeaseGenerationMismatch {
                expected: version.generation(),
                candidate: request.generation(),
            });
        }
        if request.lease_epoch() != version.lease_epoch() {
            return Err(NetworkReadinessDependencyError::LeaseEpochMismatch);
        }
        let binding = lease
            .binding()
            .ok_or(NetworkReadinessDependencyError::MissingBinding)?;
        if binding.provider_handle().provider_id() != requirement.provider_id() {
            return Err(NetworkReadinessDependencyError::ProviderMismatch);
        }
        let active_lifetime = lease
            .active_lifetime()
            .ok_or(NetworkReadinessDependencyError::MissingLifetime)?;
        if active_lifetime != lifetime {
            return Err(NetworkReadinessDependencyError::LifetimeMismatch);
        }

        Ok(Self {
            requirement,
            version,
            port_lease_id: request.lease_id().clone(),
            lifetime,
        })
    }

    /// Desired condition backed by this dependency.
    pub fn requirement(&self) -> &NetworkReadinessRequirement {
        &self.requirement
    }

    /// Desired plan/resource generation and allocation epoch.
    pub fn version(&self) -> &NetworkResourceVersion {
        &self.version
    }

    /// Stable host-global lease identity.
    pub fn port_lease_id(&self) -> &PortLeaseId {
        &self.port_lease_id
    }

    /// Exact process/provider lifetime fenced by the lease.
    pub const fn lifetime(&self) -> PortLeaseLifetime {
        self.lifetime
    }
}

fn validate_version(
    plan: &NetworkPlan,
    requirement: &NetworkReadinessRequirement,
    version: &NetworkResourceVersion,
) -> Result<(), NetworkReadinessDependencyError> {
    if version.plan_id() != plan.plan_id() {
        return Err(NetworkReadinessDependencyError::PlanIdentityMismatch);
    }
    if version.resource_id() != requirement.resource_id() {
        return Err(NetworkReadinessDependencyError::ResourceIdentityMismatch);
    }
    if version.generation() != plan.generation() {
        return Err(NetworkReadinessDependencyError::PlanGenerationMismatch {
            expected: plan.generation(),
            candidate: version.generation(),
        });
    }
    if version.plan_digest() != plan.digest() {
        return Err(NetworkReadinessDependencyError::PlanDigestMismatch);
    }
    Ok(())
}

/// Invalid durable readiness dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkReadinessDependencyError {
    /// The desired plan does not contain this exact provider requirement.
    RequirementNotInPlan,
    /// The resource version belongs to another plan.
    PlanIdentityMismatch,
    /// The resource version does not name the required resource.
    ResourceIdentityMismatch,
    /// The resource version does not carry the desired plan generation.
    PlanGenerationMismatch {
        expected: NetworkResourceGeneration,
        candidate: NetworkResourceGeneration,
    },
    /// The resource version does not carry the complete desired plan digest.
    PlanDigestMismatch,
    /// The port lease is not currently Active authority.
    LeaseNotActive { phase: PortLeasePhase },
    /// The port lease belongs to another resource.
    LeaseOwnerMismatch,
    /// The port lease generation does not match the resource version.
    LeaseGenerationMismatch {
        expected: NetworkResourceGeneration,
        candidate: NetworkResourceGeneration,
    },
    /// The port lease epoch does not match the resource version.
    LeaseEpochMismatch,
    /// The Active lease has no adopted provider binding.
    MissingBinding,
    /// The adopted binding belongs to another provider registration.
    ProviderMismatch,
    /// The Active lease has no durable effect lifetime.
    MissingLifetime,
    /// The supplied lifetime is not the lease's exact active lifetime.
    LifetimeMismatch,
}

impl Display for NetworkReadinessDependencyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequirementNotInPlan => formatter
                .write_str("readiness requirement is not present in the desired network plan"),
            Self::PlanIdentityMismatch => {
                formatter.write_str("readiness dependency belongs to another network plan")
            }
            Self::ResourceIdentityMismatch => formatter
                .write_str("readiness dependency does not name the required network resource"),
            Self::PlanGenerationMismatch {
                expected,
                candidate,
            } => write!(
                formatter,
                "readiness dependency generation {} does not match desired generation {}",
                candidate.as_u64(),
                expected.as_u64()
            ),
            Self::PlanDigestMismatch => {
                formatter.write_str("readiness dependency carries a different plan digest")
            }
            Self::LeaseNotActive { phase } => {
                write!(formatter, "readiness port lease is not Active: {phase:?}")
            }
            Self::LeaseOwnerMismatch => {
                formatter.write_str("readiness port lease belongs to another resource")
            }
            Self::LeaseGenerationMismatch {
                expected,
                candidate,
            } => write!(
                formatter,
                "readiness port lease generation {} does not match resource generation {}",
                candidate.as_u64(),
                expected.as_u64()
            ),
            Self::LeaseEpochMismatch => {
                formatter.write_str("readiness port lease carries a different lease epoch")
            }
            Self::MissingBinding => {
                formatter.write_str("Active readiness port lease has no provider binding")
            }
            Self::ProviderMismatch => {
                formatter.write_str("readiness port lease belongs to another provider")
            }
            Self::MissingLifetime => {
                formatter.write_str("Active readiness port lease has no effect lifetime")
            }
            Self::LifetimeMismatch => {
                formatter.write_str("readiness port lease lifetime is not current")
            }
        }
    }
}

impl StdError for NetworkReadinessDependencyError {}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NetworkReadinessDependencyWire {
    requirement: NetworkReadinessRequirement,
    version: NetworkResourceVersion,
    port_lease_id: PortLeaseId,
    lifetime: PortLeaseLifetime,
}

impl TryFrom<NetworkReadinessDependencyWire> for NetworkReadinessDependency {
    type Error = NetworkReadinessDependencyError;

    fn try_from(wire: NetworkReadinessDependencyWire) -> Result<Self, Self::Error> {
        if wire.version.resource_id() != wire.requirement.resource_id() {
            return Err(NetworkReadinessDependencyError::ResourceIdentityMismatch);
        }
        Ok(Self {
            requirement: wire.requirement,
            version: wire.version,
            port_lease_id: wire.port_lease_id,
            lifetime: wire.lifetime,
        })
    }
}

/// One observed condition tied to an exact durable readiness dependency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "NetworkReadinessEvidenceWire")]
pub struct NetworkReadinessEvidence {
    dependency: NetworkReadinessDependency,
    condition: NetworkCondition,
}

impl NetworkReadinessEvidence {
    /// Construct evidence only when its condition kind matches desired intent.
    pub fn new(
        dependency: NetworkReadinessDependency,
        condition: NetworkCondition,
    ) -> Result<Self, NetworkReadinessEvidenceError> {
        if condition.kind() != dependency.requirement.condition_kind() {
            return Err(NetworkReadinessEvidenceError::ConditionKindMismatch);
        }
        Ok(Self {
            dependency,
            condition,
        })
    }

    /// Exact durable dependency observed by this evidence.
    pub fn dependency(&self) -> &NetworkReadinessDependency {
        &self.dependency
    }

    /// Honest tri-state observation.
    pub const fn condition(&self) -> NetworkCondition {
        self.condition
    }
}

/// Invalid observed readiness evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkReadinessEvidenceError {
    /// The observed condition is not the condition desired by the dependency.
    ConditionKindMismatch,
}

impl Display for NetworkReadinessEvidenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("readiness evidence condition kind does not match its dependency")
    }
}

impl StdError for NetworkReadinessEvidenceError {}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NetworkReadinessEvidenceWire {
    dependency: NetworkReadinessDependency,
    condition: NetworkCondition,
}

impl TryFrom<NetworkReadinessEvidenceWire> for NetworkReadinessEvidence {
    type Error = NetworkReadinessEvidenceError;

    fn try_from(wire: NetworkReadinessEvidenceWire) -> Result<Self, Self::Error> {
        Self::new(wire.dependency, wire.condition)
    }
}

/// Why current durable/observed values do not satisfy desired readiness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkReadinessEvaluationError {
    /// A dependency does not correspond to any exact desired requirement.
    ForeignDependency {
        requirement: NetworkReadinessRequirement,
    },
    /// Two durable dependency values claim one desired requirement.
    DuplicateDependency {
        requirement: NetworkReadinessRequirement,
    },
    /// No durable dependency exists for a desired requirement.
    MissingDependency {
        requirement: NetworkReadinessRequirement,
    },
    /// A durable dependency belongs to another plan identity.
    PlanIdentityMismatch,
    /// A durable dependency names another resource.
    ResourceIdentityMismatch,
    /// The dependency generation predates desired state.
    StaleGeneration {
        desired: NetworkResourceGeneration,
        candidate: NetworkResourceGeneration,
    },
    /// The dependency generation is ahead of desired state.
    FutureGeneration {
        desired: NetworkResourceGeneration,
        candidate: NetworkResourceGeneration,
    },
    /// An equal-generation dependency carries a different desired digest.
    PlanDigestMismatch,
    /// Evidence carries a different allocation epoch.
    LeaseEpochMismatch,
    /// Evidence carries a different stable port lease.
    PortLeaseMismatch,
    /// Evidence names a different provider requirement.
    ProviderMismatch,
    /// Evidence came from a different process/provider lifetime.
    LifetimeMismatch,
    /// Evidence does not correspond to any exact current dependency.
    ForeignEvidence {
        requirement: NetworkReadinessRequirement,
    },
    /// The same observed evidence value appeared more than once.
    DuplicateEvidence {
        requirement: NetworkReadinessRequirement,
    },
    /// Multiple observations disagree for one exact dependency.
    ConflictingEvidence {
        requirement: NetworkReadinessRequirement,
    },
    /// No observation exists for a desired requirement.
    MissingEvidence {
        requirement: NetworkReadinessRequirement,
    },
    /// The exact observation is honestly false or unknown.
    ConditionUnsatisfied {
        requirement: NetworkReadinessRequirement,
        state: NetworkConditionState,
    },
}

impl Display for NetworkReadinessEvaluationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignDependency { requirement } => write!(
                formatter,
                "foreign readiness dependency for {:?}",
                requirement.resource_id()
            ),
            Self::DuplicateDependency { requirement } => write!(
                formatter,
                "duplicate readiness dependency for {:?}",
                requirement.resource_id()
            ),
            Self::MissingDependency { requirement } => write!(
                formatter,
                "missing readiness dependency for {:?}",
                requirement.resource_id()
            ),
            Self::PlanIdentityMismatch => {
                formatter.write_str("readiness value belongs to another network plan")
            }
            Self::ResourceIdentityMismatch => {
                formatter.write_str("readiness value belongs to another network resource")
            }
            Self::StaleGeneration { desired, candidate } => write!(
                formatter,
                "stale readiness generation {}; desired generation is {}",
                candidate.as_u64(),
                desired.as_u64()
            ),
            Self::FutureGeneration { desired, candidate } => write!(
                formatter,
                "future readiness generation {}; desired generation is {}",
                candidate.as_u64(),
                desired.as_u64()
            ),
            Self::PlanDigestMismatch => {
                formatter.write_str("readiness value carries a different plan digest")
            }
            Self::LeaseEpochMismatch => {
                formatter.write_str("readiness evidence carries a different lease epoch")
            }
            Self::PortLeaseMismatch => {
                formatter.write_str("readiness evidence carries a different port lease")
            }
            Self::ProviderMismatch => {
                formatter.write_str("readiness evidence names a different provider")
            }
            Self::LifetimeMismatch => {
                formatter.write_str("readiness evidence carries a different effect lifetime")
            }
            Self::ForeignEvidence { requirement } => write!(
                formatter,
                "foreign readiness evidence for {:?}",
                requirement.resource_id()
            ),
            Self::DuplicateEvidence { requirement } => write!(
                formatter,
                "duplicate readiness evidence for {:?}",
                requirement.resource_id()
            ),
            Self::ConflictingEvidence { requirement } => write!(
                formatter,
                "conflicting readiness evidence for {:?}",
                requirement.resource_id()
            ),
            Self::MissingEvidence { requirement } => write!(
                formatter,
                "missing readiness evidence for {:?}",
                requirement.resource_id()
            ),
            Self::ConditionUnsatisfied { requirement, state } => write!(
                formatter,
                "readiness condition {:?} for {:?} is {state:?}",
                requirement.condition_kind(),
                requirement.resource_id()
            ),
        }
    }
}

impl StdError for NetworkReadinessEvaluationError {}

impl NetworkPlan {
    /// Evaluate exact durable dependencies and observations without effects.
    pub fn evaluate_readiness(
        &self,
        dependencies: &[NetworkReadinessDependency],
        evidence: &[NetworkReadinessEvidence],
    ) -> Result<(), NetworkReadinessEvaluationError> {
        for dependency in dependencies {
            let Some(requirement) = self
                .readiness_requirements()
                .iter()
                .find(|requirement| same_requirement_key(requirement, dependency.requirement()))
            else {
                if self.readiness_requirements().iter().any(|requirement| {
                    same_requirement_shape(requirement, dependency.requirement())
                }) {
                    return Err(NetworkReadinessEvaluationError::ProviderMismatch);
                }
                return Err(NetworkReadinessEvaluationError::ForeignDependency {
                    requirement: dependency.requirement().clone(),
                });
            };
            debug_assert_eq!(requirement, dependency.requirement());
            validate_current_version(self, dependency.version())?;
            if dependencies
                .iter()
                .filter(|candidate| {
                    same_requirement_key(candidate.requirement(), dependency.requirement())
                })
                .count()
                != 1
            {
                return Err(NetworkReadinessEvaluationError::DuplicateDependency {
                    requirement: dependency.requirement().clone(),
                });
            }
        }

        for observation in evidence {
            if self.readiness_requirements().iter().any(|requirement| {
                same_requirement_key(requirement, observation.dependency().requirement())
            }) {
                continue;
            }
            if self.readiness_requirements().iter().any(|requirement| {
                same_requirement_shape(requirement, observation.dependency().requirement())
            }) {
                return Err(NetworkReadinessEvaluationError::ProviderMismatch);
            }
            return Err(NetworkReadinessEvaluationError::ForeignEvidence {
                requirement: observation.dependency().requirement().clone(),
            });
        }

        for requirement in self.readiness_requirements() {
            let Some(dependency) = dependencies
                .iter()
                .find(|dependency| dependency.requirement() == requirement)
            else {
                return Err(NetworkReadinessEvaluationError::MissingDependency {
                    requirement: requirement.clone(),
                });
            };

            let matching: Vec<_> = evidence
                .iter()
                .filter(|observation| {
                    same_requirement_key(observation.dependency().requirement(), requirement)
                })
                .collect();
            if matching.is_empty() {
                return Err(NetworkReadinessEvaluationError::MissingEvidence {
                    requirement: requirement.clone(),
                });
            }
            if matching.len() > 1 {
                let error = if matching.windows(2).all(|pair| pair[0] == pair[1]) {
                    NetworkReadinessEvaluationError::DuplicateEvidence {
                        requirement: requirement.clone(),
                    }
                } else {
                    NetworkReadinessEvaluationError::ConflictingEvidence {
                        requirement: requirement.clone(),
                    }
                };
                return Err(error);
            }
            let observation = matching[0];
            compare_dependencies(dependency, observation.dependency())?;
            if observation.condition().state() != NetworkConditionState::True {
                return Err(NetworkReadinessEvaluationError::ConditionUnsatisfied {
                    requirement: requirement.clone(),
                    state: observation.condition().state(),
                });
            }
        }

        Ok(())
    }
}

fn same_requirement_key(
    left: &NetworkReadinessRequirement,
    right: &NetworkReadinessRequirement,
) -> bool {
    left == right
}

fn same_requirement_shape(
    left: &NetworkReadinessRequirement,
    right: &NetworkReadinessRequirement,
) -> bool {
    left.resource_id() == right.resource_id() && left.condition_kind() == right.condition_kind()
}

fn validate_current_version(
    plan: &NetworkPlan,
    version: &NetworkResourceVersion,
) -> Result<(), NetworkReadinessEvaluationError> {
    if version.plan_id() != plan.plan_id() {
        return Err(NetworkReadinessEvaluationError::PlanIdentityMismatch);
    }
    match version.generation().cmp(&plan.generation()) {
        std::cmp::Ordering::Less => {
            return Err(NetworkReadinessEvaluationError::StaleGeneration {
                desired: plan.generation(),
                candidate: version.generation(),
            });
        }
        std::cmp::Ordering::Greater => {
            return Err(NetworkReadinessEvaluationError::FutureGeneration {
                desired: plan.generation(),
                candidate: version.generation(),
            });
        }
        std::cmp::Ordering::Equal => {}
    }
    if version.plan_digest() != plan.digest() {
        return Err(NetworkReadinessEvaluationError::PlanDigestMismatch);
    }
    Ok(())
}

fn compare_dependencies(
    expected: &NetworkReadinessDependency,
    candidate: &NetworkReadinessDependency,
) -> Result<(), NetworkReadinessEvaluationError> {
    if candidate.version().plan_id() != expected.version().plan_id() {
        return Err(NetworkReadinessEvaluationError::PlanIdentityMismatch);
    }
    if candidate.version().resource_id() != expected.version().resource_id() {
        return Err(NetworkReadinessEvaluationError::ResourceIdentityMismatch);
    }
    match candidate
        .version()
        .generation()
        .cmp(&expected.version().generation())
    {
        std::cmp::Ordering::Less => {
            return Err(NetworkReadinessEvaluationError::StaleGeneration {
                desired: expected.version().generation(),
                candidate: candidate.version().generation(),
            });
        }
        std::cmp::Ordering::Greater => {
            return Err(NetworkReadinessEvaluationError::FutureGeneration {
                desired: expected.version().generation(),
                candidate: candidate.version().generation(),
            });
        }
        std::cmp::Ordering::Equal => {}
    }
    if candidate.version().plan_digest() != expected.version().plan_digest() {
        return Err(NetworkReadinessEvaluationError::PlanDigestMismatch);
    }
    if candidate.version().lease_epoch() != expected.version().lease_epoch() {
        return Err(NetworkReadinessEvaluationError::LeaseEpochMismatch);
    }
    if candidate.port_lease_id() != expected.port_lease_id() {
        return Err(NetworkReadinessEvaluationError::PortLeaseMismatch);
    }
    if candidate.requirement().provider_id() != expected.requirement().provider_id() {
        return Err(NetworkReadinessEvaluationError::ProviderMismatch);
    }
    if candidate.lifetime() != expected.lifetime() {
        return Err(NetworkReadinessEvaluationError::LifetimeMismatch);
    }
    Ok(())
}
