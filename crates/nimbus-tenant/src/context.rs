use nimbus_core::{Error, PrincipalContext, Result, TenantId};
use nimbus_runtime::{RuntimeBundle, RuntimePolicy};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

use super::authority::TenantIsolationAuthority;
use super::{
    RuntimeIsolationTier, RuntimePolicyAdmission, TenantIsolationDecision, TenantIsolationMode,
    TenantIsolationPolicyInput, TenantServiceGrantPolicyDecision, TenantStoragePolicyDecision,
    TenantWorkloadIdentity, TenantWorkloadLocation, runtime_admission,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantIsolationContext {
    pub(super) tenant_id: TenantId,
    pub(super) authority: TenantIsolationAuthority,
    pub(super) surface: &'static str,
    pub(super) deployment_generation: Option<u64>,
    pub(super) location: TenantWorkloadLocation,
}

impl TenantIsolationContext {
    pub fn operator(tenant_id: TenantId, surface: &'static str) -> Self {
        Self {
            tenant_id,
            authority: TenantIsolationAuthority::Operator,
            surface,
            deployment_generation: None,
            location: TenantWorkloadLocation::default(),
        }
    }

    pub fn application(
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

    pub fn system(tenant_id: TenantId, surface: &'static str) -> Self {
        Self {
            tenant_id,
            authority: TenantIsolationAuthority::System,
            surface,
            deployment_generation: None,
            location: TenantWorkloadLocation::default(),
        }
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn ensure_system_or_operator_authority(&self, context: &str) -> Result<()> {
        if self.authority.is_system_or_operator() {
            return Ok(());
        }
        Err(Error::PermissionDenied(format!(
            "{context} requires system/operator authority, but caller is {}",
            self.authority.describe()
        )))
    }

    pub fn reauthorize_application(
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

    pub fn with_deployment_generation(mut self, generation: u64) -> Self {
        self.deployment_generation = Some(generation);
        self
    }

    pub fn with_workload_location(mut self, location: TenantWorkloadLocation) -> Self {
        self.location = location;
        self
    }

    pub fn admit_decision(
        &self,
        input: TenantIsolationPolicyInput,
    ) -> Result<TenantIsolationDecision> {
        TenantIsolationDecision::admit(self, input)
    }

    pub fn ensure_tenant_matches(&self, actual: &TenantId, context: &str) -> Result<()> {
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
        match runtime_admission::validate_production_in_process_untrusted_policy(policy.limits()) {
            Ok(()) => RuntimePolicyAdmission::AdmitInProcess,
            Err(rejection) => RuntimePolicyAdmission::Route(rejection.into_route()),
        }
    }

    pub fn ensure_application_principal_tenant_access(&self, context: &str) -> Result<()> {
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
pub(super) struct TenantPrincipalClaim<'a> {
    pub(super) name: &'static str,
    pub(super) value: &'a str,
}

pub(super) fn principal_tenant_claim(
    principal: &PrincipalContext,
) -> Option<TenantPrincipalClaim<'_>> {
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

pub fn admit_runtime_invocation_decision(
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
