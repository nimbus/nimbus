//! Portable workload-provision source, attempt, result, and disposition values.

use nimbus_network::{NetworkCapabilitySelectionEvidence, NetworkPlanDigest, NetworkProviderId};

use super::*;

mod dispatch;

pub use dispatch::{
    WorkloadProvisionAbsenceEvidence, WorkloadProvisionCommandId, WorkloadProvisionCommandMode,
    WorkloadProvisionDispatchAuthorization, WorkloadProvisionDispatchClaim,
    WorkloadProvisionDispatchEpoch, WorkloadProvisionInspectionResult,
    WorkloadProvisionProviderTarget,
};

define_decimal_counter!(
    WorkloadProvisionSourceGeneration,
    "workload provision source generation must be canonical unsigned decimal text"
);
define_sha256_digest!(
    WorkloadProvisionSourceDigest,
    b"nimbus.workloads.provision.source.digest.v1\0",
    "workload provision source digest must be 64 lowercase hexadecimal characters"
);
define_derived_id!(WorkloadProvisionAttemptId, "wpa");
define_derived_id!(WorkloadExecutionProviderId, "wep");

impl WorkloadExecutionProviderId {
    /// Derive a stable execution-provider identity from its registration key.
    pub fn for_registration_key(registration_key: &str) -> Self {
        Self(derive_id(
            Self::PREFIX,
            b"nimbus.workloads.execution.provider.id.v1",
            &[registration_key],
        ))
    }
}

/// Closed source family authenticated for one provision intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadProvisionSourceKind {
    /// One independently addressed sandbox.
    StandaloneSandbox,
    /// One service definition backed by a sandbox.
    SandboxBackedService,
}

/// Stable logical source identity, distinct from a deployment generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadProvisionSourceIdentity {
    kind: WorkloadProvisionSourceKind,
    stable_name: String,
    profile: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WorkloadProvisionSourceIdentityWire {
    kind: WorkloadProvisionSourceKind,
    stable_name: String,
    profile: Option<String>,
}

impl<'de> Deserialize<'de> for WorkloadProvisionSourceIdentity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WorkloadProvisionSourceIdentityWire::deserialize(deserializer)?;
        Self::new(wire.kind, wire.stable_name, wire.profile).map_err(serde::de::Error::custom)
    }
}

impl WorkloadProvisionSourceIdentity {
    /// Construct a strict identity for a standalone sandbox source.
    pub fn standalone_sandbox(
        stable_resource_id: impl Into<String>,
        profile: impl Into<String>,
    ) -> Result<Self, WorkloadSagaError> {
        Self::new(
            WorkloadProvisionSourceKind::StandaloneSandbox,
            stable_resource_id.into(),
            Some(profile.into()),
        )
    }

    /// Construct a strict identity for a sandbox-backed service source.
    pub fn sandbox_backed_service(
        service_name: impl Into<String>,
    ) -> Result<Self, WorkloadSagaError> {
        Self::new(
            WorkloadProvisionSourceKind::SandboxBackedService,
            service_name.into(),
            None,
        )
    }

    fn new(
        kind: WorkloadProvisionSourceKind,
        stable_name: String,
        profile: Option<String>,
    ) -> Result<Self, WorkloadSagaError> {
        validate_source_text(&stable_name, "provision source stable name")?;
        match (kind, profile.as_deref()) {
            (WorkloadProvisionSourceKind::StandaloneSandbox, Some(profile)) => {
                validate_source_text(profile, "provision source profile")?;
            }
            (WorkloadProvisionSourceKind::StandaloneSandbox, None) => {
                return Err(WorkloadSagaError::InvalidIntent(
                    "standalone sandbox source requires a profile",
                ));
            }
            (WorkloadProvisionSourceKind::SandboxBackedService, Some(_)) => {
                return Err(WorkloadSagaError::InvalidIntent(
                    "sandbox-backed service source cannot carry a standalone profile",
                ));
            }
            (WorkloadProvisionSourceKind::SandboxBackedService, None) => {}
        }
        Ok(Self {
            kind,
            stable_name,
            profile,
        })
    }

    /// Closed source family.
    pub const fn kind(&self) -> WorkloadProvisionSourceKind {
        self.kind
    }

    /// Stable source name within its tenant and family.
    pub fn stable_name(&self) -> &str {
        &self.stable_name
    }

    /// Standalone sandbox profile, absent for service sources.
    pub fn profile(&self) -> Option<&str> {
        self.profile.as_deref()
    }
}

fn validate_source_text(value: &str, label: &'static str) -> Result<(), WorkloadSagaError> {
    if value.is_empty()
        || value.len() > 256
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(WorkloadSagaError::InvalidIntent(label));
    }
    Ok(())
}

/// Non-empty source-owned resource version, independent of workload generation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct WorkloadProvisionSourceResourceVersion(String);

impl WorkloadProvisionSourceResourceVersion {
    /// Construct a bounded source-owned version value.
    pub fn new(value: impl Into<String>) -> Result<Self, WorkloadSagaError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 512
            || value.trim() != value
            || value.chars().any(char::is_control)
        {
            return Err(WorkloadSagaError::InvalidIntent(
                "provision source resource version must be 1-512 non-control characters",
            ));
        }
        Ok(Self(value))
    }

    /// Source-owned resource version text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for WorkloadProvisionSourceResourceVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkloadProvisionSourceDigestPayload<'a> {
    source_identity: &'a WorkloadProvisionSourceIdentity,
    source_generation: WorkloadProvisionSourceGeneration,
    resource_version: &'a WorkloadProvisionSourceResourceVersion,
    executable_content_digest: WorkloadExecutableContentDigest,
    attachment_provider_id: &'a NetworkProviderId,
    execution_provider_id: &'a WorkloadExecutionProviderId,
}

/// Durable authenticated snapshot of the executable source owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkloadProvisionSourceEvidence {
    /// One standalone sandbox resource and profile.
    StandaloneSandbox {
        source_identity: WorkloadProvisionSourceIdentity,
        source_generation: WorkloadProvisionSourceGeneration,
        resource_version: WorkloadProvisionSourceResourceVersion,
        source_digest: WorkloadProvisionSourceDigest,
        attachment_provider_id: NetworkProviderId,
        execution_provider_id: WorkloadExecutionProviderId,
    },
    /// One service definition whose executable source is a sandbox.
    SandboxBackedService {
        source_identity: WorkloadProvisionSourceIdentity,
        source_generation: WorkloadProvisionSourceGeneration,
        resource_version: WorkloadProvisionSourceResourceVersion,
        source_digest: WorkloadProvisionSourceDigest,
        attachment_provider_id: NetworkProviderId,
        execution_provider_id: WorkloadExecutionProviderId,
    },
}

impl WorkloadProvisionSourceEvidence {
    /// Authenticate a standalone sandbox source snapshot.
    pub fn standalone_sandbox(
        source_identity: WorkloadProvisionSourceIdentity,
        source_generation: WorkloadProvisionSourceGeneration,
        resource_version: WorkloadProvisionSourceResourceVersion,
        executable_content_digest: WorkloadExecutableContentDigest,
        attachment_provider_id: NetworkProviderId,
        execution_provider_id: WorkloadExecutionProviderId,
    ) -> Result<Self, WorkloadSagaError> {
        if source_identity.kind() != WorkloadProvisionSourceKind::StandaloneSandbox {
            return Err(WorkloadSagaError::InvalidIntent(
                "standalone source evidence requires a standalone source identity",
            ));
        }
        let source_digest = derive_source_digest(
            &source_identity,
            source_generation,
            &resource_version,
            executable_content_digest,
            &attachment_provider_id,
            &execution_provider_id,
        )?;
        Ok(Self::StandaloneSandbox {
            source_identity,
            source_generation,
            resource_version,
            source_digest,
            attachment_provider_id,
            execution_provider_id,
        })
    }

    /// Authenticate a sandbox-backed service source snapshot.
    pub fn sandbox_backed_service(
        source_identity: WorkloadProvisionSourceIdentity,
        source_generation: WorkloadProvisionSourceGeneration,
        resource_version: WorkloadProvisionSourceResourceVersion,
        executable_content_digest: WorkloadExecutableContentDigest,
        attachment_provider_id: NetworkProviderId,
        execution_provider_id: WorkloadExecutionProviderId,
    ) -> Result<Self, WorkloadSagaError> {
        if source_identity.kind() != WorkloadProvisionSourceKind::SandboxBackedService {
            return Err(WorkloadSagaError::InvalidIntent(
                "service source evidence requires a sandbox-backed service identity",
            ));
        }
        let source_digest = derive_source_digest(
            &source_identity,
            source_generation,
            &resource_version,
            executable_content_digest,
            &attachment_provider_id,
            &execution_provider_id,
        )?;
        Ok(Self::SandboxBackedService {
            source_identity,
            source_generation,
            resource_version,
            source_digest,
            attachment_provider_id,
            execution_provider_id,
        })
    }

    /// Validate variant identity and the digest-to-executable correlation.
    pub(super) fn validate(
        &self,
        executable_content_digest: WorkloadExecutableContentDigest,
    ) -> Result<(), WorkloadSagaError> {
        let expected_kind = match self {
            Self::StandaloneSandbox { .. } => WorkloadProvisionSourceKind::StandaloneSandbox,
            Self::SandboxBackedService { .. } => WorkloadProvisionSourceKind::SandboxBackedService,
        };
        if self.source_identity().kind() != expected_kind {
            return Err(WorkloadSagaError::InvalidIntent(
                "provision source evidence kind does not match its source identity",
            ));
        }
        let expected = derive_source_digest(
            self.source_identity(),
            self.source_generation(),
            self.resource_version(),
            executable_content_digest,
            self.attachment_provider_id(),
            self.execution_provider_id(),
        )?;
        if self.source_digest() != expected {
            return Err(WorkloadSagaError::InvalidDigest(
                "provision source digest does not match source and executable evidence",
            ));
        }
        Ok(())
    }

    /// Authenticate exact executable bytes against this source-owned snapshot.
    pub fn authenticate_executable(
        &self,
        executable: &WorkloadExecutableIntent,
    ) -> Result<(), WorkloadSagaError> {
        executable.validate()?;
        self.validate(executable.content_digest())
    }

    /// Stable logical source identity.
    pub fn source_identity(&self) -> &WorkloadProvisionSourceIdentity {
        match self {
            Self::StandaloneSandbox {
                source_identity, ..
            }
            | Self::SandboxBackedService {
                source_identity, ..
            } => source_identity,
        }
    }

    /// Source-owned generation, independent of deployment generation.
    pub const fn source_generation(&self) -> WorkloadProvisionSourceGeneration {
        match self {
            Self::StandaloneSandbox {
                source_generation, ..
            }
            | Self::SandboxBackedService {
                source_generation, ..
            } => *source_generation,
        }
    }

    /// Source-owned resource version.
    pub fn resource_version(&self) -> &WorkloadProvisionSourceResourceVersion {
        match self {
            Self::StandaloneSandbox {
                resource_version, ..
            }
            | Self::SandboxBackedService {
                resource_version, ..
            } => resource_version,
        }
    }

    /// Digest binding source metadata to executable content.
    pub const fn source_digest(&self) -> WorkloadProvisionSourceDigest {
        match self {
            Self::StandaloneSandbox { source_digest, .. }
            | Self::SandboxBackedService { source_digest, .. } => *source_digest,
        }
    }

    /// Exact attachment provider required by the source owner.
    pub fn attachment_provider_id(&self) -> &NetworkProviderId {
        match self {
            Self::StandaloneSandbox {
                attachment_provider_id,
                ..
            }
            | Self::SandboxBackedService {
                attachment_provider_id,
                ..
            } => attachment_provider_id,
        }
    }

    /// Exact execution provider required by the source owner.
    pub fn execution_provider_id(&self) -> &WorkloadExecutionProviderId {
        match self {
            Self::StandaloneSandbox {
                execution_provider_id,
                ..
            }
            | Self::SandboxBackedService {
                execution_provider_id,
                ..
            } => execution_provider_id,
        }
    }

    pub(super) const fn required_workload_kind(&self) -> DesiredWorkloadKind {
        match self {
            Self::StandaloneSandbox { .. } => DesiredWorkloadKind::Sandbox,
            Self::SandboxBackedService { .. } => DesiredWorkloadKind::Service,
        }
    }
}

fn derive_source_digest(
    source_identity: &WorkloadProvisionSourceIdentity,
    source_generation: WorkloadProvisionSourceGeneration,
    resource_version: &WorkloadProvisionSourceResourceVersion,
    executable_content_digest: WorkloadExecutableContentDigest,
    attachment_provider_id: &NetworkProviderId,
    execution_provider_id: &WorkloadExecutionProviderId,
) -> Result<WorkloadProvisionSourceDigest, WorkloadSagaError> {
    let payload = WorkloadProvisionSourceDigestPayload {
        source_identity,
        source_generation,
        resource_version,
        executable_content_digest,
        attachment_provider_id,
        execution_provider_id,
    };
    let encoded = serde_json::to_vec(&payload).map_err(|_| {
        WorkloadSagaError::InvalidIntent("provision source evidence cannot be encoded")
    })?;
    Ok(WorkloadProvisionSourceDigest::sha256(encoded))
}

/// Closed provision operation issued by the compute reducer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadProvisionStep {
    ReserveNetwork,
    PrepareWorkload,
    AttachNetwork,
    InspectActivationPrerequisites,
    ActivateWorkload,
    InspectWorkloadReadiness,
    Publish,
    ObservePublication,
}

/// Exact typed subject set for one provision operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum WorkloadProvisionSubjects {
    Network(WorkloadNetworkReference),
    Execution(WorkloadExecutionReference),
    Readiness {
        network: WorkloadNetworkReference,
        execution: WorkloadExecutionReference,
    },
    Publication(WorkloadPublicationReference),
}

/// Exact successful observation produced by one provision step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkloadProvisionSuccessEvidence {
    NetworkReserved {
        reference: WorkloadNetworkReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
    WorkloadPrepared {
        reference: WorkloadExecutionReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
    NetworkAttached {
        reference: WorkloadNetworkReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
    ActivationPrerequisitesReady {
        network: WorkloadNetworkReference,
        execution: WorkloadExecutionReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
    WorkloadActivated {
        reference: WorkloadExecutionReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
    WorkloadReady {
        network: WorkloadNetworkReference,
        execution: WorkloadExecutionReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
    Published {
        reference: WorkloadPublicationReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
    PublicationObserved {
        reference: WorkloadPublicationReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
}

impl WorkloadProvisionSuccessEvidence {
    /// Provision step this evidence can complete.
    pub const fn step(&self) -> WorkloadProvisionStep {
        match self {
            Self::NetworkReserved { .. } => WorkloadProvisionStep::ReserveNetwork,
            Self::WorkloadPrepared { .. } => WorkloadProvisionStep::PrepareWorkload,
            Self::NetworkAttached { .. } => WorkloadProvisionStep::AttachNetwork,
            Self::ActivationPrerequisitesReady { .. } => {
                WorkloadProvisionStep::InspectActivationPrerequisites
            }
            Self::WorkloadActivated { .. } => WorkloadProvisionStep::ActivateWorkload,
            Self::WorkloadReady { .. } => WorkloadProvisionStep::InspectWorkloadReadiness,
            Self::Published { .. } => WorkloadProvisionStep::Publish,
            Self::PublicationObserved { .. } => WorkloadProvisionStep::ObservePublication,
        }
    }
}

/// Successful prerequisite inspection retained by its activation attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadProvisionPrerequisiteEvidence {
    attempt_id: WorkloadProvisionAttemptId,
    evidence: WorkloadProvisionSuccessEvidence,
}

impl WorkloadProvisionPrerequisiteEvidence {
    /// Retain only exact activation-prerequisite readiness evidence.
    pub fn new(
        attempt_id: WorkloadProvisionAttemptId,
        evidence: WorkloadProvisionSuccessEvidence,
    ) -> Result<Self, WorkloadSagaError> {
        if evidence.step() != WorkloadProvisionStep::InspectActivationPrerequisites {
            return Err(WorkloadSagaError::InvalidEvidence(
                "activation attempt prerequisite must be activation-prerequisite readiness",
            ));
        }
        Ok(Self {
            attempt_id,
            evidence,
        })
    }

    pub fn attempt_id(&self) -> &WorkloadProvisionAttemptId {
        &self.attempt_id
    }

    pub fn evidence(&self) -> &WorkloadProvisionSuccessEvidence {
        &self.evidence
    }
}

/// Complete semantic payload from which an attempt ID is derived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadProvisionAttemptInput {
    pub key: WorkloadSagaKey,
    pub saga_id: WorkloadSagaId,
    pub issuing_revision: WorkloadSagaRevision,
    pub generation: WorkloadGeneration,
    pub desired_digest: WorkloadDesiredDigest,
    pub required_node: NodeIdentity,
    pub source_digest: WorkloadProvisionSourceDigest,
    pub execution_provider_id: WorkloadExecutionProviderId,
    pub network_plan_digest: NetworkPlanDigest,
    pub selection_evidence: Option<NetworkCapabilitySelectionEvidence>,
    pub source_phase: WorkloadSagaPhase,
    pub target_phase: WorkloadSagaPhase,
    pub step: WorkloadProvisionStep,
    pub subjects: WorkloadProvisionSubjects,
    pub prerequisite: Option<WorkloadProvisionPrerequisiteEvidence>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkloadProvisionAttemptIdentityPayload<'a> {
    key: &'a WorkloadSagaKey,
    saga_id: &'a WorkloadSagaId,
    issuing_revision: WorkloadSagaRevision,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    required_node: &'a NodeIdentity,
    source_digest: WorkloadProvisionSourceDigest,
    execution_provider_id: &'a WorkloadExecutionProviderId,
    network_plan_digest: NetworkPlanDigest,
    selection_evidence: &'a Option<NetworkCapabilitySelectionEvidence>,
    source_phase: WorkloadSagaPhase,
    target_phase: WorkloadSagaPhase,
    step: WorkloadProvisionStep,
    subjects: &'a WorkloadProvisionSubjects,
    prerequisite: &'a Option<WorkloadProvisionPrerequisiteEvidence>,
}

/// Durable compute-issued fence persisted before a provider effect can start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadProvisionAttempt {
    attempt_id: WorkloadProvisionAttemptId,
    key: WorkloadSagaKey,
    saga_id: WorkloadSagaId,
    issuing_revision: WorkloadSagaRevision,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    required_node: NodeIdentity,
    source_digest: WorkloadProvisionSourceDigest,
    execution_provider_id: WorkloadExecutionProviderId,
    network_plan_digest: NetworkPlanDigest,
    selection_evidence: Option<NetworkCapabilitySelectionEvidence>,
    source_phase: WorkloadSagaPhase,
    target_phase: WorkloadSagaPhase,
    step: WorkloadProvisionStep,
    subjects: WorkloadProvisionSubjects,
    prerequisite: Option<WorkloadProvisionPrerequisiteEvidence>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WorkloadProvisionAttemptWire {
    attempt_id: WorkloadProvisionAttemptId,
    key: WorkloadSagaKey,
    saga_id: WorkloadSagaId,
    issuing_revision: WorkloadSagaRevision,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    required_node: NodeIdentity,
    source_digest: WorkloadProvisionSourceDigest,
    execution_provider_id: WorkloadExecutionProviderId,
    network_plan_digest: NetworkPlanDigest,
    selection_evidence: Option<NetworkCapabilitySelectionEvidence>,
    source_phase: WorkloadSagaPhase,
    target_phase: WorkloadSagaPhase,
    step: WorkloadProvisionStep,
    subjects: WorkloadProvisionSubjects,
    prerequisite: Option<WorkloadProvisionPrerequisiteEvidence>,
}

impl<'de> Deserialize<'de> for WorkloadProvisionAttempt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WorkloadProvisionAttemptWire::deserialize(deserializer)?;
        let expected_id = wire.attempt_id;
        let attempt = Self::new(WorkloadProvisionAttemptInput {
            key: wire.key,
            saga_id: wire.saga_id,
            issuing_revision: wire.issuing_revision,
            generation: wire.generation,
            desired_digest: wire.desired_digest,
            required_node: wire.required_node,
            source_digest: wire.source_digest,
            execution_provider_id: wire.execution_provider_id,
            network_plan_digest: wire.network_plan_digest,
            selection_evidence: wire.selection_evidence,
            source_phase: wire.source_phase,
            target_phase: wire.target_phase,
            step: wire.step,
            subjects: wire.subjects,
            prerequisite: wire.prerequisite,
        })
        .map_err(serde::de::Error::custom)?;
        if attempt.attempt_id != expected_id {
            return Err(serde::de::Error::custom(
                "workload provision attempt id does not bind its complete payload",
            ));
        }
        Ok(attempt)
    }
}

impl WorkloadProvisionAttempt {
    /// Validate and derive one exact portable attempt.
    pub fn new(input: WorkloadProvisionAttemptInput) -> Result<Self, WorkloadSagaError> {
        validate_attempt_input(&input)?;
        let encoded = serde_json::to_vec(&WorkloadProvisionAttemptIdentityPayload {
            key: &input.key,
            saga_id: &input.saga_id,
            issuing_revision: input.issuing_revision,
            generation: input.generation,
            desired_digest: input.desired_digest,
            required_node: &input.required_node,
            source_digest: input.source_digest,
            execution_provider_id: &input.execution_provider_id,
            network_plan_digest: input.network_plan_digest,
            selection_evidence: &input.selection_evidence,
            source_phase: input.source_phase,
            target_phase: input.target_phase,
            step: input.step,
            subjects: &input.subjects,
            prerequisite: &input.prerequisite,
        })
        .map_err(|_| WorkloadSagaError::InvalidIntent("provision attempt cannot be encoded"))?;
        let canonical = std::str::from_utf8(&encoded)
            .map_err(|_| WorkloadSagaError::InvalidIntent("provision attempt is not UTF-8"))?;
        Ok(Self {
            attempt_id: WorkloadProvisionAttemptId(derive_id(
                WorkloadProvisionAttemptId::PREFIX,
                b"nimbus.workloads.provision.attempt.id.v1",
                &[canonical],
            )),
            key: input.key,
            saga_id: input.saga_id,
            issuing_revision: input.issuing_revision,
            generation: input.generation,
            desired_digest: input.desired_digest,
            required_node: input.required_node,
            source_digest: input.source_digest,
            execution_provider_id: input.execution_provider_id,
            network_plan_digest: input.network_plan_digest,
            selection_evidence: input.selection_evidence,
            source_phase: input.source_phase,
            target_phase: input.target_phase,
            step: input.step,
            subjects: input.subjects,
            prerequisite: input.prerequisite,
        })
    }

    pub fn attempt_id(&self) -> &WorkloadProvisionAttemptId {
        &self.attempt_id
    }

    pub fn key(&self) -> &WorkloadSagaKey {
        &self.key
    }

    pub fn saga_id(&self) -> &WorkloadSagaId {
        &self.saga_id
    }

    pub const fn issuing_revision(&self) -> WorkloadSagaRevision {
        self.issuing_revision
    }

    pub const fn generation(&self) -> WorkloadGeneration {
        self.generation
    }

    pub const fn desired_digest(&self) -> WorkloadDesiredDigest {
        self.desired_digest
    }

    pub fn required_node(&self) -> &NodeIdentity {
        &self.required_node
    }

    pub const fn source_digest(&self) -> WorkloadProvisionSourceDigest {
        self.source_digest
    }

    pub fn execution_provider_id(&self) -> &WorkloadExecutionProviderId {
        &self.execution_provider_id
    }

    pub const fn network_plan_digest(&self) -> NetworkPlanDigest {
        self.network_plan_digest
    }

    pub fn selection_evidence(&self) -> Option<&NetworkCapabilitySelectionEvidence> {
        self.selection_evidence.as_ref()
    }

    pub const fn source_phase(&self) -> WorkloadSagaPhase {
        self.source_phase
    }

    pub const fn target_phase(&self) -> WorkloadSagaPhase {
        self.target_phase
    }

    pub const fn step(&self) -> WorkloadProvisionStep {
        self.step
    }

    pub fn subjects(&self) -> &WorkloadProvisionSubjects {
        &self.subjects
    }

    pub fn prerequisite(&self) -> Option<&WorkloadProvisionPrerequisiteEvidence> {
        self.prerequisite.as_ref()
    }
}

fn validate_attempt_input(input: &WorkloadProvisionAttemptInput) -> Result<(), WorkloadSagaError> {
    if input.saga_id != input.key.saga_id() {
        return Err(WorkloadSagaError::InvalidIdentity(
            "provision attempt saga id does not match its workload key",
        ));
    }
    let phases_valid = matches!(
        (input.step, input.source_phase, input.target_phase),
        (
            WorkloadProvisionStep::ReserveNetwork,
            WorkloadSagaPhase::IntentCommitted,
            WorkloadSagaPhase::NetworkReserved
        ) | (
            WorkloadProvisionStep::PrepareWorkload,
            WorkloadSagaPhase::NetworkReserved,
            WorkloadSagaPhase::WorkloadPrepared
        ) | (
            WorkloadProvisionStep::AttachNetwork,
            WorkloadSagaPhase::WorkloadPrepared,
            WorkloadSagaPhase::NetworkAttached
        ) | (
            WorkloadProvisionStep::InspectActivationPrerequisites,
            WorkloadSagaPhase::NetworkAttached,
            WorkloadSagaPhase::NetworkAttached
        ) | (
            WorkloadProvisionStep::ActivateWorkload,
            WorkloadSagaPhase::NetworkAttached,
            WorkloadSagaPhase::WorkloadActivated
        ) | (
            WorkloadProvisionStep::InspectWorkloadReadiness,
            WorkloadSagaPhase::WorkloadActivated,
            WorkloadSagaPhase::Ready
        ) | (
            WorkloadProvisionStep::Publish,
            WorkloadSagaPhase::Ready,
            WorkloadSagaPhase::Published
        ) | (
            WorkloadProvisionStep::ObservePublication,
            WorkloadSagaPhase::Published,
            WorkloadSagaPhase::Observed
        )
    );
    if !phases_valid {
        return Err(WorkloadSagaError::InvalidTransition(
            "provision step does not match its source and target phases",
        ));
    }
    let subjects_valid = matches!(
        (input.step, &input.subjects),
        (
            WorkloadProvisionStep::ReserveNetwork | WorkloadProvisionStep::AttachNetwork,
            WorkloadProvisionSubjects::Network(_)
        ) | (
            WorkloadProvisionStep::PrepareWorkload | WorkloadProvisionStep::ActivateWorkload,
            WorkloadProvisionSubjects::Execution(_)
        ) | (
            WorkloadProvisionStep::InspectActivationPrerequisites
                | WorkloadProvisionStep::InspectWorkloadReadiness,
            WorkloadProvisionSubjects::Readiness { .. }
        ) | (
            WorkloadProvisionStep::Publish | WorkloadProvisionStep::ObservePublication,
            WorkloadProvisionSubjects::Publication(_)
        )
    );
    if !subjects_valid {
        return Err(WorkloadSagaError::InvalidEvidence(
            "provision step does not match its typed subjects",
        ));
    }
    if (input.step == WorkloadProvisionStep::ActivateWorkload) != input.prerequisite.is_some() {
        return Err(WorkloadSagaError::InvalidEvidence(
            "only activation requires exact prerequisite inspection evidence",
        ));
    }
    Ok(())
}

/// Closed provider-effect outcome accepted by the pure reducer.
#[expect(
    clippy::large_enum_variant,
    reason = "strict portable success evidence stays inline at this low-rate reducer boundary"
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkloadProvisionEffectResult {
    Succeeded {
        attempt_id: WorkloadProvisionAttemptId,
        evidence: WorkloadProvisionSuccessEvidence,
    },
    DefiniteFailure {
        attempt_id: WorkloadProvisionAttemptId,
        failure: WorkloadFailureEvidence,
    },
    Ambiguous {
        attempt_id: WorkloadProvisionAttemptId,
    },
}

/// Durable provision outcome orthogonal to the last completed phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum WorkloadProvisionDisposition {
    Ready,
    DispatchPending(WorkloadProvisionDispatchClaim),
    InspectionRequired(WorkloadProvisionDispatchClaim),
    DefiniteFailure {
        claim: WorkloadProvisionDispatchClaim,
        failure: WorkloadFailureEvidence,
    },
}

impl WorkloadProvisionDisposition {
    /// Exact attempt retained by a non-ready disposition.
    pub fn attempt(&self) -> Option<&WorkloadProvisionAttempt> {
        self.claim().map(WorkloadProvisionDispatchClaim::attempt)
    }

    /// Exact durable dispatch claim retained by a non-ready disposition.
    pub fn claim(&self) -> Option<&WorkloadProvisionDispatchClaim> {
        match self {
            Self::Ready => None,
            Self::DispatchPending(claim)
            | Self::InspectionRequired(claim)
            | Self::DefiniteFailure { claim, .. } => Some(claim),
        }
    }

    /// Whether this generation is halted until explicit compensation.
    pub const fn is_definite_failure(&self) -> bool {
        matches!(self, Self::DefiniteFailure { .. })
    }
}

#[cfg(test)]
#[path = "provision/tests.rs"]
mod tests;
