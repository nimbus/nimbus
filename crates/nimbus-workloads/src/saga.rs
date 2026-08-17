//! Portable workload-saga identity, evidence, and transition vocabulary.

use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

use nimbus_core::{TenantId, WorkloadId};
use nimbus_network::PublishedEndpointId;
use nimbus_tenant::TenantIsolationDecisionId;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::{DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity, TenantWorkloadUid};

/// Portable saga format understood by this crate.
pub const WORKLOAD_SAGA_FORMAT_VERSION: u32 = 7;

/// A rejected workload-saga value or transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkloadSagaError {
    InvalidIdentity(&'static str),
    InvalidCounter(&'static str),
    InvalidDigest(&'static str),
    InvalidIntent(&'static str),
    InvalidEvidence(&'static str),
    InvalidTransition(&'static str),
    StaleGeneration {
        current: WorkloadGeneration,
        candidate: WorkloadGeneration,
    },
    EqualGenerationConflict(WorkloadGeneration),
    GenerationOverflow,
    RevisionOverflow,
    RestartEpochOverflow,
}

impl Display for WorkloadSagaError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentity(message)
            | Self::InvalidCounter(message)
            | Self::InvalidDigest(message)
            | Self::InvalidIntent(message)
            | Self::InvalidEvidence(message)
            | Self::InvalidTransition(message) => formatter.write_str(message),
            Self::StaleGeneration { current, candidate } => write!(
                formatter,
                "workload generation {candidate} is stale relative to {current}"
            ),
            Self::EqualGenerationConflict(generation) => write!(
                formatter,
                "workload generation {generation} has divergent desired content"
            ),
            Self::GenerationOverflow => formatter.write_str("workload generation overflow"),
            Self::RevisionOverflow => formatter.write_str("workload saga revision overflow"),
            Self::RestartEpochOverflow => formatter.write_str("workload restart epoch overflow"),
        }
    }
}

impl StdError for WorkloadSagaError {}

fn parse_decimal(value: &str, label: &'static str) -> Result<u64, WorkloadSagaError> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || value.bytes().any(|byte| !byte.is_ascii_digit())
    {
        return Err(WorkloadSagaError::InvalidCounter(label));
    }
    value
        .parse()
        .map_err(|_| WorkloadSagaError::InvalidCounter(label))
}

macro_rules! define_decimal_counter {
    ($name:ident, $label:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u64);

        impl $name {
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

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                parse_decimal(&value, $label)
                    .map(Self)
                    .map_err(serde::de::Error::custom)
            }
        }
    };
}

define_decimal_counter!(
    WorkloadGeneration,
    "workload generation must be canonical unsigned decimal text"
);
define_decimal_counter!(
    WorkloadSagaRevision,
    "workload saga revision must be canonical unsigned decimal text"
);

fn parse_sha256(value: &str, label: &'static str) -> Result<[u8; 32], WorkloadSagaError> {
    if value.len() != 64
        || value
            .bytes()
            .any(|byte| !byte.is_ascii_digit() && !(b'a'..=b'f').contains(&byte))
    {
        return Err(WorkloadSagaError::InvalidDigest(label));
    }
    let mut bytes = [0_u8; 32];
    for (index, output) in bytes.iter_mut().enumerate() {
        let offset = index * 2;
        *output = u8::from_str_radix(&value[offset..offset + 2], 16)
            .map_err(|_| WorkloadSagaError::InvalidDigest(label))?;
    }
    Ok(bytes)
}

macro_rules! define_sha256_digest {
    ($name:ident, $domain:literal, $label:literal) => {
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name([u8; 32]);

        impl $name {
            pub fn sha256(value: impl AsRef<[u8]>) -> Self {
                let mut hasher = Sha256::new();
                hasher.update($domain);
                hasher.update(value.as_ref());
                Self(hasher.finalize().into())
            }

            pub const fn from_bytes(value: [u8; 32]) -> Self {
                Self(value)
            }

            pub const fn as_bytes(&self) -> &[u8; 32] {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
                for byte in self.0 {
                    write!(formatter, "{byte:02x}")?;
                }
                Ok(())
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_tuple(stringify!($name))
                    .field(&self.to_string())
                    .finish()
            }
        }

        impl FromStr for $name {
            type Err = WorkloadSagaError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                parse_sha256(value, $label).map(Self)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                String::deserialize(deserializer)?
                    .parse()
                    .map_err(serde::de::Error::custom)
            }
        }
    };
}

define_sha256_digest!(
    WorkloadDesiredDigest,
    b"nimbus.workloads.desired.digest.v4\0",
    "workload desired digest must be 64 lowercase hexadecimal characters"
);
define_sha256_digest!(
    WorkloadExecutableContentDigest,
    b"nimbus.workloads.executable.content.v1\0",
    "workload executable content digest must be 64 lowercase hexadecimal characters"
);
define_sha256_digest!(
    WorkloadOwnerEvidenceDigest,
    b"nimbus.workloads.owner-evidence.digest.v1\0",
    "workload owner evidence digest must be 64 lowercase hexadecimal characters"
);
define_sha256_digest!(
    WorkloadTerminalEvidenceDigest,
    b"nimbus.workloads.terminal-evidence.digest.v1\0",
    "workload terminal evidence digest must be 64 lowercase hexadecimal characters"
);

fn derive_id(prefix: &str, domain: &[u8], components: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    for component in components {
        hasher.update(
            u64::try_from(component.len())
                .expect("a Rust string length fits u64 on supported targets")
                .to_be_bytes(),
        );
        hasher.update(component.as_bytes());
    }
    format!("{prefix}_{:x}", hasher.finalize())
}

fn validate_id(value: &str, prefix: &'static str) -> Result<(), WorkloadSagaError> {
    let Some(digest) = value
        .strip_prefix(prefix)
        .and_then(|remainder| remainder.strip_prefix('_'))
    else {
        return Err(WorkloadSagaError::InvalidIdentity(
            "workload saga identity has the wrong domain prefix",
        ));
    };
    parse_sha256(
        digest,
        "workload saga identity must contain a canonical SHA-256 digest",
    )?;
    Ok(())
}

macro_rules! define_derived_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub const PREFIX: &'static str = $prefix;

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = WorkloadSagaError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                value.to_owned().try_into()
            }
        }

        impl TryFrom<String> for $name {
            type Error = WorkloadSagaError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                validate_id(&value, Self::PREFIX)?;
                Ok(Self(value))
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

define_derived_id!(WorkloadSagaId, "wsg");
define_derived_id!(WorkloadExecutionId, "wex");
define_derived_id!(WorkloadSagaTransitionId, "wst");

/// Stable logical workload key shared by every desired generation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadSagaKey {
    tenant_id: TenantId,
    workload_id: WorkloadId,
}

impl WorkloadSagaKey {
    pub fn new(tenant_id: TenantId, workload_id: WorkloadId) -> Self {
        Self {
            tenant_id,
            workload_id,
        }
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn workload_id(&self) -> &WorkloadId {
        &self.workload_id
    }

    pub fn saga_id(&self) -> WorkloadSagaId {
        WorkloadSagaId(derive_id(
            WorkloadSagaId::PREFIX,
            b"nimbus.workloads.saga.id.v1",
            &[self.tenant_id.as_str(), self.workload_id.as_str()],
        ))
    }
}

impl WorkloadExecutionId {
    pub fn for_execution(
        workload_uid: &TenantWorkloadUid,
        node_identity: &NodeIdentity,
        generation: WorkloadGeneration,
    ) -> Self {
        let generation = generation.to_string();
        Self(derive_id(
            Self::PREFIX,
            b"nimbus.workloads.execution.id.v1",
            &[workload_uid.as_str(), node_identity.as_str(), &generation],
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadActivationIntent {
    PrepareOnly,
    ActivateWhenAttached,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadPublicationIntent {
    Withheld,
    PublishWhenReady,
}

/// Admission evidence that binds a desired generation to tenant policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadAdmissionEvidence {
    decision_id: TenantIsolationDecisionId,
    workload_uid: TenantWorkloadUid,
    assigned_node: NodeIdentity,
}

impl WorkloadAdmissionEvidence {
    pub fn new(
        decision_id: TenantIsolationDecisionId,
        workload_uid: TenantWorkloadUid,
        assigned_node: NodeIdentity,
    ) -> Self {
        Self {
            decision_id,
            workload_uid,
            assigned_node,
        }
    }

    pub fn decision_id(&self) -> &TenantIsolationDecisionId {
        &self.decision_id
    }

    pub fn workload_uid(&self) -> &TenantWorkloadUid {
        &self.workload_uid
    }

    pub fn assigned_node(&self) -> &NodeIdentity {
        &self.assigned_node
    }
}

/// Complete desired content for one generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadSagaIntent {
    kind: DesiredWorkloadKind,
    desired_state: DesiredWorkloadState,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    executable: WorkloadExecutableIntent,
    source: WorkloadProvisionSourceEvidence,
    restart_policy: WorkloadRestartPolicy,
    network: WorkloadNetworkIntent,
    activation: WorkloadActivationIntent,
    publication: WorkloadPublicationIntent,
    admission: WorkloadAdmissionEvidence,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WorkloadSagaIntentWire {
    kind: DesiredWorkloadKind,
    desired_state: DesiredWorkloadState,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    executable: WorkloadExecutableIntent,
    source: WorkloadProvisionSourceEvidence,
    restart_policy: WorkloadRestartPolicy,
    network: WorkloadNetworkIntent,
    activation: WorkloadActivationIntent,
    publication: WorkloadPublicationIntent,
    admission: WorkloadAdmissionEvidence,
}

impl<'de> Deserialize<'de> for WorkloadSagaIntent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WorkloadSagaIntentWire::deserialize(deserializer)?;
        let expected_digest = wire.desired_digest;
        let intent = Self::new_with_restart_policy(
            wire.kind,
            wire.desired_state,
            wire.generation,
            wire.executable,
            wire.source,
            wire.restart_policy,
            wire.network,
            wire.activation,
            wire.publication,
            wire.admission,
        )
        .map_err(serde::de::Error::custom)?;
        if intent.desired_digest != expected_digest {
            return Err(serde::de::Error::custom(
                "workload desired digest does not match complete desired intent",
            ));
        }
        Ok(intent)
    }
}

impl WorkloadSagaIntent {
    #[allow(clippy::too_many_arguments)]
    pub fn new_without_automatic_restart(
        kind: DesiredWorkloadKind,
        desired_state: DesiredWorkloadState,
        generation: WorkloadGeneration,
        executable: WorkloadExecutableIntent,
        source: WorkloadProvisionSourceEvidence,
        network: WorkloadNetworkIntent,
        activation: WorkloadActivationIntent,
        publication: WorkloadPublicationIntent,
        admission: WorkloadAdmissionEvidence,
    ) -> Result<Self, WorkloadSagaError> {
        Self::new_with_restart_policy(
            kind,
            desired_state,
            generation,
            executable,
            source,
            WorkloadRestartPolicy::Never,
            network,
            activation,
            publication,
            admission,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_restart_policy(
        kind: DesiredWorkloadKind,
        desired_state: DesiredWorkloadState,
        generation: WorkloadGeneration,
        executable: WorkloadExecutableIntent,
        source: WorkloadProvisionSourceEvidence,
        restart_policy: WorkloadRestartPolicy,
        network: WorkloadNetworkIntent,
        activation: WorkloadActivationIntent,
        publication: WorkloadPublicationIntent,
        admission: WorkloadAdmissionEvidence,
    ) -> Result<Self, WorkloadSagaError> {
        executable.validate()?;
        source.validate(executable.content_digest())?;
        let desired_digest = derive_desired_digest(
            kind,
            desired_state,
            generation,
            &executable,
            &source,
            restart_policy,
            &network,
            activation,
            publication,
            &admission,
        )?;
        let intent = Self {
            kind,
            desired_state,
            generation,
            desired_digest,
            executable,
            source,
            restart_policy,
            network,
            activation,
            publication,
            admission,
        };
        intent.validate()?;
        Ok(intent)
    }

    pub(super) fn validate(&self) -> Result<(), WorkloadSagaError> {
        self.executable.validate()?;
        self.source.validate(self.executable.content_digest())?;
        if self.kind != self.source.required_workload_kind() {
            return Err(WorkloadSagaError::InvalidIntent(
                "desired workload kind does not match provision source kind",
            ));
        }
        if self.desired_digest
            != derive_desired_digest(
                self.kind,
                self.desired_state,
                self.generation,
                &self.executable,
                &self.source,
                self.restart_policy,
                &self.network,
                self.activation,
                self.publication,
                &self.admission,
            )?
        {
            return Err(WorkloadSagaError::InvalidDigest(
                "workload desired digest does not match complete desired intent",
            ));
        }
        if self.generation.as_u64() != self.network.generation().as_u64() {
            return Err(WorkloadSagaError::InvalidIntent(
                "network generation must match workload generation",
            ));
        }
        match self
            .network
            .compiled_plan()
            .content()
            .capability_selection()
        {
            Some(selection)
                if selection.attachment_provider_id() != self.source.attachment_provider_id() =>
            {
                return Err(WorkloadSagaError::InvalidIntent(
                    "provision source attachment provider must match network selection",
                ));
            }
            Some(_) => {}
            None if self
                .network
                .compiled_plan()
                .content()
                .capability_selection_evidence()
                .is_some() =>
            {
                return Err(WorkloadSagaError::InvalidIntent(
                    "resource-free network intent cannot retain provider evidence",
                ));
            }
            None => {}
        }
        if self.activation != self.network.compiled_plan().content().activation() {
            return Err(WorkloadSagaError::InvalidIntent(
                "network activation must match workload activation",
            ));
        }
        if self.publication != self.network.compiled_plan().content().publication() {
            return Err(WorkloadSagaError::InvalidIntent(
                "network publication must match workload publication",
            ));
        }
        if self.desired_state == DesiredWorkloadState::Stopped
            && (self.activation != WorkloadActivationIntent::PrepareOnly
                || self.publication != WorkloadPublicationIntent::Withheld)
        {
            return Err(WorkloadSagaError::InvalidIntent(
                "stopped intent must prepare nothing and withhold publication",
            ));
        }
        Ok(())
    }

    pub fn kind(&self) -> DesiredWorkloadKind {
        self.kind
    }

    pub fn desired_state(&self) -> DesiredWorkloadState {
        self.desired_state
    }

    pub fn generation(&self) -> WorkloadGeneration {
        self.generation
    }

    pub fn desired_digest(&self) -> WorkloadDesiredDigest {
        self.desired_digest
    }

    pub fn executable(&self) -> &WorkloadExecutableIntent {
        &self.executable
    }

    pub fn source(&self) -> &WorkloadProvisionSourceEvidence {
        &self.source
    }

    pub fn restart_policy(&self) -> WorkloadRestartPolicy {
        self.restart_policy
    }

    pub fn network(&self) -> &WorkloadNetworkIntent {
        &self.network
    }

    pub fn activation(&self) -> WorkloadActivationIntent {
        self.activation
    }

    pub fn publication(&self) -> WorkloadPublicationIntent {
        self.publication
    }

    pub fn admission(&self) -> &WorkloadAdmissionEvidence {
        &self.admission
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkloadDesiredDigestPayload<'a> {
    kind: DesiredWorkloadKind,
    desired_state: DesiredWorkloadState,
    generation: WorkloadGeneration,
    executable: &'a WorkloadExecutableIntent,
    source: &'a WorkloadProvisionSourceEvidence,
    restart_policy: WorkloadRestartPolicy,
    network: &'a WorkloadNetworkIntent,
    activation: WorkloadActivationIntent,
    publication: WorkloadPublicationIntent,
    admission: &'a WorkloadAdmissionEvidence,
}

#[allow(clippy::too_many_arguments)]
fn derive_desired_digest(
    kind: DesiredWorkloadKind,
    desired_state: DesiredWorkloadState,
    generation: WorkloadGeneration,
    executable: &WorkloadExecutableIntent,
    source: &WorkloadProvisionSourceEvidence,
    restart_policy: WorkloadRestartPolicy,
    network: &WorkloadNetworkIntent,
    activation: WorkloadActivationIntent,
    publication: WorkloadPublicationIntent,
    admission: &WorkloadAdmissionEvidence,
) -> Result<WorkloadDesiredDigest, WorkloadSagaError> {
    let payload = WorkloadDesiredDigestPayload {
        kind,
        desired_state,
        generation,
        executable,
        source,
        restart_policy,
        network,
        activation,
        publication,
        admission,
    };
    let canonical = serde_json::to_vec(&payload).map_err(|_| {
        WorkloadSagaError::InvalidIntent("complete workload desired intent cannot be encoded")
    })?;
    Ok(WorkloadDesiredDigest::sha256(canonical))
}

/// Stable reference to one generation-scoped execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadExecutionReference {
    workload_uid: TenantWorkloadUid,
    node_identity: NodeIdentity,
    execution_id: WorkloadExecutionId,
    restart_epoch: WorkloadRestartEpoch,
    attempt_id: WorkloadExecutionAttemptId,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WorkloadExecutionReferenceWire {
    workload_uid: TenantWorkloadUid,
    node_identity: NodeIdentity,
    execution_id: WorkloadExecutionId,
    restart_epoch: WorkloadRestartEpoch,
    attempt_id: WorkloadExecutionAttemptId,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
}

impl<'de> Deserialize<'de> for WorkloadExecutionReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WorkloadExecutionReferenceWire::deserialize(deserializer)?;
        let reference = Self {
            workload_uid: wire.workload_uid,
            node_identity: wire.node_identity,
            execution_id: wire.execution_id,
            restart_epoch: wire.restart_epoch,
            attempt_id: wire.attempt_id,
            generation: wire.generation,
            desired_digest: wire.desired_digest,
        };
        reference
            .validate_intrinsic()
            .map_err(serde::de::Error::custom)?;
        Ok(reference)
    }
}

impl WorkloadExecutionReference {
    pub fn for_intent(intent: &WorkloadSagaIntent) -> Self {
        Self::for_restart_epoch(intent, WorkloadRestartEpoch::new(0))
    }

    pub fn for_restart_epoch(
        intent: &WorkloadSagaIntent,
        restart_epoch: WorkloadRestartEpoch,
    ) -> Self {
        let node_identity = intent.admission.assigned_node.clone();
        let workload_uid = intent.admission.workload_uid.clone();
        let execution_id =
            WorkloadExecutionId::for_execution(&workload_uid, &node_identity, intent.generation);
        Self {
            attempt_id: WorkloadExecutionAttemptId::for_execution(&execution_id, restart_epoch),
            execution_id,
            restart_epoch,
            workload_uid,
            node_identity,
            generation: intent.generation,
            desired_digest: intent.desired_digest,
        }
    }

    pub fn workload_uid(&self) -> &TenantWorkloadUid {
        &self.workload_uid
    }

    pub fn node_identity(&self) -> &NodeIdentity {
        &self.node_identity
    }

    pub fn execution_id(&self) -> &WorkloadExecutionId {
        &self.execution_id
    }

    pub fn attempt_id(&self) -> &WorkloadExecutionAttemptId {
        &self.attempt_id
    }

    pub fn restart_epoch(&self) -> WorkloadRestartEpoch {
        self.restart_epoch
    }

    pub fn generation(&self) -> WorkloadGeneration {
        self.generation
    }

    pub fn desired_digest(&self) -> WorkloadDesiredDigest {
        self.desired_digest
    }

    fn validate_intrinsic(&self) -> Result<(), WorkloadSagaError> {
        if self.execution_id
            != WorkloadExecutionId::for_execution(
                &self.workload_uid,
                &self.node_identity,
                self.generation,
            )
        {
            return Err(WorkloadSagaError::InvalidEvidence(
                "execution reference id does not match its workload, node, and generation",
            ));
        }
        if self.attempt_id
            != WorkloadExecutionAttemptId::for_execution(&self.execution_id, self.restart_epoch)
        {
            return Err(WorkloadSagaError::InvalidEvidence(
                "execution attempt id does not match its execution and restart epoch",
            ));
        }
        Ok(())
    }

    fn validate_for(&self, intent: &WorkloadSagaIntent) -> Result<(), WorkloadSagaError> {
        self.validate_intrinsic()?;
        if self.workload_uid == *intent.admission.workload_uid()
            && self.node_identity == *intent.admission.assigned_node()
            && self.execution_id
                == WorkloadExecutionId::for_execution(
                    intent.admission.workload_uid(),
                    intent.admission.assigned_node(),
                    intent.generation,
                )
            && self.generation == intent.generation
            && self.desired_digest == intent.desired_digest
        {
            Ok(())
        } else {
            Err(WorkloadSagaError::InvalidEvidence(
                "execution reference is crossed or stale",
            ))
        }
    }
}

/// Stable reference to the exact endpoint set governed by publication.
///
/// A withheld workload with no listeners carries an empty set so teardown can
/// record explicit empty-batch withdrawal without inventing an endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadPublicationReference {
    endpoints: Vec<PublishedEndpointId>,
    network: WorkloadNetworkReference,
    execution: WorkloadExecutionReference,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WorkloadPublicationReferenceWire {
    endpoints: Vec<PublishedEndpointId>,
    network: WorkloadNetworkReference,
    execution: WorkloadExecutionReference,
}

impl<'de> Deserialize<'de> for WorkloadPublicationReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WorkloadPublicationReferenceWire::deserialize(deserializer)?;
        let reference = Self {
            endpoints: wire.endpoints,
            network: wire.network,
            execution: wire.execution,
        };
        reference
            .validate_intrinsic()
            .map_err(serde::de::Error::custom)?;
        Ok(reference)
    }
}

impl WorkloadPublicationReference {
    pub fn new(
        endpoints: impl IntoIterator<Item = PublishedEndpointId>,
        intent: &WorkloadSagaIntent,
    ) -> Result<Self, WorkloadSagaError> {
        Self::for_execution(
            endpoints,
            intent,
            WorkloadExecutionReference::for_intent(intent),
        )
    }

    pub fn for_execution(
        endpoints: impl IntoIterator<Item = PublishedEndpointId>,
        intent: &WorkloadSagaIntent,
        execution: WorkloadExecutionReference,
    ) -> Result<Self, WorkloadSagaError> {
        let mut endpoints: Vec<_> = endpoints.into_iter().collect();
        if endpoints.is_empty()
            && (intent.publication() != WorkloadPublicationIntent::Withheld
                || !intent
                    .network()
                    .compiled_plan()
                    .content()
                    .listeners()
                    .is_empty())
        {
            return Err(WorkloadSagaError::InvalidEvidence(
                "empty publication reference requires a withheld zero-listener intent",
            ));
        }
        let original_len = endpoints.len();
        endpoints.sort();
        endpoints.dedup();
        if endpoints.len() != original_len {
            return Err(WorkloadSagaError::InvalidEvidence(
                "publication reference contains a duplicate endpoint",
            ));
        }
        execution.validate_for(intent)?;
        Ok(Self {
            endpoints,
            network: WorkloadNetworkReference::for_intent(intent),
            execution,
        })
    }

    pub fn endpoints(&self) -> &[PublishedEndpointId] {
        &self.endpoints
    }

    pub fn network(&self) -> &WorkloadNetworkReference {
        &self.network
    }

    pub fn execution(&self) -> &WorkloadExecutionReference {
        &self.execution
    }

    fn validate_intrinsic(&self) -> Result<(), WorkloadSagaError> {
        if !self.endpoints.windows(2).all(|pair| pair[0] < pair[1]) {
            return Err(WorkloadSagaError::InvalidEvidence(
                "publication reference is unsorted or duplicated",
            ));
        }
        Ok(())
    }

    fn validate_for(&self, intent: &WorkloadSagaIntent) -> Result<(), WorkloadSagaError> {
        self.validate_intrinsic()?;
        if self.network != WorkloadNetworkReference::for_intent(intent) {
            return Err(WorkloadSagaError::InvalidEvidence(
                "publication reference is crossed with another network intent",
            ));
        }
        if self.endpoints.is_empty()
            && (intent.publication() != WorkloadPublicationIntent::Withheld
                || !intent
                    .network()
                    .compiled_plan()
                    .content()
                    .listeners()
                    .is_empty())
        {
            return Err(WorkloadSagaError::InvalidEvidence(
                "empty publication reference crosses a nonempty or publishable intent",
            ));
        }
        self.execution.validate_for(intent)?;
        Ok(())
    }
}

/// Exact stable subjects retained across a saga phase.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadEffectReferences {
    network: Option<WorkloadNetworkReference>,
    execution: Option<WorkloadExecutionReference>,
    publication: Option<WorkloadPublicationReference>,
}

impl WorkloadEffectReferences {
    pub fn new(
        network: Option<WorkloadNetworkReference>,
        execution: Option<WorkloadExecutionReference>,
        publication: Option<WorkloadPublicationReference>,
    ) -> Self {
        Self {
            network,
            execution,
            publication,
        }
    }

    pub fn provision(
        intent: &WorkloadSagaIntent,
        publication: Option<WorkloadPublicationReference>,
    ) -> Result<Self, WorkloadSagaError> {
        Ok(Self {
            network: Some(WorkloadNetworkReference::for_intent(intent)),
            execution: Some(WorkloadExecutionReference::for_intent(intent)),
            publication,
        })
    }

    pub fn network(&self) -> Option<&WorkloadNetworkReference> {
        self.network.as_ref()
    }

    pub fn execution(&self) -> Option<&WorkloadExecutionReference> {
        self.execution.as_ref()
    }

    pub fn publication(&self) -> Option<&WorkloadPublicationReference> {
        self.publication.as_ref()
    }

    pub fn is_empty(&self) -> bool {
        self.network.is_none() && self.execution.is_none() && self.publication.is_none()
    }

    pub fn len(&self) -> usize {
        usize::from(self.network.is_some())
            + usize::from(self.execution.is_some())
            + usize::from(self.publication.is_some())
    }

    fn validate_for(&self, intent: &WorkloadSagaIntent) -> Result<(), WorkloadSagaError> {
        if let Some(network) = &self.network
            && network != &WorkloadNetworkReference::for_intent(intent)
        {
            return Err(WorkloadSagaError::InvalidEvidence(
                "network reference is crossed or stale",
            ));
        }
        if let Some(execution) = &self.execution {
            execution.validate_for(intent)?;
        }
        if let Some(publication) = &self.publication {
            publication.validate_for(intent)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadSagaPhase {
    IntentCommitted,
    NetworkReserved,
    WorkloadPrepared,
    NetworkAttached,
    WorkloadActivated,
    Ready,
    Published,
    Observed,
    WithdrawalCommitted,
    Withdrawn,
    Drained,
    WorkloadStopped,
    NetworkDetached,
    NetworkReleased,
    Recorded,
    CleanupPending,
}

/// Stable phase order for bounded durable-saga recovery scans.
///
/// The list covers every phase. Recovery adapters must still apply
/// [`WorkloadSagaRecord::requires_recovery`] because phase alone cannot
/// distinguish quiescent records such as prepare-only attachments or a
/// `Recorded` record without a queued successor.
pub const WORKLOAD_SAGA_RECOVERY_ORDER: [WorkloadSagaPhase; 16] = [
    WorkloadSagaPhase::IntentCommitted,
    WorkloadSagaPhase::NetworkReserved,
    WorkloadSagaPhase::WorkloadPrepared,
    WorkloadSagaPhase::NetworkAttached,
    WorkloadSagaPhase::WorkloadActivated,
    WorkloadSagaPhase::Ready,
    WorkloadSagaPhase::Published,
    WorkloadSagaPhase::Observed,
    WorkloadSagaPhase::WithdrawalCommitted,
    WorkloadSagaPhase::Withdrawn,
    WorkloadSagaPhase::Drained,
    WorkloadSagaPhase::WorkloadStopped,
    WorkloadSagaPhase::NetworkDetached,
    WorkloadSagaPhase::NetworkReleased,
    WorkloadSagaPhase::CleanupPending,
    WorkloadSagaPhase::Recorded,
];

impl WorkloadSagaPhase {
    pub fn is_provision(self) -> bool {
        matches!(
            self,
            Self::IntentCommitted
                | Self::NetworkReserved
                | Self::WorkloadPrepared
                | Self::NetworkAttached
                | Self::WorkloadActivated
                | Self::Ready
                | Self::Published
                | Self::Observed
        )
    }

    pub fn is_teardown(self) -> bool {
        matches!(
            self,
            Self::WithdrawalCommitted
                | Self::Withdrawn
                | Self::Drained
                | Self::WorkloadStopped
                | Self::NetworkDetached
                | Self::NetworkReleased
        )
    }

    pub fn is_recoverable(self) -> bool {
        !matches!(self, Self::Observed | Self::Recorded)
    }

    /// Returns this phase's stable rank in [`WORKLOAD_SAGA_RECOVERY_ORDER`].
    pub const fn recovery_order(self) -> u8 {
        match self {
            Self::IntentCommitted => 0,
            Self::NetworkReserved => 1,
            Self::WorkloadPrepared => 2,
            Self::NetworkAttached => 3,
            Self::WorkloadActivated => 4,
            Self::Ready => 5,
            Self::Published => 6,
            Self::Observed => 7,
            Self::WithdrawalCommitted => 8,
            Self::Withdrawn => 9,
            Self::Drained => 10,
            Self::WorkloadStopped => 11,
            Self::NetworkDetached => 12,
            Self::NetworkReleased => 13,
            Self::CleanupPending => 14,
            Self::Recorded => 15,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "snake_case")]
pub enum WorkloadOwnerObservation {
    NetworkReserved {
        reference: WorkloadNetworkReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
    ExecutionPrepared {
        reference: WorkloadExecutionReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
    NetworkAttached {
        reference: WorkloadNetworkReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
    ExecutionActivated {
        reference: WorkloadExecutionReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
    Ready {
        network: WorkloadNetworkReference,
        execution: WorkloadExecutionReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
    PublicationPresent {
        reference: WorkloadPublicationReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
    PublicationObserved {
        reference: WorkloadPublicationReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OwnerObservationKind {
    NetworkReserved,
    ExecutionPrepared,
    NetworkAttached,
    ExecutionActivated,
    Ready,
    PublicationPresent,
    PublicationObserved,
}

impl WorkloadOwnerObservation {
    fn kind(&self) -> OwnerObservationKind {
        match self {
            Self::NetworkReserved { .. } => OwnerObservationKind::NetworkReserved,
            Self::ExecutionPrepared { .. } => OwnerObservationKind::ExecutionPrepared,
            Self::NetworkAttached { .. } => OwnerObservationKind::NetworkAttached,
            Self::ExecutionActivated { .. } => OwnerObservationKind::ExecutionActivated,
            Self::Ready { .. } => OwnerObservationKind::Ready,
            Self::PublicationPresent { .. } => OwnerObservationKind::PublicationPresent,
            Self::PublicationObserved { .. } => OwnerObservationKind::PublicationObserved,
        }
    }

    fn matches(&self, references: &WorkloadEffectReferences) -> bool {
        match self {
            Self::NetworkReserved { reference, .. } | Self::NetworkAttached { reference, .. } => {
                references.network.as_ref() == Some(reference)
            }
            Self::ExecutionPrepared { reference, .. }
            | Self::ExecutionActivated { reference, .. } => {
                references.execution.as_ref() == Some(reference)
            }
            Self::Ready {
                network, execution, ..
            } => {
                references.network.as_ref() == Some(network)
                    && references.execution.as_ref() == Some(execution)
            }
            Self::PublicationPresent { reference, .. }
            | Self::PublicationObserved { reference, .. } => {
                references.publication.as_ref() == Some(reference)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "snake_case")]
pub enum WorkloadTerminalObservation {
    PublicationAbsent {
        reference: WorkloadPublicationReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
    ExecutionDrained {
        reference: WorkloadExecutionReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
    ExecutionStopped {
        reference: WorkloadExecutionReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
    NetworkDetached {
        reference: WorkloadNetworkReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
    NetworkReleased {
        reference: WorkloadNetworkReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalObservationKind {
    PublicationAbsent,
    ExecutionDrained,
    ExecutionStopped,
    NetworkDetached,
    NetworkReleased,
}

impl WorkloadTerminalObservation {
    fn kind(&self) -> TerminalObservationKind {
        match self {
            Self::PublicationAbsent { .. } => TerminalObservationKind::PublicationAbsent,
            Self::ExecutionDrained { .. } => TerminalObservationKind::ExecutionDrained,
            Self::ExecutionStopped { .. } => TerminalObservationKind::ExecutionStopped,
            Self::NetworkDetached { .. } => TerminalObservationKind::NetworkDetached,
            Self::NetworkReleased { .. } => TerminalObservationKind::NetworkReleased,
        }
    }

    fn matches(&self, references: &WorkloadEffectReferences) -> bool {
        match self {
            Self::PublicationAbsent { reference, .. } => {
                references.publication.as_ref() == Some(reference)
            }
            Self::ExecutionDrained { reference, .. } | Self::ExecutionStopped { reference, .. } => {
                references.execution.as_ref() == Some(reference)
            }
            Self::NetworkDetached { reference, .. } | Self::NetworkReleased { reference, .. } => {
                references.network.as_ref() == Some(reference)
            }
        }
    }
}

impl WorkloadTerminalEvidenceDigest {
    pub fn for_observations(
        observations: &[WorkloadTerminalObservation],
    ) -> Result<Self, WorkloadSagaError> {
        let bytes = serde_json::to_vec(observations).map_err(|_| {
            WorkloadSagaError::InvalidEvidence("terminal observations cannot be encoded")
        })?;
        Ok(Self::sha256(bytes))
    }

    pub(crate) fn for_teardown(
        observations: &[WorkloadTerminalObservation],
        restart_settlement: Option<&WorkloadRestartTeardownSettlement>,
    ) -> Result<Self, WorkloadSagaError> {
        let Some(restart_settlement) = restart_settlement else {
            return Self::for_observations(observations);
        };
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct RestartSettlementTerminalEvidence<'a> {
            domain: &'static str,
            observations: &'a [WorkloadTerminalObservation],
            restart_settlement: &'a WorkloadRestartTeardownSettlement,
        }
        let bytes = serde_json::to_vec(&RestartSettlementTerminalEvidence {
            domain: "nimbus.workloads.restart-settlement-terminal.v1",
            observations,
            restart_settlement,
        })
        .map_err(|_| {
            WorkloadSagaError::InvalidEvidence(
                "restart settlement terminal evidence cannot be encoded",
            )
        })?;
        Ok(Self::sha256(bytes))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "snake_case")]
pub enum WorkloadInspectionRequirement {
    Network {
        reference: WorkloadNetworkReference,
        expected_phase: WorkloadSagaPhase,
    },
    Execution {
        reference: WorkloadExecutionReference,
        expected_phase: WorkloadSagaPhase,
    },
    Publication {
        reference: WorkloadPublicationReference,
        expected_phase: WorkloadSagaPhase,
    },
}

impl WorkloadInspectionRequirement {
    fn matches_index(&self, index: usize, references: &WorkloadEffectReferences) -> bool {
        let mut expected = Vec::with_capacity(references.len());
        if let Some(reference) = &references.network {
            expected.push(InspectionSubject::Network(reference));
        }
        if let Some(reference) = &references.execution {
            expected.push(InspectionSubject::Execution(reference));
        }
        if let Some(reference) = &references.publication {
            expected.push(InspectionSubject::Publication(reference));
        }
        matches!(
            (self, expected.get(index)),
            (Self::Network { reference, .. }, Some(InspectionSubject::Network(expected)))
                if reference == *expected
        ) || matches!(
            (self, expected.get(index)),
            (Self::Execution { reference, .. }, Some(InspectionSubject::Execution(expected)))
                if reference == *expected
        ) || matches!(
            (self, expected.get(index)),
            (Self::Publication { reference, .. }, Some(InspectionSubject::Publication(expected)))
                if reference == *expected
        )
    }
}

enum InspectionSubject<'a> {
    Network(&'a WorkloadNetworkReference),
    Execution(&'a WorkloadExecutionReference),
    Publication(&'a WorkloadPublicationReference),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadProvisionDetail {
    references: WorkloadEffectReferences,
    observations: Vec<WorkloadOwnerObservation>,
}

impl WorkloadProvisionDetail {
    pub fn references(&self) -> &WorkloadEffectReferences {
        &self.references
    }

    pub fn observations(&self) -> &[WorkloadOwnerObservation] {
        &self.observations
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadTeardownDetail {
    origin: WorkloadSagaPhase,
    retained_references: WorkloadEffectReferences,
    terminal_observations: Vec<WorkloadTerminalObservation>,
}

impl WorkloadTeardownDetail {
    pub fn origin(&self) -> WorkloadSagaPhase {
        self.origin
    }

    pub fn retained_references(&self) -> &WorkloadEffectReferences {
        &self.retained_references
    }

    pub fn terminal_observations(&self) -> &[WorkloadTerminalObservation] {
        &self.terminal_observations
    }

    /// Exact execution settled by terminal teardown evidence, when execution
    /// authority existed for this workload generation.
    pub fn terminal_execution_reference(&self) -> Option<&WorkloadExecutionReference> {
        self.terminal_observations
            .iter()
            .find_map(|observation| match observation {
                WorkloadTerminalObservation::ExecutionStopped { reference, .. } => Some(reference),
                _ => None,
            })
            .or_else(|| {
                self.terminal_observations
                    .iter()
                    .find_map(|observation| match observation {
                        WorkloadTerminalObservation::ExecutionDrained { reference, .. } => {
                            Some(reference)
                        }
                        _ => None,
                    })
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadCleanupPendingDetail {
    last_safe_phase: WorkloadSagaPhase,
    retained_references: WorkloadEffectReferences,
    inspections: Vec<WorkloadInspectionRequirement>,
}

impl WorkloadCleanupPendingDetail {
    pub fn last_safe_phase(&self) -> WorkloadSagaPhase {
        self.last_safe_phase
    }

    pub fn retained_references(&self) -> &WorkloadEffectReferences {
        &self.retained_references
    }

    pub fn inspections(&self) -> &[WorkloadInspectionRequirement] {
        &self.inspections
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadRecordedDetail {
    completed_generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    terminal_evidence_digest: WorkloadTerminalEvidenceDigest,
    terminal_execution: Option<WorkloadExecutionReference>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WorkloadRecordedDetailWire {
    completed_generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    terminal_evidence_digest: WorkloadTerminalEvidenceDigest,
    #[serde(deserialize_with = "deserialize_required_terminal_execution")]
    terminal_execution: Option<WorkloadExecutionReference>,
}

fn deserialize_required_terminal_execution<'de, D>(
    deserializer: D,
) -> Result<Option<WorkloadExecutionReference>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::deserialize(deserializer)
}

impl<'de> Deserialize<'de> for WorkloadRecordedDetail {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WorkloadRecordedDetailWire::deserialize(deserializer)?;
        Ok(Self {
            completed_generation: wire.completed_generation,
            desired_digest: wire.desired_digest,
            terminal_evidence_digest: wire.terminal_evidence_digest,
            terminal_execution: wire.terminal_execution,
        })
    }
}

impl WorkloadRecordedDetail {
    pub fn completed_generation(&self) -> WorkloadGeneration {
        self.completed_generation
    }

    pub fn desired_digest(&self) -> WorkloadDesiredDigest {
        self.desired_digest
    }

    pub fn terminal_evidence_digest(&self) -> WorkloadTerminalEvidenceDigest {
        self.terminal_evidence_digest
    }

    pub fn terminal_execution_reference(&self) -> Option<&WorkloadExecutionReference> {
        self.terminal_execution.as_ref()
    }
}

/// Closed phase-specific workload evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    deny_unknown_fields,
    tag = "kind",
    content = "value",
    rename_all = "snake_case"
)]
pub enum WorkloadPhaseDetail {
    Intent,
    Provision(WorkloadProvisionDetail),
    Teardown(WorkloadTeardownDetail),
    CleanupPending(WorkloadCleanupPendingDetail),
    Recorded(WorkloadRecordedDetail),
}

impl WorkloadPhaseDetail {
    pub fn intent() -> Self {
        Self::Intent
    }

    pub fn provision(
        phase: WorkloadSagaPhase,
        intent: &WorkloadSagaIntent,
        references: WorkloadEffectReferences,
        observations: Vec<WorkloadOwnerObservation>,
    ) -> Result<Self, WorkloadSagaError> {
        let detail = Self::Provision(WorkloadProvisionDetail {
            references,
            observations,
        });
        validate_phase_detail(phase, intent, &detail)?;
        Ok(detail)
    }

    pub fn teardown(
        phase: WorkloadSagaPhase,
        intent: &WorkloadSagaIntent,
        origin: WorkloadSagaPhase,
        retained_references: WorkloadEffectReferences,
        terminal_observations: Vec<WorkloadTerminalObservation>,
    ) -> Result<Self, WorkloadSagaError> {
        let detail = Self::Teardown(WorkloadTeardownDetail {
            origin,
            retained_references,
            terminal_observations,
        });
        validate_phase_detail(phase, intent, &detail)?;
        Ok(detail)
    }

    pub fn cleanup_pending(
        intent: &WorkloadSagaIntent,
        last_safe_phase: WorkloadSagaPhase,
        retained_references: WorkloadEffectReferences,
        inspections: Vec<WorkloadInspectionRequirement>,
    ) -> Result<Self, WorkloadSagaError> {
        let detail = Self::CleanupPending(WorkloadCleanupPendingDetail {
            last_safe_phase,
            retained_references,
            inspections,
        });
        validate_phase_detail(WorkloadSagaPhase::CleanupPending, intent, &detail)?;
        Ok(detail)
    }

    pub fn recorded(
        intent: &WorkloadSagaIntent,
        terminal_evidence_digest: WorkloadTerminalEvidenceDigest,
        terminal_execution: Option<WorkloadExecutionReference>,
    ) -> Self {
        Self::Recorded(WorkloadRecordedDetail {
            completed_generation: intent.generation,
            desired_digest: intent.desired_digest,
            terminal_evidence_digest,
            terminal_execution,
        })
    }

    pub fn references(&self) -> WorkloadEffectReferences {
        match self {
            Self::Intent | Self::Recorded(_) => WorkloadEffectReferences::default(),
            Self::Provision(detail) => detail.references.clone(),
            Self::Teardown(detail) => detail.retained_references.clone(),
            Self::CleanupPending(detail) => detail.retained_references.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadFailureEvidence {
    code: String,
    redacted_evidence_digest: WorkloadOwnerEvidenceDigest,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WorkloadFailureEvidenceWire {
    code: String,
    redacted_evidence_digest: WorkloadOwnerEvidenceDigest,
}

impl<'de> Deserialize<'de> for WorkloadFailureEvidence {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WorkloadFailureEvidenceWire::deserialize(deserializer)?;
        Self::new(wire.code, wire.redacted_evidence_digest).map_err(serde::de::Error::custom)
    }
}

impl WorkloadFailureEvidence {
    pub fn new(
        code: impl Into<String>,
        redacted_evidence_digest: WorkloadOwnerEvidenceDigest,
    ) -> Result<Self, WorkloadSagaError> {
        let code = code.into();
        let failure = Self {
            code,
            redacted_evidence_digest,
        };
        failure.validate()?;
        Ok(failure)
    }

    pub(super) fn validate(&self) -> Result<(), WorkloadSagaError> {
        if self.code.is_empty()
            || self.code.len() > 96
            || self
                .code
                .bytes()
                .any(|byte| !byte.is_ascii_lowercase() && !byte.is_ascii_digit() && byte != b'_')
        {
            return Err(WorkloadSagaError::InvalidEvidence(
                "failure code must be 1-96 lowercase ASCII letters, digits, or underscores",
            ));
        }
        Ok(())
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn redacted_evidence_digest(&self) -> WorkloadOwnerEvidenceDigest {
        self.redacted_evidence_digest
    }
}
mod executable;
mod provision;
mod restart;
mod state;
mod teardown;

mod network;
#[cfg(test)]
pub(crate) mod test_support;

pub use executable::{
    MAX_WORKLOAD_EXECUTABLE_CONTENT_BYTES, WORKLOAD_EXECUTABLE_FORMAT_VERSION,
    WorkloadExecutableEncoding, WorkloadExecutableIntent,
};
pub use network::{WorkloadNetworkIntent, WorkloadNetworkReference};
pub use provision::{
    WorkloadExecutionProviderId, WorkloadProvisionAbsenceEvidence, WorkloadProvisionAbsenceOrigin,
    WorkloadProvisionAttempt, WorkloadProvisionAttemptId, WorkloadProvisionAttemptInput,
    WorkloadProvisionCommandId, WorkloadProvisionCommandMode,
    WorkloadProvisionDispatchAuthorization, WorkloadProvisionDispatchClaim,
    WorkloadProvisionDispatchEpoch, WorkloadProvisionDisposition, WorkloadProvisionEffectResult,
    WorkloadProvisionInspectionResult, WorkloadProvisionPrerequisiteEvidence,
    WorkloadProvisionProviderTarget, WorkloadProvisionRepublicationRetryEvidence,
    WorkloadProvisionSourceDigest, WorkloadProvisionSourceEvidence,
    WorkloadProvisionSourceGeneration, WorkloadProvisionSourceIdentity,
    WorkloadProvisionSourceKind, WorkloadProvisionSourceResourceVersion, WorkloadProvisionStep,
    WorkloadProvisionSubjects, WorkloadProvisionSuccessEvidence,
};
pub use restart::{
    ActiveWorkloadRestart, MAX_WORKLOAD_RESTART_COMPLETION_HISTORY, WorkloadExecutionAttemptId,
    WorkloadInspectionVersion, WorkloadRestartAbsenceEvidence, WorkloadRestartAdmission,
    WorkloadRestartAdmissionInput, WorkloadRestartAdmissionUpdate, WorkloadRestartCommandClaim,
    WorkloadRestartCommandId, WorkloadRestartCommandReceipt, WorkloadRestartDispatchAuthorization,
    WorkloadRestartDispatchEpoch, WorkloadRestartDisposition, WorkloadRestartEffectResult,
    WorkloadRestartEpoch, WorkloadRestartEvidenceDigest, WorkloadRestartHistory,
    WorkloadRestartNotBeforeUnixMillis, WorkloadRestartPhase, WorkloadRestartPolicy,
    WorkloadRestartRecoveryDecision, WorkloadRestartRequestId, WorkloadRestartState,
    WorkloadRestartStep, WorkloadRestartTrigger,
};
use state::validate_phase_detail;
pub use state::{WorkloadSagaIntentUpdate, WorkloadSagaRecord, WorkloadSagaTransition};
pub use teardown::{
    ProposedWorkloadTeardownTransition, WorkloadProvisionTeardownAbsence,
    WorkloadRestartTeardownSettlement, WorkloadTeardownAttempt, WorkloadTeardownAttemptId,
    WorkloadTeardownAttemptInput, WorkloadTeardownCause, WorkloadTeardownClaim,
    WorkloadTeardownCommandId, WorkloadTeardownCommandMode, WorkloadTeardownContext,
    WorkloadTeardownDecision, WorkloadTeardownDispatchAuthorization, WorkloadTeardownDispatchEpoch,
    WorkloadTeardownDisposition, WorkloadTeardownEffectResult, WorkloadTeardownInspectionResult,
    WorkloadTeardownProviderTarget, WorkloadTeardownReceipt, WorkloadTeardownReceiptPrefix,
    WorkloadTeardownResultConfirmation, WorkloadTeardownRetryEvidence, WorkloadTeardownStep,
    WorkloadTeardownSubjects, WorkloadTeardownSuccessEvidence, WorkloadTeardownSuccessorFence,
};
#[cfg(test)]
#[path = "saga/tests.rs"]
mod tests;
