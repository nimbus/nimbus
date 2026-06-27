pub mod bson_bridge;
pub mod commands;
pub mod connection;
pub mod credential_registry;
pub mod error;
pub mod wire;

mod auth;

pub use credential_registry::{CredentialBinding, CredentialRegistry, MongoAuth};

use std::error::Error as StdError;
use std::fmt;

use ring::rand::{SecureRandom, SystemRandom};

/// The single SCRAM credential the MongoDB adapter authenticates against.
///
/// There is exactly one credential, and it is tenant-agnostic: the tenant is
/// chosen from the requested database name (see
/// `commands::tenant::resolve_tenant_id`), not from the authenticated user. A
/// caller who knows this one username and password can therefore reach every
/// tenant by varying the database name on the wire.
///
/// This is only safe because the listener binds loopback-only
/// (`guard_bind_address` in `nimbus-server`), so the credential
/// never leaves the host. Before this adapter may bind any non-loopback
/// address, the credential model must change to bind each credential to a
/// specific tenant so that authentication — not the database name — decides
/// tenant access.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub username: String,
    pub password: String,
    pub salt: [u8; 16],
    pub iterations: u32,
    /// Whether this credential authenticates a specific tenant (credential->TenantId)
    /// rather than being the single tenant-agnostic credential. Always `false` today;
    /// set when per-tenant credential binding lands (M9a, issue #23). The `nimbus-server`
    /// bind guard refuses any non-loopback bind while this is `false`.
    tenant_bound: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuthConfigError {
    message: &'static str,
}

impl fmt::Display for AuthConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl StdError for AuthConfigError {}

impl AuthConfig {
    pub fn new(username: String, password: String) -> Self {
        Self::try_new(username, password)
            .expect("secure random source must be available for MongoDB SCRAM auth config")
    }

    pub fn try_new(username: String, password: String) -> Result<Self, AuthConfigError> {
        let mut salt = [0u8; 16];
        SystemRandom::new()
            .fill(&mut salt)
            .map_err(|_| AuthConfigError {
                message: "failed to generate MongoDB SCRAM salt from the operating system CSPRNG",
            })?;

        Ok(Self {
            username,
            password,
            salt,
            iterations: 4096,
            // The shipped credential is tenant-agnostic; per-tenant binding is M9(a) (#23).
            tenant_bound: false,
        })
    }

    /// Whether this credential authenticates a specific tenant.
    ///
    /// `false` for the single tenant-agnostic credential the adapter ships with today
    /// (tenant chosen from the wire `$db`). The `nimbus-server` bind guard
    /// (`guard_bind_address`) refuses any non-loopback bind while this is `false`,
    /// because a network-reachable listener under a tenant-agnostic credential would
    /// expose every tenant. M9(a) — credential->TenantId binding, mirroring the DynamoDB
    /// `AccessKeyRegistry` — makes this `true` for bound credentials, the prerequisite
    /// for a non-loopback bind. See issue #23.
    pub fn is_tenant_bound(&self) -> bool {
        self.tenant_bound
    }
}

#[cfg(test)]
mod auth_config_tests {
    use super::AuthConfig;

    #[test]
    fn auth_config_is_unbound_by_default() {
        let auth = AuthConfig::new("admin".into(), "admin".into());
        assert!(
            !auth.is_tenant_bound(),
            "the single shipped credential is tenant-agnostic (unbound); the non-loopback bind \
             guard must stay fail-closed until credential->TenantId binding lands (M9a, #23)"
        );
    }
}
