pub mod bson_bridge;
pub mod commands;
pub mod connection;
pub mod error;
pub mod wire;

mod auth;

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
/// (`guard_listener_is_loopback_only` in `nimbus-server`), so the credential
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
        })
    }
}
