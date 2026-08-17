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
//! `guard_bind_address` refusing any non-loopback bind while the credential is
//! unbound. Before the adapter may bind a routable address, each SCRAM credential
//! must be bound to a specific tenant (mirroring DynamoDB's `AccessKeyRegistry`);
//! until then the loopback guard must stay.

use std::sync::Arc;

use nimbus_core::{PrincipalContext, TenantId};
use nimbus_engine::Engine;
use nimbus_tenant::TenantIsolationContext;

use super::super::error::{MongoError, UNAUTHORIZED};

pub const DEFAULT_TENANT: &str = "default";

/// Well-known `verified_claims` key under which a successful bound-mode SCRAM
/// handshake records the authenticated tenant. Written by
/// `ConnectionState::authenticated_principal`; read only through
/// [`authenticated_tenant_from_principal`].
pub(crate) const AUTHENTICATED_TENANT_CLAIM: &str = "nimbus_authenticated_tenant";

fn resolve_tenant_id(db_name: &str) -> Result<TenantId, MongoError> {
    let tenant_id = match db_name {
        "admin" | "local" | "config" => TenantId::new(DEFAULT_TENANT).map_err(MongoError::from),
        other => TenantId::new(other).map_err(MongoError::from),
    }?;
    if tenant_id.is_nimbus_reserved() {
        return Err(reserved_tenant_refused(&tenant_id));
    }
    Ok(tenant_id)
}

fn reserved_tenant_refused(referenced: &TenantId) -> MongoError {
    MongoError::Command {
        code: UNAUTHORIZED.code,
        code_name: UNAUTHORIZED.code_name.into(),
        message: format!(
            "MongoDB database {referenced} names a reserved Nimbus tenant; application database \
             selection is refused"
        ),
    }
}

/// Read back the authentication-bound tenant carried in the principal.
///
/// Returns `Some` only in bound mode, where the SCRAM handshake recorded the
/// tenant under [`AUTHENTICATED_TENANT_CLAIM`]. Centralizes the read so every
/// tenant-resolution path observes the same source of truth.
pub(crate) fn authenticated_tenant_from_principal(
    principal: &PrincipalContext,
) -> Option<TenantId> {
    let raw = principal
        .verified_claims
        .get(AUTHENTICATED_TENANT_CLAIM)?
        .as_str()?;
    TenantId::new(raw).ok()
}

/// Refusal returned when a bound connection's wire `$db` names a tenant other
/// than the one authentication bound.
fn cross_tenant_refused(authorized: &TenantId, referenced: &TenantId) -> MongoError {
    MongoError::Command {
        code: UNAUTHORIZED.code,
        code_name: UNAUTHORIZED.code_name.into(),
        message: format!(
            "authenticated credential authorized tenant {authorized}, but the command referenced \
             tenant {referenced}; the wire $db cannot select a tenant other than the one \
             authentication bound"
        ),
    }
}

pub fn resolve_tenant_context(
    db_name: &str,
    surface: &'static str,
    principal: &PrincipalContext,
) -> Result<TenantIsolationContext, MongoError> {
    let db_tenant = resolve_tenant_id(db_name)?;

    // The flip: in bound mode authentication decides the tenant. The wire `$db`
    // may only *select within* the authenticated tenant — a `$db` naming any
    // other tenant is refused. In unbound (loopback-only) mode there is no
    // authenticated tenant, so today's behavior holds and the `$db` decides.
    let tenant_id = match authenticated_tenant_from_principal(principal) {
        Some(auth_tenant) => {
            if db_tenant != auth_tenant {
                return Err(cross_tenant_refused(&auth_tenant, &db_tenant));
            }
            auth_tenant
        }
        None => db_tenant,
    };

    let context = TenantIsolationContext::application(tenant_id, principal.clone(), surface);
    // Still pinned: the context tenant (chosen by AUTH in bound mode) must match
    // the wire `$db`. Non-tautological now — in bound mode the context tenant
    // came from authentication, not from `$db`.
    ensure_database_matches_context(&context, db_name, "MongoDB database selection")?;
    Ok(context)
}

/// The tenant context a session falls back to when it has no stored context.
///
/// Fail-closed for bound credentials: a session must never silently fall back to
/// the literal `default` tenant when authentication already bound this
/// connection to a specific tenant. In unbound mode the `default` tenant is
/// used, matching today's behavior.
pub fn default_tenant_context(
    surface: &'static str,
    principal: &PrincipalContext,
) -> TenantIsolationContext {
    let tenant_id = authenticated_tenant_from_principal(principal).unwrap_or_else(|| {
        TenantId::new(DEFAULT_TENANT).expect("default tenant id should be valid")
    });
    TenantIsolationContext::application(tenant_id, principal.clone(), surface)
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
    engine
        .ensure_tenant_ready_blocking(context.tenant_id().clone())
        .map(|_| ())
        .map_err(MongoError::from)
}

pub async fn ensure_tenant_async(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
) -> Result<(), MongoError> {
    engine
        .ensure_tenant_ready_async(context.tenant_id().clone())
        .await
        .map(|_| ())
        .map_err(MongoError::from)
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

    fn bound_principal(tenant: &str) -> PrincipalContext {
        PrincipalContext {
            authenticated: true,
            claims: serde_json::Map::from_iter([(
                "subject".to_string(),
                serde_json::json!("mongodb-test-user"),
            )]),
            verified_claims: serde_json::Map::from_iter([(
                AUTHENTICATED_TENANT_CLAIM.to_string(),
                serde_json::json!(tenant),
            )]),
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
    fn unbound_principal_refuses_reserved_database_before_context_creation() {
        for reserved in ["_nimbus", "_reserved"] {
            let error = resolve_tenant_context(reserved, "mongodb test", &test_principal())
                .expect_err("an unbound wire database must not select a reserved tenant");
            match error {
                MongoError::Command { code, message, .. } => {
                    assert_eq!(code, UNAUTHORIZED.code);
                    assert!(message.contains("reserved Nimbus tenant"));
                }
                other => panic!("expected command error, got {other:?}"),
            }
        }
    }

    #[test]
    fn authenticated_tenant_round_trips_through_principal() {
        let principal = bound_principal("tenant-a");
        assert_eq!(
            authenticated_tenant_from_principal(&principal),
            Some(TenantId::new("tenant-a").unwrap())
        );
        assert_eq!(authenticated_tenant_from_principal(&test_principal()), None);
    }

    #[test]
    fn bound_principal_authentication_decides_the_tenant() {
        // The authenticated tenant matches the wire $db: allowed, and the
        // context tenant is the authenticated one.
        let principal = bound_principal("tenant-a");
        let context = resolve_tenant_context("tenant-a", "mongodb test", &principal)
            .expect("matching $db should resolve");
        assert_eq!(context.tenant_id().as_str(), "tenant-a");
    }

    #[test]
    fn bound_principal_refuses_cross_tenant_db() {
        // The wire $db names a different tenant than authentication bound:
        // refused, regardless of the $db value.
        let principal = bound_principal("tenant-a");
        let error = resolve_tenant_context("tenant-b", "mongodb test", &principal)
            .expect_err("cross-tenant $db must be refused in bound mode");
        match error {
            MongoError::Command { code, message, .. } => {
                assert_eq!(code, UNAUTHORIZED.code);
                assert!(
                    message.contains("authorized tenant tenant-a"),
                    "error should name the authorized tenant: {message}"
                );
                assert!(
                    message.contains("referenced tenant tenant-b"),
                    "error should name the referenced tenant: {message}"
                );
            }
            other => panic!("expected command error, got {other:?}"),
        }
    }

    #[test]
    fn bound_principal_refuses_internal_db_mapping_to_default() {
        // Internal databases map to the `default` tenant; a bound tenant-a
        // connection must not reach `default`, so this is refused (fail-closed).
        let principal = bound_principal("tenant-a");
        let error = resolve_tenant_context("admin", "mongodb test", &principal)
            .expect_err("a bound connection must not reach the default tenant via admin");
        match error {
            MongoError::Command { code, message, .. } => {
                assert_eq!(code, UNAUTHORIZED.code);
                assert!(
                    message.contains("authorized tenant tenant-a"),
                    "error should name the authorized tenant: {message}"
                );
                assert!(
                    message.contains("referenced tenant default"),
                    "error should name the internal-db-mapped default tenant, proving the \
                     `admin` -> `default` mapping actually ran rather than failing for an \
                     unrelated reason: {message}"
                );
            }
            other => panic!("expected command error, got {other:?}"),
        }
    }

    #[test]
    fn bound_session_fallback_uses_authenticated_tenant_not_default() {
        // The session fallback (commit/abort/endSession) must never reach the
        // literal `default` tenant for a bound connection.
        let principal = bound_principal("tenant-a");
        let context = default_tenant_context("mongodb transaction commit", &principal);
        assert_eq!(context.tenant_id().as_str(), "tenant-a");
    }

    #[test]
    fn unbound_session_fallback_uses_default_tenant() {
        let context = default_tenant_context("mongodb transaction commit", &test_principal());
        assert_eq!(context.tenant_id().as_str(), DEFAULT_TENANT);
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
