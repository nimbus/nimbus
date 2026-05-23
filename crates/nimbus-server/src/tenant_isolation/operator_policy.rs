use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use nimbus_core::{Error, Result, TenantId};
use nimbus_runtime::{RuntimeLimits, RuntimePolicy};
use nimbus_sandbox::{PublishedEndpointProtocol, SandboxBackendKind, SandboxResourceCharge};
use serde::{Deserialize, Serialize};

use super::{
    RuntimeIsolationTier, TenantAuditRedactionPolicy, TenantImagePolicyDecision,
    TenantIsolationContext, TenantIsolationDecision, TenantIsolationMode,
    TenantNetworkEndpointDecision, TenantNetworkPolicyDecision, TenantQuotaPolicyDecision,
    TenantRuntimePolicyAdmission, TenantSecretPolicyDecision, TenantServiceGrantPolicyDecision,
    TenantStoragePolicyDecision, TenantVolumePolicyDecision, TenantWorkloadIdentity,
    TenantWorkloadKind,
};

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
    pub defaults: OperatorPolicyDefaults,
    pub workloads: Vec<OperatorPolicyWorkload>,
}

impl OperatorPolicyDocument {
    pub fn validate(&self) -> Result<()> {
        self.validate_shape()
    }

    pub fn evaluate(&self) -> Result<OperatorPolicyEvaluation> {
        self.validate_shape()?;
        let tenant_id = TenantId::new(self.tenant.clone())?;
        let mut decisions = Vec::with_capacity(self.workloads.len());
        for workload in &self.workloads {
            decisions.push(self.evaluate_workload(&tenant_id, workload)?);
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
        let audit_redactions = workload
            .audit
            .redacted_fields
            .clone()
            .unwrap_or_else(|| self.defaults.audit_redactions.clone());
        let image_reference = workload.image.reference.clone();
        let endpoint_summaries = workload.network.endpoint_summaries();
        let trace = workload.trace(mode);

        let mut quotas = TenantQuotaPolicyDecision::default()
            .with_runtime_budget(runtime_policy.tenant_budget());
        if let Some(charge) = workload.quotas.sandbox_charge {
            quotas = quotas.with_sandbox_charge(charge);
        }

        let decision = context.admit_decision(
            super::TenantIsolationPolicyInput::new(identity)
                .with_runtime_policy(&context, &runtime_policy, workload.runtime.tier, mode)
                .with_services(TenantServiceGrantPolicyDecision::new(services.clone()))
                .with_network(workload.network.to_decision())
                .with_storage(TenantStoragePolicyDecision::namespace(
                    storage_namespace.clone(),
                ))
                .with_volumes(TenantVolumePolicyDecision::new(
                    workload.volumes.named.clone(),
                ))
                .with_image(workload.image.to_decision())
                .with_secrets(TenantSecretPolicyDecision::handles(
                    workload.secrets.handles.clone(),
                ))
                .with_quotas(quotas)
                .with_audit_redactions(TenantAuditRedactionPolicy {
                    redacted_fields: audit_redactions,
                }),
        )?;

        Ok(OperatorPolicyDecisionEvaluation {
            workload_key: workload.key(),
            decision_id: decision.id().as_str().to_string(),
            tenant_id: decision.tenant_id().as_str().to_string(),
            runtime_tier: decision.runtime().tier(),
            runtime_admission: decision.runtime().admission().clone(),
            services,
            network_endpoints: endpoint_summaries,
            storage_namespace,
            image_reference,
            secret_handle_count: workload.secrets.handles.len(),
            trace,
            decision,
        })
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

    fn trace(&self, mode: TenantIsolationMode) -> Vec<String> {
        let mut trace = vec![
            format!("tenant isolation mode: {}", mode.as_str()),
            format!("runtime profile: {}", self.runtime.profile.label()),
            format!("runtime tier: {}", self.runtime.tier.label()),
            format!("service grants: {}", join_or_none(&self.services.allow)),
            format!(
                "network endpoints: {}",
                join_or_none(&self.network.endpoint_summaries())
            ),
            format!(
                "storage namespace: {}",
                self.storage.namespace.as_deref().unwrap_or("tenant")
            ),
            format!("named volumes: {}", join_or_none(&self.volumes.named)),
            format!("secret handles: {}", self.secrets.handles.len()),
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
        let mut services = self.allow.clone();
        services.sort();
        services.dedup();
        services
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
}

impl OperatorNetworkPolicy {
    fn to_decision(&self) -> TenantNetworkPolicyDecision {
        TenantNetworkPolicyDecision::new(self.endpoints.iter().map(|endpoint| {
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
    }

    fn endpoint_summaries(&self) -> Vec<String> {
        self.endpoints
            .iter()
            .map(OperatorNetworkEndpointPolicy::summary)
            .collect()
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
        )
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
        let mut decision = self
            .reference
            .as_ref()
            .map(TenantImagePolicyDecision::digest_pinned)
            .unwrap_or_default();
        for registry in &self.allowed_registries {
            decision = decision.with_allowed_registry(registry.clone());
        }
        if let Some(signature) = &self.signature {
            decision =
                decision.require_signature(signature.issuer.clone(), signature.subject.clone());
        }
        if let Some(provenance) = &self.provenance {
            decision = decision
                .require_provenance(provenance.builder_id.clone(), provenance.predicates.clone());
        }
        if self.sbom_required {
            decision = decision.require_sbom();
        }
        if self.allow_local_build {
            decision = decision.allow_local_build();
        }
        decision
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
        if let Some(reference) = &self.reference
            && !is_sha256_digest_pinned(reference)
        {
            return invalid_policy(format!(
                "workload `{workload_key}` image.reference must be pinned with @sha256:<64 hex chars>"
            ));
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
                "  storage_namespace: {}\n",
                decision.storage_namespace
            ));
            output.push_str(&format!(
                "  secret_handle_count: {}\n",
                decision.secret_handle_count
            ));
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
    pub runtime_admission: TenantRuntimePolicyAdmission,
    pub services: Vec<String>,
    pub network_endpoints: Vec<String>,
    pub storage_namespace: String,
    pub image_reference: Option<String>,
    pub secret_handle_count: usize,
    pub trace: Vec<String>,
    #[serde(skip_serializing)]
    pub decision: TenantIsolationDecision,
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
            output.push_str(&format!("~ {}\n", summary.workload_key));
            for change in &summary.changes {
                output.push_str(&format!("  {change}\n"));
            }
        }
        output
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperatorPolicyDiffSummary {
    pub workload_key: String,
    pub changes: Vec<String>,
}

impl OperatorPolicyDiffSummary {
    fn between(
        previous: &OperatorPolicyDecisionEvaluation,
        next: &OperatorPolicyDecisionEvaluation,
    ) -> Option<Self> {
        let mut changes = Vec::new();
        record_vec_delta(&mut changes, "services", &previous.services, &next.services);
        record_vec_delta(
            &mut changes,
            "network endpoints",
            &previous.network_endpoints,
            &next.network_endpoints,
        );
        if previous.storage_namespace != next.storage_namespace {
            changes.push(format!(
                "storage namespace changed: {} -> {}",
                previous.storage_namespace, next.storage_namespace
            ));
        }
        if previous.image_reference != next.image_reference {
            changes.push(format!(
                "image reference changed: {} -> {}",
                previous.image_reference.as_deref().unwrap_or("none"),
                next.image_reference.as_deref().unwrap_or("none")
            ));
        }
        if previous.secret_handle_count != next.secret_handle_count {
            changes.push(format!(
                "secret handle count changed: {} -> {}",
                previous.secret_handle_count, next.secret_handle_count
            ));
        }
        if admission_label(&previous.runtime_admission) != admission_label(&next.runtime_admission)
        {
            changes.push(format!(
                "runtime admission changed: {} -> {}",
                admission_label(&previous.runtime_admission),
                admission_label(&next.runtime_admission)
            ));
        }
        (!changes.is_empty()).then(|| Self {
            workload_key: next.workload_key.clone(),
            changes,
        })
    }
}

fn record_vec_delta(changes: &mut Vec<String>, label: &str, previous: &[String], next: &[String]) {
    let previous: BTreeSet<_> = previous.iter().cloned().collect();
    let next: BTreeSet<_> = next.iter().cloned().collect();
    let added: Vec<_> = next.difference(&previous).cloned().collect();
    let removed: Vec<_> = previous.difference(&next).cloned().collect();
    if !added.is_empty() {
        changes.push(format!("{label} added: {}", added.join(", ")));
    }
    if !removed.is_empty() {
        changes.push(format!("{label} removed: {}", removed.join(", ")));
    }
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
    if namespace.trim().is_empty()
        || namespace == "*"
        || namespace.contains('/')
        || namespace.contains(char::is_whitespace)
    {
        return invalid_policy(format!(
            "{field} must be `tenant` or a concrete namespace without whitespace, slash, or wildcard"
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

fn is_sha256_digest_pinned(image_reference: &str) -> bool {
    let Some((_, digest)) = image_reference.rsplit_once("@sha256:") else {
        return false;
    };
    digest.len() == 64 && digest.as_bytes().iter().all(u8::is_ascii_hexdigit)
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
mod tests {
    use super::*;

    const VALID_POLICY: &str = include_str!("../../tests/fixtures/policy/valid-enterprise.yaml");
    const INVALID_WILDCARD: &str =
        include_str!("../../tests/fixtures/policy/invalid-wildcard.yaml");
    const INVALID_PORT: &str = include_str!("../../tests/fixtures/policy/invalid-port.yaml");
    const INVALID_SECRET: &str = include_str!("../../tests/fixtures/policy/invalid-secret.yaml");
    const INVALID_IMAGE: &str = include_str!("../../tests/fixtures/policy/invalid-image.yaml");
    const UNKNOWN_FIELD: &str = include_str!("../../tests/fixtures/policy/unknown-field.yaml");
    const NODE_ROUTE: &str = include_str!("../../tests/fixtures/policy/node-route.yaml");
    const DIFF_FROM: &str = include_str!("../../tests/fixtures/policy/diff-from.yaml");
    const DIFF_TO: &str = include_str!("../../tests/fixtures/policy/diff-to.yaml");

    fn parse_policy(body: &str) -> OperatorPolicyDocument {
        serde_yaml::from_str(body).expect("policy fixture should parse")
    }

    #[test]
    fn valid_policy_fixture_compiles_to_tenant_isolation_decision() {
        let policy = parse_policy(VALID_POLICY);

        let evaluation = policy.evaluate().expect("policy should evaluate");

        assert_eq!(evaluation.tenant_id, "tenant-a");
        assert_eq!(evaluation.decision_count, 1);
        let decision = &evaluation.decisions[0];
        assert_eq!(decision.workload_key, "runtime_function/messages:send");
        assert_eq!(decision.storage_namespace, "tenant-a");
        assert_eq!(decision.services, vec!["db".to_string()]);
        assert_eq!(decision.network_endpoints.len(), 1);
        assert_eq!(
            decision.runtime_admission,
            TenantRuntimePolicyAdmission::AdmitInProcess
        );
        assert!(
            decision.decision_id.starts_with("tid_"),
            "decision id should come from TenantIsolationDecision"
        );
        assert!(
            decision
                .decision
                .to_audit_record()
                .workload_stable_id
                .contains("messages%3Asend"),
            "compiled decision should produce normal tenant-isolation audit evidence"
        );

        let rendered = evaluation.render_explain_text();
        assert!(rendered.contains("runtime_function/messages:send"));
        assert!(rendered.contains("runtime_admission: admit_in_process"));
        assert!(rendered.contains("storage_namespace: tenant-a"));
    }

    #[test]
    fn policy_fixture_rejects_unknown_fields_at_parse_time() {
        let error = serde_yaml::from_str::<OperatorPolicyDocument>(UNKNOWN_FIELD)
            .expect_err("unknown fields should be rejected");
        assert!(
            error.to_string().contains("unknown field"),
            "serde should report strict parsing failure: {error}"
        );
    }

    #[test]
    fn policy_fixture_rejects_wildcard_hosts_and_unsafe_image_defaults() {
        let policy = parse_policy(INVALID_WILDCARD);

        let error = policy
            .evaluate()
            .expect_err("wildcard policy should fail closed");

        assert!(
            error.to_string().contains("wildcard"),
            "error should name the unsafe wildcard: {error}"
        );
    }

    #[test]
    fn policy_fixtures_reject_invalid_port_secret_and_image_shapes() {
        let cases = [
            (INVALID_PORT, "host_port must not be 0"),
            (INVALID_SECRET, "looks like inline secret material"),
            (INVALID_IMAGE, "image.digest_required=false is unsafe"),
        ];

        for (body, expected) in cases {
            let policy = parse_policy(body);
            let error = match policy.evaluate() {
                Ok(_) => panic!("policy should reject `{expected}`"),
                Err(error) => error,
            };
            assert!(
                error.to_string().contains(expected),
                "error should contain `{expected}`: {error}"
            );
        }
    }

    #[test]
    fn node_profile_routes_away_from_production_in_process_untrusted() {
        let policy = parse_policy(NODE_ROUTE);

        let evaluation = policy.evaluate().expect("policy should evaluate");

        let admission = &evaluation.decisions[0].runtime_admission;
        assert!(
            matches!(
                admission,
                TenantRuntimePolicyAdmission::Route {
                    recommended_tier: RuntimeIsolationTier::MicroVmService,
                    ..
                }
            ),
            "existing runtime admission should route broad Node grants: {admission:?}"
        );
        let rendered = evaluation.render_explain_text();
        assert!(rendered.contains("route_to_microvm_service"));
    }

    #[test]
    fn policy_diff_reports_authority_deltas() {
        let from = parse_policy(DIFF_FROM);
        let to = parse_policy(DIFF_TO);

        let diff = OperatorPolicyDiff::between(&from, &to).expect("diff should evaluate");

        assert_eq!(diff.added_workloads.len(), 1);
        assert_eq!(diff.changed_workloads.len(), 1);
        let rendered = diff.render_text();
        assert!(rendered.contains("+ runtime_function/messages:list"));
        assert!(rendered.contains("~ runtime_function/messages:send"));
        assert!(rendered.contains("services added: cache"));
        assert!(rendered.contains("network endpoints added: cache/redis"));
    }
}
