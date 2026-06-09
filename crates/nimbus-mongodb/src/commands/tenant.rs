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
) -> Result<TenantIsolationContext, MongoError> {
    let tenant_id = resolve_tenant_id(db_name)?;
    let context =
        TenantIsolationContext::application(tenant_id, PrincipalContext::system(), surface);
    ensure_database_matches_context(&context, db_name, "MongoDB database selection")?;
    Ok(context)
}

pub fn default_tenant_context(surface: &'static str) -> TenantIsolationContext {
    TenantIsolationContext::system(
        TenantId::new(DEFAULT_TENANT).expect("default tenant id should be valid"),
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

    #[test]
    fn resolves_mongodb_database_to_tenant_context() {
        let context =
            resolve_tenant_context("tenant-a", "mongodb test").expect("context should resolve");

        assert_eq!(context.tenant_id().as_str(), "tenant-a");
    }

    #[test]
    fn maps_mongodb_internal_databases_to_default_tenant_context() {
        for db_name in ["admin", "local", "config"] {
            let context =
                resolve_tenant_context(db_name, "mongodb test").expect("context should resolve");

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
