//! Adapter configuration consumed by the server's listener composition.
//!
//! `DynamoDbConfig` is owned by `nimbus-dynamodb` (the adapter owns its config
//! type); `nimbus-server` adds `ServeOptions::with_dynamodb(DynamoDbConfig)` and
//! binds the listener. Mirrors the `MongoDbConfig { bind_addr, .. }` precedent.
//! The SigV4 `auth_mode` toggle is added in D7.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use nimbus_core::TenantId;

use crate::tenant::AccessKeyRegistry;

/// Configuration for the DynamoDB-compatible HTTP listener.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamoDbConfig {
    /// Address the DynamoDB listener binds to.
    pub bind_addr: SocketAddr,
    /// AWS access-key id → Nimbus tenant bindings. Every authenticated request
    /// resolves its tenant through this registry (see [`AccessKeyRegistry`]);
    /// an empty registry rejects every request as `UnrecognizedClientException`.
    pub access_keys: AccessKeyRegistry,
}

impl DynamoDbConfig {
    /// Default port, matching the DynamoDB Local convention so
    /// `--endpoint-url http://localhost:8000` works out of the box.
    pub const DEFAULT_PORT: u16 = 8000;

    /// Bind to `127.0.0.1:<port>` (localhost-only, like the other Nimbus
    /// adapter listeners).
    #[must_use]
    pub fn new(port: u16) -> Self {
        Self {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
            access_keys: AccessKeyRegistry::new(),
        }
    }

    /// Bind to an explicit socket address.
    #[must_use]
    pub fn with_bind_addr(mut self, bind_addr: SocketAddr) -> Self {
        self.bind_addr = bind_addr;
        self
    }

    /// Bind an AWS access-key id to a Nimbus tenant (builder style). Requests
    /// authenticated with this access key are scoped to `tenant`.
    #[must_use]
    pub fn with_access_key(mut self, access_key_id: impl Into<String>, tenant: TenantId) -> Self {
        self.access_keys = self.access_keys.bind(access_key_id, tenant);
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
    fn with_access_key_binds_tenant() {
        let tenant = TenantId::new("acme").expect("valid tenant");
        let cfg = DynamoDbConfig::new(8000)
            .with_access_key("AKIAACME", tenant.clone())
            .with_access_key("AKIAGLOBEX", TenantId::new("globex").expect("valid tenant"));
        assert_eq!(cfg.access_keys.len(), 2);
        assert_eq!(cfg.access_keys.resolve("AKIAACME").unwrap(), &tenant);
    }
}
