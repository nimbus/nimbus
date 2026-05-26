use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use nimbus_core::{Error, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::super::evidence::redact_evidence_text;

pub const DEFAULT_OPERATOR_EXTERNAL_POLICY_TIMEOUT: Duration = Duration::from_secs(2);

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
    pub policy_bundle_hash: Option<String>,
    pub input_digest: String,
    pub timeout_millis: u64,
}

pub trait OperatorExternalPolicyBackend: Send + Sync {
    fn evaluate(
        &self,
        request: &OperatorExternalPolicyRequest,
    ) -> OperatorExternalPolicyBackendResult<OperatorExternalPolicyDecision>;
}

#[derive(Clone)]
pub struct OperatorExternalPolicyEngine {
    backend: Arc<dyn OperatorExternalPolicyBackend>,
    timeout: Duration,
    policy_bundle_hash: Option<String>,
}

impl OperatorExternalPolicyEngine {
    pub fn new(backend: impl OperatorExternalPolicyBackend + 'static) -> Self {
        Self::from_arc(Arc::new(backend))
    }

    pub fn from_arc(backend: Arc<dyn OperatorExternalPolicyBackend>) -> Self {
        Self {
            backend,
            timeout: DEFAULT_OPERATOR_EXTERNAL_POLICY_TIMEOUT,
            policy_bundle_hash: None,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Result<Self> {
        if timeout.is_zero() {
            return Err(Error::InvalidInput(
                "external policy timeout must be greater than 0".to_string(),
            ));
        }
        self.timeout = timeout;
        Ok(self)
    }

    pub fn with_policy_bundle_hash(
        mut self,
        policy_bundle_hash: impl Into<String>,
    ) -> Result<Self> {
        let policy_bundle_hash = policy_bundle_hash.into();
        if policy_bundle_hash.trim().is_empty() {
            return Err(Error::InvalidInput(
                "external policy bundle hash must be non-empty".to_string(),
            ));
        }
        self.policy_bundle_hash = Some(policy_bundle_hash);
        Ok(self)
    }
}

impl std::fmt::Debug for OperatorExternalPolicyEngine {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OperatorExternalPolicyEngine")
            .field("timeout", &self.timeout)
            .field("policy_bundle_hash", &self.policy_bundle_hash)
            .finish_non_exhaustive()
    }
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
        request: &OperatorExternalPolicyRequest,
    ) -> OperatorExternalPolicyBackendResult<OperatorExternalPolicyEvidence> {
        self.backend.validate()?;
        if self.reason.trim().is_empty() {
            return Err(OperatorExternalPolicyBackendError::malformed_output(
                "external policy decision reason cannot be empty",
            ));
        }
        let outcome = self.outcome;
        Ok(OperatorExternalPolicyEvidence {
            backend: self.backend,
            outcome,
            reason_code: outcome.reason_code().to_owned(),
            reason: redact_evidence_text(&self.reason),
            policy_bundle_hash: request.policy_bundle_hash.clone(),
            input_digest: request.input_digest.clone(),
            timeout_millis: request.timeout_millis,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperatorExternalPolicyEvidence {
    pub backend: OperatorExternalPolicyBackendIdentity,
    pub outcome: OperatorExternalPolicyOutcome,
    pub reason_code: String,
    pub reason: String,
    pub policy_bundle_hash: Option<String>,
    pub input_digest: String,
    pub timeout_millis: u64,
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

    pub fn reason_code(self) -> &'static str {
        match self {
            Self::Allow => "external_policy_allowed",
            Self::Deny => "external_policy_denied",
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
        write!(
            formatter,
            "{}: {}",
            self.kind.label(),
            redact_evidence_text(&self.message)
        )
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

impl OperatorExternalPolicyRequest {
    pub(super) fn with_engine_metadata(
        mut self,
        timeout: Duration,
        policy_bundle_hash: Option<String>,
    ) -> Result<Self> {
        self.timeout_millis = timeout_millis(timeout)?;
        self.policy_bundle_hash = policy_bundle_hash;
        self.input_digest = self.compute_input_digest()?;
        Ok(self)
    }

    fn compute_input_digest(&self) -> Result<String> {
        let mut digest_input = self.clone();
        digest_input.input_digest.clear();
        let bytes = serde_json::to_vec(&digest_input).map_err(|error| {
            Error::Serialization(format!(
                "failed to serialize external policy input: {error}"
            ))
        })?;
        Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
    }
}

pub(super) fn evaluate_external_policy_backend(
    engine: &OperatorExternalPolicyEngine,
    request: OperatorExternalPolicyRequest,
) -> Result<OperatorExternalPolicyEvidence> {
    let request =
        request.with_engine_metadata(engine.timeout, engine.policy_bundle_hash.clone())?;
    let (sender, receiver) = mpsc::sync_channel(1);
    let backend = Arc::clone(&engine.backend);
    let request_for_backend = request.clone();
    thread::Builder::new()
        .name("nimbus-external-policy".to_string())
        .spawn(move || {
            let _ = sender.send(backend.evaluate(&request_for_backend));
        })
        .map_err(|error| {
            Error::InvalidInput(format!(
                "operator policy external backend failed closed for workload `{}`: unavailable: failed to spawn evaluation worker: {error}",
                request.workload_key
            ))
        })?;

    let decision = match receiver.recv_timeout(engine.timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            return Err(Error::InvalidInput(format!(
                "operator policy external backend failed closed for workload `{}`: timeout: external policy backend exceeded {}ms",
                request.workload_key, request.timeout_millis
            )));
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            return Err(Error::InvalidInput(format!(
                "operator policy external backend failed closed for workload `{}`: unavailable: external policy worker exited without a decision",
                request.workload_key
            )));
        }
    }
    .map_err(|error| {
        Error::InvalidInput(format!(
            "operator policy external backend failed closed for workload `{}`: {error}",
            request.workload_key
        ))
    })?;
    let evidence = decision.into_evidence(&request).map_err(|error| {
        Error::InvalidInput(format!(
            "operator policy external backend failed closed for workload `{}`: {error}",
            request.workload_key
        ))
    })?;
    if matches!(evidence.outcome, OperatorExternalPolicyOutcome::Deny) {
        return Err(Error::InvalidInput(format!(
            "operator policy external backend `{}@{}` denied workload `{}` [{}]: {}",
            evidence.backend.name,
            evidence.backend.version,
            request.workload_key,
            evidence.reason_code,
            evidence.reason
        )));
    }
    Ok(evidence)
}

fn timeout_millis(timeout: Duration) -> Result<u64> {
    let timeout_millis: u64 = timeout.as_millis().try_into().map_err(|_| {
        Error::InvalidInput("external policy timeout is too large to render in milliseconds".into())
    })?;
    if timeout_millis == 0 {
        return Err(Error::InvalidInput(
            "external policy timeout must be at least 1ms".to_string(),
        ));
    }
    Ok(timeout_millis)
}
