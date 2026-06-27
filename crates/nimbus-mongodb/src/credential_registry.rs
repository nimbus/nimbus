//! Per-credential → Nimbus tenant binding for the MongoDB adapter (M9a).
//!
//! **The contract this restores.** A tenant boundary is only as strong as
//! whatever *decides* the tenant. The trustworthy model — first implemented by
//! the DynamoDB adapter's `AccessKeyRegistry` (`crates/nimbus-dynamodb/src/
//! tenant.rs`) — binds each credential to exactly one [`TenantId`], so
//! authentication alone fixes the tenant and no request-supplied field can
//! broaden it. A wire-supplied namespace token (a MongoDB `$db` name) may then
//! only *select within* the already-authenticated tenant, never widen it.
//!
//! [`CredentialRegistry`] is the MongoDB analogue: it maps each SCRAM username
//! to a [`CredentialBinding`] (its tenant plus per-credential SCRAM-SHA-256
//! material). [`MongoAuth`] is the two-mode auth carrier the SCRAM flow and the
//! command dispatcher take:
//!
//! - [`MongoAuth::Unbound`] wraps the single tenant-agnostic [`AuthConfig`] the
//!   adapter ships with today. Authentication does not decide a tenant; the wire
//!   `$db` does. Safe only because the listener binds loopback-only
//!   (`guard_bind_address` in `nimbus-server` refuses a non-loopback bind while
//!   the credential is unbound).
//! - [`MongoAuth::Bound`] wraps a [`CredentialRegistry`]. Authentication fixes
//!   the tenant: a successful SCRAM handshake resolves the username to its bound
//!   tenant, and the command path refuses any `$db` naming a different tenant.

use std::collections::BTreeMap;

use nimbus_core::TenantId;
use ring::rand::{SecureRandom, SystemRandom};

use crate::AuthConfig;
use crate::error::{AUTHENTICATION_FAILED, MongoError};

/// Tenants whose id begins with this prefix are Nimbus-internal. A SCRAM
/// credential must never bind or resolve to one, or an authenticated request
/// could reach an internal store. nimbus-core exposes no shared reserved-tenant
/// check today, so the prefix is defined locally per adapter, mirroring the
/// DynamoDB `AccessKeyRegistry` (`RESERVED_TENANT_PREFIX`).
pub(crate) const RESERVED_TENANT_PREFIX: &str = "_nimbus";

/// Whether `tenant` is a reserved Nimbus-internal tenant (see
/// [`RESERVED_TENANT_PREFIX`]).
#[must_use]
pub(crate) fn is_reserved_tenant(tenant: &TenantId) -> bool {
    tenant.as_str().starts_with(RESERVED_TENANT_PREFIX)
}

/// One SCRAM credential's binding: the tenant it authenticates, plus the
/// per-credential SCRAM-SHA-256 material (password + salt + iterations).
#[derive(Debug, Clone)]
pub struct CredentialBinding {
    /// The Nimbus tenant this credential is scoped to. Authentication of this
    /// credential fixes the tenant; the wire `$db` may not widen it.
    pub tenant: TenantId,
    /// The credential's SCRAM password.
    pub password: String,
    /// Per-credential PBKDF2 salt, generated from the OS CSPRNG at bind time.
    pub salt: [u8; 16],
    /// PBKDF2 iteration count (4096, matching [`AuthConfig`]).
    pub iterations: u32,
}

/// Configured bindings from SCRAM username to Nimbus tenant.
///
/// Strict by default: an empty registry rejects every username, and an unknown
/// username resolves to an authentication failure. There is no `$db`-decides-the
/// -tenant fallback in bound mode.
#[derive(Debug, Clone, Default)]
pub struct CredentialRegistry {
    bindings: BTreeMap<String, CredentialBinding>,
}

impl CredentialRegistry {
    /// An empty registry. Rejects every username until a binding is added.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind a SCRAM username to a tenant with its password. Generates a fresh
    /// per-credential salt from the OS CSPRNG. Builder style.
    ///
    /// # Panics
    /// Panics if the operating system CSPRNG is unavailable, matching
    /// [`AuthConfig::new`].
    #[must_use]
    pub fn bind(
        mut self,
        username: impl Into<String>,
        tenant: TenantId,
        password: impl Into<String>,
    ) -> Self {
        let mut salt = [0u8; 16];
        SystemRandom::new()
            .fill(&mut salt)
            .expect("secure random source must be available for MongoDB SCRAM credential binding");
        self.bindings.insert(
            username.into(),
            CredentialBinding {
                tenant,
                password: password.into(),
                salt,
                iterations: 4096,
            },
        );
        self
    }

    /// Resolve a SCRAM username to its binding.
    ///
    /// # Errors
    /// An `AuthenticationFailed` [`MongoError`] if the username has no binding,
    /// or if it is (mis-)bound to a reserved Nimbus-internal tenant — such a
    /// binding is refused so it can never expose an internal store, regardless
    /// of how it was configured.
    pub fn resolve(&self, username: &str) -> Result<&CredentialBinding, MongoError> {
        let binding = self
            .bindings
            .get(username)
            .ok_or_else(authentication_failed)?;
        if is_reserved_tenant(&binding.tenant) {
            return Err(authentication_failed());
        }
        Ok(binding)
    }

    /// Whether any credentials are configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Number of configured bindings.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bindings.len()
    }
}

/// The `AuthenticationFailed` error returned for an unknown or refused username.
/// Deliberately generic — it does not reveal whether the username exists.
fn authentication_failed() -> MongoError {
    MongoError::Command {
        code: AUTHENTICATION_FAILED.code,
        code_name: AUTHENTICATION_FAILED.code_name.into(),
        message: "authentication failed".into(),
    }
}

/// The two-mode auth carrier the SCRAM flow and command dispatch take.
///
/// The mode — not a per-request claim — decides whether authentication binds a
/// tenant. In [`MongoAuth::Bound`] every successful handshake fixes a tenant, so
/// there is no path where a bound, authenticated command falls back to letting
/// the wire `$db` choose the tenant.
pub enum MongoAuth<'a> {
    /// The single tenant-agnostic credential (`$db` decides the tenant).
    Unbound(&'a AuthConfig),
    /// Per-username credential bindings (authentication decides the tenant).
    Bound(&'a CredentialRegistry),
}

impl MongoAuth<'_> {
    /// Whether authentication fixes a specific tenant ([`MongoAuth::Bound`]).
    ///
    /// Mirrors [`AuthConfig::is_tenant_bound`]; the `nimbus-server` bind guard
    /// refuses a non-loopback bind unless this is `true`.
    #[must_use]
    pub fn is_tenant_bound(&self) -> bool {
        matches!(self, MongoAuth::Bound(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant(id: &str) -> TenantId {
        TenantId::new(id).expect("valid tenant id")
    }

    fn auth_code(error: &MongoError) -> i32 {
        match error {
            MongoError::Command { code, .. } => *code,
            other => panic!("expected command error, got {other:?}"),
        }
    }

    #[test]
    fn resolves_known_usernames_to_their_tenants() {
        let registry = CredentialRegistry::new()
            .bind("user-a", tenant("tenant-a"), "secret-a")
            .bind("user-b", tenant("tenant-b"), "secret-b");
        assert_eq!(registry.len(), 2);
        assert!(!registry.is_empty());
        assert_eq!(
            registry.resolve("user-a").unwrap().tenant,
            tenant("tenant-a")
        );
        assert_eq!(
            registry.resolve("user-b").unwrap().tenant,
            tenant("tenant-b")
        );
    }

    #[test]
    fn each_binding_gets_a_unique_salt() {
        let registry = CredentialRegistry::new()
            .bind("user-a", tenant("tenant-a"), "secret")
            .bind("user-b", tenant("tenant-b"), "secret");
        assert_ne!(
            registry.resolve("user-a").unwrap().salt,
            registry.resolve("user-b").unwrap().salt
        );
        assert_eq!(registry.resolve("user-a").unwrap().iterations, 4096);
    }

    #[test]
    fn unknown_username_is_authentication_failed() {
        let registry = CredentialRegistry::new().bind("user-a", tenant("tenant-a"), "secret-a");
        let error = registry.resolve("nobody").unwrap_err();
        assert_eq!(auth_code(&error), AUTHENTICATION_FAILED.code);
    }

    #[test]
    fn empty_registry_rejects_everything() {
        let registry = CredentialRegistry::new();
        assert!(registry.is_empty());
        let error = registry.resolve("user-a").unwrap_err();
        assert_eq!(auth_code(&error), AUTHENTICATION_FAILED.code);
    }

    #[test]
    fn binding_to_a_reserved_tenant_is_refused() {
        let registry =
            CredentialRegistry::new().bind("user-evil", tenant("_nimbus_internal"), "secret");
        let error = registry.resolve("user-evil").unwrap_err();
        assert_eq!(auth_code(&error), AUTHENTICATION_FAILED.code);
    }

    #[test]
    fn is_reserved_tenant_flags_the_internal_prefix() {
        assert!(is_reserved_tenant(&tenant("_nimbus_internal")));
        assert!(!is_reserved_tenant(&tenant("tenant-a")));
    }

    #[test]
    fn mongo_auth_mode_reports_tenant_binding() {
        let registry = CredentialRegistry::new().bind("user-a", tenant("tenant-a"), "secret");
        assert!(MongoAuth::Bound(&registry).is_tenant_bound());

        let config = AuthConfig::new("admin".into(), "admin".into());
        assert!(!MongoAuth::Unbound(&config).is_tenant_bound());
    }
}
