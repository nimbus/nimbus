use nimbus_core::{Error, Result};
use nimbus_runtime::RuntimeLimits;
use nimbus_sandbox::{PublishedEndpointProtocol, SandboxBackendKind, SandboxResourceCharge};
use serde::{Deserialize, Serialize};

use super::{
    RuntimeIsolationTier, TenantImagePolicyDecision, TenantIsolationMode,
    TenantNetworkEndpointDecision, TenantNetworkPolicyDecision, TenantWorkloadIdentity,
    TenantWorkloadKind,
};

mod diff;
mod draft;
mod egress;
mod evaluation;
mod explanation;
mod external;
mod formatting;
mod prove;
mod reload;
mod validation;

pub use diff::{OperatorPolicyDiff, OperatorPolicyDiffSummary, OperatorPolicyLifecycle};
pub use draft::{
    OperatorDeniedEgressEvent, OperatorPolicyDraft, OperatorPolicyDraftApproval,
    OperatorPolicyDraftKind, OperatorPolicyDraftStatus,
};
pub use egress::{OperatorSandboxEgressPolicy, OperatorSandboxEgressRulePolicy};
use evaluation::OperatorPolicyTraceInput;
pub use evaluation::{OperatorPolicyDecisionEvaluation, OperatorPolicyEvaluation};
pub use external::{
    OperatorExternalPolicyBackend, OperatorExternalPolicyBackendError,
    OperatorExternalPolicyBackendErrorKind, OperatorExternalPolicyBackendIdentity,
    OperatorExternalPolicyBackendResult, OperatorExternalPolicyDecision,
    OperatorExternalPolicyEngine, OperatorExternalPolicyEvidence, OperatorExternalPolicyOutcome,
    OperatorExternalPolicyRequest,
};
use formatting::{join_or_none, normalized_strings, protocol_label};
pub use prove::{
    OperatorPolicyAcceptedRisk, OperatorPolicyAdvisory, OperatorPolicyAdvisoryKind,
    OperatorPolicyAdvisorySeverity, OperatorPolicyProofReport,
};
pub use reload::{OperatorPolicyReloadOutcome, OperatorPolicyReloadState};
use validation::invalid_policy;

pub const OPERATOR_POLICY_SCHEMA_VERSION: u32 = 1;

pub(super) const DEFAULT_REDACTED_FIELDS: [&str; 4] = [
    "principal_claims",
    "bearer_claims",
    "secret_handles",
    "raw_credentials",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorPolicyDocument {
    pub schema_version: u32,
    pub tenant: String,
    #[serde(default)]
    pub metadata: OperatorPolicyMetadata,
    #[serde(default)]
    pub accepted_risks: Vec<OperatorPolicyAcceptedRisk>,
    #[serde(default)]
    pub defaults: OperatorPolicyDefaults,
    pub workloads: Vec<OperatorPolicyWorkload>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorPolicyMetadata {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorPolicyDefaults {
    #[serde(default)]
    pub tenant_isolation_mode: TenantIsolationMode,
    #[serde(default = "default_storage_namespace")]
    pub storage_namespace: String,
    #[serde(default = "default_redacted_fields")]
    pub audit_redactions: Vec<String>,
}

impl Default for OperatorPolicyDefaults {
    fn default() -> Self {
        Self {
            tenant_isolation_mode: TenantIsolationMode::Production,
            storage_namespace: default_storage_namespace(),
            audit_redactions: default_redacted_fields(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorPolicyWorkload {
    pub kind: TenantWorkloadKind,
    pub name: String,
    #[serde(default)]
    pub runtime: OperatorRuntimePolicy,
    #[serde(default)]
    pub sandbox: OperatorSandboxPolicy,
    #[serde(default)]
    pub services: OperatorServicePolicy,
    #[serde(default)]
    pub network: OperatorNetworkPolicy,
    #[serde(default)]
    pub storage: OperatorStoragePolicy,
    #[serde(default)]
    pub volumes: OperatorVolumePolicy,
    #[serde(default)]
    pub image: OperatorImagePolicy,
    #[serde(default)]
    pub secrets: OperatorSecretPolicy,
    #[serde(default)]
    pub quotas: OperatorQuotaPolicy,
    #[serde(default)]
    pub audit: OperatorAuditPolicy,
}

impl OperatorPolicyWorkload {
    fn key(&self) -> String {
        format!("{}/{}", self.kind.label(), self.name)
    }

    fn identity(&self) -> Result<TenantWorkloadIdentity> {
        if self.name.trim().is_empty() {
            return invalid_policy("workload.name cannot be empty");
        }
        match self.kind {
            TenantWorkloadKind::RuntimeFunction => Ok(TenantWorkloadIdentity::runtime_function(
                self.name.clone(),
                self.runtime.tier,
            )),
            TenantWorkloadKind::SandboxService => {
                let sandbox_id = self.sandbox.sandbox_id.as_deref().ok_or_else(|| {
                    Error::InvalidInput(format!(
                        "operator policy workload `{}` is a sandbox_service and must set sandbox.sandbox_id",
                        self.key()
                    ))
                })?;
                let mut identity = TenantWorkloadIdentity::sandbox_service(
                    self.name.clone(),
                    sandbox_id.to_string(),
                );
                if let Some(backend) = self.sandbox.backend {
                    identity = identity.with_sandbox_backend(backend);
                }
                Ok(identity)
            }
            TenantWorkloadKind::HttpRequest | TenantWorkloadKind::SystemTask => {
                Ok(TenantWorkloadIdentity::new(self.kind, self.name.clone()))
            }
        }
    }

    fn trace(&self, input: OperatorPolicyTraceInput<'_>) -> Vec<String> {
        let mut trace = vec![
            format!("tenant isolation mode: {}", input.mode.as_str()),
            format!("runtime profile: {}", self.runtime.profile.label()),
            format!("runtime tier: {}", self.runtime.tier.label()),
            format!("service grants: {}", join_or_none(input.services)),
            format!(
                "network endpoints: {}",
                join_or_none(input.network_endpoint_summaries)
            ),
            format!(
                "sandbox egress: {}",
                join_or_none(input.sandbox_egress_summaries)
            ),
            format!("storage namespace: {}", input.storage_namespace),
            format!("named volumes: {}", join_or_none(input.named_volumes)),
            format!("secret handles: {}", input.secret_handle_count),
        ];
        if let Some(image) = &self.image.reference {
            trace.push(format!("image reference: {image}"));
        }
        trace
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorRuntimeProfile {
    #[default]
    WebStandard,
    Node20,
    Node22,
    Node24,
}

impl OperatorRuntimeProfile {
    fn label(self) -> &'static str {
        match self {
            Self::WebStandard => "web_standard",
            Self::Node20 => "node20",
            Self::Node22 => "node22",
            Self::Node24 => "node24",
        }
    }

    fn runtime_limits(self) -> RuntimeLimits {
        match self {
            Self::WebStandard => RuntimeLimits::application_web_standard(),
            Self::Node20 => RuntimeLimits::application_node20(),
            Self::Node22 => RuntimeLimits::application_node22(),
            Self::Node24 => RuntimeLimits::application_node24(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorRuntimePolicy {
    #[serde(default)]
    pub profile: OperatorRuntimeProfile,
    #[serde(default = "default_runtime_tier")]
    pub tier: RuntimeIsolationTier,
    #[serde(default)]
    pub tenant_isolation_mode: Option<TenantIsolationMode>,
}

impl Default for OperatorRuntimePolicy {
    fn default() -> Self {
        Self {
            profile: OperatorRuntimeProfile::WebStandard,
            tier: RuntimeIsolationTier::InProcessUntrusted,
            tenant_isolation_mode: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorSandboxPolicy {
    pub backend: Option<SandboxBackendKind>,
    pub sandbox_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorServicePolicy {
    #[serde(default)]
    pub allow: Vec<String>,
}

impl OperatorServicePolicy {
    fn normalized_services(&self) -> Vec<String> {
        normalized_strings(&self.allow)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorNetworkPolicy {
    #[serde(default)]
    pub endpoints: Vec<OperatorNetworkEndpointPolicy>,
    #[serde(default)]
    pub egress: OperatorSandboxEgressPolicy,
}

impl OperatorNetworkPolicy {
    fn to_decision(&self) -> Result<TenantNetworkPolicyDecision> {
        TenantNetworkPolicyDecision::new(self.normalized_endpoints().into_iter().map(|endpoint| {
            let mut decision = TenantNetworkEndpointDecision::new(
                endpoint.service.clone(),
                endpoint.name.clone(),
                endpoint.protocol,
                endpoint.host.clone(),
                endpoint.host_port,
            );
            if let Some(guest_port) = endpoint.guest_port {
                decision = decision.with_guest_port(guest_port);
            }
            decision
        }))
        .with_sandbox_egress(self.egress.to_sandbox_policy())
    }

    fn endpoint_summaries(&self) -> Vec<String> {
        self.normalized_endpoints()
            .into_iter()
            .map(OperatorNetworkEndpointPolicy::summary)
            .collect()
    }

    fn egress_summaries(&self) -> Vec<String> {
        self.egress.summaries()
    }

    fn normalized_endpoints(&self) -> Vec<&OperatorNetworkEndpointPolicy> {
        let mut endpoints: Vec<_> = self.endpoints.iter().collect();
        endpoints.sort_by(|left, right| {
            left.service
                .cmp(&right.service)
                .then_with(|| left.name.cmp(&right.name))
        });
        endpoints
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorNetworkEndpointPolicy {
    pub service: String,
    pub name: String,
    pub protocol: PublishedEndpointProtocol,
    pub host: String,
    pub host_port: u16,
    pub guest_port: Option<u16>,
}

impl OperatorNetworkEndpointPolicy {
    fn summary(&self) -> String {
        let guest = self
            .guest_port
            .map(|port| format!(" -> {port}"))
            .unwrap_or_default();
        format!(
            "{}/{} {} {}:{}{}",
            self.service,
            self.name,
            protocol_label(self.protocol),
            self.host,
            self.host_port,
            guest
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorStoragePolicy {
    pub namespace: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorVolumePolicy {
    #[serde(default)]
    pub named: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorImagePolicy {
    pub reference: Option<String>,
    #[serde(default = "default_digest_required")]
    pub digest_required: bool,
    #[serde(default)]
    pub allowed_registries: Vec<String>,
    pub signature: Option<OperatorImageSignaturePolicy>,
    pub provenance: Option<OperatorImageProvenancePolicy>,
    #[serde(default)]
    pub sbom_required: bool,
    #[serde(default)]
    pub allow_local_build: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperatorPolicyImageSummary {
    pub reference: Option<String>,
    pub digest_required: bool,
    pub allowed_registries: Vec<String>,
    pub signature: Option<OperatorImageSignaturePolicy>,
    pub provenance: Option<OperatorImageProvenancePolicy>,
    pub sbom_required: bool,
    pub allow_local_build: bool,
}

impl Default for OperatorImagePolicy {
    fn default() -> Self {
        Self {
            reference: None,
            digest_required: true,
            allowed_registries: Vec::new(),
            signature: None,
            provenance: None,
            sbom_required: false,
            allow_local_build: false,
        }
    }
}

impl OperatorImagePolicy {
    fn to_decision(&self) -> TenantImagePolicyDecision {
        let mut decision = TenantImagePolicyDecision::default().require_digest_reference();
        if let Some(reference) = &self.reference {
            decision = decision.with_image_reference(reference.clone());
        }
        for registry in normalized_strings(&self.allowed_registries) {
            decision = decision.with_allowed_registry(registry.clone());
        }
        if let Some(signature) = &self.signature {
            decision =
                decision.require_signature(signature.issuer.clone(), signature.subject.clone());
        }
        if let Some(provenance) = &self.provenance {
            let predicates = normalized_strings(&provenance.predicates);
            decision = if let Some(source_uri) = &provenance.source_uri {
                decision.require_provenance_from_source(
                    provenance.builder_id.clone(),
                    source_uri.clone(),
                    predicates,
                )
            } else {
                decision.require_provenance(provenance.builder_id.clone(), predicates)
            };
        }
        if self.sbom_required {
            decision = decision.require_sbom();
        }
        if self.allow_local_build {
            decision = decision.allow_local_build();
        }
        decision
    }

    fn summary(&self) -> OperatorPolicyImageSummary {
        OperatorPolicyImageSummary {
            reference: self.reference.clone(),
            digest_required: self.digest_required,
            allowed_registries: normalized_strings(&self.allowed_registries),
            signature: self.signature.clone(),
            provenance: self
                .provenance
                .as_ref()
                .map(|provenance| OperatorImageProvenancePolicy {
                    builder_id: provenance.builder_id.clone(),
                    source_uri: provenance.source_uri.clone(),
                    predicates: normalized_strings(&provenance.predicates),
                }),
            sbom_required: self.sbom_required,
            allow_local_build: self.allow_local_build,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorImageSignaturePolicy {
    pub issuer: String,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorImageProvenancePolicy {
    pub builder_id: String,
    pub source_uri: Option<String>,
    #[serde(default)]
    pub predicates: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorSecretPolicy {
    #[serde(default)]
    pub handles: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorQuotaPolicy {
    pub sandbox_charge: Option<SandboxResourceCharge>,
}

impl OperatorQuotaPolicy {
    fn summary(&self) -> OperatorPolicyQuotaSummary {
        OperatorPolicyQuotaSummary {
            sandbox_charge: self.sandbox_charge,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperatorPolicyQuotaSummary {
    pub sandbox_charge: Option<SandboxResourceCharge>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorAuditPolicy {
    pub redacted_fields: Option<Vec<String>>,
}

fn default_runtime_tier() -> RuntimeIsolationTier {
    RuntimeIsolationTier::InProcessUntrusted
}

fn default_storage_namespace() -> String {
    "tenant".to_string()
}

fn default_redacted_fields() -> Vec<String> {
    DEFAULT_REDACTED_FIELDS
        .iter()
        .map(|field| (*field).to_string())
        .collect()
}

fn default_digest_required() -> bool {
    true
}

#[cfg(test)]
mod tests;
