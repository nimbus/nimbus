use nimbus_tenant::{TenantIsolationDecisionId, WorkloadIdentity};
use serde::Serialize;

/// Serializable workload credential claims.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CredentialClaims {
    #[serde(rename = "sub")]
    sub: String,
    #[serde(rename = "aud")]
    aud: String,
    #[serde(rename = "exp")]
    exp_epoch_ms: u64,
    #[serde(rename = "jti")]
    jti: String,
    nimbus_decision_id: String,
    nimbus_workload_subject: String,
    nimbus_workload_audit_projection: String,
    nimbus_node_id: Option<String>,
    nimbus_machine_id: Option<String>,
    nimbus_sandbox_id: Option<String>,
    nimbus_invocation_id: Option<String>,
}

impl CredentialClaims {
    pub(crate) fn new(
        identity: &WorkloadIdentity,
        decision_id: &TenantIsolationDecisionId,
        audience: String,
        exp_epoch_ms: u64,
        credential_instance_id: String,
    ) -> Self {
        let subject = identity.subject();
        Self {
            sub: subject.clone(),
            aud: audience,
            exp_epoch_ms,
            jti: credential_instance_id,
            nimbus_decision_id: decision_id.as_str().to_string(),
            nimbus_workload_subject: subject,
            nimbus_workload_audit_projection: identity.audit_projection(),
            nimbus_node_id: identity.node_id().map(str::to_string),
            nimbus_machine_id: identity.machine_id().map(str::to_string),
            nimbus_sandbox_id: identity.sandbox_id().map(str::to_string),
            nimbus_invocation_id: identity.invocation_id().map(str::to_string),
        }
    }

    pub fn sub(&self) -> &str {
        &self.sub
    }

    pub fn aud(&self) -> &str {
        &self.aud
    }

    pub fn exp_epoch_ms(&self) -> u64 {
        self.exp_epoch_ms
    }

    pub fn jti(&self) -> &str {
        &self.jti
    }

    pub fn nimbus_decision_id(&self) -> &str {
        &self.nimbus_decision_id
    }

    pub fn nimbus_workload_subject(&self) -> &str {
        &self.nimbus_workload_subject
    }

    pub fn nimbus_workload_audit_projection(&self) -> &str {
        &self.nimbus_workload_audit_projection
    }

    pub fn nimbus_node_id(&self) -> Option<&str> {
        self.nimbus_node_id.as_deref()
    }

    pub fn nimbus_machine_id(&self) -> Option<&str> {
        self.nimbus_machine_id.as_deref()
    }

    pub fn nimbus_sandbox_id(&self) -> Option<&str> {
        self.nimbus_sandbox_id.as_deref()
    }

    pub fn nimbus_invocation_id(&self) -> Option<&str> {
        self.nimbus_invocation_id.as_deref()
    }
}
