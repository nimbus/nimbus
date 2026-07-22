pub mod bson_bridge;
pub mod commands;
pub mod connection;
pub mod credential_registry;
pub mod error;
pub mod wire;

mod auth;

pub use credential_registry::{
    CredentialBinding, CredentialRegistry, CredentialSpecError, MongoAuth,
};

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
    /// Whether this standalone credential authenticates a specific tenant.
    ///
    /// `AuthConfig` represents the loopback-only, tenant-agnostic mode and is
    /// therefore always `false`. Network-reachable, tenant-bound listeners use
    /// [`MongoAuth::Bound`] and [`CredentialRegistry`] instead.
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
            // Standalone AuthConfig is the loopback-only, tenant-agnostic mode.
            tenant_bound: false,
        })
    }

    /// Whether this credential authenticates a specific tenant.
    ///
    /// This is `false` for [`AuthConfig`], whose tenant is selected from wire
    /// `$db` and whose listener is consequently loopback-only. Bound mode uses
    /// [`MongoAuth::Bound`] and fixes the tenant during SCRAM authentication.
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
            "standalone AuthConfig is tenant-agnostic; non-loopback serving must use bound mode"
        );
    }
}
