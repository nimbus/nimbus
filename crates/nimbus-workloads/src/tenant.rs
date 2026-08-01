use nimbus_core::{Error, Result, TenantId, non_empty};
use nimbus_tenant::{
    TenantAuditRedactionPolicy, TenantIsolationDecision, TenantIsolationDecisionId,
    TenantQuotaPolicyDecision, TenantServiceAccessDecision, TenantStorageAccessDecision,
    WorkloadIdentity,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod credential_projection;

pub use credential_projection::{
    TenantCredentialProjectionBinding, TenantCredentialProjectionPolicy,
    TenantCredentialProjectionRequest, TenantCredentialProjectionScope,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NodeIdentity(String);

impl NodeIdentity {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        value.into().try_into()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for NodeIdentity {
    type Error = Error;

    fn try_from(value: String) -> Result<Self> {
        Ok(Self(non_empty(value, "node identity")?))
    }
}

impl From<NodeIdentity> for String {
    fn from(value: NodeIdentity) -> Self {
        value.0
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TenantWorkloadUid(String);

impl TenantWorkloadUid {
    fn for_admitted_identity(
        identity: &WorkloadIdentity,
        decision_id: &TenantIsolationDecisionId,
    ) -> Self {
        let mut digest = Sha256::new();
        digest.update(identity.subject().as_bytes());
        digest.update(b"\0");
        digest.update(decision_id.as_str().as_bytes());
        Self(format!("twu_{:x}", digest.finalize()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for TenantWorkloadUid {
    type Error = Error;

    fn try_from(value: String) -> Result<Self> {
        validate_digest_id(&value, "twu", "tenant workload uid")?;
        Ok(Self(value))
    }
}

impl From<TenantWorkloadUid> for String {
    fn from(value: TenantWorkloadUid) -> Self {
        value.0
    }
}

fn validate_digest_id(value: &str, prefix: &str, label: &str) -> Result<()> {
    let Some(digest) = value
        .strip_prefix(prefix)
        .and_then(|rest| rest.strip_prefix('_'))
    else {
        return Err(Error::InvalidInput(format!(
            "{label} must use the `{prefix}_<sha256>` form"
        )));
    };
    if digest.len() != 64
        || digest
            .bytes()
            .any(|byte| !byte.is_ascii_digit() && !(b'a'..=b'f').contains(&byte))
    {
        return Err(Error::InvalidInput(format!(
            "{label} must contain 64 lowercase hexadecimal digest characters"
        )));
    }
    Ok(())
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
        credential_projection::authorize(&self.spec, request)
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
            surface: self.spec.surface.clone(),
            authority_class: self.spec.authority_class.clone(),
            workload_uid: self.spec.workload_uid.clone(),
            workload_subject: self.spec.workload_identity.subject(),
            generation: self.spec.generation,
            redacted_fields: self.spec.audit_redactions.redacted_fields().to_vec(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantWorkloadSpec {
    decision_id: TenantIsolationDecisionId,
    tenant_id: TenantId,
    surface: String,
    authority_class: String,
    workload_identity: WorkloadIdentity,
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
        let workload_identity = decision.workload_identity();
        let workload_uid =
            TenantWorkloadUid::for_admitted_identity(&workload_identity, decision.id());
        let generation =
            TenantWorkloadGeneration::new(workload_identity.deployment_generation().unwrap_or(0));
        let assigned_node_id = workload_identity
            .node_id()
            .map(NodeIdentity::new)
            .transpose()?;
        let runtime_invocation_id = workload_identity.invocation_id().map(ToOwned::to_owned);
        let service_projection = TenantServiceProjection::from_decision(decision)?;
        Ok(Self {
            decision_id: decision.id().clone(),
            tenant_id: decision.tenant_id().clone(),
            surface: decision.surface().to_owned(),
            authority_class: decision.authority_class().to_owned(),
            workload_identity,
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

    pub fn surface(&self) -> &str {
        &self.surface
    }

    pub fn authority_class(&self) -> &str {
        &self.authority_class
    }

    pub fn workload_uid(&self) -> &TenantWorkloadUid {
        &self.workload_uid
    }

    pub fn workload_identity(&self) -> &WorkloadIdentity {
        &self.workload_identity
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

    pub fn ensure_assigned_node_matches(&self, actual: &NodeIdentity, context: &str) -> Result<()> {
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

    pub fn ensure_request_identity(
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
    surface: String,
    authority_class: String,
    workload_uid: TenantWorkloadUid,
    workload_subject: String,
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

    pub fn surface(&self) -> &str {
        &self.surface
    }

    pub fn authority_class(&self) -> &str {
        &self.authority_class
    }

    pub fn workload_uid(&self) -> &TenantWorkloadUid {
        &self.workload_uid
    }

    pub fn workload_subject(&self) -> &str {
        &self.workload_subject
    }

    pub fn generation(&self) -> TenantWorkloadGeneration {
        self.generation
    }

    pub fn redacted_fields(&self) -> &[String] {
        &self.redacted_fields
    }
}

/// Test fixtures shared by [`tests`] and [`credential_projection::tests`],
/// kept at the `tenant` module level so both descendant test modules can use
/// them (a nested `mod tests` is only visible to its own descendants).
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use nimbus_tenant::{
        TenantIsolationContext, TenantIsolationPolicyInput, TenantServiceGrantPolicyDecision,
        TenantStoragePolicyDecision, WorkloadAttributes, WorkloadLocation,
    };

    pub(crate) fn admitted_decision(
        workload_name: &str,
        generation: u64,
    ) -> TenantIsolationDecision {
        let tenant_id = TenantId::new("tenant-a").expect("tenant id should parse");
        let context = TenantIsolationContext::operator(tenant_id, "workloads.test")
            .with_deployment_generation(generation)
            .with_workload_location(WorkloadLocation::new().with_node_id("node-a"));
        let input = TenantIsolationPolicyInput::new(WorkloadAttributes::service(workload_name))
            .with_services(TenantServiceGrantPolicyDecision::new(["db", "cache"]))
            .with_storage(TenantStoragePolicyDecision::namespace("tenant-a-storage"));

        context
            .admit_decision(input)
            .expect("decision should admit")
    }

    pub(crate) fn binding_with_credentials() -> LocalEnforcementBinding {
        let decision = admitted_decision("messages:send", 7);
        let spec = TenantWorkloadSpec::from_decision(&decision)
            .expect("spec should materialize from decision")
            .with_admitted_credential_scopes([TenantCredentialProjectionScope::new(
                "vault", "runtime",
            )
            .expect("scope should parse")]);
        LocalEnforcementBinding::from_spec(spec)
    }

    pub(crate) fn assert_error_contains<T: std::fmt::Debug>(result: Result<T>, expected: &str) {
        let error = result.expect_err("operation should fail closed");
        assert!(
            error.to_string().contains(expected),
            "expected error containing `{expected}`, got `{error}`"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{admitted_decision, assert_error_contains, binding_with_credentials};
    use super::*;

    #[test]
    fn binding_materializes_decision_derived_spec_and_projections() {
        let decision = admitted_decision("messages:send", 7);
        let binding = LocalEnforcementBinding::from_decision(&decision)
            .expect("binding should materialize from admitted decision");
        let spec = binding.spec();

        assert_eq!(spec.decision_id(), decision.id());
        assert_eq!(spec.tenant_id(), decision.tenant_id());
        assert_eq!(spec.surface(), decision.surface());
        assert_eq!(spec.authority_class(), decision.authority_class());
        assert_eq!(spec.generation().as_u64(), 7);
        assert_eq!(
            spec.assigned_node_id()
                .expect("node assignment should be present")
                .as_str(),
            "node-a"
        );
        assert!(
            spec.workload_uid().as_str().starts_with("twu_"),
            "workload UID should be derived, not caller supplied"
        );
        assert_eq!(
            binding.storage_access().namespace_name(),
            "tenant-a-storage"
        );
        binding
            .storage_access()
            .ensure_tenant_matches(decision.tenant_id(), "storage projection")
            .expect("storage projection should match admitted tenant");
        assert_error_contains(
            binding.storage_access().ensure_tenant_matches(
                &TenantId::new("tenant-b").expect("tenant id should parse"),
                "storage projection",
            ),
            "authorized tenant tenant-a",
        );
        assert_eq!(
            binding
                .service_access("db")
                .expect("db service should be admitted")
                .service_name(),
            "db"
        );
        assert_error_contains(binding.service_access("not-admitted"), "did not authorize");

        let evidence = binding.system_evidence_projection();
        assert_eq!(evidence.decision_id(), decision.id());
        assert_eq!(evidence.tenant_id(), decision.tenant_id());
        assert_eq!(evidence.surface(), "workloads.test");
        assert_eq!(evidence.authority_class(), "operator");
        assert_eq!(evidence.generation().as_u64(), 7);
        assert_eq!(evidence.workload_uid(), spec.workload_uid());
        assert!(
            evidence.workload_subject().contains("messages%3Asend"),
            "system evidence should use the admitted workload subject"
        );
        assert!(
            evidence
                .redacted_fields()
                .contains(&"raw_credentials".to_string()),
            "system evidence projection should preserve redaction metadata"
        );
    }

    #[test]
    fn egress_reload_and_policy_lifecycle_require_admitted_binding_identity() {
        let binding = binding_with_credentials();
        let spec = binding.spec();
        binding
            .authorize_egress_reload(&TenantEgressReloadRequest::for_spec(spec))
            .expect("matching egress reload should be authorized");

        let other_decision = admitted_decision("messages:list", 7);
        assert_error_contains(
            binding.authorize_egress_reload(
                &TenantEgressReloadRequest::for_spec(spec)
                    .with_decision_id(other_decision.id().clone()),
            ),
            "referenced decision",
        );

        assert_eq!(
            policy_lifecycle(TenantPolicyArea::Filesystem),
            TenantPolicyLifecycle::RecreateRequired
        );
        assert_eq!(
            policy_lifecycle(TenantPolicyArea::Placement),
            TenantPolicyLifecycle::RecreateRequired
        );
        assert_eq!(
            policy_lifecycle(TenantPolicyArea::HostBridgeGrants),
            TenantPolicyLifecycle::DynamicReload
        );
        assert_eq!(
            policy_lifecycle(TenantPolicyArea::DeletionFinalizerState),
            TenantPolicyLifecycle::ServerOwnedTransition
        );
    }

    #[test]
    fn malformed_local_enforcement_identifiers_fail_closed() {
        assert!(matches!(
            NodeIdentity::new("  "),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            TenantCredentialProjectionScope::new("vault", ""),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            TenantFinalizerRecord::new("", "cleanup"),
            Err(Error::InvalidInput(_))
        ));
    }
}
