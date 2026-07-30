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

/// Claim naming the SigV4 access-key id a request authenticated with. A table
/// access policy writes `PrincipalClaim { principal: Identity, claim:
/// "aws_access_key_id" }` against it to name a specific DynamoDB caller.
pub const ACCESS_KEY_CLAIM: &str = "aws_access_key_id";

/// Claim under which the caller's bound tenant is recorded. `nimbus-tenant`
/// reads it back through `require_matching_principal_claim`, so a principal can
/// never be paired with a context for a different tenant.
const TENANT_CLAIM: &str = "tenant_id";

/// The principal for a request authenticated as `access_key_id`, bound to
/// `tenant`.
///
/// The access-key id is the only caller identity a DynamoDB request carries, so
/// it is what the principal asserts — mirroring the MongoDB adapter, which
/// builds its principal from the SCRAM username. The bound tenant goes in
/// `verified_claims` because the registry established it, not the client.
#[must_use]
pub fn access_key_principal(access_key_id: &str, tenant: &TenantId) -> PrincipalContext {
    let mut claims = serde_json::Map::new();
    let subject = serde_json::Value::String(access_key_id.to_owned());
    claims.insert("subject".to_owned(), subject.clone());
    claims.insert("sub".to_owned(), subject.clone());
    claims.insert(ACCESS_KEY_CLAIM.to_owned(), subject);
    claims.insert(
        "provider".to_owned(),
        serde_json::Value::String("dynamodb".to_owned()),
    );

    let verified_claims = serde_json::Map::from_iter([(
        TENANT_CLAIM.to_owned(),
        serde_json::Value::String(tenant.as_str().to_owned()),
    )]);

    PrincipalContext {
        authenticated: true,
        claims,
        verified_claims,
    }
}

/// Build the tenant isolation context for an authenticated DynamoDB request.
///
/// The context carries the *caller* — the access key the request authenticated
/// with — so every engine call the adapter makes on the caller's behalf is
/// authorized as that caller rather than as Nimbus itself.
///
/// # Errors
/// `AccessDeniedException` if the principal's bound tenant is not the tenant
/// being scoped to. Callers build both from the same registry binding, so this
/// is an enforced internal invariant rather than a reachable client error.
pub fn request_context(
    tenant: TenantId,
    principal: PrincipalContext,
    surface: &'static str,
) -> Result<TenantIsolationContext, DynamoDbError> {
    let context = TenantIsolationContext::application(tenant, principal, surface);
    context
        .require_matching_principal_claim("DynamoDB request")
        .map_err(map_core_error)?;
    Ok(context)
}

/// Build the tenant isolation context for adapter-owned background work with no
/// caller — today only the TTL sweeper, which expires items on a schedule the
/// tenant configured rather than on anyone's request.
#[must_use]
pub fn maintenance_context(tenant: TenantId, surface: &'static str) -> TenantIsolationContext {
    TenantIsolationContext::system(tenant, surface)
}

/// The principal to run an engine call on user data under.
///
/// A request context yields its caller; a maintenance context has none, so its
/// work runs as `system` — the sweeper must be able to expire items regardless
/// of the table's access policy.
#[must_use]
pub fn caller_principal(context: &TenantIsolationContext) -> PrincipalContext {
    context
        .application_principal()
        .cloned()
        .unwrap_or_else(PrincipalContext::system)
}

/// The principal for the adapter's own reserved stores — the `_ddb_catalog`
/// table metadata, `_ddb_ttl` configuration, `_ddb_tags`, the `_ddb_stream_*`
/// sidecars, and the `_nimbus_ddb_system` access-key store.
///
/// Those rows are the adapter's, not the caller's: client requests cannot name
/// the tables, and control-plane bookkeeping must not become answerable to a
/// user-authored access policy. They run as `system` explicitly rather than
/// inheriting whatever principal happens to be at hand.
#[must_use]
pub fn adapter_principal() -> PrincipalContext {
    PrincipalContext::system()
}

/// A request context for in-crate tests, shaped exactly like a live request:
/// application authority carrying an access-key principal bound to `tenant`.
/// Tests use it so command handlers run the same authorization path production
/// does, instead of the permissive system authority.
#[cfg(test)]
pub(crate) fn test_context(tenant: TenantId, surface: &'static str) -> TenantIsolationContext {
    let principal = access_key_principal("AKIATEST", &tenant);
    request_context(tenant, principal, surface).expect("test principal is bound to its own tenant")
}

/// Ensure the context's tenant runtime is ready (idempotent), mapping engine
/// errors to the DynamoDB taxonomy.
///
/// Synchronous embedded callers own first-use admission. Provider transports
/// must call [`ensure_tenant_async`] first; this check then accepts the loaded
/// runtime without invoking an embedded lifecycle.
///
/// # Errors
/// A mapped `DynamoDbError` if tenant creation fails for any reason other than
/// the tenant already existing.
pub fn ensure_tenant(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
) -> Result<(), DynamoDbError> {
    engine
        .ensure_tenant_ready_blocking(context.tenant_id().clone())
        .map(|_| ())
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
    fn request_context_scopes_to_the_bound_tenant() {
        let context = test_context(tenant("acme"), "DynamoDB test");
        assert_eq!(context.tenant_id().as_str(), "acme");
        // A request for a different tenant must be rejected by the context guard.
        assert!(
            context
                .ensure_tenant_matches(&tenant("globex"), "cross-tenant probe")
                .is_err()
        );
    }

    #[test]
    fn request_context_carries_the_calling_access_key() {
        // The whole point of the request context: engine calls made on this
        // request's behalf are authorized as the access key, not as Nimbus.
        let context = request_context(
            tenant("acme"),
            access_key_principal("AKIAACME", &tenant("acme")),
            "DynamoDB test",
        )
        .expect("principal is bound to its own tenant");
        let principal = caller_principal(&context);
        assert!(principal.authenticated);
        assert_eq!(
            principal.claims.get(ACCESS_KEY_CLAIM),
            Some(&serde_json::Value::String("AKIAACME".to_owned()))
        );
        assert_ne!(principal, PrincipalContext::system());
    }

    #[test]
    fn request_context_refuses_a_principal_bound_to_another_tenant() {
        // Defense in depth against a mis-wired call site: the principal's
        // registry-established tenant must equal the context's tenant.
        let error = request_context(
            tenant("acme"),
            access_key_principal("AKIAGLOBEX", &tenant("globex")),
            "DynamoDB test",
        )
        .expect_err("mismatched tenant must be refused");
        assert!(matches!(error, DynamoDbError::AccessDeniedException(_)));
    }

    #[test]
    fn maintenance_context_has_no_caller_and_runs_as_system() {
        let context = maintenance_context(tenant("acme"), "ttl-sweeper");
        assert_eq!(context.application_principal(), None);
        assert_eq!(caller_principal(&context), PrincipalContext::system());
    }

    #[test]
    fn ensure_tenant_is_idempotent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine"));
        let context = test_context(tenant("acme"), "DynamoDB test");
        ensure_tenant(&engine, &context).expect("first create");
        ensure_tenant(&engine, &context).expect("idempotent second create");
    }
}
