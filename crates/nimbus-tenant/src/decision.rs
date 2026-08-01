use nimbus_core::{Error, Result, TenantId};
use nimbus_runtime::RuntimeBundle;
use nimbus_sandbox::{SandboxBackendKind, SandboxSpec};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    TenantAuditRedactionPolicy, TenantImagePolicyDecision, TenantIsolationAuthorityDecision,
    TenantIsolationContext, TenantIsolationPolicyInput, TenantNetworkPolicyDecision,
    TenantQuotaPolicyDecision, TenantRuntimePolicyDecision, TenantSecretPolicyDecision,
    TenantServiceGrantPolicyDecision, TenantStoragePolicyDecision, TenantVolumePolicyDecision,
    WorkloadAttributes, WorkloadIdentity, WorkloadLocation,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TenantIsolationDecisionId(String);

impl TenantIsolationDecisionId {
    fn for_fingerprint(fingerprint: &TenantIsolationDecisionFingerprint<'_>) -> Result<Self> {
        let bytes = serde_json::to_vec(fingerprint)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        let digest = Sha256::digest(bytes);
        Ok(Self(format!("tid_{digest:x}")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for TenantIsolationDecisionId {
    type Error = Error;

    fn try_from(value: String) -> Result<Self> {
        let Some(digest) = value.strip_prefix("tid_") else {
            return Err(Error::InvalidInput(
                "tenant isolation decision id must use the `tid_<sha256>` form".to_string(),
            ));
        };
        if digest.len() != 64
            || digest
                .bytes()
                .any(|byte| !byte.is_ascii_digit() && !(b'a'..=b'f').contains(&byte))
        {
            return Err(Error::InvalidInput(
                "tenant isolation decision id must contain 64 lowercase hexadecimal digest characters"
                    .to_string(),
            ));
        }
        Ok(Self(value))
    }
}

impl From<TenantIsolationDecisionId> for String {
    fn from(value: TenantIsolationDecisionId) -> Self {
        value.0
    }
}

#[derive(Serialize)]
struct TenantIsolationDecisionFingerprint<'a> {
    tenant_id: &'a str,
    surface: &'a str,
    authority: &'a TenantIsolationAuthorityDecision,
    deployment_generation: Option<u64>,
    location: &'a WorkloadLocation,
    workload: &'a WorkloadAttributes,
    runtime: &'a TenantRuntimePolicyDecision,
    services: &'a TenantServiceGrantPolicyDecision,
    network: &'a TenantNetworkPolicyDecision,
    storage: &'a TenantStoragePolicyDecision,
    volumes: &'a TenantVolumePolicyDecision,
    image: &'a TenantImagePolicyDecision,
    secrets: &'a TenantSecretPolicyDecision,
    quotas: &'a TenantQuotaPolicyDecision,
    audit_redactions: &'a TenantAuditRedactionPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantIsolationDecision {
    pub(super) id: TenantIsolationDecisionId,
    pub(super) tenant_id: TenantId,
    pub(super) surface: &'static str,
    pub(super) authority: TenantIsolationAuthorityDecision,
    pub(super) deployment_generation: Option<u64>,
    pub(super) location: WorkloadLocation,
    pub(super) workload: WorkloadAttributes,
    pub(super) runtime: TenantRuntimePolicyDecision,
    pub(super) services: TenantServiceGrantPolicyDecision,
    pub(super) network: TenantNetworkPolicyDecision,
    pub(super) storage: TenantStoragePolicyDecision,
    pub(super) volumes: TenantVolumePolicyDecision,
    pub(super) image: TenantImagePolicyDecision,
    pub(super) secrets: TenantSecretPolicyDecision,
    pub(super) quotas: TenantQuotaPolicyDecision,
    pub(super) audit_redactions: TenantAuditRedactionPolicy,
}

impl TenantIsolationDecision {
    pub(super) fn admit(
        context: &TenantIsolationContext,
        input: TenantIsolationPolicyInput,
    ) -> Result<Self> {
        context.admit_if_principal_claim_absent_or_matching("tenant isolation decision")?;
        let authority = TenantIsolationAuthorityDecision::from_context(context)?;
        let fingerprint = TenantIsolationDecisionFingerprint {
            tenant_id: context.tenant_id.as_str(),
            surface: context.surface,
            authority: &authority,
            deployment_generation: context.deployment_generation,
            location: &context.location,
            workload: &input.workload,
            runtime: &input.runtime,
            services: &input.services,
            network: &input.network,
            storage: &input.storage,
            volumes: &input.volumes,
            image: &input.image,
            secrets: &input.secrets,
            quotas: &input.quotas,
            audit_redactions: &input.audit_redactions,
        };
        let id = TenantIsolationDecisionId::for_fingerprint(&fingerprint)?;
        Ok(Self {
            id,
            tenant_id: context.tenant_id.clone(),
            surface: context.surface,
            authority,
            deployment_generation: context.deployment_generation,
            location: context.location.clone(),
            workload: input.workload,
            runtime: input.runtime,
            services: input.services,
            network: input.network,
            storage: input.storage,
            volumes: input.volumes,
            image: input.image,
            secrets: input.secrets,
            quotas: input.quotas,
            audit_redactions: input.audit_redactions,
        })
    }

    pub fn id(&self) -> &TenantIsolationDecisionId {
        &self.id
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn surface(&self) -> &'static str {
        self.surface
    }

    pub fn authority_class(&self) -> &'static str {
        self.authority.class()
    }

    pub fn workload(&self) -> &WorkloadAttributes {
        &self.workload
    }

    pub fn workload_identity(&self) -> WorkloadIdentity {
        WorkloadIdentity::from_decision(self)
    }

    pub fn runtime(&self) -> &TenantRuntimePolicyDecision {
        &self.runtime
    }

    pub fn services(&self) -> &TenantServiceGrantPolicyDecision {
        &self.services
    }

    pub fn network(&self) -> &TenantNetworkPolicyDecision {
        &self.network
    }

    pub fn storage(&self) -> &TenantStoragePolicyDecision {
        &self.storage
    }

    pub fn storage_access(&self) -> TenantStorageAccessDecision {
        TenantStorageAccessDecision {
            decision_id: self.id.clone(),
            tenant_id: self.tenant_id.clone(),
            namespace: self.storage.namespace_name().to_string(),
        }
    }

    pub fn volumes(&self) -> &TenantVolumePolicyDecision {
        &self.volumes
    }

    pub fn image(&self) -> &TenantImagePolicyDecision {
        &self.image
    }

    pub fn quotas(&self) -> &TenantQuotaPolicyDecision {
        &self.quotas
    }

    pub fn audit_redactions(&self) -> &TenantAuditRedactionPolicy {
        &self.audit_redactions
    }

    pub fn service_access(
        &self,
        service_name: &str,
        context: &str,
    ) -> Result<TenantServiceAccessDecision> {
        if self
            .services
            .services()
            .iter()
            .any(|admitted_service| admitted_service == service_name)
        {
            return Ok(TenantServiceAccessDecision {
                decision_id: self.id.clone(),
                tenant_id: self.tenant_id.clone(),
                service_name: service_name.to_owned(),
            });
        }
        Err(Error::PermissionDenied(format!(
            "tenant isolation decision {} for tenant {} did not authorize service `{service_name}` for {context}",
            self.id.as_str(),
            self.tenant_id
        )))
    }

    pub fn ensure_sandbox_spec_matches(
        &self,
        spec: &SandboxSpec,
        actual_backend: SandboxBackendKind,
        context: &str,
    ) -> Result<()> {
        if spec.backend != actual_backend {
            return Err(Error::InvalidInput(format!(
                "tenant isolation decision {} for {context} requested sandbox backend {:?}, but the configured manager backend is {:?}",
                self.id.as_str(),
                spec.backend,
                actual_backend
            )));
        }
        self.ensure_tenant_matches(&spec.tenant_id, context)
    }

    pub fn ensure_tenant_matches(&self, actual: &TenantId, context: &str) -> Result<()> {
        if actual == &self.tenant_id {
            return Ok(());
        }
        Err(Error::InvalidInput(format!(
            "tenant isolation decision {} authorized tenant {}, but {context} referenced tenant {}",
            self.id.as_str(),
            self.tenant_id,
            actual
        )))
    }

    pub fn ensure_deployment_generation_matches(
        &self,
        actual_generation: u64,
        context: &str,
    ) -> Result<()> {
        let Some(expected_generation) = self.deployment_generation else {
            return Ok(());
        };
        if expected_generation == actual_generation {
            return Ok(());
        }
        Err(Error::InvalidInput(format!(
            "tenant isolation decision {} authorized deployment generation {}, but {context} referenced deployment generation {}",
            self.id.as_str(),
            expected_generation,
            actual_generation
        )))
    }

    pub fn ensure_runtime_bundle_matches(
        &self,
        bundle: &RuntimeBundle,
        context: &str,
    ) -> Result<()> {
        let Some(tenant_label) = bundle.identity().tenant_label() else {
            return Ok(());
        };
        let actual = TenantId::new(tenant_label.to_string())?;
        self.ensure_tenant_matches(&actual, context)
    }

    pub fn to_audit_record(&self) -> TenantIsolationAuditRecord {
        TenantIsolationAuditRecord {
            decision_id: self.id.as_str().to_string(),
            tenant_id: self.tenant_id.as_str().to_string(),
            surface: self.surface.to_string(),
            authority_class: self.authority.class().to_string(),
            deployment_generation: self.deployment_generation,
            workload_subject: self.workload_identity().subject(),
            workload_audit_projection: self.workload_identity().audit_projection(),
            workload: self.workload.clone(),
            runtime: self.runtime.clone(),
            services: self.services.clone(),
            network: self.network.clone(),
            storage: self.storage.clone(),
            volumes: self.volumes.clone(),
            image: self.image.clone(),
            secret_handle_count: self.secrets.handle_count(),
            quotas: self.quotas.clone(),
            redacted_fields: self.audit_redactions.redacted_fields().to_vec(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantStorageAccessDecision {
    decision_id: TenantIsolationDecisionId,
    tenant_id: TenantId,
    namespace: String,
}

impl TenantStorageAccessDecision {
    pub fn decision_id(&self) -> &TenantIsolationDecisionId {
        &self.decision_id
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn namespace_name(&self) -> &str {
        &self.namespace
    }

    pub fn ensure_tenant_matches(&self, actual: &TenantId, context: &str) -> Result<()> {
        if actual == &self.tenant_id {
            return Ok(());
        }
        Err(Error::InvalidInput(format!(
            "tenant storage access decision {} authorized tenant {}, but {context} referenced tenant {}",
            self.decision_id.as_str(),
            self.tenant_id,
            actual
        )))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantServiceAccessDecision {
    decision_id: TenantIsolationDecisionId,
    tenant_id: TenantId,
    service_name: String,
}

impl TenantServiceAccessDecision {
    pub fn decision_id(&self) -> &TenantIsolationDecisionId {
        &self.decision_id
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn service_name(&self) -> &str {
        &self.service_name
    }

    pub fn ensure_tenant_matches(&self, actual: &TenantId, context: &str) -> Result<()> {
        if actual == &self.tenant_id {
            return Ok(());
        }
        Err(Error::InvalidInput(format!(
            "tenant service access decision {} authorized tenant {}, but {context} referenced tenant {}",
            self.decision_id.as_str(),
            self.tenant_id,
            actual
        )))
    }

    pub fn ensure_sandbox_spec_matches(
        &self,
        spec: &SandboxSpec,
        actual_backend: SandboxBackendKind,
    ) -> Result<()> {
        if spec.backend != actual_backend {
            return Err(Error::InvalidInput(format!(
                "tenant service access decision {} for service {} requested backend {:?}, but the configured manager backend is {:?}",
                self.decision_id.as_str(),
                self.service_name,
                spec.backend,
                actual_backend
            )));
        }
        if spec.service_name() != Some(self.service_name.as_str()) {
            return Err(Error::InvalidInput(format!(
                "tenant service access decision {} authorized service {}, but service definition catalog returned sandbox owner {:?}",
                self.decision_id.as_str(),
                self.service_name,
                spec.owner
            )));
        }
        self.ensure_tenant_matches(&spec.tenant_id, "sandbox-backed service sandbox spec")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantIsolationAuditRecord {
    pub(super) decision_id: String,
    pub(super) tenant_id: String,
    pub(super) surface: String,
    pub(super) authority_class: String,
    pub(super) deployment_generation: Option<u64>,
    pub(super) workload_subject: String,
    pub(super) workload_audit_projection: String,
    pub(super) workload: WorkloadAttributes,
    pub(super) runtime: TenantRuntimePolicyDecision,
    pub(super) services: TenantServiceGrantPolicyDecision,
    pub(super) network: TenantNetworkPolicyDecision,
    pub(super) storage: TenantStoragePolicyDecision,
    pub(super) volumes: TenantVolumePolicyDecision,
    pub(super) image: TenantImagePolicyDecision,
    pub(super) secret_handle_count: usize,
    pub(super) quotas: TenantQuotaPolicyDecision,
    pub(super) redacted_fields: Vec<String>,
}
