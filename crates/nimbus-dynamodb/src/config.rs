//! Adapter configuration consumed by the server's listener composition.
//!
//! `DynamoDbConfig` is owned by `nimbus-dynamodb` (the adapter owns its config
//! type); `nimbus-server` adds `ServeOptions::with_dynamodb(DynamoDbConfig)` and
//! binds the listener. Mirrors the `MongoDbConfig { bind_addr, .. }` precedent.
//!
//! Authentication is **[`AuthMode::Strict`] by default** (full SigV4
//! verification). Bind production keys with [`DynamoDbConfig::with_signed_access_key`].
//! [`DynamoDbConfig::insecure_dev_auth`] opts into the signature-skipping
//! lookup mode for local development; the server refuses to bind it to a
//! non-loopback address.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use nimbus_core::TenantId;

use crate::tenant::{AccessKeyRegistry, AuthMode};

/// Configuration for the DynamoDB-compatible HTTP listener.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamoDbConfig {
    /// Address the DynamoDB listener binds to.
    pub bind_addr: SocketAddr,
    /// AWS access-key id → Nimbus tenant bindings. Every authenticated request
    /// resolves its tenant through this registry (see [`AccessKeyRegistry`]);
    /// an empty registry rejects every request as `UnrecognizedClientException`.
    pub access_keys: AccessKeyRegistry,
    /// How often the background TTL sweeper deletes expired items across every
    /// bound tenant's tables. `None` disables the sweeper (TTL config is still
    /// stored and described, but expired items are not reclaimed — like
    /// DynamoDB Local). Defaults to every 60s.
    pub ttl_sweep_interval: Option<Duration>,
}

impl DynamoDbConfig {
    /// Default port, matching the DynamoDB Local convention so
    /// `--endpoint-url http://localhost:8000` works out of the box.
    pub const DEFAULT_PORT: u16 = 8000;

    /// Default cadence of the background TTL sweeper.
    pub const DEFAULT_TTL_SWEEP_INTERVAL: Duration = Duration::from_secs(60);

    /// Bind to `127.0.0.1:<port>` (localhost-only, like the other Nimbus
    /// adapter listeners).
    #[must_use]
    pub fn new(port: u16) -> Self {
        Self {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
            access_keys: AccessKeyRegistry::new(),
            ttl_sweep_interval: Some(Self::DEFAULT_TTL_SWEEP_INTERVAL),
        }
    }

    /// Bind to an explicit socket address.
    #[must_use]
    pub fn with_bind_addr(mut self, bind_addr: SocketAddr) -> Self {
        self.bind_addr = bind_addr;
        self
    }

    /// Bind a **secret-less** AWS access-key id to a Nimbus tenant (builder
    /// style). Such a key only authenticates under [`Self::insecure_dev_auth`]
    /// (lookup mode); under the default [`AuthMode::Strict`] it has no secret to
    /// verify against and is rejected. For production Strict mode use
    /// [`Self::with_signed_access_key`].
    #[must_use]
    pub fn with_access_key(mut self, access_key_id: impl Into<String>, tenant: TenantId) -> Self {
        self.access_keys = self.access_keys.bind(access_key_id, tenant);
        self
    }

    /// Bind an AWS access-key id to a Nimbus tenant *with its secret access
    /// key*, so the key authenticates under [`AuthMode::Strict`] (the default).
    /// Requests are scoped to `tenant` once their SigV4 signature verifies
    /// against `secret`.
    #[must_use]
    pub fn with_signed_access_key(
        mut self,
        access_key_id: impl Into<String>,
        tenant: TenantId,
        secret: impl Into<String>,
    ) -> Self {
        self.access_keys = self.access_keys.bind_signed(access_key_id, tenant, secret);
        self
    }

    /// Set the authentication mode explicitly (builder style). Prefer
    /// [`Self::insecure_dev_auth`] for the lookup escape hatch so the intent is
    /// self-documenting at the call site.
    #[must_use]
    pub fn with_auth_mode(mut self, mode: AuthMode) -> Self {
        self.access_keys = self.access_keys.with_mode(mode);
        self
    }

    /// Opt into the **insecure** signature-skipping [`AuthMode::LookupOnly`]
    /// mode for local development. Any signature is accepted for a bound key, so
    /// the server refuses to bind this mode to a non-loopback address. Never use
    /// it for a network-reachable listener.
    #[must_use]
    pub fn insecure_dev_auth(mut self) -> Self {
        self.access_keys = self.access_keys.with_mode(AuthMode::LookupOnly);
        self
    }

    /// Set the background TTL sweep cadence, or `None` to disable the sweeper.
    #[must_use]
    pub fn with_ttl_sweep_interval(mut self, interval: Option<Duration>) -> Self {
        self.ttl_sweep_interval = interval;
        self
    }
}

impl Default for DynamoDbConfig {
    fn default() -> Self {
        Self::new(Self::DEFAULT_PORT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_localhost_port_8000() {
        let cfg = DynamoDbConfig::default();
        assert_eq!(cfg.bind_addr.port(), 8000);
        assert!(cfg.bind_addr.ip().is_loopback());
    }

    #[test]
    fn new_sets_port_on_localhost() {
        let cfg = DynamoDbConfig::new(9001);
        assert_eq!(cfg.bind_addr, "127.0.0.1:9001".parse().unwrap());
    }

    #[test]
    fn with_bind_addr_overrides() {
        let addr: SocketAddr = "0.0.0.0:8123".parse().unwrap();
        let cfg = DynamoDbConfig::new(8000).with_bind_addr(addr);
        assert_eq!(cfg.bind_addr, addr);
    }

    #[test]
    fn default_has_no_access_keys() {
        // An unconfigured listener authenticates nothing: every request is
        // UnrecognizedClient until an operator binds an access key.
        assert!(DynamoDbConfig::default().access_keys.is_empty());
    }

    #[test]
    fn default_enables_the_ttl_sweeper_at_60s() {
        assert_eq!(
            DynamoDbConfig::default().ttl_sweep_interval,
            Some(Duration::from_secs(60))
        );
    }

    #[test]
    fn ttl_sweep_interval_is_configurable_and_disablable() {
        let tuned = DynamoDbConfig::new(8000).with_ttl_sweep_interval(Some(Duration::from_secs(5)));
        assert_eq!(tuned.ttl_sweep_interval, Some(Duration::from_secs(5)));
        let off = DynamoDbConfig::new(8000).with_ttl_sweep_interval(None);
        assert_eq!(off.ttl_sweep_interval, None);
    }

    #[test]
    fn auth_mode_is_strict_by_default() {
        // Secure-by-default: an unconfigured listener verifies signatures.
        let cfg = DynamoDbConfig::default();
        assert_eq!(cfg.access_keys.mode(), AuthMode::Strict);
        assert!(!cfg.access_keys.is_insecure_lookup());
    }

    #[test]
    fn insecure_dev_auth_opts_into_lookup() {
        let cfg = DynamoDbConfig::new(8000).insecure_dev_auth();
        assert_eq!(cfg.access_keys.mode(), AuthMode::LookupOnly);
        assert!(cfg.access_keys.is_insecure_lookup());
    }

    #[test]
    fn with_signed_access_key_binds_a_verifiable_key() {
        let tenant = TenantId::new("acme").expect("valid tenant");
        let cfg =
            DynamoDbConfig::new(8000).with_signed_access_key("AKIAACME", tenant.clone(), "secret");
        // Strict mode (the default) is preserved and the key resolves.
        assert_eq!(cfg.access_keys.mode(), AuthMode::Strict);
        assert_eq!(cfg.access_keys.resolve("AKIAACME").unwrap(), &tenant);
        let binding = cfg.access_keys.binding("AKIAACME").unwrap();
        assert_eq!(binding.secret.as_deref(), Some("secret"));
    }

    #[test]
    fn with_auth_mode_sets_mode_explicitly() {
        let cfg = DynamoDbConfig::new(8000).with_auth_mode(AuthMode::LookupOnly);
        assert_eq!(cfg.access_keys.mode(), AuthMode::LookupOnly);
    }

    #[test]
    fn with_access_key_binds_tenant() {
        let tenant = TenantId::new("acme").expect("valid tenant");
        let cfg = DynamoDbConfig::new(8000)
            .with_access_key("AKIAACME", tenant.clone())
            .with_access_key("AKIAGLOBEX", TenantId::new("globex").expect("valid tenant"));
        assert_eq!(cfg.access_keys.len(), 2);
        assert_eq!(cfg.access_keys.resolve("AKIAACME").unwrap(), &tenant);
    }
}
