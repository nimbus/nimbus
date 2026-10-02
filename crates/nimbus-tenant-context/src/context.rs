use nimbus_core::{Error, PrincipalContext, Result, TenantId};
use serde_json::{Map, Value};

use super::{TenantIsolationAuthority, WorkloadLocation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantIsolationContext {
    tenant_id: TenantId,
    authority: TenantIsolationAuthority,
    surface: &'static str,
    deployment_generation: Option<u64>,
    location: WorkloadLocation,
}

impl TenantIsolationContext {
    pub fn operator(tenant_id: TenantId, surface: &'static str) -> Self {
        Self {
            tenant_id,
            authority: TenantIsolationAuthority::Operator,
            surface,
            deployment_generation: None,
            location: WorkloadLocation::default(),
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
            location: WorkloadLocation::default(),
        }
    }

    pub fn system(tenant_id: TenantId, surface: &'static str) -> Self {
        Self {
            tenant_id,
            authority: TenantIsolationAuthority::System,
            surface,
            deployment_generation: None,
            location: WorkloadLocation::default(),
        }
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn authority(&self) -> &TenantIsolationAuthority {
        &self.authority
    }

    pub fn surface(&self) -> &'static str {
        self.surface
    }

    pub fn deployment_generation(&self) -> Option<u64> {
        self.deployment_generation
    }

    pub fn location(&self) -> &WorkloadLocation {
        &self.location
    }

    /// The application principal this context authorizes.
    ///
    /// `None` for operator and system authority, which represent Nimbus itself
    /// acting rather than a caller. Adapters read this back to pass the caller
    /// into engine calls that take a principal, so a table access policy sees
    /// who is actually asking.
    pub fn application_principal(&self) -> Option<&PrincipalContext> {
        match &self.authority {
            TenantIsolationAuthority::Application { principal } => Some(principal),
            TenantIsolationAuthority::Operator | TenantIsolationAuthority::System => None,
        }
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

    pub fn with_workload_location(mut self, location: WorkloadLocation) -> Self {
        self.location = location;
        self
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

    /// Admit an application principal whose tenant claim is either absent or
    /// matches this context. Use only when an upstream route has already bound
    /// the request principal to the tenant by another trusted mechanism.
    pub fn admit_if_principal_claim_absent_or_matching(&self, context: &str) -> Result<()> {
        self.validate_application_principal_claim(context, MissingPrincipalClaimPolicy::Admit)
    }

    /// Require application principals to carry a tenant claim matching this
    /// context. Use this for control-plane routes addressed by tenant id.
    pub fn require_matching_principal_claim(&self, context: &str) -> Result<()> {
        self.validate_application_principal_claim(context, MissingPrincipalClaimPolicy::Deny)
    }

    fn validate_application_principal_claim(
        &self,
        context: &str,
        missing_claim_policy: MissingPrincipalClaimPolicy,
    ) -> Result<()> {
        let TenantIsolationAuthority::Application { principal } = &self.authority else {
            return Ok(());
        };
        let Some(claim) = principal_tenant_claim(principal) else {
            return match missing_claim_policy {
                MissingPrincipalClaimPolicy::Admit => Ok(()),
                MissingPrincipalClaimPolicy::Deny => Err(Error::PermissionDenied(format!(
                    "application principal has no tenant claim, but {context} targeted tenant `{}`",
                    self.tenant_id
                ))),
            };
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
enum MissingPrincipalClaimPolicy {
    Admit,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TenantPrincipalClaim<'a> {
    pub name: &'static str,
    pub value: &'a str,
}

pub fn principal_tenant_claim(principal: &PrincipalContext) -> Option<TenantPrincipalClaim<'_>> {
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
