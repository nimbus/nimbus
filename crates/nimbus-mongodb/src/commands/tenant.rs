//! Wire `$db` name → Nimbus tenant resolution for the MongoDB adapter.
//!
//! **Cross-adapter contract (the canonical rule).** A tenant boundary is only as
//! strong as whatever *decides* the tenant. The trustworthy model — implemented
//! by the DynamoDB adapter's `AccessKeyRegistry` (`crates/nimbus-dynamodb/src/
//! tenant.rs`) — binds each credential to exactly one `TenantId`, so
//! authentication alone fixes the tenant and no request-supplied field can
//! broaden it. A wire-supplied namespace token (an access key's table prefix, a
//! MongoDB `$db` name) may then only *select within* the already-authenticated
//! tenant's scope, never widen it. "Authentication decides the tenant; a
//! wire-supplied name never does."
//!
//! **Where this adapter stands today (the open deviation, tracked as
//! launch-readiness item M9).** MongoDB authenticates the connection against a
//! single SCRAM credential and then derives the tenant from the wire database
//! name via [`resolve_tenant_id`] — *not* from the authenticated principal. A
//! caller who holds that one credential can therefore reach any tenant by
//! varying the `$db` name on the wire. [`ensure_database_matches_context`] pins
//! every command to the tenant its connection first resolved (so a session
//! scoped to `tenant-a` cannot name `tenant-b`'s database mid-stream), but it
//! cannot constrain which tenant the shared credential was allowed to pick in
//! the first place.
//!
//! This deviation from the contract is load-bearing-mitigated *only* by
//! `guard_listener_is_loopback_only` refusing any non-loopback bind. Before the
//! adapter may bind a routable address, each SCRAM credential must be bound to a
//! specific tenant (mirroring DynamoDB's `AccessKeyRegistry`); until then the
//! loopback guard must stay.

use std::sync::Arc;

use nimbus_core::{PrincipalContext, TenantId};
use nimbus_engine::Engine;
use nimbus_tenant::TenantIsolationContext;

use super::super::error::MongoError;

pub const DEFAULT_TENANT: &str = "default";

fn resolve_tenant_id(db_name: &str) -> Result<TenantId, MongoError> {
    match db_name {
        "admin" | "local" | "config" => TenantId::new(DEFAULT_TENANT).map_err(MongoError::from),
        other => TenantId::new(other).map_err(MongoError::from),
    }
}

pub fn resolve_tenant_context(
    db_name: &str,
    surface: &'static str,
    principal: &PrincipalContext,
) -> Result<TenantIsolationContext, MongoError> {
    let tenant_id = resolve_tenant_id(db_name)?;
    let context = TenantIsolationContext::application(tenant_id, principal.clone(), surface);
    ensure_database_matches_context(&context, db_name, "MongoDB database selection")?;
    Ok(context)
}

pub fn default_tenant_context(
    surface: &'static str,
    principal: &PrincipalContext,
) -> TenantIsolationContext {
    TenantIsolationContext::application(
        TenantId::new(DEFAULT_TENANT).expect("default tenant id should be valid"),
        principal.clone(),
        surface,
    )
}

pub fn ensure_database_matches_context(
    context: &TenantIsolationContext,
    db_name: &str,
    operation: &str,
) -> Result<(), MongoError> {
    let tenant_id = resolve_tenant_id(db_name)?;
    context
        .ensure_tenant_matches(&tenant_id, operation)
        .map_err(MongoError::from)
}

pub fn ensure_tenant(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
) -> Result<(), MongoError> {
    match engine.create_tenant(context.tenant_id().clone()) {
        Ok(()) => Ok(()),
        Err(nimbus_core::Error::AlreadyExists(_)) => Ok(()),
        Err(e) => Err(MongoError::from(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::BAD_VALUE;

    fn test_principal() -> PrincipalContext {
        PrincipalContext {
            authenticated: true,
            claims: serde_json::Map::from_iter([(
                "subject".to_string(),
                serde_json::json!("mongodb-test-user"),
            )]),
            verified_claims: serde_json::Map::new(),
        }
    }

    #[test]
    fn resolves_mongodb_database_to_tenant_context() {
        let principal = test_principal();
        let context = resolve_tenant_context("tenant-a", "mongodb test", &principal)
            .expect("context should resolve");

        assert_eq!(context.tenant_id().as_str(), "tenant-a");
    }

    #[test]
    fn maps_mongodb_internal_databases_to_default_tenant_context() {
        let principal = test_principal();
        for db_name in ["admin", "local", "config"] {
            let context = resolve_tenant_context(db_name, "mongodb test", &principal)
                .expect("context should resolve");

            assert_eq!(context.tenant_id().as_str(), DEFAULT_TENANT);
        }
    }

    #[test]
    fn rejects_database_tenant_mismatch_before_engine_access() {
        let context = TenantIsolationContext::application(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            PrincipalContext::system(),
            "mongodb test",
        );

        let error = ensure_database_matches_context(&context, "tenant-b", "MongoDB test command")
            .expect_err("mismatched MongoDB database must be rejected");

        match error {
            MongoError::Command { code, message, .. } => {
                assert_eq!(code, BAD_VALUE.code);
                assert!(
                    message.contains("authorized tenant tenant-a"),
                    "error should name authorized tenant: {message}"
                );
                assert!(
                    message.contains("referenced tenant tenant-b"),
                    "error should name rejected tenant: {message}"
                );
            }
            other => panic!("expected command error, got {other:?}"),
        }
    }
}
