use nimbus_runtime::{
    RuntimeBackendKind, RuntimeBackendLifecyclePolicy, RuntimeBackendLockdownProfile,
    RuntimeBackendTrustTier, RuntimeBundleContentKind, RuntimeCompatibilityTarget, RuntimeGrants,
    RuntimeJavaScriptEvaluationFormat, RuntimeMode, RuntimePolicy, RuntimePreset,
    RuntimeTenantBudget,
};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

use super::TenantIsolationMode;

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
    backend_trust_tier: RuntimeBackendTrustTier,
    backend_lockdown_profile: RuntimeBackendLockdownProfile,
    backend_lifecycle_policy: RuntimeBackendLifecyclePolicy,
    bundle_content_kind: RuntimeBundleContentKind,
    javascript_evaluation_format: RuntimeJavaScriptEvaluationFormat,
    compatibility_target: RuntimeCompatibilityTarget,
    runtime_mode: RuntimeMode,
    preset: RuntimePreset,
    pub(super) grants: RuntimeGrants,
    tenant_budget: RuntimeTenantBudget,
    admission: TenantRuntimePolicyAdmission,
}

impl TenantRuntimePolicyDecision {
    pub(super) fn not_applicable() -> Self {
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
            backend_trust_tier: limits.backend_trust_tier,
            backend_lockdown_profile: limits.backend_lockdown_profile,
            backend_lifecycle_policy: limits.backend_lifecycle_policy,
            bundle_content_kind: limits.bundle_content_kind,
            javascript_evaluation_format: limits.javascript_evaluation_format,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProductionRuntimePolicyRejection {
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

    pub(super) fn into_route(self) -> RuntimeIsolationRoute {
        RuntimeIsolationRoute::new(self.reason, self.recommended_tier)
    }
}

pub(super) fn validate_production_in_process_untrusted_policy(
    limits: &nimbus_runtime::RuntimeLimits,
) -> std::result::Result<(), ProductionRuntimePolicyRejection> {
    match limits.backend_kind {
        RuntimeBackendKind::V8 | RuntimeBackendKind::BunJsc => {}
        RuntimeBackendKind::Wasmtime => {
            return Err(ProductionRuntimePolicyRejection::wasm_capability_sandbox(
                "uses Wasmtime backend before production WASM capability admission is enabled",
            ));
        }
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
    if limits.compatibility_target.is_node()
        && grants
            .env_read
            .iter()
            .any(|grant| grant == "NODE_TLS_REJECT_UNAUTHORIZED")
    {
        return Err(ProductionRuntimePolicyRejection::trusted_only(
            "includes ambient NODE_TLS_REJECT_UNAUTHORIZED env read grant",
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
                "/" | "*" | "$app_root" | "$cache_root" | "$temp_root" // 002-auth-caching-policy: filesystem grant name, not auth cache
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
