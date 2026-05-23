use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use nimbus_core::{Error, Result, TenantId};
use nimbus_runtime::{RuntimeLimits, RuntimePolicy};
use nimbus_sandbox::{
    PublishedEndpointProtocol, SandboxBackendKind, SandboxResourceCharge,
    validate_tenant_volume_name,
};
use serde::{Deserialize, Serialize};

use super::image_admission::{has_sha256_digest, parse_oci_image_reference};
use super::{
    RuntimeIsolationTier, TenantAuditRedactionPolicy, TenantImagePolicyDecision,
    TenantIsolationContext, TenantIsolationDecision, TenantIsolationMode,
    TenantNetworkEndpointDecision, TenantNetworkPolicyDecision, TenantQuotaPolicyDecision,
    TenantRuntimePolicyAdmission, TenantSecretPolicyDecision, TenantServiceGrantPolicyDecision,
    TenantStoragePolicyDecision, TenantVolumePolicyDecision, TenantWorkloadIdentity,
    TenantWorkloadKind,
};

mod draft;
mod egress;
mod external;
mod prove;
mod reload;

pub use draft::{
    OperatorDeniedEgressEvent, OperatorPolicyDraft, OperatorPolicyDraftApproval,
    OperatorPolicyDraftKind, OperatorPolicyDraftStatus,
};
pub use egress::{OperatorSandboxEgressPolicy, OperatorSandboxEgressRulePolicy};
use external::evaluate_external_policy_backend;
pub use external::{
    OperatorExternalPolicyBackend, OperatorExternalPolicyBackendError,
    OperatorExternalPolicyBackendErrorKind, OperatorExternalPolicyBackendIdentity,
    OperatorExternalPolicyBackendResult, OperatorExternalPolicyDecision,
    OperatorExternalPolicyEngine, OperatorExternalPolicyEvidence, OperatorExternalPolicyOutcome,
    OperatorExternalPolicyRequest,
};
use prove::validate_accepted_risks;
pub use prove::{
    OperatorPolicyAcceptedRisk, OperatorPolicyAdvisory, OperatorPolicyAdvisoryKind,
    OperatorPolicyAdvisorySeverity, OperatorPolicyProofReport,
};
pub use reload::{OperatorPolicyReloadOutcome, OperatorPolicyReloadState};

pub const OPERATOR_POLICY_SCHEMA_VERSION: u32 = 1;

const DEFAULT_REDACTED_FIELDS: [&str; 4] = [
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

impl OperatorPolicyDocument {
    pub fn validate(&self) -> Result<()> {
        self.evaluate().map(|_| ())
    }

    pub fn evaluate(&self) -> Result<OperatorPolicyEvaluation> {
        self.evaluate_with_external_policy(None)
    }

    pub fn evaluate_with_external_policy(
        &self,
        external_backend: Option<&OperatorExternalPolicyEngine>,
    ) -> Result<OperatorPolicyEvaluation> {
        self.validate_shape()?;
        let tenant_id = TenantId::new(self.tenant.clone())?;
        let mut decisions = Vec::with_capacity(self.workloads.len());
        for workload in &self.workloads {
            decisions.push(self.evaluate_workload(&tenant_id, workload, external_backend)?);
        }
        Ok(OperatorPolicyEvaluation {
            policy_name: self.metadata.name.clone(),
            tenant_id: tenant_id.as_str().to_string(),
            decision_count: decisions.len(),
            decisions,
        })
    }

    fn evaluate_workload(
        &self,
        tenant_id: &TenantId,
        workload: &OperatorPolicyWorkload,
        external_backend: Option<&OperatorExternalPolicyEngine>,
    ) -> Result<OperatorPolicyDecisionEvaluation> {
        let context = TenantIsolationContext::operator(tenant_id.clone(), "operator.policy");
        let mode = workload
            .runtime
            .tenant_isolation_mode
            .unwrap_or(self.defaults.tenant_isolation_mode);
        let services = workload.services.normalized_services();
        let mut runtime_limits = workload.runtime.profile.runtime_limits();
        runtime_limits.grants.service = services.clone();
        let runtime_policy = RuntimePolicy::new(runtime_limits);

        let identity = workload.identity()?;
        let storage_namespace = workload
            .storage
            .namespace
            .as_deref()
            .unwrap_or(self.defaults.storage_namespace.as_str());
        let storage_namespace = storage_namespace_for_policy(storage_namespace, tenant_id);
        let named_volumes = normalized_strings(&workload.volumes.named);
        let secret_handles = normalized_strings(&workload.secrets.handles);
        let audit_redactions = workload
            .audit
            .redacted_fields
            .clone()
            .unwrap_or_else(|| self.defaults.audit_redactions.clone());
        let audit_redactions = normalized_strings(&audit_redactions);
        let image_policy = workload.image.summary();
        let image_reference = image_policy.reference.clone();
        let endpoint_summaries = workload.network.endpoint_summaries();
        let sandbox_egress = workload.network.egress_summaries();
        let quotas_summary = workload.quotas.summary();
        let trace = workload.trace(OperatorPolicyTraceInput {
            mode,
            storage_namespace: storage_namespace.as_str(),
            services: &services,
            network_endpoint_summaries: &endpoint_summaries,
            sandbox_egress_summaries: &sandbox_egress,
            named_volumes: &named_volumes,
            secret_handle_count: secret_handles.len(),
        });

        let mut quotas = TenantQuotaPolicyDecision::default()
            .with_runtime_budget(runtime_policy.tenant_budget());
        if let Some(charge) = workload.quotas.sandbox_charge {
            quotas = quotas.with_sandbox_charge(charge);
        }

        let decision = context.admit_decision(
            super::TenantIsolationPolicyInput::new(identity)
                .with_runtime_policy(&context, &runtime_policy, workload.runtime.tier, mode)
                .with_services(TenantServiceGrantPolicyDecision::new(services.clone()))
                .with_network(workload.network.to_decision()?)
                .with_storage(TenantStoragePolicyDecision::namespace(
                    storage_namespace.clone(),
                ))
                .with_volumes(TenantVolumePolicyDecision::new(named_volumes.clone()))
                .with_image(workload.image.to_decision())
                .with_secrets(TenantSecretPolicyDecision::handles(secret_handles.clone()))
                .with_quotas(quotas)
                .with_audit_redactions(TenantAuditRedactionPolicy {
                    redacted_fields: audit_redactions,
                }),
        )?;

        let mut evaluation = OperatorPolicyDecisionEvaluation {
            workload_key: workload.key(),
            decision_id: decision.id().as_str().to_string(),
            tenant_id: decision.tenant_id().as_str().to_string(),
            runtime_tier: decision.runtime().tier(),
            runtime_profile: workload.runtime.profile,
            tenant_isolation_mode: mode,
            runtime_admission: decision.runtime().admission().clone(),
            sandbox_backend: workload.sandbox.backend,
            sandbox_id: workload.sandbox.sandbox_id.clone(),
            services,
            network_endpoints: endpoint_summaries,
            sandbox_egress,
            storage_namespace,
            named_volumes,
            image_policy,
            image_reference,
            secret_handle_count: secret_handles.len(),
            secret_handles,
            quotas: quotas_summary,
            audit_redactions: decision.audit_redactions().redacted_fields().to_vec(),
            external_policy: None,
            trace,
            decision,
        };
        if let Some(backend) = external_backend {
            let external_policy = evaluate_external_policy_backend(
                backend,
                evaluation.external_policy_request(self.metadata.name.clone()),
            )?;
            evaluation
                .trace
                .push(format!("external policy: {}", external_policy.summary()));
            evaluation.external_policy = Some(external_policy);
        }
        Ok(evaluation)
    }

    fn validate_shape(&self) -> Result<()> {
        if self.schema_version != OPERATOR_POLICY_SCHEMA_VERSION {
            return invalid_policy(format!(
                "schema_version must be {OPERATOR_POLICY_SCHEMA_VERSION}, got {}",
                self.schema_version
            ));
        }
        if self.workloads.is_empty() {
            return invalid_policy("workloads must contain at least one workload");
        }
        TenantId::new(self.tenant.clone()).map_err(|error| {
            Error::InvalidInput(format!("operator policy tenant is invalid: {error}"))
        })?;
        validate_accepted_risks(&self.accepted_risks)?;
        validate_storage_namespace(
            &self.defaults.storage_namespace,
            "defaults.storage_namespace",
        )?;
        validate_redactions(&self.defaults.audit_redactions, "defaults.audit_redactions")?;

        let mut seen = BTreeSet::new();
        for workload in &self.workloads {
            let key = workload.key();
            if !seen.insert(key.clone()) {
                return invalid_policy(format!("workload `{key}` is declared more than once"));
            }
            workload.validate(self.defaults.tenant_isolation_mode)?;
        }
        Ok(())
    }
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

    fn validate(&self, default_mode: TenantIsolationMode) -> Result<()> {
        if self.name.trim().is_empty() {
            return invalid_policy("workload.name cannot be empty");
        }
        self.runtime.validate(&self.key())?;
        self.sandbox.validate(&self.key(), self.kind)?;
        self.services.validate(&self.key())?;
        self.network.validate(&self.key(), &self.services)?;
        self.storage.validate(&self.key())?;
        self.volumes.validate(&self.key())?;
        self.image.validate(
            &self.key(),
            self.runtime.tenant_isolation_mode.unwrap_or(default_mode),
        )?;
        self.secrets.validate(&self.key())?;
        self.quotas.validate(&self.key())?;
        self.audit.validate(&self.key())?;
        Ok(())
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

struct OperatorPolicyTraceInput<'a> {
    mode: TenantIsolationMode,
    storage_namespace: &'a str,
    services: &'a [String],
    network_endpoint_summaries: &'a [String],
    sandbox_egress_summaries: &'a [String],
    named_volumes: &'a [String],
    secret_handle_count: usize,
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

impl OperatorRuntimePolicy {
    fn validate(&self, workload_key: &str) -> Result<()> {
        if matches!(
            self.profile,
            OperatorRuntimeProfile::Node20
                | OperatorRuntimeProfile::Node22
                | OperatorRuntimeProfile::Node24
        ) && matches!(self.tier, RuntimeIsolationTier::InProcessUntrusted)
        {
            // This is allowed, but it should be visible in explain output because
            // production admission routes broad Node localhost/listen grants away
            // from in-process untrusted execution.
            return Ok(());
        }
        let _ = workload_key;
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorSandboxPolicy {
    pub backend: Option<SandboxBackendKind>,
    pub sandbox_id: Option<String>,
}

impl OperatorSandboxPolicy {
    fn validate(&self, workload_key: &str, kind: TenantWorkloadKind) -> Result<()> {
        if matches!(kind, TenantWorkloadKind::SandboxService) && self.sandbox_id.is_none() {
            return invalid_policy(format!(
                "workload `{workload_key}` is a sandbox_service and must set sandbox.sandbox_id"
            ));
        }
        if let Some(sandbox_id) = &self.sandbox_id
            && sandbox_id.trim().is_empty()
        {
            return invalid_policy(format!(
                "workload `{workload_key}` sandbox_id cannot be empty"
            ));
        }
        Ok(())
    }
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

    fn validate(&self, workload_key: &str) -> Result<()> {
        validate_name_list(
            &self.allow,
            &format!("workload `{workload_key}` services.allow"),
            "service",
        )
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

    fn validate(&self, workload_key: &str, services: &OperatorServicePolicy) -> Result<()> {
        let allowed_services: BTreeSet<_> = services.allow.iter().map(String::as_str).collect();
        let mut seen = BTreeSet::new();
        for endpoint in &self.endpoints {
            endpoint.validate(workload_key)?;
            if !allowed_services.contains(endpoint.service.as_str()) {
                return invalid_policy(format!(
                    "workload `{workload_key}` network endpoint `{}` references service `{}` that is not in services.allow",
                    endpoint.name, endpoint.service
                ));
            }
            let key = format!("{}/{}", endpoint.service, endpoint.name);
            if !seen.insert(key.clone()) {
                return invalid_policy(format!(
                    "workload `{workload_key}` network endpoint `{key}` is declared more than once"
                ));
            }
        }
        self.egress.validate(workload_key)?;
        Ok(())
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
    fn validate(&self, workload_key: &str) -> Result<()> {
        validate_required_name(&self.service, "service", workload_key)?;
        validate_required_name(&self.name, "network endpoint", workload_key)?;
        validate_host(&self.host, workload_key)?;
        validate_port(self.host_port, "host_port", workload_key)?;
        if let Some(guest_port) = self.guest_port {
            validate_port(guest_port, "guest_port", workload_key)?;
        }
        Ok(())
    }

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

impl OperatorStoragePolicy {
    fn validate(&self, workload_key: &str) -> Result<()> {
        if let Some(namespace) = &self.namespace {
            validate_storage_namespace(
                namespace,
                &format!("workload `{workload_key}` storage.namespace"),
            )?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorVolumePolicy {
    #[serde(default)]
    pub named: Vec<String>,
}

impl OperatorVolumePolicy {
    fn validate(&self, workload_key: &str) -> Result<()> {
        validate_name_list(
            &self.named,
            &format!("workload `{workload_key}` volumes.named"),
            "volume",
        )?;
        for name in &self.named {
            validate_tenant_volume_name(name).map_err(|error| {
                Error::InvalidInput(format!(
                    "operator policy invalid: workload `{workload_key}` volume `{name}` is invalid: {error}"
                ))
            })?;
        }
        Ok(())
    }
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
            decision = decision.require_provenance(
                provenance.builder_id.clone(),
                normalized_strings(&provenance.predicates),
            );
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
                    predicates: normalized_strings(&provenance.predicates),
                }),
            sbom_required: self.sbom_required,
            allow_local_build: self.allow_local_build,
        }
    }

    fn validate(&self, workload_key: &str, mode: TenantIsolationMode) -> Result<()> {
        if !self.digest_required {
            return invalid_policy(format!(
                "workload `{workload_key}` image.digest_required=false is unsafe; use immutable sha256 digest references"
            ));
        }
        if matches!(mode, TenantIsolationMode::Production) && self.allow_local_build {
            return invalid_policy(format!(
                "workload `{workload_key}` image.allow_local_build=true is not allowed in production policy"
            ));
        }
        if let Some(reference) = &self.reference {
            let parsed = parse_oci_image_reference(reference).map_err(|error| {
                Error::InvalidInput(format!(
                    "operator policy invalid: workload `{workload_key}` image.reference is invalid: {error}"
                ))
            })?;
            if !has_sha256_digest(&parsed) {
                return invalid_policy(format!(
                    "workload `{workload_key}` image.reference must be pinned with @sha256:<64 hex chars>"
                ));
            }
        }
        validate_name_list(
            &self.allowed_registries,
            &format!("workload `{workload_key}` image.allowed_registries"),
            "registry",
        )?;
        if let Some(signature) = &self.signature {
            signature.validate(workload_key)?;
        }
        if let Some(provenance) = &self.provenance {
            provenance.validate(workload_key)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorImageSignaturePolicy {
    pub issuer: String,
    pub subject: String,
}

impl OperatorImageSignaturePolicy {
    fn validate(&self, workload_key: &str) -> Result<()> {
        validate_required_name(&self.issuer, "signature issuer", workload_key)?;
        validate_required_name(&self.subject, "signature subject", workload_key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorImageProvenancePolicy {
    pub builder_id: String,
    #[serde(default)]
    pub predicates: Vec<String>,
}

impl OperatorImageProvenancePolicy {
    fn validate(&self, workload_key: &str) -> Result<()> {
        validate_required_name(&self.builder_id, "provenance builder_id", workload_key)?;
        validate_name_list(
            &self.predicates,
            &format!("workload `{workload_key}` provenance.predicates"),
            "predicate",
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorSecretPolicy {
    #[serde(default)]
    pub handles: Vec<String>,
}

impl OperatorSecretPolicy {
    fn validate(&self, workload_key: &str) -> Result<()> {
        let mut seen = BTreeSet::new();
        for handle in &self.handles {
            if handle.trim().is_empty() || handle.contains(char::is_whitespace) {
                return invalid_policy(format!(
                    "workload `{workload_key}` secret handles must be non-empty references without whitespace"
                ));
            }
            if handle.starts_with("raw:") || handle.contains('=') {
                return invalid_policy(format!(
                    "workload `{workload_key}` secret handle `{handle}` looks like inline secret material"
                ));
            }
            if !seen.insert(handle) {
                return invalid_policy(format!(
                    "workload `{workload_key}` secret handle `{handle}` is declared more than once"
                ));
            }
        }
        Ok(())
    }
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

    fn validate(&self, workload_key: &str) -> Result<()> {
        if let Some(charge) = self.sandbox_charge
            && (charge.active_sandboxes == 0
                || charge.vcpus == 0
                || charge.memory_bytes == 0
                || charge.disk_bytes == 0)
        {
            return invalid_policy(format!(
                "workload `{workload_key}` sandbox_charge must set non-zero active_sandboxes, vcpus, memory_bytes, and disk_bytes"
            ));
        }
        Ok(())
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

impl OperatorAuditPolicy {
    fn validate(&self, workload_key: &str) -> Result<()> {
        if let Some(fields) = &self.redacted_fields {
            validate_redactions(
                fields,
                &format!("workload `{workload_key}` audit.redacted_fields"),
            )?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperatorPolicyEvaluation {
    pub policy_name: Option<String>,
    pub tenant_id: String,
    pub decision_count: usize,
    pub decisions: Vec<OperatorPolicyDecisionEvaluation>,
}

impl OperatorPolicyEvaluation {
    pub fn render_validate_text(&self) -> String {
        let mut output = String::new();
        output.push_str("Policy validation: allowed\n");
        if let Some(name) = &self.policy_name {
            output.push_str(&format!("Policy: {name}\n"));
        }
        output.push_str(&format!("Tenant: {}\n", self.tenant_id));
        output.push_str(&format!("Decisions: {}\n", self.decision_count));
        for decision in &self.decisions {
            output.push_str(&format!(
                "- {} -> {}\n",
                decision.workload_key, decision.decision_id
            ));
        }
        output
    }

    pub fn render_explain_text(&self) -> String {
        let mut output = String::new();
        output.push_str("Policy explanation\n");
        if let Some(name) = &self.policy_name {
            output.push_str(&format!("Policy: {name}\n"));
        }
        output.push_str(&format!("Tenant: {}\n", self.tenant_id));
        for decision in &self.decisions {
            output.push_str(&format!("\n{}\n", decision.workload_key));
            output.push_str(&format!("  decision_id: {}\n", decision.decision_id));
            output.push_str(&format!(
                "  tenant_isolation_mode: {}\n",
                decision.tenant_isolation_mode.as_str()
            ));
            output.push_str(&format!(
                "  runtime_profile: {}\n",
                decision.runtime_profile.label()
            ));
            output.push_str(&format!(
                "  runtime_tier: {}\n",
                decision.runtime_tier.label()
            ));
            output.push_str(&format!(
                "  runtime_admission: {}\n",
                admission_label(&decision.runtime_admission)
            ));
            output.push_str(&format!(
                "  services: {}\n",
                join_or_none(&decision.services)
            ));
            output.push_str(&format!(
                "  network_endpoints: {}\n",
                join_or_none(&decision.network_endpoints)
            ));
            output.push_str(&format!(
                "  sandbox_egress: {}\n",
                join_or_none(&decision.sandbox_egress)
            ));
            output.push_str(&format!(
                "  storage_namespace: {}\n",
                decision.storage_namespace
            ));
            output.push_str(&format!(
                "  named_volumes: {}\n",
                join_or_none(&decision.named_volumes)
            ));
            output.push_str(&format!(
                "  image_policy: {}\n",
                image_policy_summary(&decision.image_policy)
            ));
            output.push_str(&format!(
                "  secret_handle_count: {}\n",
                decision.secret_handle_count
            ));
            output.push_str(&format!(
                "  quotas: {}\n",
                quota_summary(decision.quotas.sandbox_charge)
            ));
            output.push_str(&format!(
                "  audit_redactions: {}\n",
                join_or_none(&decision.audit_redactions)
            ));
            if let Some(external_policy) = &decision.external_policy {
                output.push_str(&format!(
                    "  external_policy: {}\n",
                    external_policy.summary()
                ));
            }
            for trace in &decision.trace {
                output.push_str(&format!("  trace: {trace}\n"));
            }
        }
        output
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperatorPolicyDecisionEvaluation {
    pub workload_key: String,
    pub decision_id: String,
    pub tenant_id: String,
    pub runtime_tier: RuntimeIsolationTier,
    pub runtime_profile: OperatorRuntimeProfile,
    pub tenant_isolation_mode: TenantIsolationMode,
    pub runtime_admission: TenantRuntimePolicyAdmission,
    pub sandbox_backend: Option<SandboxBackendKind>,
    pub sandbox_id: Option<String>,
    pub services: Vec<String>,
    pub network_endpoints: Vec<String>,
    pub sandbox_egress: Vec<String>,
    pub storage_namespace: String,
    pub named_volumes: Vec<String>,
    pub image_policy: OperatorPolicyImageSummary,
    pub image_reference: Option<String>,
    #[serde(skip_serializing)]
    secret_handles: Vec<String>,
    pub secret_handle_count: usize,
    pub quotas: OperatorPolicyQuotaSummary,
    pub audit_redactions: Vec<String>,
    pub external_policy: Option<OperatorExternalPolicyEvidence>,
    pub trace: Vec<String>,
    #[serde(skip_serializing)]
    pub decision: TenantIsolationDecision,
}

impl OperatorPolicyDecisionEvaluation {
    fn external_policy_request(
        &self,
        policy_name: Option<String>,
    ) -> OperatorExternalPolicyRequest {
        OperatorExternalPolicyRequest {
            policy_name,
            tenant_id: self.tenant_id.clone(),
            workload_key: self.workload_key.clone(),
            decision_id: self.decision_id.clone(),
            workload_kind: self.decision.workload().kind().label().to_owned(),
            workload_name: self.decision.workload().name().to_owned(),
            runtime_tier: self.runtime_tier.label().to_owned(),
            tenant_isolation_mode: self.tenant_isolation_mode.as_str().to_owned(),
            runtime_admission: admission_label(&self.runtime_admission),
            sandbox_backend: self.sandbox_backend.map(|backend| format!("{backend:?}")),
            sandbox_id: self.sandbox_id.clone(),
            services: self.services.clone(),
            network_endpoints: self.network_endpoints.clone(),
            sandbox_egress: self.sandbox_egress.clone(),
            storage_namespace: self.storage_namespace.clone(),
            named_volumes: self.named_volumes.clone(),
            image_reference: self.image_reference.clone(),
            secret_handle_count: self.secret_handle_count,
            audit_redactions: self.audit_redactions.clone(),
            policy_bundle_hash: None,
            input_digest: String::new(),
            timeout_millis: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperatorPolicyDiff {
    pub added_workloads: Vec<OperatorPolicyDecisionEvaluation>,
    pub removed_workloads: Vec<OperatorPolicyDecisionEvaluation>,
    pub changed_workloads: Vec<OperatorPolicyDiffSummary>,
}

impl OperatorPolicyDiff {
    pub fn between(from: &OperatorPolicyDocument, to: &OperatorPolicyDocument) -> Result<Self> {
        let from = from.evaluate()?;
        let to = to.evaluate()?;
        let from_by_key: BTreeMap<_, _> = from
            .decisions
            .into_iter()
            .map(|decision| (decision.workload_key.clone(), decision))
            .collect();
        let to_by_key: BTreeMap<_, _> = to
            .decisions
            .into_iter()
            .map(|decision| (decision.workload_key.clone(), decision))
            .collect();

        let mut added_workloads = Vec::new();
        let mut removed_workloads = Vec::new();
        let mut changed_workloads = Vec::new();

        for (key, next) in &to_by_key {
            match from_by_key.get(key) {
                None => added_workloads.push(next.clone()),
                Some(previous) => {
                    if let Some(summary) = OperatorPolicyDiffSummary::between(previous, next) {
                        changed_workloads.push(summary);
                    }
                }
            }
        }
        for (key, previous) in &from_by_key {
            if !to_by_key.contains_key(key) {
                removed_workloads.push(previous.clone());
            }
        }

        Ok(Self {
            added_workloads,
            removed_workloads,
            changed_workloads,
        })
    }

    pub fn render_text(&self) -> String {
        let mut output = String::from("Policy diff\n");
        output.push_str(&format!("Lifecycle: {}\n", self.lifecycle().label()));
        if self.added_workloads.is_empty()
            && self.removed_workloads.is_empty()
            && self.changed_workloads.is_empty()
        {
            output.push_str("No authority changes.\n");
            return output;
        }
        for decision in &self.added_workloads {
            output.push_str(&format!("+ {}\n", decision.workload_key));
        }
        for decision in &self.removed_workloads {
            output.push_str(&format!("- {}\n", decision.workload_key));
        }
        for summary in &self.changed_workloads {
            output.push_str(&format!(
                "~ {} (lifecycle: {})\n",
                summary.workload_key,
                summary.lifecycle.label()
            ));
            for change in &summary.changes {
                output.push_str(&format!("  {change}\n"));
            }
        }
        output
    }

    pub fn lifecycle(&self) -> OperatorPolicyLifecycle {
        if !self.added_workloads.is_empty() || !self.removed_workloads.is_empty() {
            return OperatorPolicyLifecycle::RecreateRequired;
        }
        self.changed_workloads
            .iter()
            .map(|summary| summary.lifecycle)
            .fold(
                OperatorPolicyLifecycle::DynamicReload,
                OperatorPolicyLifecycle::max,
            )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperatorPolicyDiffSummary {
    pub workload_key: String,
    pub lifecycle: OperatorPolicyLifecycle,
    pub changes: Vec<String>,
}

impl OperatorPolicyDiffSummary {
    fn between(
        previous: &OperatorPolicyDecisionEvaluation,
        next: &OperatorPolicyDecisionEvaluation,
    ) -> Option<Self> {
        let mut changes = Vec::new();
        let mut lifecycle = OperatorPolicyLifecycle::DynamicReload;
        if previous.tenant_id != next.tenant_id {
            changes.push(format!(
                "tenant changed: {} -> {}",
                previous.tenant_id, next.tenant_id
            ));
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if previous.tenant_isolation_mode != next.tenant_isolation_mode {
            changes.push(format!(
                "tenant isolation mode changed: {} -> {}",
                previous.tenant_isolation_mode.as_str(),
                next.tenant_isolation_mode.as_str()
            ));
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if previous.runtime_profile != next.runtime_profile {
            changes.push(format!(
                "runtime profile changed: {} -> {}",
                previous.runtime_profile.label(),
                next.runtime_profile.label()
            ));
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if previous.runtime_tier != next.runtime_tier {
            changes.push(format!(
                "runtime tier changed: {} -> {}",
                previous.runtime_tier.label(),
                next.runtime_tier.label()
            ));
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if record_vec_delta(&mut changes, "services", &previous.services, &next.services) {
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if record_vec_delta(
            &mut changes,
            "network endpoints",
            &previous.network_endpoints,
            &next.network_endpoints,
        ) {
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if record_vec_delta(
            &mut changes,
            "sandbox egress",
            &previous.sandbox_egress,
            &next.sandbox_egress,
        ) {
            lifecycle = lifecycle.max(sandbox_egress_reload_lifecycle(next.sandbox_backend));
        }
        if previous.sandbox_backend != next.sandbox_backend {
            changes.push(format!(
                "sandbox backend changed: {} -> {}",
                optional_backend_label(previous.sandbox_backend),
                optional_backend_label(next.sandbox_backend)
            ));
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if previous.sandbox_id != next.sandbox_id {
            changes.push(format!(
                "sandbox id changed: {} -> {}",
                previous.sandbox_id.as_deref().unwrap_or("none"),
                next.sandbox_id.as_deref().unwrap_or("none")
            ));
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if previous.storage_namespace != next.storage_namespace {
            changes.push(format!(
                "storage namespace changed: {} -> {}",
                previous.storage_namespace, next.storage_namespace
            ));
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if record_vec_delta(
            &mut changes,
            "volumes",
            &previous.named_volumes,
            &next.named_volumes,
        ) {
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if record_image_policy_delta(&mut changes, &previous.image_policy, &next.image_policy) {
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if previous.secret_handles != next.secret_handles {
            changes.push(format!(
                "secret handles changed: count {} -> {}",
                previous.secret_handle_count, next.secret_handle_count
            ));
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if previous.quotas != next.quotas {
            changes.push(format!(
                "quotas changed: {} -> {}",
                quota_summary(previous.quotas.sandbox_charge),
                quota_summary(next.quotas.sandbox_charge)
            ));
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        record_vec_delta(
            &mut changes,
            "audit redactions",
            &previous.audit_redactions,
            &next.audit_redactions,
        );
        if admission_label(&previous.runtime_admission) != admission_label(&next.runtime_admission)
        {
            changes.push(format!(
                "runtime admission changed: {} -> {}",
                admission_label(&previous.runtime_admission),
                admission_label(&next.runtime_admission)
            ));
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if previous.decision_id != next.decision_id && changes.is_empty() {
            changes.push(format!(
                "decision authority fingerprint changed: {} -> {}",
                previous.decision_id, next.decision_id
            ));
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        (!changes.is_empty()).then(|| Self {
            workload_key: next.workload_key.clone(),
            lifecycle,
            changes,
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorPolicyLifecycle {
    #[default]
    DynamicReload,
    RecreateRequired,
}

impl OperatorPolicyLifecycle {
    pub fn label(self) -> &'static str {
        match self {
            Self::DynamicReload => "dynamic_reload",
            Self::RecreateRequired => "recreate_required",
        }
    }

    fn max(self, other: Self) -> Self {
        if matches!(self, Self::RecreateRequired) || matches!(other, Self::RecreateRequired) {
            Self::RecreateRequired
        } else {
            Self::DynamicReload
        }
    }
}

fn record_vec_delta(
    changes: &mut Vec<String>,
    label: &str,
    previous: &[String],
    next: &[String],
) -> bool {
    let previous: BTreeSet<_> = previous.iter().cloned().collect();
    let next: BTreeSet<_> = next.iter().cloned().collect();
    let added: Vec<_> = next.difference(&previous).cloned().collect();
    let removed: Vec<_> = previous.difference(&next).cloned().collect();
    let changed = !added.is_empty() || !removed.is_empty();
    if !added.is_empty() {
        changes.push(format!("{label} added: {}", added.join(", ")));
    }
    if !removed.is_empty() {
        changes.push(format!("{label} removed: {}", removed.join(", ")));
    }
    changed
}

fn sandbox_egress_reload_lifecycle(backend: Option<SandboxBackendKind>) -> OperatorPolicyLifecycle {
    match backend {
        Some(SandboxBackendKind::Container) => OperatorPolicyLifecycle::DynamicReload,
        Some(SandboxBackendKind::Krun) | None => OperatorPolicyLifecycle::RecreateRequired,
    }
}

fn record_image_policy_delta(
    changes: &mut Vec<String>,
    previous: &OperatorPolicyImageSummary,
    next: &OperatorPolicyImageSummary,
) -> bool {
    let original_len = changes.len();
    if previous.reference != next.reference {
        changes.push(format!(
            "image reference changed: {} -> {}",
            previous.reference.as_deref().unwrap_or("none"),
            next.reference.as_deref().unwrap_or("none")
        ));
    }
    if previous.digest_required != next.digest_required {
        changes.push(format!(
            "image digest required changed: {} -> {}",
            bool_label(previous.digest_required),
            bool_label(next.digest_required)
        ));
    }
    record_vec_delta(
        changes,
        "image allowed registries",
        &previous.allowed_registries,
        &next.allowed_registries,
    );
    if previous.signature != next.signature {
        changes.push(format!(
            "image signature policy changed: {} -> {}",
            signature_summary(previous.signature.as_ref()),
            signature_summary(next.signature.as_ref())
        ));
    }
    if previous.provenance != next.provenance {
        changes.push(format!(
            "image provenance policy changed: {} -> {}",
            provenance_summary(previous.provenance.as_ref()),
            provenance_summary(next.provenance.as_ref())
        ));
    }
    if previous.sbom_required != next.sbom_required {
        changes.push(format!(
            "image SBOM requirement changed: {} -> {}",
            bool_label(previous.sbom_required),
            bool_label(next.sbom_required)
        ));
    }
    if previous.allow_local_build != next.allow_local_build {
        changes.push(format!(
            "image local-build permission changed: {} -> {}",
            bool_label(previous.allow_local_build),
            bool_label(next.allow_local_build)
        ));
    }
    changes.len() != original_len
}

fn normalized_strings(values: &[String]) -> Vec<String> {
    let mut normalized = values.to_vec();
    normalized.sort();
    normalized.dedup();
    normalized
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

fn invalid_policy<T>(message: impl Into<String>) -> Result<T> {
    Err(Error::InvalidInput(format!(
        "operator policy invalid: {}",
        message.into()
    )))
}

fn validate_required_name(value: &str, label: &str, workload_key: &str) -> Result<()> {
    if value.trim().is_empty() || value == "*" {
        return invalid_policy(format!(
            "workload `{workload_key}` {label} must be a concrete non-empty value"
        ));
    }
    Ok(())
}

fn validate_name_list(values: &[String], field: &str, item_label: &str) -> Result<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        if value.trim().is_empty() || value == "*" {
            return invalid_policy(format!(
                "{field} contains an unsafe {item_label} value `{value}`"
            ));
        }
        if value.contains(char::is_whitespace) {
            return invalid_policy(format!(
                "{field} value `{value}` must not contain whitespace"
            ));
        }
        if !seen.insert(value) {
            return invalid_policy(format!("{field} value `{value}` is duplicated"));
        }
    }
    Ok(())
}

fn validate_storage_namespace(namespace: &str, field: &str) -> Result<()> {
    if namespace != "tenant" {
        return invalid_policy(format!(
            "{field} must be `tenant`; custom storage namespaces are deferred until the storage PEP consumes namespace decisions"
        ));
    }
    Ok(())
}

fn validate_redactions(fields: &[String], field: &str) -> Result<()> {
    validate_name_list(fields, field, "redaction field")?;
    for required in DEFAULT_REDACTED_FIELDS {
        if !fields.iter().any(|field| field == required) {
            return invalid_policy(format!("{field} must include `{required}`"));
        }
    }
    Ok(())
}

fn validate_host(host: &str, workload_key: &str) -> Result<()> {
    let host = host.trim();
    if host.is_empty() || matches!(host, "*" | "0.0.0.0" | "::" | "[::]") {
        return invalid_policy(format!(
            "workload `{workload_key}` network host `{host}` is a wildcard bind, not an admitted egress endpoint"
        ));
    }
    if let Ok(ip) = host.parse::<IpAddr>()
        && ip.is_unspecified()
    {
        return invalid_policy(format!(
            "workload `{workload_key}` network host `{host}` is unspecified"
        ));
    }
    Ok(())
}

fn validate_port(port: u16, field: &str, workload_key: &str) -> Result<()> {
    if port == 0 {
        return invalid_policy(format!(
            "workload `{workload_key}` network {field} must not be 0"
        ));
    }
    Ok(())
}

fn storage_namespace_for_policy(namespace: &str, tenant_id: &TenantId) -> String {
    if namespace == "tenant" {
        tenant_id.as_str().to_string()
    } else {
        namespace.to_string()
    }
}

fn optional_backend_label(backend: Option<SandboxBackendKind>) -> String {
    backend
        .map(|backend| format!("{backend:?}"))
        .unwrap_or_else(|| "none".to_string())
}

fn bool_label(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn image_policy_summary(policy: &OperatorPolicyImageSummary) -> String {
    let mut parts = vec![format!(
        "digest_required={}",
        bool_label(policy.digest_required)
    )];
    if let Some(reference) = &policy.reference {
        parts.push(format!("reference={reference}"));
    }
    if !policy.allowed_registries.is_empty() {
        parts.push(format!(
            "allowed_registries={}",
            policy.allowed_registries.join(",")
        ));
    }
    if let Some(signature) = &policy.signature {
        parts.push(format!("signature={}", signature_summary(Some(signature))));
    }
    if let Some(provenance) = &policy.provenance {
        parts.push(format!(
            "provenance={}",
            provenance_summary(Some(provenance))
        ));
    }
    if policy.sbom_required {
        parts.push("sbom_required=true".to_string());
    }
    if policy.allow_local_build {
        parts.push("allow_local_build=true".to_string());
    }
    parts.join("; ")
}

fn signature_summary(signature: Option<&OperatorImageSignaturePolicy>) -> String {
    signature
        .map(|signature| format!("issuer={}, subject={}", signature.issuer, signature.subject))
        .unwrap_or_else(|| "none".to_string())
}

fn provenance_summary(provenance: Option<&OperatorImageProvenancePolicy>) -> String {
    provenance
        .map(|provenance| {
            let predicates = join_or_none(&provenance.predicates);
            format!(
                "builder_id={}, predicates={predicates}",
                provenance.builder_id
            )
        })
        .unwrap_or_else(|| "none".to_string())
}

fn quota_summary(charge: Option<SandboxResourceCharge>) -> String {
    charge
        .map(|charge| {
            format!(
                "active_sandboxes={}, vcpus={}, memory_bytes={}, disk_bytes={}, log_bytes={}",
                charge.active_sandboxes,
                charge.vcpus,
                charge.memory_bytes,
                charge.disk_bytes,
                charge.log_bytes
            )
        })
        .unwrap_or_else(|| "none".to_string())
}

fn join_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values.join(", ")
    }
}

fn admission_label(admission: &TenantRuntimePolicyAdmission) -> String {
    match admission {
        TenantRuntimePolicyAdmission::AdmitInProcess => "admit_in_process".to_string(),
        TenantRuntimePolicyAdmission::Route {
            recommended_tier,
            reason,
        } => format!("route_to_{} ({reason})", recommended_tier.label()),
    }
}

fn protocol_label(protocol: PublishedEndpointProtocol) -> &'static str {
    match protocol {
        PublishedEndpointProtocol::Tcp => "tcp",
        PublishedEndpointProtocol::Http => "http",
        PublishedEndpointProtocol::Https => "https",
    }
}

#[cfg(test)]
mod tests;
