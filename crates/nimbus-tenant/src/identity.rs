use nimbus_core::{Error, Result};
use nimbus_runtime::RuntimeBackendKind;
use nimbus_sandbox::SandboxBackendKind;
use serde::{Deserialize, Serialize};

use super::{RuntimeIsolationTier, TenantIsolationDecision};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadKind {
    RuntimeFunction,
    Service,
    Sandbox,
    HttpRequest,
    SystemTask,
}

impl WorkloadKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::RuntimeFunction => "runtime_function",
            Self::Service => "service",
            Self::Sandbox => "sandbox",
            Self::HttpRequest => "http_request",
            Self::SystemTask => "system_task",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkloadAttributes {
    kind: WorkloadKind,
    name: String,
    runtime_tier: Option<RuntimeIsolationTier>,
    sandbox_backend: Option<SandboxBackendKind>,
    sandbox_id: Option<String>,
    invocation_id: Option<String>,
}

impl WorkloadAttributes {
    pub fn new(kind: WorkloadKind, name: impl Into<String>) -> Self {
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
        Self::new(WorkloadKind::RuntimeFunction, name).with_runtime_tier(tier)
    }

    pub fn service(name: impl Into<String>) -> Self {
        Self::new(WorkloadKind::Service, name)
    }

    pub fn sandbox(name: impl Into<String>) -> Self {
        Self::new(WorkloadKind::Sandbox, name)
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

    pub fn kind(&self) -> WorkloadKind {
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
pub struct WorkloadLocation {
    node_id: Option<String>,
    machine_id: Option<String>,
}

impl WorkloadLocation {
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
pub struct WorkloadIdentity {
    format_version: &'static str,
    tenant_id: String,
    surface: String,
    deployment_generation: Option<u64>,
    workload_kind: WorkloadKind,
    workload_name: String,
    runtime_tier: Option<RuntimeIsolationTier>,
    runtime_backend: Option<RuntimeBackendKind>,
    sandbox_backend: Option<SandboxBackendKind>,
    node_id: Option<String>,
    machine_id: Option<String>,
    sandbox_id: Option<String>,
    invocation_id: Option<String>,
}

impl WorkloadIdentity {
    const FORMAT_VERSION: &'static str = "v1";

    pub(super) fn from_decision(decision: &TenantIsolationDecision) -> Self {
        let runtime_backend = matches!(decision.workload.kind(), WorkloadKind::RuntimeFunction)
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

    pub fn subject(&self) -> String {
        format!(
            "nimbus-workload:{}{}",
            self.format_version,
            self.subject_suffix()
        )
    }

    pub fn audit_projection(&self) -> String {
        format!(
            "nimbus-workload-audit:{}{}",
            self.format_version,
            self.audit_projection_suffix()
        )
    }

    pub fn spiffe_path(&self) -> String {
        format!(
            "/nimbus/workload/{}{}",
            self.format_version,
            self.subject_suffix()
        )
    }

    pub fn audit_projection_path(&self) -> String {
        format!(
            "/nimbus/workload-audit/{}{}",
            self.format_version,
            self.audit_projection_suffix()
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

    pub fn sandbox_id(&self) -> Option<&str> {
        self.sandbox_id.as_deref()
    }

    pub fn invocation_id(&self) -> Option<&str> {
        self.invocation_id.as_deref()
    }

    fn subject_suffix(&self) -> String {
        self.path_suffix(false)
    }

    fn audit_projection_suffix(&self) -> String {
        self.path_suffix(true)
    }

    fn path_suffix(&self, include_placement: bool) -> String {
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

        let mut segments = vec![
            ("tenant", self.tenant_id.as_str()),
            ("deployment", deployment.as_str()),
            ("surface", self.surface.as_str()),
            ("kind", self.workload_kind.label()),
            ("name", self.workload_name.as_str()),
            ("runtime-tier", runtime_tier),
            ("runtime-backend", runtime_backend),
            ("sandbox-backend", sandbox_backend),
        ];
        if include_placement {
            segments.extend([
                ("node", self.node_id.as_deref().unwrap_or("none")),
                ("machine", self.machine_id.as_deref().unwrap_or("none")),
                ("sandbox", self.sandbox_id.as_deref().unwrap_or("none")),
                (
                    "invocation",
                    self.invocation_id.as_deref().unwrap_or("none"),
                ),
            ]);
        }
        segments
            .into_iter()
            .map(|(label, value)| format!("/{label}/{}", identity_path_segment(value)))
            .collect()
    }
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
        RuntimeBackendKind::BunJsc => "bun_jsc",
    }
}

fn sandbox_backend_label(kind: SandboxBackendKind) -> &'static str {
    match kind {
        SandboxBackendKind::Container => "container",
        SandboxBackendKind::Krun => "krun",
    }
}
