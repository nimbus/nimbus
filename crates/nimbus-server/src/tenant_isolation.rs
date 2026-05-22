use nimbus_core::{Error, PrincipalContext, Result, TenantId};
use serde_json::{Map, Value};
use std::net::IpAddr;

use nimbus_runtime::{
    RuntimeBackendKind, RuntimeBundle, RuntimeBundleContentKind, RuntimeGrants, RuntimeMode,
    RuntimePolicy, RuntimePreset,
};
use nimbus_sandbox::{SandboxBackendKind, SandboxSpec};

use crate::sandbox::SandboxServiceLaunch;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantIsolationMode {
    LocalDevelopment,
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

impl Default for TenantIsolationMode {
    fn default() -> Self {
        Self::Production
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeIsolationTier {
    InProcessUntrusted,
    InProcessTrustedOnly,
    WasmCapabilitySandbox,
    MicroVmService,
}

impl RuntimeIsolationTier {
    fn label(self) -> &'static str {
        match self {
            Self::InProcessUntrusted => "in_process_untrusted",
            Self::InProcessTrustedOnly => "in_process_trusted_only",
            Self::WasmCapabilitySandbox => "wasm_capability_sandbox",
            Self::MicroVmService => "microvm_service",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TenantIsolationContext {
    tenant_id: TenantId,
    authority: TenantIsolationAuthority,
    surface: &'static str,
    deployment_generation: Option<u64>,
}

impl TenantIsolationContext {
    pub(crate) fn operator(tenant_id: TenantId, surface: &'static str) -> Self {
        Self {
            tenant_id,
            authority: TenantIsolationAuthority::Operator,
            surface,
            deployment_generation: None,
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
        }
    }

    pub(crate) fn system(tenant_id: TenantId, surface: &'static str) -> Self {
        Self {
            tenant_id,
            authority: TenantIsolationAuthority::System,
            surface,
            deployment_generation: None,
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
        context
    }

    pub(crate) fn with_deployment_generation(mut self, generation: u64) -> Self {
        self.deployment_generation = Some(generation);
        self
    }

    pub(crate) fn for_service(
        &self,
        service_name: impl Into<String>,
    ) -> TenantServiceIsolationContext {
        TenantServiceIsolationContext {
            tenant: self.clone(),
            service_name: service_name.into(),
        }
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

    pub(crate) fn ensure_runtime_policy_admitted(
        &self,
        policy: &RuntimePolicy,
        tier: RuntimeIsolationTier,
        mode: TenantIsolationMode,
        context: &str,
    ) -> Result<()> {
        if !matches!(mode, TenantIsolationMode::Production) {
            return Ok(());
        }
        if !matches!(tier, RuntimeIsolationTier::InProcessUntrusted) {
            return Ok(());
        }
        validate_production_in_process_untrusted_policy(policy.limits()).map_err(|rejection| {
            Error::InvalidInput(format!(
                "tenant isolation context for {} on {} rejected {context}: production {} runtime policy {}; route via {}",
                self.authority.describe(),
                self.surface,
                tier.label(),
                rejection.reason,
                rejection.recommended_tier.label()
            ))
        })?;
        Ok(())
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
    if let Some(rest) = grant.strip_prefix('[') {
        if let Some((host, _)) = rest.split_once(']') {
            return host;
        }
    }
    if grant.matches(':').count() == 1 {
        return grant.split_once(':').map_or(grant, |(host, _)| host);
    }
    grant
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TenantServiceIsolationContext {
    tenant: TenantIsolationContext,
    service_name: String,
}

impl TenantServiceIsolationContext {
    pub(crate) fn tenant_id(&self) -> &TenantId {
        self.tenant.tenant_id()
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
                "sandbox service {} for tenant {} requested backend {:?}, but the configured manager backend is {:?}",
                self.service_name,
                self.tenant_id(),
                spec.backend,
                actual_backend
            )));
        }
        if spec.name != self.service_name {
            return Err(Error::InvalidInput(format!(
                "sandbox service catalog returned launch spec name {} for requested service {}",
                spec.name, self.service_name
            )));
        }
        self.tenant
            .ensure_tenant_matches(&spec.tenant_id, "sandbox service launch spec")
    }
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

    #[test]
    fn tenant_isolation_mode_defaults_to_production() {
        assert_eq!(
            TenantIsolationMode::default(),
            TenantIsolationMode::Production
        );
    }

    #[test]
    fn tenant_context_rejects_mismatched_tenant_before_launch() {
        let context = TenantIsolationContext::operator(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            "test",
        )
        .for_service("db");
        let launch = SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
            sparse_spec("tenant-b", "db", SandboxBackendKind::Krun),
            "postgres:16",
        ));

        let error = context
            .ensure_sandbox_launch_matches(&launch, SandboxBackendKind::Krun)
            .expect_err("mismatched tenant must be rejected before sandbox launch");
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
    fn tenant_context_rejects_mismatched_service_before_launch() {
        let context = TenantIsolationContext::operator(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            "test",
        )
        .for_service("db");
        let launch = SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
            sparse_spec("tenant-a", "cache", SandboxBackendKind::Krun),
            "redis:7",
        ));

        let error = context
            .ensure_sandbox_launch_matches(&launch, SandboxBackendKind::Krun)
            .expect_err("mismatched service name must be rejected before sandbox launch");
        assert!(
            error
                .to_string()
                .contains("returned launch spec name cache"),
            "error should name the rejected service: {error}"
        );
    }

    #[test]
    fn tenant_context_rejects_mismatched_backend_before_launch() {
        let context = TenantIsolationContext::operator(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            "test",
        )
        .for_service("db");
        let launch = SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
            sparse_spec("tenant-a", "db", SandboxBackendKind::Container),
            "postgres:16",
        ));

        let error = context
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
        .with_deployment_generation(42);

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
    }

    #[test]
    fn production_untrusted_runtime_admission_allows_web_standard_application_policy() {
        let context = test_application_context();
        let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());

        context
            .ensure_runtime_policy_admitted(
                &policy,
                RuntimeIsolationTier::InProcessUntrusted,
                TenantIsolationMode::Production,
                "runtime invocation",
            )
            .expect("web-standard application grants should be production-admissible");
    }

    #[test]
    fn production_untrusted_runtime_admission_rejects_generic_node_loopback_grants() {
        let context = test_application_context();
        let policy = RuntimePolicy::new(RuntimeLimits::application_node22());

        let error = context
            .ensure_runtime_policy_admitted(
                &policy,
                RuntimeIsolationTier::InProcessUntrusted,
                TenantIsolationMode::Production,
                "runtime invocation",
            )
            .expect_err("node loopback grants must not enter production untrusted runtime");
        assert!(
            error.to_string().contains("generic localhost"),
            "error should explain loopback authority: {error}"
        );
        assert!(
            error.to_string().contains("in_process_untrusted"),
            "error should name the runtime tier: {error}"
        );
        assert!(
            error.to_string().contains("route via microvm_service"),
            "error should name the canonical routing fallback: {error}"
        );
    }

    #[test]
    fn production_untrusted_runtime_admission_routes_trusted_grants_to_trusted_tier() {
        let context = test_application_context();
        let policy = RuntimePolicy::new(RuntimeLimits {
            grants: nimbus_runtime::RuntimeGrants {
                env_write: vec!["DEBUG".to_string()],
                ..nimbus_runtime::RuntimeGrants::application_web_standard()
            },
            ..RuntimeLimits::application_web_standard()
        });

        let error = context
            .ensure_runtime_policy_admitted(
                &policy,
                RuntimeIsolationTier::InProcessUntrusted,
                TenantIsolationMode::Production,
                "runtime invocation",
            )
            .expect_err("trusted-only grants must not enter production untrusted runtime");
        assert!(
            error.to_string().contains("env_write"),
            "error should explain the rejected grant family: {error}"
        );
        assert!(
            error
                .to_string()
                .contains("route via in_process_trusted_only"),
            "error should name the trusted-only routing fallback: {error}"
        );
    }

    #[test]
    fn production_admission_only_validates_in_process_untrusted_tier() {
        let context = test_application_context();
        let policy = RuntimePolicy::new(RuntimeLimits::application_node22());

        context
            .ensure_runtime_policy_admitted(
                &policy,
                RuntimeIsolationTier::MicroVmService,
                TenantIsolationMode::Production,
                "microvm service runtime policy",
            )
            .expect("microVM service routing owns OS isolation outside the in-process gate");
    }

    #[test]
    fn local_development_runtime_admission_preserves_node_compatibility_policy() {
        let context = test_application_context();
        let policy = RuntimePolicy::new(RuntimeLimits::application_node22());

        context
            .ensure_runtime_policy_admitted(
                &policy,
                RuntimeIsolationTier::InProcessUntrusted,
                TenantIsolationMode::LocalDevelopment,
                "runtime invocation",
            )
            .expect("local development mode should preserve Node compatibility localhost grants");
    }
}
