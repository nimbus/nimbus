use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IdentityAuditEvent {
    tenant_id: String,
    decision_id: String,
    workload_subject: String,
    workload_audit_projection: String,
    audience: String,
    outcome: IdentityAuditOutcome,
    exp_epoch_ms: Option<u64>,
    credential_instance_id: Option<String>,
}

impl IdentityAuditEvent {
    pub(crate) fn from_parts(parts: IdentityAuditEventParts) -> Self {
        Self {
            tenant_id: parts.tenant_id,
            decision_id: parts.decision_id,
            workload_subject: parts.workload_subject,
            workload_audit_projection: parts.workload_audit_projection,
            audience: parts.audience,
            outcome: parts.outcome,
            exp_epoch_ms: parts.exp_epoch_ms,
            credential_instance_id: parts.credential_instance_id,
        }
    }

    pub fn tenant_id(&self) -> &str {
        &self.tenant_id
    }

    pub fn decision_id(&self) -> &str {
        &self.decision_id
    }

    pub fn workload_subject(&self) -> &str {
        &self.workload_subject
    }

    pub fn workload_audit_projection(&self) -> &str {
        &self.workload_audit_projection
    }

    pub fn audience(&self) -> &str {
        &self.audience
    }

    pub fn outcome(&self) -> &IdentityAuditOutcome {
        &self.outcome
    }

    pub fn exp_epoch_ms(&self) -> Option<u64> {
        self.exp_epoch_ms
    }

    pub fn credential_instance_id(&self) -> Option<&str> {
        self.credential_instance_id.as_deref()
    }
}

pub(crate) struct IdentityAuditEventParts {
    pub(crate) tenant_id: String,
    pub(crate) decision_id: String,
    pub(crate) workload_subject: String,
    pub(crate) workload_audit_projection: String,
    pub(crate) audience: String,
    pub(crate) outcome: IdentityAuditOutcome,
    pub(crate) exp_epoch_ms: Option<u64>,
    pub(crate) credential_instance_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "result")]
pub enum IdentityAuditOutcome {
    Minted,
    Denied { reason: String },
}
