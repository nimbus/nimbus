use nimbus_core::{PrincipalContext, TenantId};

use super::*;

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
fn application_context_rejects_mismatched_principal_tenant_claim() {
    let context = TenantIsolationContext::application(
        TenantId::new("tenant-a").expect("tenant id should parse"),
        principal_with_tenant_claim("tenant_id", "tenant-b"),
        "test",
    );

    let error = context
        .admit_if_principal_claim_absent_or_matching("convex route tenant")
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
        .admit_if_principal_claim_absent_or_matching("convex route tenant")
        .expect("verified tenant claim should take precedence and authorize access");
}

#[test]
fn application_principal_is_readable_only_from_application_authority() {
    let principal = principal_with_tenant_claim("tenant_id", "tenant-a");
    let tenant = TenantId::new("tenant-a").expect("tenant id should parse");

    let application =
        TenantIsolationContext::application(tenant.clone(), principal.clone(), "test");
    assert_eq!(
        application.application_principal(),
        Some(&principal),
        "an application context must hand back the caller it authorizes, so adapters can pass \
         that caller into engine calls that take a principal"
    );

    // Operator and system authority represent Nimbus acting, not a caller, so
    // there is no application principal to read back.
    assert_eq!(
        TenantIsolationContext::operator(tenant.clone(), "test").application_principal(),
        None
    );
    assert_eq!(
        TenantIsolationContext::system(tenant, "test").application_principal(),
        None
    );
}

#[test]
fn application_context_can_require_tenant_claim_for_control_plane_routes() {
    let context = TenantIsolationContext::application(
        TenantId::new("tenant-a").expect("tenant id should parse"),
        PrincipalContext {
            authenticated: true,
            claims: serde_json::Map::new(),
            verified_claims: serde_json::Map::new(),
        },
        "test",
    );

    context
        .admit_if_principal_claim_absent_or_matching("convex route tenant")
        .expect("generic adapter routes may accept principals without tenant claims");
    let error = context
        .require_matching_principal_claim("service lifecycle route")
        .expect_err("service control routes must require a tenant claim");
    assert!(
        error.to_string().contains("has no tenant claim"),
        "error should explain the missing tenant claim: {error}"
    );
    assert!(
        error.to_string().contains("targeted tenant `tenant-a`"),
        "error should name the targeted tenant: {error}"
    );
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
