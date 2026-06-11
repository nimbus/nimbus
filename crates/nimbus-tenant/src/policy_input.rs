use nimbus_core::{Error, Result};
use nimbus_runtime::{RuntimePolicy, RuntimeTenantBudget};
use nimbus_sandbox::{
    CompiledSandboxEgressPolicy, PublishedEndpointProtocol, SandboxEgressAuthorization,
    SandboxEgressPolicy, SandboxEgressRequest, SandboxResourceCharge, SandboxSpec,
    validate_sandbox_mounts,
};
use serde::Serialize;

use super::{
    RuntimeIsolationTier, TenantIsolationContext, TenantIsolationMode, TenantRuntimePolicyDecision,
    WorkloadAttributes,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TenantServiceGrantPolicyDecision {
    pub(super) services: Vec<String>,
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
    pub(super) endpoints: Vec<TenantNetworkEndpointDecision>,
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

    pub fn ensure_sandbox_egress_matches(&self, spec: &SandboxSpec, context: &str) -> Result<()> {
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
    pub(super) named_volumes: Vec<String>,
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

    pub fn ensure_sandbox_mounts_match(&self, spec: &SandboxSpec, context: &str) -> Result<()> {
        validate_sandbox_mounts(&spec.mounts).map_err(|message| {
            Error::InvalidInput(format!(
                "tenant volume policy rejected invalid sandbox mounts for {context}: {message}"
            ))
        })?;
        for mount in &spec.mounts {
            let Some(volume_name) = mount.tenant_volume_name() else {
                continue;
            };
            if !self
                .named_volumes
                .iter()
                .any(|allowed| allowed == volume_name)
            {
                return Err(Error::PermissionDenied(format!(
                    "tenant volume policy did not authorize volume `{volume_name}` for {context}"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TenantImagePolicyDecision {
    pub(super) image_reference: Option<String>,
    pub(super) allowed_registries: Vec<String>,
    pub(super) digest_required: bool,
    pub(super) signature_required: bool,
    pub(super) allowed_signature_issuer: Option<String>,
    pub(super) allowed_signature_subject: Option<String>,
    pub(super) provenance_required: bool,
    pub(super) allowed_builder_id: Option<String>,
    pub(super) allowed_source_uri: Option<String>,
    pub(super) required_attestation_predicates: Vec<String>,
    pub(super) sbom_required: bool,
    pub(super) local_build_allowed: bool,
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
            allowed_source_uri: None,
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

    pub(super) fn handle_count(&self) -> usize {
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
    pub(super) redacted_fields: Vec<String>,
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

impl TenantIsolationPolicyInput {
    pub fn new(workload: WorkloadAttributes) -> Self {
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

    pub fn with_runtime_policy(
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
