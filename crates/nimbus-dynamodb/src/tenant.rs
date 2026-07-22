//! Access-key → Nimbus tenant resolution.
//!
//! DynamoDB has a flat account-level table namespace; Nimbus is multi-tenant.
//! This adapter binds each configured AWS access-key id to a Nimbus `TenantId`
//! at setup, so every request is scoped to exactly one tenant by its
//! credentials. This is a new pattern for Nimbus adapters (MongoDB/Firebase
//! resolve tenant from a request namespace token, not credentials), so it owns
//! its own resolution + isolation tests.
//!
//! The SigV4 parser (D0.8) extracts the access-key id from the `Authorization`
//! header; [`AccessKeyRegistry::resolve`] maps it to the tenant; strict
//! signature verification (the secret/region per key) lands in D7.
//!
//! Unknown access keys are rejected with `UnrecognizedClientException` — the
//! code real AWS / DynamoDB returns for an unrecognized access key (AWS SDKs
//! special-case it), which is the parity-correct choice over a generic
//! `AccessDeniedException`.

use std::collections::BTreeMap;
use std::sync::Arc;

use extenddb_core::error::DynamoDbError;
use nimbus_core::{PrincipalContext, TenantId};
use nimbus_engine::Engine;
use nimbus_tenant::TenantIsolationContext;

use crate::error::map_core_error;

/// How the adapter authenticates requests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AuthMode {
    /// Verify the full SigV4 signature against the per-key secret and reject
    /// requests outside the ±15-minute timestamp window. The default: the
    /// adapter is secure-by-default and rejects forged or replayed requests.
    #[default]
    Strict,
    /// Extract the access key and resolve it to a tenant *without* verifying the
    /// SigV4 signature. Insecure — any signature is accepted — so it is only an
    /// opt-in local-development escape hatch (`DynamoDbConfig::insecure_dev_auth`)
    /// and the server refuses to bind it to a non-loopback address.
    LookupOnly,
}

/// Tenants whose id begins with this prefix are Nimbus-internal — e.g. the
/// DynamoDB access-key store's `_nimbus_ddb_system`. An access key must never
/// bind or resolve to one, or an authenticated request could read another
/// tenant's stored credentials out of an internal table.
pub(crate) const RESERVED_TENANT_PREFIX: &str = "_nimbus";

/// Whether `tenant` is a reserved Nimbus-internal tenant (see
/// [`RESERVED_TENANT_PREFIX`]).
#[must_use]
pub(crate) fn is_reserved_tenant(tenant: &TenantId) -> bool {
    tenant.as_str().starts_with(RESERVED_TENANT_PREFIX)
}

/// One access key's binding: the tenant it scopes to, plus the secret access
/// key (required only in [`AuthMode::Strict`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    /// The Nimbus tenant this access key is scoped to.
    pub tenant: TenantId,
    /// The secret access key, used for `Strict` signature verification. `None`
    /// for lookup-only keys.
    pub secret: Option<String>,
}

/// Configured bindings from AWS access-key id to Nimbus tenant, plus the auth
/// mode applied to every request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccessKeyRegistry {
    bindings: BTreeMap<String, KeyBinding>,
    mode: AuthMode,
}

impl AccessKeyRegistry {
    /// An empty registry (no access keys configured) in the secure-by-default
    /// [`AuthMode::Strict`] mode.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind a lookup-only access-key id to a tenant (no secret; usable in
    /// `LookupOnly` mode). Builder style.
    #[must_use]
    pub fn bind(mut self, access_key_id: impl Into<String>, tenant: TenantId) -> Self {
        self.bindings.insert(
            access_key_id.into(),
            KeyBinding {
                tenant,
                secret: None,
            },
        );
        self
    }

    /// Bind an access-key id to a tenant with its secret access key, so the key
    /// can be used under `Strict` signature verification. Builder style.
    #[must_use]
    pub fn bind_signed(
        mut self,
        access_key_id: impl Into<String>,
        tenant: TenantId,
        secret: impl Into<String>,
    ) -> Self {
        self.bindings.insert(
            access_key_id.into(),
            KeyBinding {
                tenant,
                secret: Some(secret.into()),
            },
        );
        self
    }

    /// Set the authentication mode (builder style).
    #[must_use]
    pub fn with_mode(mut self, mode: AuthMode) -> Self {
        self.mode = mode;
        self
    }

    /// The configured authentication mode.
    #[must_use]
    pub fn mode(&self) -> AuthMode {
        self.mode
    }

    /// Whether this registry skips SigV4 verification ([`AuthMode::LookupOnly`]).
    /// The server uses this to refuse binding an insecure registry to a
    /// non-loopback address — the lookup escape hatch is loopback-only.
    #[must_use]
    pub fn is_insecure_lookup(&self) -> bool {
        matches!(self.mode, AuthMode::LookupOnly)
    }

    /// Resolve an access-key id to its bound tenant.
    ///
    /// # Errors
    /// `UnrecognizedClientException` if the access-key id has no binding.
    pub fn resolve(&self, access_key_id: &str) -> Result<&TenantId, DynamoDbError> {
        self.binding(access_key_id).map(|binding| &binding.tenant)
    }

    /// Resolve an access-key id to its full binding (tenant + optional secret).
    ///
    /// # Errors
    /// `UnrecognizedClientException` if the access-key id has no binding, or if
    /// it is (mis-)bound to a reserved Nimbus-internal tenant — such a binding is
    /// refused so it can never expose an internal store like the access-key
    /// catalog, regardless of how it was configured.
    pub fn binding(&self, access_key_id: &str) -> Result<&KeyBinding, DynamoDbError> {
        let binding = self
            .bindings
            .get(access_key_id)
            .ok_or_else(unrecognized_client)?;
        if is_reserved_tenant(&binding.tenant) {
            return Err(unrecognized_client());
        }
        Ok(binding)
    }

    /// Whether any access keys are configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Number of configured bindings.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// The distinct tenants bound in this registry (deduped — a tenant may have
    /// several access keys). The TTL sweeper enumerates these to find work.
    #[must_use]
    pub fn tenants(&self) -> Vec<TenantId> {
        let mut tenants: Vec<TenantId> = Vec::new();
        for binding in self.bindings.values() {
            if !tenants.contains(&binding.tenant) {
                tenants.push(binding.tenant.clone());
            }
        }
        tenants
    }
}

/// The `UnrecognizedClientException` real AWS / DynamoDB returns for an
/// unrecognized (or refused) access key.
fn unrecognized_client() -> DynamoDbError {
    DynamoDbError::UnrecognizedClientException(
        "The security token included in the request is invalid.".to_owned(),
    )
}

/// Build the tenant isolation context for a DynamoDB request scoped to `tenant`.
///
/// The principal is `system` — the adapter has already authenticated the request
/// by access key (D0.5/D7) before scoping the engine call to this tenant.
#[must_use]
pub fn tenant_context(tenant: TenantId, surface: &'static str) -> TenantIsolationContext {
    TenantIsolationContext::application(tenant, PrincipalContext::system(), surface)
}

/// Ensure the context's tenant exists in the engine (idempotent), mapping engine
/// errors to the DynamoDB taxonomy.
///
/// # Errors
/// A mapped `DynamoDbError` if tenant creation fails for any reason other than
/// the tenant already existing.
#[cfg(not(test))]
pub fn ensure_tenant(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
) -> Result<(), DynamoDbError> {
    engine
        .ensure_tenant_exists(context.tenant_id())
        .map_err(map_core_error)
}

/// Admit a tenant through the canonical persistence-provider lifecycle.
///
/// Provider-reachable composition roots must call this async contract before
/// entering the synchronous DynamoDB command core.
pub async fn ensure_tenant_async(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
) -> Result<(), DynamoDbError> {
    engine
        .ensure_tenant_ready_async(context.tenant_id().clone())
        .await
        .map(|_| ())
        .map_err(map_core_error)
}

#[cfg(test)]
pub fn ensure_tenant(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
) -> Result<(), DynamoDbError> {
    match engine.create_tenant(context.tenant_id().clone()) {
        // tenant-lifecycle: test-only
        Ok(()) | Err(nimbus_core::Error::AlreadyExists(_)) => Ok(()),
        Err(error) => Err(map_core_error(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant(id: &str) -> TenantId {
        TenantId::new(id).expect("valid tenant id")
    }

    #[test]
    fn resolves_known_access_keys_to_their_tenants() {
        let registry = AccessKeyRegistry::new()
            .bind("AKIAACME", tenant("acme"))
            .bind("AKIAGLOBEX", tenant("globex"));
        assert_eq!(registry.len(), 2);
        assert_eq!(registry.resolve("AKIAACME").unwrap(), &tenant("acme"));
        assert_eq!(registry.resolve("AKIAGLOBEX").unwrap(), &tenant("globex"));
    }

    #[test]
    fn strict_is_the_default_mode() {
        // Secure-by-default: a freshly built registry verifies signatures and is
        // not flagged as the insecure loopback-only escape hatch.
        let registry = AccessKeyRegistry::new();
        assert_eq!(registry.mode(), AuthMode::Strict);
        assert!(!registry.is_insecure_lookup());
        assert_eq!(AuthMode::default(), AuthMode::Strict);
    }

    #[test]
    fn lookup_mode_is_flagged_insecure() {
        let registry = AccessKeyRegistry::new().with_mode(AuthMode::LookupOnly);
        assert!(registry.is_insecure_lookup());
    }

    #[test]
    fn unknown_access_key_is_unrecognized_client() {
        let registry = AccessKeyRegistry::new().bind("AKIAACME", tenant("acme"));
        let err = registry.resolve("AKIANOPE").unwrap_err();
        assert!(matches!(err, DynamoDbError::UnrecognizedClientException(_)));
    }

    #[test]
    fn empty_registry_rejects_everything() {
        let registry = AccessKeyRegistry::new();
        assert!(registry.is_empty());
        assert!(matches!(
            registry.resolve("AKIAACME"),
            Err(DynamoDbError::UnrecognizedClientException(_))
        ));
    }

    #[test]
    fn binding_refuses_a_reserved_tenant() {
        // F6a: a key bound (or mis-bound) to a reserved Nimbus-internal tenant
        // must never resolve — it would expose internal stores.
        let registry = AccessKeyRegistry::new().bind("AKIAEVIL", tenant("_nimbus_ddb_system"));
        assert!(matches!(
            registry.binding("AKIAEVIL"),
            Err(DynamoDbError::UnrecognizedClientException(_))
        ));
        assert!(matches!(
            registry.resolve("AKIAEVIL"),
            Err(DynamoDbError::UnrecognizedClientException(_))
        ));
    }

    #[test]
    fn is_reserved_tenant_flags_the_internal_prefix() {
        assert!(is_reserved_tenant(&tenant("_nimbus_ddb_system")));
        assert!(is_reserved_tenant(&tenant("_nimbus_other")));
        assert!(!is_reserved_tenant(&tenant("acme")));
    }

    #[test]
    fn distinct_keys_isolate_tenants() {
        // Two access keys must never resolve to the same tenant unless bound so;
        // the binding is the only tenant authority (no cross-tenant leakage).
        let registry = AccessKeyRegistry::new()
            .bind("key-a", tenant("tenant-a"))
            .bind("key-b", tenant("tenant-b"));
        assert_ne!(
            registry.resolve("key-a").unwrap(),
            registry.resolve("key-b").unwrap()
        );
    }

    #[test]
    fn tenant_context_scopes_to_the_bound_tenant() {
        let context = tenant_context(tenant("acme"), "DynamoDB test");
        assert_eq!(context.tenant_id().as_str(), "acme");
        // A request for a different tenant must be rejected by the context guard.
        assert!(
            context
                .ensure_tenant_matches(&tenant("globex"), "cross-tenant probe")
                .is_err()
        );
    }

    #[test]
    fn ensure_tenant_is_idempotent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine"));
        let context = tenant_context(tenant("acme"), "DynamoDB test");
        ensure_tenant(&engine, &context).expect("first create");
        ensure_tenant(&engine, &context).expect("idempotent second create");
    }
}
