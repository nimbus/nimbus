use nimbus_core::{Error, PrincipalContext, Result, TenantId};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::net::IpAddr;

use nimbus_runtime::{
    RuntimeBackendKind, RuntimeBundle, RuntimeBundleContentKind, RuntimeCompatibilityTarget,
    RuntimeGrants, RuntimeMode, RuntimePolicy, RuntimePreset, RuntimeTenantBudget,
};
use nimbus_sandbox::{
    CompiledSandboxEgressPolicy, PublishedEndpointProtocol, SandboxBackendKind,
    SandboxEgressAuthorization, SandboxEgressPolicy, SandboxEgressRequest, SandboxResourceCharge,
    SandboxSpec,
};

use crate::sandbox::SandboxServiceLaunch;

mod audit_events;
mod image_admission;
mod operator_policy;

pub use audit_events::{
    TENANT_ISOLATION_EVENT_SCHEMA_VERSION, TenantIsolationEvent, TenantIsolationEventKind,
    TenantIsolationEventResult, TenantIsolationEventValue,
};
pub use image_admission::{
    TenantImageAdmission, TenantImageAdmissionSource, TenantImageAttestationEvidence,
    TenantImageSignatureEvidence, TenantImageVerificationEvidence, TenantImageVerificationProvider,
};
pub use operator_policy::{
    OPERATOR_POLICY_SCHEMA_VERSION, OperatorAuditPolicy, OperatorDeniedEgressEvent,
    OperatorExternalPolicyBackend, OperatorExternalPolicyBackendError,
    OperatorExternalPolicyBackendErrorKind, OperatorExternalPolicyBackendIdentity,
    OperatorExternalPolicyBackendResult, OperatorExternalPolicyDecision,
    OperatorExternalPolicyEvidence, OperatorExternalPolicyOutcome, OperatorExternalPolicyRequest,
    OperatorImagePolicy, OperatorImageProvenancePolicy, OperatorImageSignaturePolicy,
    OperatorNetworkEndpointPolicy, OperatorNetworkPolicy, OperatorPolicyAcceptedRisk,
    OperatorPolicyAdvisory, OperatorPolicyAdvisoryKind, OperatorPolicyAdvisorySeverity,
    OperatorPolicyDecisionEvaluation, OperatorPolicyDefaults, OperatorPolicyDiff,
    OperatorPolicyDiffSummary, OperatorPolicyDocument, OperatorPolicyDraft,
    OperatorPolicyDraftApproval, OperatorPolicyDraftKind, OperatorPolicyDraftStatus,
    OperatorPolicyEvaluation, OperatorPolicyImageSummary, OperatorPolicyLifecycle,
    OperatorPolicyMetadata, OperatorPolicyProofReport, OperatorPolicyQuotaSummary,
    OperatorPolicyReloadOutcome, OperatorPolicyReloadState, OperatorPolicyWorkload,
    OperatorQuotaPolicy, OperatorRuntimePolicy, OperatorRuntimeProfile,
    OperatorSandboxEgressPolicy, OperatorSandboxEgressRulePolicy, OperatorSandboxPolicy,
    OperatorSecretPolicy, OperatorServicePolicy, OperatorStoragePolicy, OperatorVolumePolicy,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TenantIsolationAuthority {
    Operator,
    Application { principal: PrincipalContext },
    System,
}

impl TenantIsolationAuthority {
    fn describe(&self) -> String {
        match self {
            Self::Operator => "operator".to_string(),
            Self::System => "system".to_string(),
            Self::Application { principal } if principal.authenticated => {
                "application(authenticated)".to_string()
            }
            Self::Application { .. } => "application(anonymous)".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantIsolationMode {
    LocalDevelopment,
    #[default]
    Production,
}

impl TenantIsolationMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalDevelopment => "local-development",
            Self::Production => "production",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeIsolationTier {
    InProcessUntrusted,
    InProcessTrustedOnly,
    WasmCapabilitySandbox,
    MicroVmService,
}

impl RuntimeIsolationTier {
    pub fn label(self) -> &'static str {
        match self {
            Self::InProcessUntrusted => "in_process_untrusted",
            Self::InProcessTrustedOnly => "in_process_trusted_only",
            Self::WasmCapabilitySandbox => "wasm_capability_sandbox",
            Self::MicroVmService => "microvm_service",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuntimePolicyAdmission {
    AdmitInProcess,
    Route(RuntimeIsolationRoute),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeIsolationRoute {
    reason: String,
    recommended_tier: RuntimeIsolationTier,
}

impl RuntimeIsolationRoute {
    fn new(reason: impl Into<String>, recommended_tier: RuntimeIsolationTier) -> Self {
        Self {
            reason: reason.into(),
            recommended_tier,
        }
    }

    pub(crate) fn reason(&self) -> &str {
        &self.reason
    }

    pub(crate) fn recommended_tier(&self) -> RuntimeIsolationTier {
        self.recommended_tier
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum TenantIsolationAuthorityDecision {
    Operator,
    Application {
        authenticated: bool,
        principal_snapshot_digest: String,
        tenant_claim_name: Option<&'static str>,
    },
    System,
}

impl TenantIsolationAuthorityDecision {
    fn from_context(context: &TenantIsolationContext) -> Result<Self> {
        match &context.authority {
            TenantIsolationAuthority::Operator => Ok(Self::Operator),
            TenantIsolationAuthority::System => Ok(Self::System),
            TenantIsolationAuthority::Application { principal } => {
                let snapshot = principal.snapshot()?;
                Ok(Self::Application {
                    authenticated: principal.authenticated,
                    principal_snapshot_digest: snapshot.digest,
                    tenant_claim_name: principal_tenant_claim(principal).map(|claim| claim.name),
                })
            }
        }
    }

    pub fn class(&self) -> &'static str {
        match self {
            Self::Operator => "operator",
            Self::Application { .. } => "application",
            Self::System => "system",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantWorkloadKind {
    RuntimeFunction,
    SandboxService,
    HttpRequest,
    SystemTask,
}

impl TenantWorkloadKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::RuntimeFunction => "runtime_function",
            Self::SandboxService => "sandbox_service",
            Self::HttpRequest => "http_request",
            Self::SystemTask => "system_task",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantWorkloadIdentity {
    kind: TenantWorkloadKind,
    name: String,
    runtime_tier: Option<RuntimeIsolationTier>,
    sandbox_backend: Option<SandboxBackendKind>,
    sandbox_id: Option<String>,
    invocation_id: Option<String>,
}

impl TenantWorkloadIdentity {
    pub fn new(kind: TenantWorkloadKind, name: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
            runtime_tier: None,
            sandbox_backend: None,
            sandbox_id: None,
            invocation_id: None,
        }
    }

    pub fn runtime_function(name: impl Into<String>, tier: RuntimeIsolationTier) -> Self {
        Self::new(TenantWorkloadKind::RuntimeFunction, name).with_runtime_tier(tier)
    }

    pub fn sandbox_service(name: impl Into<String>, sandbox_id: impl Into<String>) -> Self {
        Self::new(TenantWorkloadKind::SandboxService, name).with_sandbox_id(sandbox_id)
    }

    pub fn with_runtime_tier(mut self, tier: RuntimeIsolationTier) -> Self {
        self.runtime_tier = Some(tier);
        self
    }

    pub fn with_sandbox_backend(mut self, backend: SandboxBackendKind) -> Self {
        self.sandbox_backend = Some(backend);
        self
    }

    pub fn with_sandbox_id(mut self, sandbox_id: impl Into<String>) -> Self {
        self.sandbox_id = Some(sandbox_id.into());
        self
    }

    pub fn with_invocation_id(mut self, invocation_id: impl Into<String>) -> Self {
        self.invocation_id = Some(invocation_id.into());
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> TenantWorkloadKind {
        self.kind
    }

    pub fn runtime_tier(&self) -> Option<RuntimeIsolationTier> {
        self.runtime_tier
    }

    pub fn sandbox_backend(&self) -> Option<SandboxBackendKind> {
        self.sandbox_backend
    }

    pub fn sandbox_id(&self) -> Option<&str> {
        self.sandbox_id.as_deref()
    }

    pub fn invocation_id(&self) -> Option<&str> {
        self.invocation_id.as_deref()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TenantWorkloadLocation {
    node_id: Option<String>,
    machine_id: Option<String>,
}

impl TenantWorkloadLocation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_node_id(mut self, node_id: impl Into<String>) -> Self {
        self.node_id = Some(node_id.into());
        self
    }

    pub fn with_machine_id(mut self, machine_id: impl Into<String>) -> Self {
        self.machine_id = Some(machine_id.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantWorkloadStableIdentity {
    format_version: &'static str,
    tenant_id: String,
    surface: String,
    deployment_generation: Option<u64>,
    workload_kind: TenantWorkloadKind,
    workload_name: String,
    runtime_tier: Option<RuntimeIsolationTier>,
    runtime_backend: Option<RuntimeBackendKind>,
    sandbox_backend: Option<SandboxBackendKind>,
    node_id: Option<String>,
    machine_id: Option<String>,
    sandbox_id: Option<String>,
    invocation_id: Option<String>,
}

impl TenantWorkloadStableIdentity {
    const FORMAT_VERSION: &'static str = "v1";

    fn from_decision(decision: &TenantIsolationDecision) -> Self {
        let runtime_backend = matches!(
            decision.workload.kind(),
            TenantWorkloadKind::RuntimeFunction
        )
        .then_some(decision.runtime.backend_kind());
        Self {
            format_version: Self::FORMAT_VERSION,
            tenant_id: decision.tenant_id.as_str().to_string(),
            surface: decision.surface.to_string(),
            deployment_generation: decision.deployment_generation,
            workload_kind: decision.workload.kind(),
            workload_name: decision.workload.name().to_string(),
            runtime_tier: decision.workload.runtime_tier(),
            runtime_backend,
            sandbox_backend: decision.workload.sandbox_backend(),
            node_id: decision.location.node_id.clone(),
            machine_id: decision.location.machine_id.clone(),
            sandbox_id: decision.workload.sandbox_id.clone(),
            invocation_id: decision.workload.invocation_id.clone(),
        }
    }

    pub fn stable_id(&self) -> String {
        format!(
            "nimbus-workload:{}{}",
            self.format_version,
            self.path_suffix()
        )
    }

    pub fn spiffe_path(&self) -> String {
        format!(
            "/nimbus/workload/{}{}",
            self.format_version,
            self.path_suffix()
        )
    }

    pub fn spiffe_id(&self, trust_domain: &str) -> Result<String> {
        let trust_domain = validate_spiffe_trust_domain(trust_domain)?;
        Ok(format!("spiffe://{}{}", trust_domain, self.spiffe_path()))
    }

    pub fn tenant_id(&self) -> &str {
        &self.tenant_id
    }

    pub fn deployment_generation(&self) -> Option<u64> {
        self.deployment_generation
    }

    pub fn node_id(&self) -> Option<&str> {
        self.node_id.as_deref()
    }

    pub fn machine_id(&self) -> Option<&str> {
        self.machine_id.as_deref()
    }

    fn path_suffix(&self) -> String {
        let deployment = self
            .deployment_generation
            .map(|generation| generation.to_string())
            .unwrap_or_else(|| "none".to_string());
        let runtime_tier = self
            .runtime_tier
            .map(RuntimeIsolationTier::label)
            .unwrap_or("none");
        let runtime_backend = self
            .runtime_backend
            .map(runtime_backend_label)
            .unwrap_or("none");
        let sandbox_backend = self
            .sandbox_backend
            .map(sandbox_backend_label)
            .unwrap_or("none");

        [
            ("tenant", self.tenant_id.as_str()),
            ("deployment", deployment.as_str()),
            ("surface", self.surface.as_str()),
            ("kind", self.workload_kind.label()),
            ("name", self.workload_name.as_str()),
            ("runtime-tier", runtime_tier),
            ("runtime-backend", runtime_backend),
            ("sandbox-backend", sandbox_backend),
            ("node", self.node_id.as_deref().unwrap_or("none")),
            ("machine", self.machine_id.as_deref().unwrap_or("none")),
            ("sandbox", self.sandbox_id.as_deref().unwrap_or("none")),
            (
                "invocation",
                self.invocation_id.as_deref().unwrap_or("none"),
            ),
        ]
        .into_iter()
        .map(|(label, value)| format!("/{label}/{}", identity_path_segment(value)))
        .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum TenantRuntimePolicyAdmission {
    AdmitInProcess,
    Route {
        recommended_tier: RuntimeIsolationTier,
        reason: String,
    },
}

impl From<RuntimePolicyAdmission> for TenantRuntimePolicyAdmission {
    fn from(admission: RuntimePolicyAdmission) -> Self {
        match admission {
            RuntimePolicyAdmission::AdmitInProcess => Self::AdmitInProcess,
            RuntimePolicyAdmission::Route(route) => Self::Route {
                recommended_tier: route.recommended_tier(),
                reason: route.reason().to_string(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantRuntimePolicyDecision {
    tier: RuntimeIsolationTier,
    tenant_isolation_mode: TenantIsolationMode,
    backend_kind: RuntimeBackendKind,
    bundle_content_kind: RuntimeBundleContentKind,
    compatibility_target: RuntimeCompatibilityTarget,
    runtime_mode: RuntimeMode,
    preset: RuntimePreset,
    grants: RuntimeGrants,
    tenant_budget: RuntimeTenantBudget,
    admission: TenantRuntimePolicyAdmission,
}

impl TenantRuntimePolicyDecision {
    fn not_applicable() -> Self {
        let policy = RuntimePolicy::default();
        Self::from_runtime_policy(
            &policy,
            RuntimeIsolationTier::InProcessUntrusted,
            TenantIsolationMode::Production,
            RuntimePolicyAdmission::AdmitInProcess,
        )
    }

    pub(crate) fn from_runtime_policy(
        policy: &RuntimePolicy,
        tier: RuntimeIsolationTier,
        tenant_isolation_mode: TenantIsolationMode,
        admission: RuntimePolicyAdmission,
    ) -> Self {
        let limits = policy.limits();
        Self {
            tier,
            tenant_isolation_mode,
            backend_kind: limits.backend_kind,
            bundle_content_kind: limits.bundle_content_kind,
            compatibility_target: limits.compatibility_target,
            runtime_mode: limits.mode,
            preset: limits.preset,
            grants: limits.grants.clone(),
            tenant_budget: policy.tenant_budget(),
            admission: admission.into(),
        }
    }

    pub fn grants(&self) -> &RuntimeGrants {
        &self.grants
    }

    pub fn admission(&self) -> &TenantRuntimePolicyAdmission {
        &self.admission
    }

    pub fn tier(&self) -> RuntimeIsolationTier {
        self.tier
    }

    pub fn backend_kind(&self) -> RuntimeBackendKind {
        self.backend_kind
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TenantServiceGrantPolicyDecision {
    services: Vec<String>,
}

impl TenantServiceGrantPolicyDecision {
    pub fn new(services: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            services: services.into_iter().map(Into::into).collect(),
        }
    }

    pub fn services(&self) -> &[String] {
        &self.services
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantNetworkEndpointDecision {
    service_name: String,
    endpoint_name: String,
    protocol: PublishedEndpointProtocol,
    host: String,
    host_port: u16,
    guest_port: Option<u16>,
}

impl TenantNetworkEndpointDecision {
    pub fn new(
        service_name: impl Into<String>,
        endpoint_name: impl Into<String>,
        protocol: PublishedEndpointProtocol,
        host: impl Into<String>,
        host_port: u16,
    ) -> Self {
        Self {
            service_name: service_name.into(),
            endpoint_name: endpoint_name.into(),
            protocol,
            host: host.into(),
            host_port,
            guest_port: None,
        }
    }

    pub fn with_guest_port(mut self, guest_port: u16) -> Self {
        self.guest_port = Some(guest_port);
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TenantNetworkPolicyDecision {
    endpoints: Vec<TenantNetworkEndpointDecision>,
    public_exposure_allowed: bool,
    generic_loopback_allowed: bool,
    sandbox_egress: CompiledSandboxEgressPolicy,
}

impl TenantNetworkPolicyDecision {
    pub fn new(endpoints: impl IntoIterator<Item = TenantNetworkEndpointDecision>) -> Self {
        Self {
            endpoints: endpoints.into_iter().collect(),
            public_exposure_allowed: false,
            generic_loopback_allowed: false,
            sandbox_egress: CompiledSandboxEgressPolicy::deny_all(),
        }
    }

    pub fn endpoints(&self) -> &[TenantNetworkEndpointDecision] {
        &self.endpoints
    }

    pub fn with_sandbox_egress(mut self, sandbox_egress: SandboxEgressPolicy) -> Result<Self> {
        self.sandbox_egress = sandbox_egress.compile().map_err(|message| {
            Error::InvalidInput(format!("invalid sandbox egress policy: {message}"))
        })?;
        Ok(self)
    }

    pub fn sandbox_egress(&self) -> &SandboxEgressPolicy {
        self.sandbox_egress.policy()
    }

    pub fn authorize_sandbox_egress(
        &self,
        request: &SandboxEgressRequest,
    ) -> SandboxEgressAuthorization {
        self.sandbox_egress.authorize(request)
    }

    pub(crate) fn ensure_sandbox_egress_matches(
        &self,
        spec: &SandboxSpec,
        context: &str,
    ) -> Result<()> {
        let spec_egress = spec.egress.compile().map_err(|message| {
            Error::InvalidInput(format!(
                "tenant network policy rejected invalid sandbox egress policy for {context}: {message}"
            ))
        })?;
        if spec_egress == self.sandbox_egress {
            return Ok(());
        }
        Err(Error::InvalidInput(format!(
            "tenant network policy did not authorize sandbox egress policy for {context}"
        )))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantStoragePolicyDecision {
    namespace: String,
}

impl TenantStoragePolicyDecision {
    pub fn namespace(namespace: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
        }
    }

    pub fn namespace_name(&self) -> &str {
        &self.namespace
    }
}

impl Default for TenantStoragePolicyDecision {
    fn default() -> Self {
        Self::namespace("tenant")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TenantVolumePolicyDecision {
    named_volumes: Vec<String>,
    host_binds_allowed: bool,
}

impl TenantVolumePolicyDecision {
    pub fn new(named_volumes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            named_volumes: named_volumes.into_iter().map(Into::into).collect(),
            host_binds_allowed: false,
        }
    }

    pub fn named_volumes(&self) -> &[String] {
        &self.named_volumes
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TenantImagePolicyDecision {
    image_reference: Option<String>,
    allowed_registries: Vec<String>,
    digest_required: bool,
    signature_required: bool,
    allowed_signature_issuer: Option<String>,
    allowed_signature_subject: Option<String>,
    provenance_required: bool,
    allowed_builder_id: Option<String>,
    required_attestation_predicates: Vec<String>,
    sbom_required: bool,
    local_build_allowed: bool,
}

impl TenantImagePolicyDecision {
    pub fn digest_pinned(image_reference: impl Into<String>) -> Self {
        Self {
            image_reference: Some(image_reference.into()),
            allowed_registries: Vec::new(),
            digest_required: true,
            signature_required: false,
            allowed_signature_issuer: None,
            allowed_signature_subject: None,
            provenance_required: false,
            allowed_builder_id: None,
            required_attestation_predicates: Vec::new(),
            sbom_required: false,
            local_build_allowed: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TenantSecretPolicyDecision {
    handles: Vec<String>,
    ambient_materialization_allowed: bool,
}

impl TenantSecretPolicyDecision {
    pub fn handles(handles: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            handles: handles.into_iter().map(Into::into).collect(),
            ambient_materialization_allowed: false,
        }
    }

    fn handle_count(&self) -> usize {
        self.handles.len()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TenantQuotaPolicyDecision {
    runtime_budget: Option<RuntimeTenantBudget>,
    sandbox_charge: Option<SandboxResourceCharge>,
}

impl TenantQuotaPolicyDecision {
    pub fn with_runtime_budget(mut self, budget: RuntimeTenantBudget) -> Self {
        self.runtime_budget = Some(budget);
        self
    }

    pub fn with_sandbox_charge(mut self, charge: SandboxResourceCharge) -> Self {
        self.sandbox_charge = Some(charge);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantAuditRedactionPolicy {
    redacted_fields: Vec<String>,
}

impl TenantAuditRedactionPolicy {
    pub fn redacted_fields(&self) -> &[String] {
        &self.redacted_fields
    }
}

impl Default for TenantAuditRedactionPolicy {
    fn default() -> Self {
        Self {
            redacted_fields: vec![
                "principal_claims".to_string(),
                "bearer_claims".to_string(),
                "secret_handles".to_string(),
                "raw_credentials".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantIsolationPolicyInput {
    workload: TenantWorkloadIdentity,
    runtime: TenantRuntimePolicyDecision,
    services: TenantServiceGrantPolicyDecision,
    network: TenantNetworkPolicyDecision,
    storage: TenantStoragePolicyDecision,
    volumes: TenantVolumePolicyDecision,
    image: TenantImagePolicyDecision,
    secrets: TenantSecretPolicyDecision,
    quotas: TenantQuotaPolicyDecision,
    audit_redactions: TenantAuditRedactionPolicy,
}

impl TenantIsolationPolicyInput {
    pub fn new(workload: TenantWorkloadIdentity) -> Self {
        Self {
            workload,
            runtime: TenantRuntimePolicyDecision::not_applicable(),
            services: TenantServiceGrantPolicyDecision::default(),
            network: TenantNetworkPolicyDecision::default(),
            storage: TenantStoragePolicyDecision::default(),
            volumes: TenantVolumePolicyDecision::default(),
            image: TenantImagePolicyDecision::default(),
            secrets: TenantSecretPolicyDecision::default(),
            quotas: TenantQuotaPolicyDecision::default(),
            audit_redactions: TenantAuditRedactionPolicy::default(),
        }
    }

    pub(crate) fn with_runtime_policy(
        mut self,
        context: &TenantIsolationContext,
        policy: &RuntimePolicy,
        tier: RuntimeIsolationTier,
        mode: TenantIsolationMode,
    ) -> Self {
        let admission = context.admit_runtime_policy(policy, tier, mode);
        self.runtime =
            TenantRuntimePolicyDecision::from_runtime_policy(policy, tier, mode, admission);
        self
    }

    pub fn with_services(mut self, services: TenantServiceGrantPolicyDecision) -> Self {
        self.services = services;
        self
    }

    pub fn with_network(mut self, network: TenantNetworkPolicyDecision) -> Self {
        self.network = network;
        self
    }

    pub fn with_storage(mut self, storage: TenantStoragePolicyDecision) -> Self {
        self.storage = storage;
        self
    }

    pub fn with_volumes(mut self, volumes: TenantVolumePolicyDecision) -> Self {
        self.volumes = volumes;
        self
    }

    pub fn with_image(mut self, image: TenantImagePolicyDecision) -> Self {
        self.image = image;
        self
    }

    pub fn with_secrets(mut self, secrets: TenantSecretPolicyDecision) -> Self {
        self.secrets = secrets;
        self
    }

    pub fn with_quotas(mut self, quotas: TenantQuotaPolicyDecision) -> Self {
        self.quotas = quotas;
        self
    }

    pub fn with_audit_redactions(mut self, audit_redactions: TenantAuditRedactionPolicy) -> Self {
        self.audit_redactions = audit_redactions;
        self
    }
}

#[derive(Serialize)]
struct TenantIsolationDecisionFingerprint<'a> {
    tenant_id: &'a str,
    surface: &'a str,
    authority: &'a TenantIsolationAuthorityDecision,
    deployment_generation: Option<u64>,
    location: &'a TenantWorkloadLocation,
    workload: &'a TenantWorkloadIdentity,
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
    id: TenantIsolationDecisionId,
    tenant_id: TenantId,
    surface: &'static str,
    authority: TenantIsolationAuthorityDecision,
    deployment_generation: Option<u64>,
    location: TenantWorkloadLocation,
    workload: TenantWorkloadIdentity,
    runtime: TenantRuntimePolicyDecision,
    services: TenantServiceGrantPolicyDecision,
    network: TenantNetworkPolicyDecision,
    storage: TenantStoragePolicyDecision,
    volumes: TenantVolumePolicyDecision,
    image: TenantImagePolicyDecision,
    secrets: TenantSecretPolicyDecision,
    quotas: TenantQuotaPolicyDecision,
    audit_redactions: TenantAuditRedactionPolicy,
}

impl TenantIsolationDecision {
    fn admit(context: &TenantIsolationContext, input: TenantIsolationPolicyInput) -> Result<Self> {
        context.ensure_application_principal_tenant_access("tenant isolation decision")?;
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

    pub fn workload(&self) -> &TenantWorkloadIdentity {
        &self.workload
    }

    pub fn workload_stable_identity(&self) -> TenantWorkloadStableIdentity {
        TenantWorkloadStableIdentity::from_decision(self)
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
            namespace: self.storage.namespace.clone(),
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
            workload_stable_id: self.workload_stable_identity().stable_id(),
            workload: self.workload.clone(),
            runtime: self.runtime.clone(),
            services: self.services.clone(),
            network: self.network.clone(),
            storage: self.storage.clone(),
            volumes: self.volumes.clone(),
            image: self.image.clone(),
            secret_handle_count: self.secrets.handle_count(),
            quotas: self.quotas.clone(),
            redacted_fields: self.audit_redactions.redacted_fields.clone(),
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

    pub(crate) fn ensure_sandbox_launch_matches(
        &self,
        launch: &SandboxServiceLaunch,
        actual_backend: SandboxBackendKind,
    ) -> Result<()> {
        let spec = launch.spec();
        self.ensure_sandbox_spec_matches(spec, actual_backend)
    }

    pub(crate) fn ensure_sandbox_spec_matches(
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
        if spec.name != self.service_name {
            return Err(Error::InvalidInput(format!(
                "tenant service access decision {} authorized service {}, but sandbox service catalog returned launch spec name {}",
                self.decision_id.as_str(),
                self.service_name,
                spec.name
            )));
        }
        self.ensure_tenant_matches(&spec.tenant_id, "sandbox service launch spec")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantIsolationAuditRecord {
    decision_id: String,
    tenant_id: String,
    surface: String,
    authority_class: String,
    deployment_generation: Option<u64>,
    workload_stable_id: String,
    workload: TenantWorkloadIdentity,
    runtime: TenantRuntimePolicyDecision,
    services: TenantServiceGrantPolicyDecision,
    network: TenantNetworkPolicyDecision,
    storage: TenantStoragePolicyDecision,
    volumes: TenantVolumePolicyDecision,
    image: TenantImagePolicyDecision,
    secret_handle_count: usize,
    quotas: TenantQuotaPolicyDecision,
    redacted_fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TenantIsolationContext {
    tenant_id: TenantId,
    authority: TenantIsolationAuthority,
    surface: &'static str,
    deployment_generation: Option<u64>,
    location: TenantWorkloadLocation,
}

impl TenantIsolationContext {
    pub(crate) fn operator(tenant_id: TenantId, surface: &'static str) -> Self {
        Self {
            tenant_id,
            authority: TenantIsolationAuthority::Operator,
            surface,
            deployment_generation: None,
            location: TenantWorkloadLocation::default(),
        }
    }

    pub(crate) fn application(
        tenant_id: TenantId,
        principal: PrincipalContext,
        surface: &'static str,
    ) -> Self {
        Self {
            tenant_id,
            authority: TenantIsolationAuthority::Application { principal },
            surface,
            deployment_generation: None,
            location: TenantWorkloadLocation::default(),
        }
    }

    pub(crate) fn system(tenant_id: TenantId, surface: &'static str) -> Self {
        Self {
            tenant_id,
            authority: TenantIsolationAuthority::System,
            surface,
            deployment_generation: None,
            location: TenantWorkloadLocation::default(),
        }
    }

    pub(crate) fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub(crate) fn reauthorize_application(
        &self,
        principal: PrincipalContext,
        surface: &'static str,
    ) -> Self {
        let mut context = Self::application(self.tenant_id.clone(), principal, surface);
        if let Some(generation) = self.deployment_generation {
            context = context.with_deployment_generation(generation);
        }
        context = context.with_workload_location(self.location.clone());
        context
    }

    pub(crate) fn with_deployment_generation(mut self, generation: u64) -> Self {
        self.deployment_generation = Some(generation);
        self
    }

    pub(crate) fn with_workload_location(mut self, location: TenantWorkloadLocation) -> Self {
        self.location = location;
        self
    }

    pub(crate) fn admit_decision(
        &self,
        input: TenantIsolationPolicyInput,
    ) -> Result<TenantIsolationDecision> {
        TenantIsolationDecision::admit(self, input)
    }

    pub(crate) fn ensure_tenant_matches(&self, actual: &TenantId, context: &str) -> Result<()> {
        if actual == &self.tenant_id {
            return Ok(());
        }
        Err(Error::InvalidInput(format!(
            "tenant isolation context for {} on {} authorized tenant {}, but {context} referenced tenant {}",
            self.authority.describe(),
            self.surface,
            self.tenant_id,
            actual
        )))
    }

    pub(crate) fn ensure_runtime_bundle_matches(
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

    pub(crate) fn ensure_deployment_generation_matches(
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
            "tenant isolation context for {} on {} authorized deployment generation {}, but {context} referenced deployment generation {}",
            self.authority.describe(),
            self.surface,
            expected_generation,
            actual_generation
        )))
    }

    pub(crate) fn admit_runtime_policy(
        &self,
        policy: &RuntimePolicy,
        tier: RuntimeIsolationTier,
        mode: TenantIsolationMode,
    ) -> RuntimePolicyAdmission {
        if !matches!(mode, TenantIsolationMode::Production) {
            return RuntimePolicyAdmission::AdmitInProcess;
        }
        if !matches!(tier, RuntimeIsolationTier::InProcessUntrusted) {
            return RuntimePolicyAdmission::AdmitInProcess;
        }
        match validate_production_in_process_untrusted_policy(policy.limits()) {
            Ok(()) => RuntimePolicyAdmission::AdmitInProcess,
            Err(rejection) => RuntimePolicyAdmission::Route(rejection.into_route()),
        }
    }

    pub(crate) fn ensure_application_principal_tenant_access(&self, context: &str) -> Result<()> {
        let TenantIsolationAuthority::Application { principal } = &self.authority else {
            return Ok(());
        };
        let Some(claim) = principal_tenant_claim(principal) else {
            return Ok(());
        };
        if claim.value == self.tenant_id.as_str() {
            return Ok(());
        }
        Err(Error::PermissionDenied(format!(
            "application principal claim `{}` authorizes tenant `{}`, but {context} targeted tenant `{}`",
            claim.name, claim.value, self.tenant_id
        )))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TenantPrincipalClaim<'a> {
    name: &'static str,
    value: &'a str,
}

fn principal_tenant_claim(principal: &PrincipalContext) -> Option<TenantPrincipalClaim<'_>> {
    const CLAIM_NAMES: [&str; 4] = [
        "tenant_id",
        "tenantId",
        "nimbus_tenant_id",
        "nimbusTenantId",
    ];
    for claims in [&principal.verified_claims, &principal.claims] {
        if let Some(claim) = tenant_claim_from_map(claims, CLAIM_NAMES) {
            return Some(claim);
        }
    }
    None
}

fn validate_spiffe_trust_domain(trust_domain: &str) -> Result<&str> {
    let trust_domain = trust_domain.trim();
    if trust_domain.is_empty() {
        return Err(Error::InvalidInput(
            "SPIFFE trust domain cannot be empty".to_string(),
        ));
    }
    if trust_domain.contains("://")
        || trust_domain.contains('/')
        || trust_domain.chars().any(char::is_whitespace)
    {
        return Err(Error::InvalidInput(format!(
            "SPIFFE trust domain `{trust_domain}` must not include a scheme, slash, or whitespace"
        )));
    }
    Ok(trust_domain)
}

fn identity_path_segment(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::new();
    for byte in value.as_bytes().iter().copied() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    encoded
}

fn runtime_backend_label(kind: RuntimeBackendKind) -> &'static str {
    match kind {
        RuntimeBackendKind::V8 => "v8",
    }
}

fn sandbox_backend_label(kind: SandboxBackendKind) -> &'static str {
    match kind {
        SandboxBackendKind::Container => "container",
        SandboxBackendKind::Krun => "krun",
    }
}

pub(crate) fn admit_runtime_invocation_decision(
    context: &TenantIsolationContext,
    function_name: &str,
    invocation_id: Option<&str>,
    policy: &RuntimePolicy,
    tier: RuntimeIsolationTier,
    mode: TenantIsolationMode,
    service_names: impl IntoIterator<Item = String>,
) -> Result<TenantIsolationDecision> {
    let mut admitted_services = BTreeSet::new();
    admitted_services.extend(policy.limits().grants.service.iter().cloned());
    admitted_services.extend(service_names);
    let mut workload = TenantWorkloadIdentity::runtime_function(function_name, tier);
    if let Some(invocation_id) = invocation_id {
        workload = workload.with_invocation_id(invocation_id);
    }
    context.admit_decision(
        TenantIsolationPolicyInput::new(workload)
            .with_runtime_policy(context, policy, tier, mode)
            .with_services(TenantServiceGrantPolicyDecision::new(admitted_services))
            .with_storage(TenantStoragePolicyDecision::namespace(
                context.tenant_id.as_str(),
            )),
    )
}

fn tenant_claim_from_map<'a>(
    claims: &'a Map<String, Value>,
    claim_names: impl IntoIterator<Item = &'static str>,
) -> Option<TenantPrincipalClaim<'a>> {
    claim_names.into_iter().find_map(|name| {
        claims
            .get(name)
            .and_then(Value::as_str)
            .map(|value| TenantPrincipalClaim { name, value })
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProductionRuntimePolicyRejection {
    reason: String,
    recommended_tier: RuntimeIsolationTier,
}

impl ProductionRuntimePolicyRejection {
    fn new(reason: impl Into<String>, recommended_tier: RuntimeIsolationTier) -> Self {
        Self {
            reason: reason.into(),
            recommended_tier,
        }
    }

    fn trusted_only(reason: impl Into<String>) -> Self {
        Self::new(reason, RuntimeIsolationTier::InProcessTrustedOnly)
    }

    fn microvm_service(reason: impl Into<String>) -> Self {
        Self::new(reason, RuntimeIsolationTier::MicroVmService)
    }

    fn wasm_capability_sandbox(reason: impl Into<String>) -> Self {
        Self::new(reason, RuntimeIsolationTier::WasmCapabilitySandbox)
    }

    fn into_route(self) -> RuntimeIsolationRoute {
        RuntimeIsolationRoute::new(self.reason, self.recommended_tier)
    }
}

fn validate_production_in_process_untrusted_policy(
    limits: &nimbus_runtime::RuntimeLimits,
) -> std::result::Result<(), ProductionRuntimePolicyRejection> {
    match limits.backend_kind {
        RuntimeBackendKind::V8 => {}
    }
    if !matches!(
        limits.bundle_content_kind,
        RuntimeBundleContentKind::JavaScript
    ) {
        return Err(ProductionRuntimePolicyRejection::wasm_capability_sandbox(
            format!(
                "uses unsupported bundle content kind {:?}",
                limits.bundle_content_kind
            ),
        ));
    }
    if matches!(limits.mode, RuntimeMode::Privileged) {
        return Err(ProductionRuntimePolicyRejection::trusted_only(
            "uses privileged runtime mode",
        ));
    }
    if !matches!(
        limits.preset,
        RuntimePreset::Application | RuntimePreset::Code
    ) {
        return Err(ProductionRuntimePolicyRejection::trusted_only(format!(
            "uses {:?} preset, which is not an untrusted application preset",
            limits.preset
        )));
    }

    let grants = &limits.grants;
    reject_microvm_grant_family("run", &grants.run)?;
    reject_microvm_grant_family("ffi", &grants.ffi)?;
    reject_trusted_grant_family("env_write", &grants.env_write)?;
    reject_trusted_grant_family("identity", &grants.identity)?;
    reject_trusted_grant_family("tool", &grants.tool)?;
    if let Some(grant) = grants
        .net_connect
        .iter()
        .find(|grant| is_loopback_or_wildcard_network_grant(grant))
    {
        return Err(ProductionRuntimePolicyRejection::microvm_service(format!(
            "includes generic localhost or wildcard network authority `{grant}`"
        )));
    }
    if !grants.net_listen.is_empty() {
        return Err(ProductionRuntimePolicyRejection::microvm_service(format!(
            "includes network listen grants {}; production tenant services must expose endpoints through Nimbus service policy",
            format_grants(&grants.net_listen)
        )));
    }
    reject_microvm_grant_family("worker", &grants.worker)?;
    if grants.sys.iter().any(|grant| grant == "inspector") {
        return Err(ProductionRuntimePolicyRejection::trusted_only(
            "includes inspector sys grant",
        ));
    }
    if let Some(grant) = broad_filesystem_grant(grants) {
        return Err(ProductionRuntimePolicyRejection::microvm_service(format!(
            "includes broad filesystem/package-loading grant `{grant}`"
        )));
    }
    Ok(())
}

fn reject_microvm_grant_family(
    family: &str,
    grants: &[String],
) -> std::result::Result<(), ProductionRuntimePolicyRejection> {
    reject_grant_family(family, grants, RuntimeIsolationTier::MicroVmService)
}

fn reject_trusted_grant_family(
    family: &str,
    grants: &[String],
) -> std::result::Result<(), ProductionRuntimePolicyRejection> {
    reject_grant_family(family, grants, RuntimeIsolationTier::InProcessTrustedOnly)
}

fn reject_grant_family(
    family: &str,
    grants: &[String],
    recommended_tier: RuntimeIsolationTier,
) -> std::result::Result<(), ProductionRuntimePolicyRejection> {
    if grants.is_empty() {
        return Ok(());
    }
    Err(ProductionRuntimePolicyRejection::new(
        format!("includes {family} grants {}", format_grants(grants)),
        recommended_tier,
    ))
}

fn format_grants(grants: &[String]) -> String {
    grants
        .iter()
        .map(|grant| format!("`{grant}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn broad_filesystem_grant(grants: &RuntimeGrants) -> Option<&str> {
    grants
        .read
        .iter()
        .chain(grants.write.iter())
        .map(String::as_str)
        .find(|grant| {
            matches!(
                grant.trim(),
                "/" | "*" | "$app_root" | "$cache_root" | "$temp_root"
            )
        })
}

fn is_loopback_or_wildcard_network_grant(grant: &str) -> bool {
    let host = network_grant_host(grant);
    if matches!(host, "*" | "localhost") {
        return true;
    }
    host.parse::<IpAddr>()
        .is_ok_and(|ip| ip.is_loopback() || ip.is_unspecified())
}

fn network_grant_host(grant: &str) -> &str {
    let grant = grant.trim();
    if let Some(rest) = grant.strip_prefix('[')
        && let Some((host, _)) = rest.split_once(']')
    {
        return host;
    }
    if grant.matches(':').count() == 1 {
        return grant.split_once(':').map_or(grant, |(host, _)| host);
    }
    grant
}

#[cfg(test)]
mod tests {
    use nimbus_runtime::{RuntimeLimits, RuntimePolicy};
    use nimbus_sandbox::{
        SandboxBackendKind, SandboxFilesystemSpec, SandboxImageLaunchSpec, SandboxProcessSpec,
    };

    use super::*;

    fn sparse_spec(tenant: &str, name: &str, backend: SandboxBackendKind) -> SandboxSpec {
        SandboxSpec::new(
            TenantId::new(tenant).expect("tenant id should parse"),
            name,
            backend,
            SandboxFilesystemSpec::new(""),
            SandboxProcessSpec::new(Vec::<String>::new()),
        )
    }

    fn tenant_labeled_bundle(tenant: &str) -> RuntimeBundle {
        RuntimeBundle::for_tenant(
            "bundle.mjs",
            "0000000000000000000000000000000000000000000000000000000000000000",
            tenant,
        )
        .expect("test runtime bundle should build")
    }

    fn test_application_context() -> TenantIsolationContext {
        TenantIsolationContext::application(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            PrincipalContext::anonymous(),
            "test",
        )
    }

    fn tenant_decision_input(
        context: &TenantIsolationContext,
        policy: &RuntimePolicy,
    ) -> TenantIsolationPolicyInput {
        TenantIsolationPolicyInput::new(
            TenantWorkloadIdentity::runtime_function(
                "messages:send",
                RuntimeIsolationTier::InProcessUntrusted,
            )
            .with_invocation_id("invoke-1"),
        )
        .with_runtime_policy(
            context,
            policy,
            RuntimeIsolationTier::InProcessUntrusted,
            TenantIsolationMode::Production,
        )
        .with_services(TenantServiceGrantPolicyDecision::new(["db"]))
        .with_network(TenantNetworkPolicyDecision::new([
            TenantNetworkEndpointDecision::new(
                "db",
                "postgres",
                PublishedEndpointProtocol::Tcp,
                "127.0.0.1",
                15432,
            )
            .with_guest_port(5432),
        ]))
        .with_storage(TenantStoragePolicyDecision::namespace("tenant-a"))
        .with_volumes(TenantVolumePolicyDecision::new(["cache"]))
        .with_image(TenantImagePolicyDecision::digest_pinned(
            "registry.example.com/app@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ))
        .with_secrets(TenantSecretPolicyDecision::handles(["prod/db/password"]))
        .with_quotas(
            TenantQuotaPolicyDecision::default()
                .with_runtime_budget(policy.tenant_budget())
                .with_sandbox_charge(SandboxResourceCharge {
                    active_sandboxes: 1,
                    vcpus: 1,
                    memory_bytes: 512 * 1024 * 1024,
                    disk_bytes: 10 * 1024 * 1024 * 1024,
                    log_bytes: 64 * 1024 * 1024,
                }),
        )
    }

    fn principal_with_tenant_claim(claim: &'static str, tenant: &str) -> PrincipalContext {
        PrincipalContext {
            authenticated: true,
            claims: serde_json::Map::from_iter([(
                claim.to_string(),
                serde_json::Value::String(tenant.to_string()),
            )]),
            verified_claims: serde_json::Map::new(),
        }
    }

    fn production_untrusted_route(policy: &RuntimePolicy) -> RuntimeIsolationRoute {
        let context = test_application_context();
        match context.admit_runtime_policy(
            policy,
            RuntimeIsolationTier::InProcessUntrusted,
            TenantIsolationMode::Production,
        ) {
            RuntimePolicyAdmission::Route(route) => route,
            RuntimePolicyAdmission::AdmitInProcess => {
                panic!("policy should have produced a runtime isolation route")
            }
        }
    }

    #[test]
    fn tenant_isolation_decision_has_stable_id_and_audit_safe_redaction() {
        let principal = PrincipalContext {
            authenticated: true,
            claims: serde_json::Map::from_iter([
                (
                    "tenant_id".to_string(),
                    serde_json::Value::String("tenant-a".to_string()),
                ),
                (
                    "email".to_string(),
                    serde_json::Value::String("operator@example.com".to_string()),
                ),
            ]),
            verified_claims: serde_json::Map::new(),
        };
        let context = TenantIsolationContext::application(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            principal,
            "convex.runtime",
        )
        .with_deployment_generation(7);
        let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
        let input = tenant_decision_input(&context, &policy);

        let decision = context
            .admit_decision(input.clone())
            .expect("decision should admit matching tenant authority");
        let same_decision = context
            .admit_decision(input)
            .expect("same decision inputs should admit again");

        assert_eq!(
            decision.id(),
            same_decision.id(),
            "decision IDs must be stable for identical admitted inputs"
        );
        assert_eq!(decision.tenant_id().as_str(), "tenant-a");
        assert_eq!(decision.workload().name(), "messages:send");
        assert_eq!(decision.storage().namespace_name(), "tenant-a");
        assert_eq!(decision.network().endpoints().len(), 1);
        assert!(matches!(
            decision.runtime().admission(),
            TenantRuntimePolicyAdmission::AdmitInProcess
        ));

        let audit = decision.to_audit_record();
        let serialized =
            serde_json::to_string(&audit).expect("audit record should serialize to JSON");
        assert!(
            serialized.contains(decision.id().as_str()),
            "audit record should carry the decision ID: {serialized}"
        );
        assert!(
            serialized.contains("\"secret_handle_count\":1"),
            "audit record should expose secret counts without handles: {serialized}"
        );
        assert!(
            !serialized.contains("prod/db/password"),
            "audit record must not leak raw secret handles: {serialized}"
        );
        assert!(
            !serialized.contains("operator@example.com"),
            "audit record must not leak principal claims: {serialized}"
        );
        assert!(
            decision
                .audit_redactions()
                .redacted_fields()
                .contains(&"secret_handles".to_string()),
            "decision should advertise secret-handle redaction"
        );
        assert!(
            decision
                .audit_redactions()
                .redacted_fields()
                .contains(&"principal_claims".to_string()),
            "decision should advertise principal-claim redaction"
        );

        let changed_workload = TenantIsolationPolicyInput::new(
            TenantWorkloadIdentity::runtime_function(
                "messages:list",
                RuntimeIsolationTier::InProcessUntrusted,
            )
            .with_invocation_id("invoke-1"),
        )
        .with_runtime_policy(
            &context,
            &policy,
            RuntimeIsolationTier::InProcessUntrusted,
            TenantIsolationMode::Production,
        );
        let changed_decision = context
            .admit_decision(changed_workload)
            .expect("changed workload should still admit");
        assert_ne!(
            decision.id(),
            changed_decision.id(),
            "decision ID must change when workload identity changes"
        );
    }

    #[test]
    fn tenant_workload_stable_identity_includes_location_and_spiffe_shape() {
        let principal = principal_with_tenant_claim("tenant_id", "tenant-a");
        let context = TenantIsolationContext::application(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            principal,
            "convex.runtime",
        )
        .with_deployment_generation(7)
        .with_workload_location(
            TenantWorkloadLocation::new()
                .with_node_id("node-a")
                .with_machine_id("default"),
        );
        let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
        let decision = context
            .admit_decision(tenant_decision_input(&context, &policy))
            .expect("decision should admit matching tenant authority");

        let identity = decision.workload_stable_identity();

        assert_eq!(identity.tenant_id(), "tenant-a");
        assert_eq!(identity.deployment_generation(), Some(7));
        assert_eq!(identity.node_id(), Some("node-a"));
        assert_eq!(identity.machine_id(), Some("default"));
        assert_eq!(
            identity.stable_id(),
            "nimbus-workload:v1/tenant/tenant-a/deployment/7/surface/convex.runtime/kind/runtime_function/name/messages%3Asend/runtime-tier/in_process_untrusted/runtime-backend/v8/sandbox-backend/none/node/node-a/machine/default/sandbox/none/invocation/invoke-1"
        );
        assert_eq!(
            identity.spiffe_path(),
            "/nimbus/workload/v1/tenant/tenant-a/deployment/7/surface/convex.runtime/kind/runtime_function/name/messages%3Asend/runtime-tier/in_process_untrusted/runtime-backend/v8/sandbox-backend/none/node/node-a/machine/default/sandbox/none/invocation/invoke-1"
        );
        assert_eq!(
            identity
                .spiffe_id("nimbus.local")
                .expect("trust domain should be valid"),
            "spiffe://nimbus.local/nimbus/workload/v1/tenant/tenant-a/deployment/7/surface/convex.runtime/kind/runtime_function/name/messages%3Asend/runtime-tier/in_process_untrusted/runtime-backend/v8/sandbox-backend/none/node/node-a/machine/default/sandbox/none/invocation/invoke-1"
        );

        let audit_json = serde_json::to_string(&decision.to_audit_record())
            .expect("audit record should serialize");
        assert!(
            audit_json.contains("\"workload_stable_id\""),
            "audit record should expose the canonical workload identity: {audit_json}"
        );
        assert!(
            audit_json.contains("messages%3Asend"),
            "audit record should use the stable escaped workload name: {audit_json}"
        );
    }

    #[test]
    fn tenant_workload_stable_identity_distinguishes_sandbox_backend_and_location() {
        let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
        let context_a = test_application_context()
            .with_deployment_generation(9)
            .with_workload_location(
                TenantWorkloadLocation::new()
                    .with_node_id("node-a")
                    .with_machine_id("machine-a"),
            );
        let context_b = test_application_context()
            .with_deployment_generation(9)
            .with_workload_location(
                TenantWorkloadLocation::new()
                    .with_node_id("node-b")
                    .with_machine_id("machine-b"),
            );
        let input = TenantIsolationPolicyInput::new(
            TenantWorkloadIdentity::sandbox_service("db:primary", "sandbox-1")
                .with_sandbox_backend(SandboxBackendKind::Krun),
        )
        .with_runtime_policy(
            &context_a,
            &policy,
            RuntimeIsolationTier::MicroVmService,
            TenantIsolationMode::Production,
        );

        let decision_a = context_a
            .admit_decision(input.clone())
            .expect("first location should admit");
        let decision_b = context_b
            .admit_decision(input)
            .expect("second location should admit");

        assert_ne!(
            decision_a.id(),
            decision_b.id(),
            "location must participate in the immutable decision fingerprint"
        );
        assert_eq!(
            decision_a.workload_stable_identity().stable_id(),
            "nimbus-workload:v1/tenant/tenant-a/deployment/9/surface/test/kind/sandbox_service/name/db%3Aprimary/runtime-tier/none/runtime-backend/none/sandbox-backend/krun/node/node-a/machine/machine-a/sandbox/sandbox-1/invocation/none"
        );
        assert_eq!(
            decision_b.workload_stable_identity().stable_id(),
            "nimbus-workload:v1/tenant/tenant-a/deployment/9/surface/test/kind/sandbox_service/name/db%3Aprimary/runtime-tier/none/runtime-backend/none/sandbox-backend/krun/node/node-b/machine/machine-b/sandbox/sandbox-1/invocation/none"
        );
    }

    #[test]
    fn tenant_workload_stable_identity_rejects_invalid_spiffe_trust_domains() {
        let context = test_application_context();
        let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
        let decision = context
            .admit_decision(tenant_decision_input(&context, &policy))
            .expect("decision should admit");
        let identity = decision.workload_stable_identity();

        for trust_domain in [
            "",
            "spiffe://nimbus.local",
            "nimbus.local/path",
            "nimbus local",
        ] {
            let error = identity
                .spiffe_id(trust_domain)
                .expect_err("invalid trust domain should be rejected");
            assert!(
                error.to_string().contains("SPIFFE trust domain"),
                "error should name the invalid trust-domain field: {error}"
            );
        }
    }

    #[test]
    fn tenant_isolation_decision_clones_inputs_so_policy_cannot_widen_after_admission() {
        let context = test_application_context().with_deployment_generation(11);
        let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
        let mut input = tenant_decision_input(&context, &policy);

        let decision = context
            .admit_decision(input.clone())
            .expect("decision should admit initial policy input");

        input.services.services.push("other-tenant-db".to_string());
        input
            .network
            .endpoints
            .push(TenantNetworkEndpointDecision::new(
                "other-tenant-db",
                "postgres",
                PublishedEndpointProtocol::Tcp,
                "127.0.0.1",
                25432,
            ));
        input
            .volumes
            .named_volumes
            .push("other-tenant-cache".to_string());
        input.runtime.grants.run.push("npm".to_string());

        assert_eq!(
            decision.services().services(),
            &["db".to_string()],
            "admitted service grants should be immutable snapshots"
        );
        assert_eq!(
            decision.network().endpoints().len(),
            1,
            "admitted endpoint grants should be immutable snapshots"
        );
        assert_eq!(
            decision.volumes().named_volumes(),
            &["cache".to_string()],
            "admitted volume grants should be immutable snapshots"
        );
        assert!(
            decision.runtime().grants().run.is_empty(),
            "admitted runtime grants should not widen after input mutation"
        );

        let tenant_b = TenantId::new("tenant-b").expect("tenant id should parse");
        let error = decision
            .ensure_tenant_matches(&tenant_b, "lower seam forged tenant")
            .expect_err("decision must remain tenant-bound");
        assert!(
            error.to_string().contains("authorized tenant tenant-a"),
            "error should name the admitted tenant: {error}"
        );

        let error = decision
            .ensure_deployment_generation_matches(12, "stale runtime invocation")
            .expect_err("decision must remain deployment-bound");
        assert!(
            error
                .to_string()
                .contains("authorized deployment generation 11"),
            "error should name the admitted deployment generation: {error}"
        );
    }

    #[test]
    fn tenant_isolation_decision_issues_narrow_service_and_storage_access() {
        let context = test_application_context();
        let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
        let decision = context
            .admit_decision(tenant_decision_input(&context, &policy))
            .expect("decision should admit");

        let service = decision
            .service_access("db", "host bridge service lookup")
            .expect("admitted service should receive a narrow access decision");
        assert_eq!(service.decision_id(), decision.id());
        assert_eq!(service.tenant_id().as_str(), "tenant-a");
        assert_eq!(service.service_name(), "db");

        let storage = decision.storage_access();
        assert_eq!(storage.decision_id(), decision.id());
        assert_eq!(storage.tenant_id().as_str(), "tenant-a");
        assert_eq!(storage.namespace_name(), "tenant-a");

        let tenant_b = TenantId::new("tenant-b").expect("tenant id should parse");
        let error = storage
            .ensure_tenant_matches(&tenant_b, "runtime storage host operation")
            .expect_err("storage projection must reject a forged lower-seam tenant");
        assert!(
            error.to_string().contains("authorized tenant tenant-a"),
            "error should name the admitted tenant: {error}"
        );
    }

    #[test]
    fn tenant_isolation_decision_rejects_unadmitted_service_grants() {
        let context = test_application_context();
        let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
        let decision = context
            .admit_decision(tenant_decision_input(&context, &policy))
            .expect("decision should admit");

        let error = decision
            .service_access("other-tenant-db", "host bridge service lookup")
            .expect_err("service access must be limited to the admitted grant set");
        assert!(
            error.to_string().contains("permission denied"),
            "error should map to permission denial: {error}"
        );
        assert!(
            error
                .to_string()
                .contains("did not authorize service `other-tenant-db`"),
            "error should name the rejected service: {error}"
        );
    }

    #[test]
    fn tenant_isolation_decision_rejects_mismatched_application_claims() {
        let context = TenantIsolationContext::application(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            principal_with_tenant_claim("tenant_id", "tenant-b"),
            "convex.runtime",
        );
        let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
        let input = tenant_decision_input(&context, &policy);

        let error = context
            .admit_decision(input)
            .expect_err("mismatched application claims must not receive a decision");

        assert!(
            error.to_string().contains("permission denied"),
            "error should map to permission denial: {error}"
        );
        assert!(
            error.to_string().contains("tenant `tenant-b`"),
            "error should name the claimed tenant: {error}"
        );
    }

    #[test]
    fn tenant_isolation_mode_defaults_to_production() {
        assert_eq!(
            TenantIsolationMode::default(),
            TenantIsolationMode::Production
        );
    }

    #[test]
    fn tenant_isolation_decision_rejects_mismatched_sandbox_launch() {
        let context = test_application_context();
        let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
        let decision = context
            .admit_decision(tenant_decision_input(&context, &policy))
            .expect("decision should admit");
        let service = decision
            .service_access("db", "sandbox service launch")
            .expect("db service should be admitted");
        let launch = SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
            sparse_spec("tenant-b", "db", SandboxBackendKind::Krun),
            "postgres:16",
        ));

        let error = service
            .ensure_sandbox_launch_matches(&launch, SandboxBackendKind::Krun)
            .expect_err("service projection must reject a forged launch tenant");
        assert!(
            error.to_string().contains("authorized tenant tenant-a"),
            "error should name the admitted tenant: {error}"
        );
        assert!(
            error.to_string().contains("referenced tenant tenant-b"),
            "error should name the forged tenant: {error}"
        );
    }

    #[test]
    fn tenant_isolation_decision_rejects_mismatched_runtime_bundle_before_invocation() {
        let context = test_application_context();
        let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
        let decision = context
            .admit_decision(tenant_decision_input(&context, &policy))
            .expect("decision should admit");
        let bundle = tenant_labeled_bundle("tenant-b");

        let error = decision
            .ensure_runtime_bundle_matches(&bundle, "runtime bundle")
            .expect_err("decision must reject a forged runtime bundle tenant");
        assert!(
            error.to_string().contains("authorized tenant tenant-a"),
            "error should name the authorized tenant: {error}"
        );
        assert!(
            error.to_string().contains("referenced tenant tenant-b"),
            "error should name the rejected tenant: {error}"
        );
    }

    #[test]
    fn tenant_isolation_decision_rejects_mismatched_service_before_launch() {
        let context = test_application_context();
        let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
        let decision = context
            .admit_decision(tenant_decision_input(&context, &policy))
            .expect("decision should admit");
        let service = decision
            .service_access("db", "sandbox service launch")
            .expect("db service should be admitted");
        let launch = SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
            sparse_spec("tenant-a", "cache", SandboxBackendKind::Krun),
            "redis:7",
        ));

        let error = service
            .ensure_sandbox_launch_matches(&launch, SandboxBackendKind::Krun)
            .expect_err("mismatched service name must be rejected before sandbox launch");
        assert!(
            error.to_string().contains("authorized service db"),
            "error should name the rejected service: {error}"
        );
    }

    #[test]
    fn tenant_isolation_decision_rejects_mismatched_backend_before_launch() {
        let context = test_application_context();
        let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
        let decision = context
            .admit_decision(tenant_decision_input(&context, &policy))
            .expect("decision should admit");
        let service = decision
            .service_access("db", "sandbox service launch")
            .expect("db service should be admitted");
        let launch = SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
            sparse_spec("tenant-a", "db", SandboxBackendKind::Container),
            "postgres:16",
        ));

        let error = service
            .ensure_sandbox_launch_matches(&launch, SandboxBackendKind::Krun)
            .expect_err("mismatched backend must be rejected before sandbox launch");
        assert!(
            error.to_string().contains("requested backend Container"),
            "error should name the rejected backend: {error}"
        );
    }

    #[test]
    fn tenant_context_rejects_mismatched_runtime_bundle_before_invocation() {
        let context = TenantIsolationContext::application(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            PrincipalContext::anonymous(),
            "test",
        );
        let bundle = tenant_labeled_bundle("tenant-b");

        let error = context
            .ensure_runtime_bundle_matches(&bundle, "runtime bundle")
            .expect_err("mismatched runtime bundle tenant must be rejected before invocation");
        assert!(
            error.to_string().contains("authorized tenant tenant-a"),
            "error should name the authorized tenant: {error}"
        );
        assert!(
            error.to_string().contains("referenced tenant tenant-b"),
            "error should name the rejected tenant: {error}"
        );
    }

    #[test]
    fn application_context_rejects_mismatched_principal_tenant_claim() {
        let context = TenantIsolationContext::application(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            principal_with_tenant_claim("tenant_id", "tenant-b"),
            "test",
        );

        let error = context
            .ensure_application_principal_tenant_access("convex route tenant")
            .expect_err("mismatched application tenant claim must be rejected");
        assert!(
            error.to_string().contains("permission denied"),
            "error should map to permission denial: {error}"
        );
        assert!(
            error.to_string().contains("authorizes tenant `tenant-b`"),
            "error should name the authorized tenant claim: {error}"
        );
        assert!(
            error.to_string().contains("targeted tenant `tenant-a`"),
            "error should name the rejected target tenant: {error}"
        );
    }

    #[test]
    fn application_context_allows_matching_verified_principal_tenant_claim() {
        let mut principal = principal_with_tenant_claim("tenant_id", "tenant-b");
        principal.verified_claims = serde_json::Map::from_iter([(
            "tenantId".to_string(),
            serde_json::Value::String("tenant-a".to_string()),
        )]);
        let context = TenantIsolationContext::application(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            principal,
            "test",
        );

        context
            .ensure_application_principal_tenant_access("convex route tenant")
            .expect("verified tenant claim should take precedence and authorize access");
    }

    #[test]
    fn tenant_context_rejects_mismatched_deployment_before_invocation() {
        let context = TenantIsolationContext::application(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            PrincipalContext::anonymous(),
            "test",
        )
        .with_deployment_generation(7);

        let error = context
            .ensure_deployment_generation_matches(8, "runtime invocation")
            .expect_err("mismatched deployment generation must be rejected");
        assert!(
            error
                .to_string()
                .contains("authorized deployment generation 7"),
            "error should name the authorized deployment generation: {error}"
        );
        assert!(
            error
                .to_string()
                .contains("referenced deployment generation 8"),
            "error should name the rejected deployment generation: {error}"
        );
    }

    #[test]
    fn reauthorized_application_context_preserves_tenant_and_deployment_generation() {
        let context = TenantIsolationContext::application(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            PrincipalContext::anonymous(),
            "convex websocket route",
        )
        .with_deployment_generation(42)
        .with_workload_location(
            TenantWorkloadLocation::new()
                .with_node_id("node-source")
                .with_machine_id("machine-source"),
        );

        let derived =
            context.reauthorize_application(PrincipalContext::system(), "convex subscription");

        derived
            .ensure_tenant_matches(
                &TenantId::new("tenant-a").expect("tenant id should parse"),
                "derived context tenant",
            )
            .expect("derived context should preserve tenant identity");
        derived
            .ensure_deployment_generation_matches(42, "derived context deployment")
            .expect("derived context should preserve deployment generation");
        let error = derived
            .ensure_deployment_generation_matches(43, "stale subscription runtime")
            .expect_err("derived context must still reject stale deployment generations");
        assert!(
            error
                .to_string()
                .contains("authorized deployment generation 42"),
            "error should name the preserved deployment generation: {error}"
        );

        let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
        let decision = derived
            .admit_decision(tenant_decision_input(&derived, &policy))
            .expect("derived context should still admit matching tenant");
        let identity = decision.workload_stable_identity();
        assert_eq!(identity.node_id(), Some("node-source"));
        assert_eq!(identity.machine_id(), Some("machine-source"));
    }

    #[test]
    fn production_untrusted_runtime_admission_allows_web_standard_application_policy() {
        let context = test_application_context();
        let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());

        assert_eq!(
            context.admit_runtime_policy(
                &policy,
                RuntimeIsolationTier::InProcessUntrusted,
                TenantIsolationMode::Production,
            ),
            RuntimePolicyAdmission::AdmitInProcess,
            "web-standard application grants should be production-admissible"
        );
    }

    #[test]
    fn production_untrusted_runtime_admission_rejects_generic_node_loopback_grants() {
        let policy = RuntimePolicy::new(RuntimeLimits::application_node22());

        let route = production_untrusted_route(&policy);

        assert!(
            route.reason().contains("generic localhost"),
            "route should explain loopback authority: {route:?}"
        );
        assert_eq!(
            route.recommended_tier(),
            RuntimeIsolationTier::MicroVmService,
            "route should name the canonical routing fallback"
        );
    }

    #[test]
    fn production_untrusted_runtime_admission_routes_package_manager_run_to_microvm() {
        let policy = RuntimePolicy::new(RuntimeLimits {
            grants: nimbus_runtime::RuntimeGrants {
                run: vec!["npm".to_string()],
                ..nimbus_runtime::RuntimeGrants::application_web_standard()
            },
            ..RuntimeLimits::application_node22()
        });

        let route = production_untrusted_route(&policy);

        assert_eq!(
            route.recommended_tier(),
            RuntimeIsolationTier::MicroVmService
        );
        assert!(
            route.reason().contains("run grants `npm`"),
            "route should name the package-manager subprocess authority: {route:?}"
        );
    }

    #[test]
    fn production_untrusted_runtime_admission_routes_native_addon_package_loading_to_microvm() {
        let policy = RuntimePolicy::new(RuntimeLimits {
            grants: nimbus_runtime::RuntimeGrants {
                read: vec![
                    "$generated_root".to_string(),
                    "$app_root".to_string(),
                    "$cache_root".to_string(),
                ],
                ..nimbus_runtime::RuntimeGrants::application_web_standard()
            },
            ..RuntimeLimits::application_web_standard()
        });

        let route = production_untrusted_route(&policy);

        assert_eq!(
            route.recommended_tier(),
            RuntimeIsolationTier::MicroVmService
        );
        assert!(
            route.reason().contains("broad filesystem/package-loading"),
            "route should name the native-addon/package-loading authority: {route:?}"
        );
    }

    #[test]
    fn production_untrusted_runtime_admission_routes_trusted_grants_to_trusted_tier() {
        let policy = RuntimePolicy::new(RuntimeLimits {
            grants: nimbus_runtime::RuntimeGrants {
                env_write: vec!["DEBUG".to_string()],
                ..nimbus_runtime::RuntimeGrants::application_web_standard()
            },
            ..RuntimeLimits::application_web_standard()
        });

        let route = production_untrusted_route(&policy);

        assert!(
            route.reason().contains("env_write"),
            "route should explain the rejected grant family: {route:?}"
        );
        assert_eq!(
            route.recommended_tier(),
            RuntimeIsolationTier::InProcessTrustedOnly,
            "route should name the trusted-only routing fallback"
        );
    }

    #[test]
    fn production_admission_only_validates_in_process_untrusted_tier() {
        let context = test_application_context();
        let policy = RuntimePolicy::new(RuntimeLimits::application_node22());

        assert_eq!(
            context.admit_runtime_policy(
                &policy,
                RuntimeIsolationTier::MicroVmService,
                TenantIsolationMode::Production,
            ),
            RuntimePolicyAdmission::AdmitInProcess,
            "microVM service routing owns OS isolation outside the in-process gate"
        );
    }

    #[test]
    fn local_development_runtime_admission_preserves_node_compatibility_policy() {
        let context = test_application_context();
        let policy = RuntimePolicy::new(RuntimeLimits::application_node22());

        assert_eq!(
            context.admit_runtime_policy(
                &policy,
                RuntimeIsolationTier::InProcessUntrusted,
                TenantIsolationMode::LocalDevelopment,
            ),
            RuntimePolicyAdmission::AdmitInProcess,
            "local development mode should preserve Node compatibility localhost grants"
        );
    }
}
