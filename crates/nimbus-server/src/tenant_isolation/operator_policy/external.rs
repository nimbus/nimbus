use nimbus_core::{Error, Result};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperatorExternalPolicyRequest {
    pub policy_name: Option<String>,
    pub tenant_id: String,
    pub workload_key: String,
    pub decision_id: String,
    pub workload_kind: String,
    pub workload_name: String,
    pub runtime_tier: String,
    pub tenant_isolation_mode: String,
    pub runtime_admission: String,
    pub sandbox_backend: Option<String>,
    pub sandbox_id: Option<String>,
    pub services: Vec<String>,
    pub network_endpoints: Vec<String>,
    pub sandbox_egress: Vec<String>,
    pub storage_namespace: String,
    pub named_volumes: Vec<String>,
    pub image_reference: Option<String>,
    pub secret_handle_count: usize,
    pub audit_redactions: Vec<String>,
}

pub trait OperatorExternalPolicyBackend: Send + Sync {
    fn evaluate(
        &self,
        request: &OperatorExternalPolicyRequest,
    ) -> OperatorExternalPolicyBackendResult<OperatorExternalPolicyDecision>;
}

pub type OperatorExternalPolicyBackendResult<T> =
    std::result::Result<T, OperatorExternalPolicyBackendError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperatorExternalPolicyDecision {
    pub backend: OperatorExternalPolicyBackendIdentity,
    pub outcome: OperatorExternalPolicyOutcome,
    pub reason: String,
}

impl OperatorExternalPolicyDecision {
    pub fn allow(
        backend_name: impl Into<String>,
        backend_version: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            backend: OperatorExternalPolicyBackendIdentity::new(backend_name, backend_version),
            outcome: OperatorExternalPolicyOutcome::Allow,
            reason: reason.into(),
        }
    }

    pub fn deny(
        backend_name: impl Into<String>,
        backend_version: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            backend: OperatorExternalPolicyBackendIdentity::new(backend_name, backend_version),
            outcome: OperatorExternalPolicyOutcome::Deny,
            reason: reason.into(),
        }
    }

    pub fn into_evidence(
        self,
    ) -> OperatorExternalPolicyBackendResult<OperatorExternalPolicyEvidence> {
        self.backend.validate()?;
        if self.reason.trim().is_empty() {
            return Err(OperatorExternalPolicyBackendError::malformed_output(
                "external policy decision reason cannot be empty",
            ));
        }
        Ok(OperatorExternalPolicyEvidence {
            backend: self.backend,
            outcome: self.outcome,
            reason: self.reason,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperatorExternalPolicyEvidence {
    pub backend: OperatorExternalPolicyBackendIdentity,
    pub outcome: OperatorExternalPolicyOutcome,
    pub reason: String,
}

impl OperatorExternalPolicyEvidence {
    pub fn summary(&self) -> String {
        format!(
            "{} via {}@{} ({})",
            self.outcome.label(),
            self.backend.name,
            self.backend.version,
            self.reason
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperatorExternalPolicyBackendIdentity {
    pub name: String,
    pub version: String,
}

impl OperatorExternalPolicyBackendIdentity {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }

    fn validate(&self) -> OperatorExternalPolicyBackendResult<()> {
        if self.name.trim().is_empty() {
            return Err(OperatorExternalPolicyBackendError::malformed_output(
                "external policy backend name cannot be empty",
            ));
        }
        if self.version.trim().is_empty() {
            return Err(OperatorExternalPolicyBackendError::malformed_output(
                "external policy backend version cannot be empty",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorExternalPolicyOutcome {
    Allow,
    Deny,
}

impl OperatorExternalPolicyOutcome {
    pub fn label(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorExternalPolicyBackendError {
    pub kind: OperatorExternalPolicyBackendErrorKind,
    pub message: String,
}

impl OperatorExternalPolicyBackendError {
    pub fn malformed_output(message: impl Into<String>) -> Self {
        Self {
            kind: OperatorExternalPolicyBackendErrorKind::MalformedOutput,
            message: message.into(),
        }
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self {
            kind: OperatorExternalPolicyBackendErrorKind::Timeout,
            message: message.into(),
        }
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            kind: OperatorExternalPolicyBackendErrorKind::Unavailable,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for OperatorExternalPolicyBackendError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.kind.label(), self.message)
    }
}

impl std::error::Error for OperatorExternalPolicyBackendError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorExternalPolicyBackendErrorKind {
    MalformedOutput,
    Timeout,
    Unavailable,
}

impl OperatorExternalPolicyBackendErrorKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::MalformedOutput => "malformed_output",
            Self::Timeout => "timeout",
            Self::Unavailable => "unavailable",
        }
    }
}

pub(super) fn evaluate_external_policy_backend(
    backend: &dyn OperatorExternalPolicyBackend,
    request: OperatorExternalPolicyRequest,
) -> Result<OperatorExternalPolicyEvidence> {
    let decision = backend.evaluate(&request).map_err(|error| {
        Error::InvalidInput(format!(
            "operator policy external backend failed closed for workload `{}`: {error}",
            request.workload_key
        ))
    })?;
    let evidence = decision.into_evidence().map_err(|error| {
        Error::InvalidInput(format!(
            "operator policy external backend failed closed for workload `{}`: {error}",
            request.workload_key
        ))
    })?;
    if matches!(evidence.outcome, OperatorExternalPolicyOutcome::Deny) {
        return Err(Error::InvalidInput(format!(
            "operator policy external backend `{}@{}` denied workload `{}`: {}",
            evidence.backend.name, evidence.backend.version, request.workload_key, evidence.reason
        )));
    }
    Ok(evidence)
}
