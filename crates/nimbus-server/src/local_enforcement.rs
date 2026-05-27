use std::collections::BTreeMap;

use nimbus_core::{Error, Result, TenantId};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::tenant::{
    TenantAuditRedactionPolicy, TenantIsolationDecision, TenantIsolationDecisionId,
    TenantQuotaPolicyDecision, TenantServiceAccessDecision, TenantStorageAccessDecision,
    TenantWorkloadStableIdentity,
};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct NodeIdentity(String);

impl NodeIdentity {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        Ok(Self(non_empty(value, "node identity")?))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct TenantWorkloadGeneration(u64);

impl TenantWorkloadGeneration {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct TenantWorkloadUid(String);

impl TenantWorkloadUid {
    fn for_admitted_identity(
        identity: &TenantWorkloadStableIdentity,
        decision_id: &TenantIsolationDecisionId,
    ) -> Self {
        let mut digest = Sha256::new();
        digest.update(identity.stable_id().as_bytes());
        digest.update(b"\0");
        digest.update(decision_id.as_str().as_bytes());
        Self(format!("twu_{:x}", digest.finalize()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalEnforcementBinding {
    spec: TenantWorkloadSpec,
}

impl LocalEnforcementBinding {
    pub fn from_decision(decision: &TenantIsolationDecision) -> Result<Self> {
        Ok(Self {
            spec: TenantWorkloadSpec::from_decision(decision)?,
        })
    }

    pub fn from_spec(spec: TenantWorkloadSpec) -> Self {
        Self { spec }
    }

    pub fn spec(&self) -> &TenantWorkloadSpec {
        &self.spec
    }

    pub fn storage_access(&self) -> &TenantStorageAccessDecision {
        self.spec.storage_projection.access()
    }

    pub fn service_access(&self, service_name: &str) -> Result<&TenantServiceAccessDecision> {
        self.spec.service_projection.access(service_name)
    }

    pub fn authorize_credential_projection(
        &self,
        request: &TenantCredentialProjectionRequest,
    ) -> Result<TenantCredentialProjectionBinding> {
        self.spec.ensure_request_identity(
            &request.workload_uid,
            request.generation,
            &request.decision_id,
            "credential projection",
        )?;
        if let Some(request_node) = &request.requester_node_id {
            self.spec.ensure_assigned_node_matches(
                request_node,
                "node-mediated credential projection",
            )?;
        }
        if self.spec.runtime_invocation_id.as_deref() != request.runtime_invocation_id.as_deref() {
            return Err(Error::PermissionDenied(format!(
                "credential projection for workload {} referenced invocation {:?}, but admitted invocation is {:?}",
                self.spec.workload_uid.as_str(),
                request.runtime_invocation_id.as_deref(),
                self.spec.runtime_invocation_id.as_deref()
            )));
        }
        if !request.redaction_metadata_present {
            return Err(Error::InvalidInput(
                "credential projection request is missing redaction metadata".to_string(),
            ));
        }
        if request.echo_back_subject.is_some() {
            return Err(Error::PermissionDenied(
                "credential projection request attempted to echo back a subject".to_string(),
            ));
        }
        let scope = self
            .spec
            .credential_projection
            .scope(&request.provider, &request.audience)?;
        Ok(TenantCredentialProjectionBinding {
            workload_uid: self.spec.workload_uid.clone(),
            generation: self.spec.generation,
            decision_id: self.spec.decision_id.clone(),
            scope: scope.clone(),
            subject: self.spec.workload_stable_identity.stable_id(),
            redacted_fields: self.spec.audit_redactions.redacted_fields().to_vec(),
        })
    }

    pub fn authorize_egress_reload(&self, request: &TenantEgressReloadRequest) -> Result<()> {
        self.spec.ensure_request_identity(
            &request.workload_uid,
            request.generation,
            &request.decision_id,
            "egress reload",
        )
    }

    pub fn system_evidence_projection(&self) -> TenantSystemEvidenceProjection {
        TenantSystemEvidenceProjection {
            decision_id: self.spec.decision_id.clone(),
            tenant_id: self.spec.tenant_id.clone(),
            workload_uid: self.spec.workload_uid.clone(),
            workload_stable_id: self.spec.workload_stable_identity.stable_id(),
            generation: self.spec.generation,
            redacted_fields: self.spec.audit_redactions.redacted_fields().to_vec(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantWorkloadSpec {
    decision_id: TenantIsolationDecisionId,
    tenant_id: TenantId,
    workload_stable_identity: TenantWorkloadStableIdentity,
    workload_uid: TenantWorkloadUid,
    generation: TenantWorkloadGeneration,
    assigned_node_id: Option<NodeIdentity>,
    runtime_invocation_id: Option<String>,
    storage_projection: TenantStorageProjection,
    service_projection: TenantServiceProjection,
    credential_projection: TenantCredentialProjectionPolicy,
    resources: TenantWorkloadResourcePolicy,
    deletion: TenantWorkloadDeletionState,
    audit_redactions: TenantAuditRedactionPolicy,
}

impl TenantWorkloadSpec {
    pub fn from_decision(decision: &TenantIsolationDecision) -> Result<Self> {
        let workload_stable_identity = decision.workload_stable_identity();
        let workload_uid =
            TenantWorkloadUid::for_admitted_identity(&workload_stable_identity, decision.id());
        let generation = TenantWorkloadGeneration::new(
            workload_stable_identity
                .deployment_generation()
                .unwrap_or(0),
        );
        let assigned_node_id = workload_stable_identity
            .node_id()
            .map(NodeIdentity::new)
            .transpose()?;
        let runtime_invocation_id = workload_stable_identity
            .invocation_id()
            .map(ToOwned::to_owned);
        let service_projection = TenantServiceProjection::from_decision(decision)?;
        Ok(Self {
            decision_id: decision.id().clone(),
            tenant_id: decision.tenant_id().clone(),
            workload_stable_identity,
            workload_uid,
            generation,
            assigned_node_id,
            runtime_invocation_id,
            storage_projection: TenantStorageProjection::new(decision.storage_access()),
            service_projection,
            credential_projection: TenantCredentialProjectionPolicy::default(),
            resources: TenantWorkloadResourcePolicy::new(decision.quotas().clone()),
            deletion: TenantWorkloadDeletionState::Active,
            audit_redactions: decision.audit_redactions().clone(),
        })
    }

    pub fn with_admitted_credential_scopes(
        mut self,
        scopes: impl IntoIterator<Item = TenantCredentialProjectionScope>,
    ) -> Self {
        self.credential_projection = TenantCredentialProjectionPolicy::new(scopes);
        self
    }

    pub fn mark_deleting_server_owned(
        mut self,
        finalizers: impl IntoIterator<Item = TenantFinalizerRecord>,
    ) -> Self {
        self.deletion = TenantWorkloadDeletionState::Deleting {
            finalizers: finalizers.into_iter().collect(),
        };
        self
    }

    pub fn decision_id(&self) -> &TenantIsolationDecisionId {
        &self.decision_id
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn workload_uid(&self) -> &TenantWorkloadUid {
        &self.workload_uid
    }

    pub fn workload_stable_identity(&self) -> &TenantWorkloadStableIdentity {
        &self.workload_stable_identity
    }

    pub fn generation(&self) -> TenantWorkloadGeneration {
        self.generation
    }

    pub fn assigned_node_id(&self) -> Option<&NodeIdentity> {
        self.assigned_node_id.as_ref()
    }

    pub fn storage_projection(&self) -> &TenantStorageProjection {
        &self.storage_projection
    }

    pub fn service_projection(&self) -> &TenantServiceProjection {
        &self.service_projection
    }

    pub fn credential_projection(&self) -> &TenantCredentialProjectionPolicy {
        &self.credential_projection
    }

    pub fn resources(&self) -> &TenantWorkloadResourcePolicy {
        &self.resources
    }

    pub fn deletion(&self) -> &TenantWorkloadDeletionState {
        &self.deletion
    }

    fn ensure_assigned_node_matches(&self, actual: &NodeIdentity, context: &str) -> Result<()> {
        let Some(expected) = &self.assigned_node_id else {
            return Err(Error::PermissionDenied(format!(
                "{context} targeted workload {}, but the admitted spec has no assigned node",
                self.workload_uid.as_str()
            )));
        };
        if expected == actual {
            return Ok(());
        }
        Err(Error::PermissionDenied(format!(
            "{context} targeted node {}, but workload {} is assigned to node {}",
            actual.as_str(),
            self.workload_uid.as_str(),
            expected.as_str()
        )))
    }

    fn ensure_request_identity(
        &self,
        workload_uid: &TenantWorkloadUid,
        generation: TenantWorkloadGeneration,
        decision_id: &TenantIsolationDecisionId,
        context: &str,
    ) -> Result<()> {
        if workload_uid != &self.workload_uid {
            return Err(Error::PermissionDenied(format!(
                "{context} referenced workload {}, but admitted workload is {}",
                workload_uid.as_str(),
                self.workload_uid.as_str()
            )));
        }
        if generation != self.generation {
            return Err(Error::PermissionDenied(format!(
                "{context} referenced generation {}, but admitted generation is {} for workload {}",
                generation.as_u64(),
                self.generation.as_u64(),
                self.workload_uid.as_str()
            )));
        }
        if decision_id != &self.decision_id {
            return Err(Error::PermissionDenied(format!(
                "{context} referenced decision {}, but admitted decision is {} for workload {}",
                decision_id.as_str(),
                self.decision_id.as_str(),
                self.workload_uid.as_str()
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantStorageProjection {
    access: TenantStorageAccessDecision,
}

impl TenantStorageProjection {
    pub fn new(access: TenantStorageAccessDecision) -> Self {
        Self { access }
    }

    pub fn access(&self) -> &TenantStorageAccessDecision {
        &self.access
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantServiceProjection {
    services: Vec<TenantServiceAccessDecision>,
}

impl TenantServiceProjection {
    pub fn from_decision(decision: &TenantIsolationDecision) -> Result<Self> {
        let services = decision
            .services()
            .services()
            .iter()
            .map(|service| decision.service_access(service, "local enforcement service projection"))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { services })
    }

    pub fn access(&self, service_name: &str) -> Result<&TenantServiceAccessDecision> {
        self.services
            .iter()
            .find(|service| service.service_name() == service_name)
            .ok_or_else(|| {
                Error::PermissionDenied(format!(
                    "local enforcement binding did not authorize service `{service_name}`"
                ))
            })
    }

    pub fn services(&self) -> &[TenantServiceAccessDecision] {
        &self.services
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TenantCredentialProjectionPolicy {
    scopes: Vec<TenantCredentialProjectionScope>,
}

impl TenantCredentialProjectionPolicy {
    pub fn new(scopes: impl IntoIterator<Item = TenantCredentialProjectionScope>) -> Self {
        Self {
            scopes: scopes.into_iter().collect(),
        }
    }

    pub fn scopes(&self) -> &[TenantCredentialProjectionScope] {
        &self.scopes
    }

    fn scope(&self, provider: &str, audience: &str) -> Result<&TenantCredentialProjectionScope> {
        self.scopes
            .iter()
            .find(|scope| scope.provider() == provider && scope.audience() == audience)
            .ok_or_else(|| {
                Error::PermissionDenied(format!(
                    "credential projection did not admit provider `{provider}` with audience `{audience}`"
                ))
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantCredentialProjectionScope {
    provider: String,
    audience: String,
}

impl TenantCredentialProjectionScope {
    pub fn new(provider: impl Into<String>, audience: impl Into<String>) -> Result<Self> {
        Ok(Self {
            provider: non_empty(provider, "credential provider")?,
            audience: non_empty(audience, "credential audience")?,
        })
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }

    pub fn audience(&self) -> &str {
        &self.audience
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantCredentialProjectionRequest {
    workload_uid: TenantWorkloadUid,
    generation: TenantWorkloadGeneration,
    decision_id: TenantIsolationDecisionId,
    requester_node_id: Option<NodeIdentity>,
    runtime_invocation_id: Option<String>,
    provider: String,
    audience: String,
    redaction_metadata_present: bool,
    echo_back_subject: Option<String>,
}

impl TenantCredentialProjectionRequest {
    pub fn node_mediated(
        spec: &TenantWorkloadSpec,
        requester_node_id: NodeIdentity,
        provider: impl Into<String>,
        audience: impl Into<String>,
    ) -> Result<Self> {
        Self::for_spec(spec, provider, audience).map(|request| {
            request
                .with_requester_node_id(Some(requester_node_id))
                .with_runtime_invocation_id(spec.runtime_invocation_id.clone())
        })
    }

    pub fn server_owned(
        spec: &TenantWorkloadSpec,
        provider: impl Into<String>,
        audience: impl Into<String>,
    ) -> Result<Self> {
        Self::for_spec(spec, provider, audience)
            .map(|request| request.with_runtime_invocation_id(spec.runtime_invocation_id.clone()))
    }

    fn for_spec(
        spec: &TenantWorkloadSpec,
        provider: impl Into<String>,
        audience: impl Into<String>,
    ) -> Result<Self> {
        Ok(Self {
            workload_uid: spec.workload_uid.clone(),
            generation: spec.generation,
            decision_id: spec.decision_id.clone(),
            requester_node_id: None,
            runtime_invocation_id: None,
            provider: non_empty(provider, "credential provider")?,
            audience: non_empty(audience, "credential audience")?,
            redaction_metadata_present: true,
            echo_back_subject: None,
        })
    }

    pub fn with_requester_node_id(mut self, requester_node_id: Option<NodeIdentity>) -> Self {
        self.requester_node_id = requester_node_id;
        self
    }

    pub fn with_runtime_invocation_id(mut self, runtime_invocation_id: Option<String>) -> Self {
        self.runtime_invocation_id = runtime_invocation_id;
        self
    }

    pub fn with_generation(mut self, generation: TenantWorkloadGeneration) -> Self {
        self.generation = generation;
        self
    }

    pub fn with_workload_uid(mut self, workload_uid: TenantWorkloadUid) -> Self {
        self.workload_uid = workload_uid;
        self
    }

    pub fn with_decision_id(mut self, decision_id: TenantIsolationDecisionId) -> Self {
        self.decision_id = decision_id;
        self
    }

    pub fn without_redaction_metadata(mut self) -> Self {
        self.redaction_metadata_present = false;
        self
    }

    pub fn with_echo_back_subject(mut self, subject: impl Into<String>) -> Self {
        self.echo_back_subject = Some(subject.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantCredentialProjectionBinding {
    workload_uid: TenantWorkloadUid,
    generation: TenantWorkloadGeneration,
    decision_id: TenantIsolationDecisionId,
    scope: TenantCredentialProjectionScope,
    subject: String,
    redacted_fields: Vec<String>,
}

impl TenantCredentialProjectionBinding {
    pub fn workload_uid(&self) -> &TenantWorkloadUid {
        &self.workload_uid
    }

    pub fn generation(&self) -> TenantWorkloadGeneration {
        self.generation
    }

    pub fn decision_id(&self) -> &TenantIsolationDecisionId {
        &self.decision_id
    }

    pub fn scope(&self) -> &TenantCredentialProjectionScope {
        &self.scope
    }

    pub fn subject(&self) -> &str {
        &self.subject
    }

    pub fn redacted_fields(&self) -> &[String] {
        &self.redacted_fields
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantWorkloadResourcePolicy {
    admitted_quotas: TenantQuotaPolicyDecision,
}

impl TenantWorkloadResourcePolicy {
    pub fn new(admitted_quotas: TenantQuotaPolicyDecision) -> Self {
        Self { admitted_quotas }
    }

    pub fn admitted_quotas(&self) -> &TenantQuotaPolicyDecision {
        &self.admitted_quotas
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TenantObservedResourceUsage {
    pub active_sandboxes: u64,
    pub cpu_millis: u64,
    pub memory_bytes: u64,
    pub disk_bytes: u64,
    pub log_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum TenantWorkloadDeletionState {
    Active,
    Deleting {
        finalizers: Vec<TenantFinalizerRecord>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct TenantFinalizerRecord {
    owner: String,
    key: String,
}

impl TenantFinalizerRecord {
    pub fn new(owner: impl Into<String>, key: impl Into<String>) -> Result<Self> {
        Ok(Self {
            owner: non_empty(owner, "finalizer owner")?,
            key: non_empty(key, "finalizer key")?,
        })
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn key(&self) -> &str {
        &self.key
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantWorkloadPhase {
    #[default]
    Pending,
    Bound,
    Running,
    Ready,
    Deleting,
    Degraded,
    Denied,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantWorkloadConditionType {
    Admitted,
    Bound,
    LifecyclePlanned,
    UnitSubmitted,
    Running,
    Ready,
    PolicyReloaded,
    RecreateRequired,
    Deleting,
    Degraded,
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantWorkloadConditionStatus {
    True,
    False,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantWorkloadCondition {
    condition_type: TenantWorkloadConditionType,
    status: TenantWorkloadConditionStatus,
    reason: String,
    message: Option<String>,
}

impl TenantWorkloadCondition {
    pub fn new(
        condition_type: TenantWorkloadConditionType,
        status: TenantWorkloadConditionStatus,
        reason: impl Into<String>,
    ) -> Result<Self> {
        Ok(Self {
            condition_type,
            status,
            reason: non_empty(reason, "condition reason")?,
            message: None,
        })
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Result<Self> {
        self.message = Some(non_empty(message, "condition message")?);
        Ok(self)
    }

    pub fn condition_type(&self) -> &TenantWorkloadConditionType {
        &self.condition_type
    }

    pub fn status(&self) -> TenantWorkloadConditionStatus {
        self.status
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantWorkloadStatusPatchTarget {
    Status,
    Lease,
    Heartbeat,
    Logs,
    Diagnostics,
    Evidence,
    CleanupProgress,
    Spec,
    Labels,
    Policy,
    Grants,
    QuotaHardLimits,
    Placement,
    Credentials,
    Admission,
    DeletionAuthority,
    UserData,
}

impl TenantWorkloadStatusPatchTarget {
    fn is_observed_only(self) -> bool {
        matches!(
            self,
            Self::Status
                | Self::Lease
                | Self::Heartbeat
                | Self::Logs
                | Self::Diagnostics
                | Self::Evidence
                | Self::CleanupProgress
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantWorkloadStatusPatch {
    workload_uid: TenantWorkloadUid,
    observed_generation: TenantWorkloadGeneration,
    decision_id: TenantIsolationDecisionId,
    writer_node_id: Option<NodeIdentity>,
    target: TenantWorkloadStatusPatchTarget,
    phase: TenantWorkloadPhase,
    conditions: Vec<TenantWorkloadCondition>,
    observed_usage: TenantObservedResourceUsage,
    evidence_correlation_ids: Vec<String>,
}

impl TenantWorkloadStatusPatch {
    pub fn for_target(spec: &TenantWorkloadSpec, target: TenantWorkloadStatusPatchTarget) -> Self {
        Self {
            workload_uid: spec.workload_uid.clone(),
            observed_generation: spec.generation,
            decision_id: spec.decision_id.clone(),
            writer_node_id: spec.assigned_node_id.clone(),
            target,
            phase: TenantWorkloadPhase::Pending,
            conditions: Vec::new(),
            observed_usage: TenantObservedResourceUsage::default(),
            evidence_correlation_ids: Vec::new(),
        }
    }

    pub fn observed_status(spec: &TenantWorkloadSpec) -> Self {
        Self::for_target(spec, TenantWorkloadStatusPatchTarget::Status)
    }

    pub fn with_writer_node_id(mut self, writer_node_id: Option<NodeIdentity>) -> Self {
        self.writer_node_id = writer_node_id;
        self
    }

    pub fn with_workload_uid(mut self, workload_uid: TenantWorkloadUid) -> Self {
        self.workload_uid = workload_uid;
        self
    }

    pub fn with_observed_generation(mut self, generation: TenantWorkloadGeneration) -> Self {
        self.observed_generation = generation;
        self
    }

    pub fn with_decision_id(mut self, decision_id: TenantIsolationDecisionId) -> Self {
        self.decision_id = decision_id;
        self
    }

    pub fn with_phase(mut self, phase: TenantWorkloadPhase) -> Self {
        self.phase = phase;
        self
    }

    pub fn with_conditions(
        mut self,
        conditions: impl IntoIterator<Item = TenantWorkloadCondition>,
    ) -> Self {
        self.conditions = conditions.into_iter().collect();
        self
    }

    pub fn with_observed_usage(mut self, usage: TenantObservedResourceUsage) -> Self {
        self.observed_usage = usage;
        self
    }

    pub fn with_evidence_correlation_ids(
        mut self,
        ids: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.evidence_correlation_ids = ids.into_iter().map(Into::into).collect();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantWorkloadStatus {
    workload_uid: TenantWorkloadUid,
    observed_generation: TenantWorkloadGeneration,
    decision_id: TenantIsolationDecisionId,
    writer_node_id: NodeIdentity,
    target: TenantWorkloadStatusPatchTarget,
    phase: TenantWorkloadPhase,
    conditions: Vec<TenantWorkloadCondition>,
    observed_usage: TenantObservedResourceUsage,
    evidence_correlation_ids: Vec<String>,
}

impl TenantWorkloadStatus {
    pub fn workload_uid(&self) -> &TenantWorkloadUid {
        &self.workload_uid
    }

    pub fn observed_generation(&self) -> TenantWorkloadGeneration {
        self.observed_generation
    }

    pub fn decision_id(&self) -> &TenantIsolationDecisionId {
        &self.decision_id
    }

    pub fn writer_node_id(&self) -> &NodeIdentity {
        &self.writer_node_id
    }

    pub fn target(&self) -> TenantWorkloadStatusPatchTarget {
        self.target
    }

    pub fn phase(&self) -> TenantWorkloadPhase {
        self.phase
    }

    pub fn conditions(&self) -> &[TenantWorkloadCondition] {
        &self.conditions
    }

    pub fn observed_usage(&self) -> &TenantObservedResourceUsage {
        &self.observed_usage
    }

    pub fn evidence_correlation_ids(&self) -> &[String] {
        &self.evidence_correlation_ids
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NodeStatusAuthorizer;

impl NodeStatusAuthorizer {
    pub fn authorize(
        &self,
        spec: &TenantWorkloadSpec,
        patch: TenantWorkloadStatusPatch,
    ) -> Result<TenantWorkloadStatus> {
        if !patch.target.is_observed_only() {
            return Err(Error::PermissionDenied(format!(
                "node status patch target {:?} is desired state, not observed status",
                patch.target
            )));
        }
        spec.ensure_request_identity(
            &patch.workload_uid,
            patch.observed_generation,
            &patch.decision_id,
            "node status patch",
        )?;
        let Some(writer_node_id) = patch.writer_node_id else {
            return Err(Error::PermissionDenied(format!(
                "node status patch for workload {} did not include a writer node",
                spec.workload_uid.as_str()
            )));
        };
        spec.ensure_assigned_node_matches(&writer_node_id, "node status patch")?;
        Ok(TenantWorkloadStatus {
            workload_uid: patch.workload_uid,
            observed_generation: patch.observed_generation,
            decision_id: patch.decision_id,
            writer_node_id,
            target: patch.target,
            phase: patch.phase,
            conditions: merge_conditions_by_type(patch.conditions),
            observed_usage: patch.observed_usage,
            evidence_correlation_ids: patch.evidence_correlation_ids,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantEgressReloadRequest {
    workload_uid: TenantWorkloadUid,
    generation: TenantWorkloadGeneration,
    decision_id: TenantIsolationDecisionId,
}

impl TenantEgressReloadRequest {
    pub fn for_spec(spec: &TenantWorkloadSpec) -> Self {
        Self {
            workload_uid: spec.workload_uid.clone(),
            generation: spec.generation,
            decision_id: spec.decision_id.clone(),
        }
    }

    pub fn with_decision_id(mut self, decision_id: TenantIsolationDecisionId) -> Self {
        self.decision_id = decision_id;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantPolicyArea {
    Filesystem,
    UidGid,
    Capabilities,
    Devices,
    RuntimeBackend,
    Placement,
    StorageNamespace,
    HostBridgeGrants,
    EgressProxyRules,
    CredentialProjection,
    DeletionFinalizerState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantPolicyLifecycle {
    DynamicReload,
    RecreateRequired,
    ServerOwnedTransition,
}

pub fn policy_lifecycle(area: TenantPolicyArea) -> TenantPolicyLifecycle {
    match area {
        TenantPolicyArea::EgressProxyRules
        | TenantPolicyArea::HostBridgeGrants
        | TenantPolicyArea::CredentialProjection => TenantPolicyLifecycle::DynamicReload,
        TenantPolicyArea::DeletionFinalizerState => TenantPolicyLifecycle::ServerOwnedTransition,
        TenantPolicyArea::Filesystem
        | TenantPolicyArea::UidGid
        | TenantPolicyArea::Capabilities
        | TenantPolicyArea::Devices
        | TenantPolicyArea::RuntimeBackend
        | TenantPolicyArea::Placement
        | TenantPolicyArea::StorageNamespace => TenantPolicyLifecycle::RecreateRequired,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantSystemEvidenceProjection {
    decision_id: TenantIsolationDecisionId,
    tenant_id: TenantId,
    workload_uid: TenantWorkloadUid,
    workload_stable_id: String,
    generation: TenantWorkloadGeneration,
    redacted_fields: Vec<String>,
}

impl TenantSystemEvidenceProjection {
    pub fn decision_id(&self) -> &TenantIsolationDecisionId {
        &self.decision_id
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn workload_uid(&self) -> &TenantWorkloadUid {
        &self.workload_uid
    }

    pub fn workload_stable_id(&self) -> &str {
        &self.workload_stable_id
    }

    pub fn generation(&self) -> TenantWorkloadGeneration {
        self.generation
    }

    pub fn redacted_fields(&self) -> &[String] {
        &self.redacted_fields
    }
}

fn merge_conditions_by_type(
    conditions: impl IntoIterator<Item = TenantWorkloadCondition>,
) -> Vec<TenantWorkloadCondition> {
    let mut by_type = BTreeMap::new();
    for condition in conditions {
        by_type.insert(condition.condition_type.clone(), condition);
    }
    by_type.into_values().collect()
}

fn non_empty(value: impl Into<String>, field: &str) -> Result<String> {
    let value = value.into();
    if value.trim().is_empty() {
        return Err(Error::InvalidInput(format!("{field} must not be empty")));
    }
    Ok(value)
}
