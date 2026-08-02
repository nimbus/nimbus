use std::collections::BTreeSet;
use std::net::IpAddr;

use nimbus_core::{Error, Result, is_valid_dns_hostname};
use nimbus_egress::{CompiledEgressPolicy, EgressAuthorization, EgressPolicy, EgressRequest};
use nimbus_network::EndpointProtocol;
use nimbus_runtime::{RuntimePolicy, RuntimeTenantBudget};
use nimbus_sandbox::{SandboxResourceCharge, SandboxSpec, validate_sandbox_mounts};
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
    protocol: EndpointProtocol,
    host: String,
    host_port: u16,
    guest_port: Option<u16>,
}

impl TenantNetworkEndpointDecision {
    pub fn new(
        service_name: impl Into<String>,
        endpoint_name: impl Into<String>,
        protocol: EndpointProtocol,
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

    /// Logical admitted service that owns this endpoint.
    pub fn service_name(&self) -> &str {
        &self.service_name
    }

    /// Stable endpoint name within the admitted service.
    pub fn endpoint_name(&self) -> &str {
        &self.endpoint_name
    }

    /// Admitted transport protocol.
    pub const fn protocol(&self) -> EndpointProtocol {
        self.protocol
    }

    /// Desired bare DNS name or IP literal, before any provider observation.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Desired host-side port.
    pub const fn host_port(&self) -> u16 {
        self.host_port
    }

    /// Optional desired sandbox guest port.
    pub const fn guest_port(&self) -> Option<u16> {
        self.guest_port
    }
}

const fn endpoint_protocol_sort_key(protocol: EndpointProtocol) -> u8 {
    match protocol {
        EndpointProtocol::Tcp => 0,
        EndpointProtocol::Http => 1,
        EndpointProtocol::Https => 2,
    }
}

pub(super) struct NetworkEndpointValidationInput<'a> {
    service_name: &'a str,
    endpoint_name: &'a str,
    host: &'a str,
    host_port: u16,
    guest_port: Option<u16>,
}

impl<'a> NetworkEndpointValidationInput<'a> {
    pub(super) const fn new(
        service_name: &'a str,
        endpoint_name: &'a str,
        host: &'a str,
        host_port: u16,
        guest_port: Option<u16>,
    ) -> Self {
        Self {
            service_name,
            endpoint_name,
            host,
            host_port,
            guest_port,
        }
    }
}

pub(super) fn validate_network_endpoints<'endpoint, 'service>(
    endpoints: impl IntoIterator<Item = NetworkEndpointValidationInput<'endpoint>>,
    admitted_services: impl IntoIterator<Item = &'service str>,
) -> std::result::Result<(), String> {
    let admitted_services: BTreeSet<_> = admitted_services.into_iter().collect();
    let mut seen = BTreeSet::new();
    for endpoint in endpoints {
        validate_concrete_name(endpoint.service_name, "service")?;
        validate_concrete_name(endpoint.endpoint_name, "network endpoint")?;
        validate_network_host(endpoint.host)?;
        validate_network_port(endpoint.host_port, "host_port")?;
        if let Some(guest_port) = endpoint.guest_port {
            validate_network_port(guest_port, "guest_port")?;
        }
        if !admitted_services.contains(endpoint.service_name) {
            return Err(format!(
                "network endpoint `{}` references service `{}` that is not in services.allow",
                endpoint.endpoint_name, endpoint.service_name
            ));
        }
        let key = (endpoint.service_name, endpoint.endpoint_name);
        if !seen.insert(key) {
            return Err(format!(
                "network endpoint `{}/{}` is declared more than once",
                endpoint.service_name, endpoint.endpoint_name
            ));
        }
    }
    Ok(())
}

fn validate_concrete_name(value: &str, label: &str) -> std::result::Result<(), String> {
    if value.trim().is_empty() || value == "*" {
        return Err(format!("{label} must be a concrete non-empty value"));
    }
    if value.contains(char::is_whitespace) {
        return Err(format!("{label} `{value}` must not contain whitespace"));
    }
    Ok(())
}

fn validate_network_host(host: &str) -> std::result::Result<(), String> {
    if host.trim().is_empty() {
        return Err("network host must be a concrete non-empty value".to_owned());
    }
    if host != host.trim() || host.contains(char::is_whitespace) {
        return Err(format!("network host `{host}` must not contain whitespace"));
    }
    if host == "*" || host.contains('*') {
        return Err(format!(
            "network host `{host}` is a wildcard bind, not an admitted egress endpoint"
        ));
    }
    if host.contains("://")
        || host.contains('/')
        || host.contains('\\')
        || host.contains('@')
        || host.starts_with('[')
        || host.ends_with(']')
    {
        return Err(format!(
            "network host `{host}` must be a bare DNS name or IP literal, not a URL or authority"
        ));
    }
    if let Ok(ip) = host.parse::<IpAddr>()
        && ip.is_unspecified()
    {
        return Err(format!("network host `{host}` is unspecified"));
    }
    if host.parse::<IpAddr>().is_ok() {
        return Ok(());
    }
    if host.contains(':') {
        return Err(format!(
            "network host `{host}` must not include a port or brackets"
        ));
    }
    if !is_valid_dns_hostname(host) {
        return Err(format!(
            "network host `{host}` must be a valid DNS hostname or IP literal"
        ));
    }
    Ok(())
}

fn validate_network_port(port: u16, field: &str) -> std::result::Result<(), String> {
    if port == 0 {
        return Err(format!("network {field} must not be 0"));
    }
    Ok(())
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TenantNetworkPolicyDecision {
    pub(super) endpoints: Vec<TenantNetworkEndpointDecision>,
    sandbox_egress: CompiledEgressPolicy,
}

impl TenantNetworkPolicyDecision {
    pub fn new(endpoints: impl IntoIterator<Item = TenantNetworkEndpointDecision>) -> Self {
        let mut endpoints: Vec<_> = endpoints.into_iter().collect();
        endpoints.sort_by(|left, right| {
            (
                left.service_name.as_str(),
                left.endpoint_name.as_str(),
                endpoint_protocol_sort_key(left.protocol),
                left.host.as_str(),
                left.host_port,
                left.guest_port,
            )
                .cmp(&(
                    right.service_name.as_str(),
                    right.endpoint_name.as_str(),
                    endpoint_protocol_sort_key(right.protocol),
                    right.host.as_str(),
                    right.host_port,
                    right.guest_port,
                ))
        });
        Self {
            endpoints,
            sandbox_egress: CompiledEgressPolicy::deny_all(),
        }
    }

    pub fn endpoints(&self) -> &[TenantNetworkEndpointDecision] {
        &self.endpoints
    }

    pub(super) fn validate_for_admission(
        &self,
        services: &TenantServiceGrantPolicyDecision,
    ) -> Result<()> {
        validate_network_endpoints(
            self.endpoints.iter().map(|endpoint| {
                NetworkEndpointValidationInput::new(
                    endpoint.service_name(),
                    endpoint.endpoint_name(),
                    endpoint.host(),
                    endpoint.host_port(),
                    endpoint.guest_port(),
                )
            }),
            services.services().iter().map(String::as_str),
        )
        .map_err(|message| Error::InvalidInput(format!("tenant network policy invalid: {message}")))
    }

    pub fn with_sandbox_egress(mut self, sandbox_egress: EgressPolicy) -> Result<Self> {
        self.sandbox_egress = sandbox_egress.compile().map_err(|message| {
            Error::InvalidInput(format!("invalid sandbox egress policy: {message}"))
        })?;
        Ok(self)
    }

    pub fn sandbox_egress(&self) -> &EgressPolicy {
        self.sandbox_egress.policy()
    }

    pub fn authorize_sandbox_egress(&self, request: &EgressRequest) -> EgressAuthorization {
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
