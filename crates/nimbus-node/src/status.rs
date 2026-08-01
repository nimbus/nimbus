use std::collections::BTreeMap;

use nimbus_core::{Error, Result, non_empty};
use nimbus_tenant::{
    TenantIsolationDecisionId, TenantIsolationEvent, TenantIsolationEventKind,
    TenantIsolationEventResult, TenantIsolationEventValue,
};
use nimbus_workloads::{
    NodeIdentity, TenantFinalizerRecord, TenantSystemEvidenceProjection, TenantWorkloadSpec,
    TenantWorkloadUid, WorkloadExecutionId, WorkloadGeneration,
};
use serde::Serialize;

use crate::host_lifecycle::{
    HostLifecycleBackendCapabilities, HostLifecycleJournalSelectorEvidence,
    TenantWorkloadLifecycleEvidence,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TenantObservedResourceUsage {
    pub active_sandboxes: u64,
    pub cpu_millis: u64,
    pub memory_bytes: u64,
    pub disk_bytes: u64,
    pub log_bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TenantNodeObservationIds {
    node_lease_id: Option<String>,
    heartbeat_id: Option<String>,
}

impl TenantNodeObservationIds {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_node_lease_id(mut self, id: impl Into<String>) -> Result<Self> {
        self.node_lease_id = Some(evidence_id(id, "node lease id")?);
        Ok(self)
    }

    pub fn with_heartbeat_id(mut self, id: impl Into<String>) -> Result<Self> {
        self.heartbeat_id = Some(evidence_id(id, "node heartbeat id")?);
        Ok(self)
    }

    pub fn node_lease_id(&self) -> Option<&str> {
        self.node_lease_id.as_deref()
    }

    pub fn heartbeat_id(&self) -> Option<&str> {
        self.heartbeat_id.as_deref()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TenantWorkloadCleanupProgress {
    finalizers_pending: Vec<TenantFinalizerRecord>,
    finalizers_completed: Vec<TenantFinalizerRecord>,
    retained_bytes: u64,
}

impl TenantWorkloadCleanupProgress {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_pending_finalizers(
        mut self,
        finalizers: impl IntoIterator<Item = TenantFinalizerRecord>,
    ) -> Self {
        self.finalizers_pending = finalizers.into_iter().collect();
        self
    }

    pub fn with_completed_finalizers(
        mut self,
        finalizers: impl IntoIterator<Item = TenantFinalizerRecord>,
    ) -> Self {
        self.finalizers_completed = finalizers.into_iter().collect();
        self
    }

    pub fn with_retained_bytes(mut self, retained_bytes: u64) -> Self {
        self.retained_bytes = retained_bytes;
        self
    }

    pub fn retained_bytes(&self) -> u64 {
        self.retained_bytes
    }

    pub fn finalizers_pending(&self) -> &[TenantFinalizerRecord] {
        &self.finalizers_pending
    }

    pub fn finalizers_completed(&self) -> &[TenantFinalizerRecord] {
        &self.finalizers_completed
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

impl TenantWorkloadPhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Bound => "bound",
            Self::Running => "running",
            Self::Ready => "ready",
            Self::Deleting => "deleting",
            Self::Degraded => "degraded",
            Self::Denied => "denied",
        }
    }
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TenantWorkloadDiagnostics {
    backend_capabilities: Vec<HostLifecycleBackendCapabilities>,
    actionable_failure_reasons: Vec<String>,
}

impl TenantWorkloadDiagnostics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_backend_capabilities(
        mut self,
        capabilities: impl IntoIterator<Item = HostLifecycleBackendCapabilities>,
    ) -> Self {
        self.backend_capabilities = capabilities.into_iter().collect();
        self
    }

    pub fn with_actionable_failure_reason(mut self, reason: impl Into<String>) -> Result<Self> {
        self.actionable_failure_reasons
            .push(non_empty(reason, "diagnostic failure reason")?);
        Ok(self)
    }

    pub fn backend_capabilities(&self) -> &[HostLifecycleBackendCapabilities] {
        &self.backend_capabilities
    }

    pub fn actionable_failure_reasons(&self) -> &[String] {
        &self.actionable_failure_reasons
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

    pub fn label(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Lease => "lease",
            Self::Heartbeat => "heartbeat",
            Self::Logs => "logs",
            Self::Diagnostics => "diagnostics",
            Self::Evidence => "evidence",
            Self::CleanupProgress => "cleanup_progress",
            Self::Spec => "spec",
            Self::Labels => "labels",
            Self::Policy => "policy",
            Self::Grants => "grants",
            Self::QuotaHardLimits => "quota_hard_limits",
            Self::Placement => "placement",
            Self::Credentials => "credentials",
            Self::Admission => "admission",
            Self::DeletionAuthority => "deletion_authority",
            Self::UserData => "user_data",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantWorkloadStatusPatch {
    workload_uid: TenantWorkloadUid,
    observed_generation: WorkloadGeneration,
    decision_id: TenantIsolationDecisionId,
    writer_node_id: Option<NodeIdentity>,
    target: TenantWorkloadStatusPatchTarget,
    phase: TenantWorkloadPhase,
    conditions: Vec<TenantWorkloadCondition>,
    observed_usage: TenantObservedResourceUsage,
    node_observation_ids: TenantNodeObservationIds,
    lifecycle_evidence: Option<TenantWorkloadLifecycleEvidence>,
    cleanup_progress: Option<TenantWorkloadCleanupProgress>,
    diagnostics: TenantWorkloadDiagnostics,
    evidence_correlation_ids: Vec<String>,
}

impl TenantWorkloadStatusPatch {
    pub fn for_target(spec: &TenantWorkloadSpec, target: TenantWorkloadStatusPatchTarget) -> Self {
        Self {
            workload_uid: spec.workload_uid().clone(),
            observed_generation: spec.generation(),
            decision_id: spec.decision_id().clone(),
            writer_node_id: spec.assigned_node_id().cloned(),
            target,
            phase: TenantWorkloadPhase::Pending,
            conditions: Vec::new(),
            observed_usage: TenantObservedResourceUsage::default(),
            node_observation_ids: TenantNodeObservationIds::default(),
            lifecycle_evidence: None,
            cleanup_progress: None,
            diagnostics: TenantWorkloadDiagnostics::default(),
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

    pub fn with_observed_generation(mut self, generation: WorkloadGeneration) -> Self {
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

    pub fn with_node_observation_ids(mut self, ids: TenantNodeObservationIds) -> Self {
        self.node_observation_ids = ids;
        self
    }

    pub fn with_lifecycle_evidence(mut self, evidence: TenantWorkloadLifecycleEvidence) -> Self {
        self.lifecycle_evidence = Some(evidence);
        self
    }

    pub fn with_cleanup_progress(mut self, progress: TenantWorkloadCleanupProgress) -> Self {
        self.cleanup_progress = Some(progress);
        self
    }

    pub fn with_diagnostics(mut self, diagnostics: TenantWorkloadDiagnostics) -> Self {
        self.diagnostics = diagnostics;
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
    observed_generation: WorkloadGeneration,
    decision_id: TenantIsolationDecisionId,
    writer_node_id: NodeIdentity,
    target: TenantWorkloadStatusPatchTarget,
    phase: TenantWorkloadPhase,
    conditions: Vec<TenantWorkloadCondition>,
    observed_usage: TenantObservedResourceUsage,
    node_observation_ids: TenantNodeObservationIds,
    lifecycle_evidence: Option<TenantWorkloadLifecycleEvidence>,
    cleanup_progress: Option<TenantWorkloadCleanupProgress>,
    diagnostics: TenantWorkloadDiagnostics,
    evidence_correlation_ids: Vec<String>,
}

impl TenantWorkloadStatus {
    pub fn workload_uid(&self) -> &TenantWorkloadUid {
        &self.workload_uid
    }

    pub fn observed_generation(&self) -> WorkloadGeneration {
        self.observed_generation
    }

    pub fn execution_id(&self) -> WorkloadExecutionId {
        WorkloadExecutionId::for_execution(
            &self.workload_uid,
            &self.writer_node_id,
            self.observed_generation,
        )
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

    pub fn node_observation_ids(&self) -> &TenantNodeObservationIds {
        &self.node_observation_ids
    }

    pub fn lifecycle_evidence(&self) -> Option<&TenantWorkloadLifecycleEvidence> {
        self.lifecycle_evidence.as_ref()
    }

    pub fn cleanup_progress(&self) -> Option<&TenantWorkloadCleanupProgress> {
        self.cleanup_progress.as_ref()
    }

    pub fn diagnostics(&self) -> &TenantWorkloadDiagnostics {
        &self.diagnostics
    }

    pub fn evidence_correlation_ids(&self) -> &[String] {
        &self.evidence_correlation_ids
    }

    pub fn metric_labels(&self) -> TenantWorkloadMetricLabels {
        TenantWorkloadMetricLabels {
            backend: self
                .lifecycle_evidence
                .as_ref()
                .map(|evidence| evidence.backend().label().to_owned())
                .unwrap_or_else(|| "none".to_string()),
            phase: self.phase.label().to_owned(),
            target: self.target.label().to_owned(),
        }
    }

    pub fn lifecycle_audit_event(
        &self,
        projection: &TenantSystemEvidenceProjection,
    ) -> TenantIsolationEvent {
        let mut event = TenantIsolationEvent::without_decision(
            TenantIsolationEventKind::LifecycleStatus,
            projection.tenant_id().as_str(),
            projection.surface(),
            "system",
            TenantIsolationEventResult::Observed,
            "node_status_observed",
        )
        .with_admitted_decision_id(&self.decision_id)
        .with_attribute("observed_generation", self.observed_generation.as_u64())
        .with_attribute("phase", self.phase.label())
        .with_attribute("target", self.target.label())
        .with_attribute("writer_node_id", self.writer_node_id.as_str());

        if let Some(node_lease_id) = self.node_observation_ids.node_lease_id() {
            event = event.with_attribute("node_lease_id", node_lease_id);
        }
        if let Some(heartbeat_id) = self.node_observation_ids.heartbeat_id() {
            event = event.with_attribute("heartbeat_id", heartbeat_id);
        }
        if let Some(lifecycle) = &self.lifecycle_evidence {
            event = event
                .with_attribute("host_lifecycle_backend", lifecycle.backend().label())
                .with_attribute("host_unit_name", lifecycle.unit_name());
            if let Some(job_path) = lifecycle.job_path() {
                event = event.with_attribute("host_job_path", job_path);
            }
            if let Some(process_id) = lifecycle.process_id() {
                event = event.with_attribute("host_process_id", process_id);
            }
            if let Some(cgroup_path) = lifecycle.cgroup_path() {
                event = event.with_attribute("host_cgroup_path", cgroup_path);
            }
            event = event.with_attribute(
                "host_journal_selectors",
                TenantIsolationEventValue::StringList(
                    lifecycle
                        .journal_selectors()
                        .iter()
                        .map(HostLifecycleJournalSelectorEvidence::label)
                        .collect(),
                ),
            );
        }
        if let Some(cleanup) = &self.cleanup_progress {
            event = event
                .with_attribute("cleanup_retained_bytes", cleanup.retained_bytes())
                .with_attribute(
                    "cleanup_pending_finalizers",
                    cleanup.finalizers_pending().len(),
                )
                .with_attribute(
                    "cleanup_completed_finalizers",
                    cleanup.finalizers_completed().len(),
                );
        }
        for correlation_id in &self.evidence_correlation_ids {
            event = event.with_correlation_id(
                format!("evidence_{}", stable_evidence_key(correlation_id)),
                correlation_id,
            );
        }
        event
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantWorkloadMetricLabels {
    backend: String,
    phase: String,
    target: String,
}

impl TenantWorkloadMetricLabels {
    pub fn backend(&self) -> &str {
        &self.backend
    }

    pub fn phase(&self) -> &str {
        &self.phase
    }

    pub fn target(&self) -> &str {
        &self.target
    }
}

pub fn ensure_status_matches_projection(
    projection: &TenantSystemEvidenceProjection,
    status: &TenantWorkloadStatus,
) -> Result<()> {
    if status.workload_uid() != projection.workload_uid() {
        return Err(Error::PermissionDenied(format!(
            "system evidence projection references workload {}, but status is for {}",
            projection.workload_uid().as_str(),
            status.workload_uid().as_str()
        )));
    }
    if status.decision_id() != projection.decision_id() {
        return Err(Error::PermissionDenied(format!(
            "system evidence projection references decision {}, but status is for {}",
            projection.decision_id().as_str(),
            status.decision_id().as_str()
        )));
    }
    if status.observed_generation() != projection.generation() {
        return Err(Error::PermissionDenied(format!(
            "system evidence projection references generation {}, but status observed generation {}",
            projection.generation().as_u64(),
            status.observed_generation().as_u64()
        )));
    }
    Ok(())
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
        if patch.cleanup_progress.is_some()
            && patch.target != TenantWorkloadStatusPatchTarget::CleanupProgress
        {
            return Err(Error::PermissionDenied(
                "cleanup progress is observed cleanup status, not general status".to_string(),
            ));
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
                spec.workload_uid().as_str()
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
            node_observation_ids: patch.node_observation_ids,
            lifecycle_evidence: patch.lifecycle_evidence,
            cleanup_progress: patch.cleanup_progress,
            diagnostics: patch.diagnostics,
            evidence_correlation_ids: patch.evidence_correlation_ids,
        })
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

fn evidence_id(value: impl Into<String>, field: &str) -> Result<String> {
    let value = non_empty(value, field)?;
    if value.contains('\0') || value.contains('\n') {
        return Err(Error::InvalidInput(format!(
            "{field} must not contain control characters"
        )));
    }
    Ok(value)
}

fn stable_evidence_key(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').chars().take(32).collect::<String>();
    if out.is_empty() {
        "id".to_string()
    } else {
        out
    }
}
