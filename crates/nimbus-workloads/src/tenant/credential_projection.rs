//! Credential-projection cluster: the admitted scopes a workload may request
//! (`TenantCredentialProjectionPolicy`/`Scope`), the request a caller builds to
//! ask for one (`TenantCredentialProjectionRequest`), and the fail-closed
//! authorization result (`TenantCredentialProjectionBinding`). This cluster is
//! self-contained aside from [`authorize`], which is the fail-closed check
//! [`super::LocalEnforcementBinding::authorize_credential_projection`]
//! delegates to.

use nimbus_core::{Error, Result, non_empty};
use nimbus_tenant::TenantIsolationDecisionId;
use serde::Serialize;

use super::{NodeIdentity, TenantWorkloadSpec, TenantWorkloadUid};
use crate::WorkloadGeneration;

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
    generation: WorkloadGeneration,
    decision_id: TenantIsolationDecisionId,
    requester_node_id: Option<NodeIdentity>,
    runtime_invocation_id: Option<String>,
    provider: String,
    audience: String,
    redaction_metadata_present: bool,
    echo_back_workload_subject: Option<String>,
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
            echo_back_workload_subject: None,
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

    pub fn with_generation(mut self, generation: WorkloadGeneration) -> Self {
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

    pub fn with_echo_back_workload_subject(mut self, subject: impl Into<String>) -> Self {
        self.echo_back_workload_subject = Some(subject.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantCredentialProjectionBinding {
    workload_uid: TenantWorkloadUid,
    generation: WorkloadGeneration,
    decision_id: TenantIsolationDecisionId,
    scope: TenantCredentialProjectionScope,
    workload_subject: String,
    redacted_fields: Vec<String>,
}

impl TenantCredentialProjectionBinding {
    pub fn workload_uid(&self) -> &TenantWorkloadUid {
        &self.workload_uid
    }

    pub fn generation(&self) -> WorkloadGeneration {
        self.generation
    }

    pub fn decision_id(&self) -> &TenantIsolationDecisionId {
        &self.decision_id
    }

    pub fn scope(&self) -> &TenantCredentialProjectionScope {
        &self.scope
    }

    pub fn workload_subject(&self) -> &str {
        &self.workload_subject
    }

    pub fn redacted_fields(&self) -> &[String] {
        &self.redacted_fields
    }
}

/// Fail-closed authorization check backing
/// [`super::LocalEnforcementBinding::authorize_credential_projection`]: the
/// request must match the admitted workload identity, node, invocation, and
/// carry redaction metadata without an attacker-supplied subject, before its
/// provider/audience pair is checked against the admitted scopes.
pub(super) fn authorize(
    spec: &TenantWorkloadSpec,
    request: &TenantCredentialProjectionRequest,
) -> Result<TenantCredentialProjectionBinding> {
    spec.ensure_request_identity(
        &request.workload_uid,
        request.generation,
        &request.decision_id,
        "credential projection",
    )?;
    if let Some(request_node) = &request.requester_node_id {
        spec.ensure_assigned_node_matches(request_node, "node-mediated credential projection")?;
    }
    if spec.runtime_invocation_id.as_deref() != request.runtime_invocation_id.as_deref() {
        return Err(Error::PermissionDenied(format!(
            "credential projection for workload {} referenced invocation {:?}, but admitted invocation is {:?}",
            spec.workload_uid.as_str(),
            request.runtime_invocation_id.as_deref(),
            spec.runtime_invocation_id.as_deref()
        )));
    }
    if !request.redaction_metadata_present {
        return Err(Error::InvalidInput(
            "credential projection request is missing redaction metadata".to_string(),
        ));
    }
    if request.echo_back_workload_subject.is_some() {
        return Err(Error::PermissionDenied(
            "credential projection request attempted to echo back a subject".to_string(),
        ));
    }
    let scope = spec
        .credential_projection
        .scope(&request.provider, &request.audience)?;
    Ok(TenantCredentialProjectionBinding {
        workload_uid: spec.workload_uid.clone(),
        generation: spec.generation,
        decision_id: spec.decision_id.clone(),
        scope: scope.clone(),
        workload_subject: spec.workload_identity.subject(),
        redacted_fields: spec.audit_redactions.redacted_fields().to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{
        admitted_decision, assert_error_contains, binding_with_credentials,
    };
    use super::*;
    use crate::tenant::LocalEnforcementBinding;

    #[test]
    fn credential_projection_requires_admitted_scope_node_generation_and_redaction() {
        let binding = binding_with_credentials();
        let spec = binding.spec();
        let request = TenantCredentialProjectionRequest::node_mediated(
            spec,
            NodeIdentity::new("node-a").expect("node should parse"),
            "vault",
            "runtime",
        )
        .expect("credential request should build");
        let projection = binding
            .authorize_credential_projection(&request)
            .expect("matching credential projection should be admitted");

        assert_eq!(projection.workload_uid(), spec.workload_uid());
        assert_eq!(projection.generation(), spec.generation());
        assert_eq!(projection.decision_id(), spec.decision_id());
        assert_eq!(projection.scope().provider(), "vault");
        assert_eq!(projection.scope().audience(), "runtime");
        assert_eq!(
            projection.workload_subject(),
            spec.workload_identity().subject()
        );
        assert!(
            projection
                .redacted_fields()
                .contains(&"raw_credentials".to_string())
        );

        assert_error_contains(
            binding.authorize_credential_projection(
                &TenantCredentialProjectionRequest::node_mediated(
                    spec,
                    NodeIdentity::new("node-a").expect("node should parse"),
                    "vault",
                    "wrong-audience",
                )
                .expect("credential request should build"),
            ),
            "did not admit provider `vault` with audience `wrong-audience`",
        );
        assert_error_contains(
            binding.authorize_credential_projection(
                &TenantCredentialProjectionRequest::node_mediated(
                    spec,
                    NodeIdentity::new("node-b").expect("node should parse"),
                    "vault",
                    "runtime",
                )
                .expect("credential request should build"),
            ),
            "assigned to node node-a",
        );
        assert_error_contains(
            binding.authorize_credential_projection(
                &TenantCredentialProjectionRequest::node_mediated(
                    spec,
                    NodeIdentity::new("node-a").expect("node should parse"),
                    "vault",
                    "runtime",
                )
                .expect("credential request should build")
                .with_generation(WorkloadGeneration::new(6)),
            ),
            "referenced generation 6",
        );
        assert_error_contains(
            binding.authorize_credential_projection(
                &TenantCredentialProjectionRequest::node_mediated(
                    spec,
                    NodeIdentity::new("node-a").expect("node should parse"),
                    "vault",
                    "runtime",
                )
                .expect("credential request should build")
                .without_redaction_metadata(),
            ),
            "missing redaction metadata",
        );
        assert_error_contains(
            binding.authorize_credential_projection(
                &TenantCredentialProjectionRequest::node_mediated(
                    spec,
                    NodeIdentity::new("node-a").expect("node should parse"),
                    "vault",
                    "runtime",
                )
                .expect("credential request should build")
                .with_echo_back_workload_subject("spiffe://attacker"),
            ),
            "echo back a subject",
        );

        let no_grant_binding =
            LocalEnforcementBinding::from_decision(&admitted_decision("messages:send", 7))
                .expect("binding should materialize");
        assert_error_contains(
            no_grant_binding.authorize_credential_projection(
                &TenantCredentialProjectionRequest::node_mediated(
                    no_grant_binding.spec(),
                    NodeIdentity::new("node-a").expect("node should parse"),
                    "vault",
                    "runtime",
                )
                .expect("credential request should build"),
            ),
            "did not admit provider",
        );
    }
}
