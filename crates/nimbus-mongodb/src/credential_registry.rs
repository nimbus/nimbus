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
use std::fmt;

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

    /// Parse an operator credential spec (the `NIMBUS_MONGODB_CREDENTIALS`
    /// value) into a registry.
    ///
    /// The format mirrors the DynamoDB `NIMBUS_DYNAMODB_ACCESS_KEYS` convention
    /// (the operator-path parser in `nimbus-bin`): comma-separated entries, each
    /// `USERNAME:TENANT:PASSWORD`. Surrounding whitespace on each entry is
    /// trimmed and empty entries are skipped (so a stray or trailing comma is
    /// harmless), exactly as the DynamoDB env path does. A non-empty entry that
    /// is not three colon-separated segments, has an empty segment, names an
    /// invalid tenant id, or names a reserved Nimbus-internal tenant is a hard
    /// error so the operator sees a clean refusal at boot rather than a silent
    /// auth failure later. The password is the third segment taken whole, so it
    /// may itself contain `:`; usernames and tenant ids may not.
    ///
    /// This is the same parser the served listener is built from, so a test that
    /// parses a spec here exercises exactly what the operator path ingests.
    ///
    /// # Errors
    /// A [`CredentialSpecError`] naming the offending entry and the expected
    /// format when an entry cannot be parsed or binds a reserved tenant.
    pub fn from_operator_spec(spec: &str) -> Result<Self, CredentialSpecError> {
        let mut registry = Self::new();
        for entry in spec
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            let mut parts = entry.splitn(3, ':');
            let (Some(username), Some(tenant), Some(password)) =
                (parts.next(), parts.next(), parts.next())
            else {
                return Err(spec_error(format!(
                    "invalid MongoDB credential binding `{entry}`: expected USERNAME:TENANT:PASSWORD"
                )));
            };
            if username.is_empty() || tenant.is_empty() || password.is_empty() {
                return Err(spec_error(format!(
                    "invalid MongoDB credential binding `{entry}`: every segment must be non-empty"
                )));
            }
            let tenant_id = TenantId::new(tenant).map_err(|error| {
                spec_error(format!(
                    "invalid MongoDB credential binding `{entry}`: {error}"
                ))
            })?;
            if is_reserved_tenant(&tenant_id) {
                return Err(spec_error(format!(
                    "invalid MongoDB credential binding `{entry}`: tenant `{tenant}` is reserved \
                     for Nimbus-internal use"
                )));
            }
            registry = registry.bind(username, tenant_id, password);
        }
        Ok(registry)
    }
}

/// Failure parsing an operator credential spec (see
/// [`CredentialRegistry::from_operator_spec`]).
///
/// Carries an operator-facing message that names the offending entry and the
/// expected `USERNAME:TENANT:PASSWORD` format, mirroring the DynamoDB access-key
/// binding parse errors so an operator can fix the env value without reading
/// source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialSpecError {
    message: String,
}

impl fmt::Display for CredentialSpecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CredentialSpecError {}

fn spec_error(message: String) -> CredentialSpecError {
    CredentialSpecError { message }
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
    fn from_operator_spec_parses_username_tenant_password_entries() {
        // The operator-path format: comma-separated USERNAME:TENANT:PASSWORD.
        let registry = CredentialRegistry::from_operator_spec(
            "user-a:tenant-a:secret-a,user-b:tenant-b:secret-b",
        )
        .expect("a well-formed spec should parse");
        assert_eq!(registry.len(), 2);
        let a = registry.resolve("user-a").expect("user-a resolves");
        assert_eq!(a.tenant, tenant("tenant-a"));
        assert_eq!(a.password, "secret-a");
        assert_eq!(a.iterations, 4096);
        let b = registry.resolve("user-b").expect("user-b resolves");
        assert_eq!(b.tenant, tenant("tenant-b"));
        assert_eq!(b.password, "secret-b");
        // Per-binding salts are still unique through the parsed path.
        assert_ne!(a.salt, b.salt);
    }

    #[test]
    fn from_operator_spec_trims_and_skips_empty_entries() {
        // Mirrors the DynamoDB env path: surrounding whitespace is trimmed and
        // empty entries (e.g. a trailing comma) are skipped, not errors.
        let registry = CredentialRegistry::from_operator_spec(
            " user-a:tenant-a:secret-a , , user-b:tenant-b:secret-b ,",
        )
        .expect("trimmed entries with empties should parse");
        assert_eq!(registry.len(), 2);
        assert!(registry.resolve("user-a").is_ok());
        assert!(registry.resolve("user-b").is_ok());

        // An empty (or whitespace-only) spec yields an empty registry, not an
        // error — the caller decides whether bound mode applies.
        assert!(
            CredentialRegistry::from_operator_spec("   ")
                .expect("whitespace-only spec parses")
                .is_empty()
        );
    }

    #[test]
    fn from_operator_spec_password_may_contain_colons() {
        // The password is the third segment taken whole, so a base64-ish or
        // URI-ish password containing `:` survives intact.
        let registry = CredentialRegistry::from_operator_spec("user-a:tenant-a:p:a:ss")
            .expect("password with colons should parse");
        assert_eq!(registry.resolve("user-a").unwrap().password, "p:a:ss");
    }

    #[test]
    fn from_operator_spec_rejects_malformed_entries() {
        let error = CredentialRegistry::from_operator_spec("user-a:tenant-a")
            .expect_err("a two-segment entry must be rejected");
        assert!(
            error.to_string().contains("USERNAME:TENANT:PASSWORD"),
            "malformed-entry error must name the expected format: {error}"
        );
    }

    #[test]
    fn from_operator_spec_rejects_empty_segments() {
        let error = CredentialRegistry::from_operator_spec("user-a::secret-a")
            .expect_err("an empty tenant segment must be rejected");
        assert!(
            error
                .to_string()
                .contains("every segment must be non-empty"),
            "empty-segment error must explain the constraint: {error}"
        );
    }

    #[test]
    fn from_operator_spec_refuses_reserved_tenant() {
        // The registry refuses a reserved tenant at resolve time; ingestion
        // surfaces it as a clean operator error at parse time instead.
        let error = CredentialRegistry::from_operator_spec("user-evil:_nimbus_internal:secret")
            .expect_err("binding a reserved Nimbus-internal tenant must be refused");
        assert!(
            error.to_string().contains("reserved"),
            "reserved-tenant error must explain the refusal: {error}"
        );
    }

    #[test]
    fn mongo_auth_mode_reports_tenant_binding() {
        let registry = CredentialRegistry::new().bind("user-a", tenant("tenant-a"), "secret");
        assert!(MongoAuth::Bound(&registry).is_tenant_bound());

        let config = AuthConfig::new("admin".into(), "admin".into());
        assert!(!MongoAuth::Unbound(&config).is_tenant_bound());
    }
}
