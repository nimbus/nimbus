use nimbus_core::PrincipalContext;
use serde_json::Value;

pub(super) fn principal_has_exact_service_grant(
    principal: &PrincipalContext,
    service_name: &str,
) -> bool {
    let mut found_exact = false;
    for claims in [&principal.verified_claims, &principal.claims] {
        for claim_name in [
            "nimbus_service_grants",
            "nimbusServiceGrants",
            "service_grants",
            "serviceGrants",
        ] {
            let Some(value) = claims.get(claim_name) else {
                continue;
            };
            if service_grant_value_contains_wildcard(value) {
                return false;
            }
            found_exact |= service_grant_value_contains_exact(value, service_name);
        }
    }
    found_exact
}

fn service_grant_value_contains_wildcard(value: &Value) -> bool {
    match value {
        Value::String(grant) => {
            matches!(grant.as_str(), "*" | "all" | "service:*" | "services:*")
        }
        Value::Array(grants) => grants.iter().any(service_grant_value_contains_wildcard),
        _ => false,
    }
}

fn service_grant_value_contains_exact(value: &Value, service_name: &str) -> bool {
    match value {
        Value::String(grant) => grant == service_name,
        Value::Array(grants) => grants
            .iter()
            .any(|grant| service_grant_value_contains_exact(grant, service_name)),
        _ => false,
    }
}
