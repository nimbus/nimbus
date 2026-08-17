pub mod listener;

use std::net::SocketAddr;
use std::sync::Arc;

use nimbus_engine::Engine;

use super::wire::{WireProtocolAdapter, WireProtocolTasks};

pub use listener::MongoAuthSource;
pub use nimbus_mongodb::AuthConfig;
pub use nimbus_mongodb::CredentialRegistry;

/// A MongoDB wire-protocol listener configuration.
///
/// Mode-aware: an **unbound** config ([`new`](Self::new)/[`localhost`](Self::localhost))
/// authenticates against the single tenant-agnostic credential and is loopback-only,
/// while a **bound** config ([`bound`](Self::bound)) authenticates per-tenant
/// credential bindings and may bind a non-loopback address. The mode flows into both
/// the bind guard and the served dispatch path so the two never disagree.
#[derive(Debug, Clone)]
pub struct MongoDbConfig {
    pub bind_addr: SocketAddr,
    auth: MongoAuthSource,
}

impl MongoDbConfig {
    /// Unbound: the single tenant-agnostic credential. Loopback-only — the bind
    /// guard refuses a non-loopback address in this mode.
    pub fn new(bind_addr: SocketAddr, auth: AuthConfig) -> Self {
        Self {
            bind_addr,
            auth: MongoAuthSource::Unbound(Arc::new(auth)),
        }
    }

    pub fn localhost(port: u16, auth: AuthConfig) -> Self {
        Self::new(SocketAddr::from(([127, 0, 0, 1], port)), auth)
    }

    /// Bound: per-tenant credential bindings. Authentication decides the tenant,
    /// so the bind guard permits a non-loopback address.
    pub fn bound(bind_addr: SocketAddr, registry: CredentialRegistry) -> Self {
        Self {
            bind_addr,
            auth: MongoAuthSource::Bound(Arc::new(registry)),
        }
    }

    /// The single tenant-agnostic credential, present only in unbound mode.
    /// `None` in bound mode (there is no single credential to expose).
    #[must_use]
    pub fn auth_config(&self) -> Option<&AuthConfig> {
        match &self.auth {
            MongoAuthSource::Unbound(auth) => Some(auth),
            MongoAuthSource::Bound(_) => None,
        }
    }

    /// Whether authentication binds a specific tenant (bound mode).
    #[must_use]
    pub fn is_tenant_bound(&self) -> bool {
        self.auth.is_tenant_bound()
    }
}

impl WireProtocolAdapter for MongoDbConfig {
    fn name(&self) -> &'static str {
        "mongodb"
    }

    fn protocol(&self) -> &'static str {
        "tcp"
    }

    fn bind_addr(&self) -> SocketAddr {
        self.bind_addr
    }

    fn guard(&self, addr: SocketAddr) -> std::io::Result<()> {
        listener::guard_bind_address(addr, self.auth.is_tenant_bound())
    }

    fn build_tasks(self: Box<Self>, engine: Arc<Engine>) -> std::io::Result<WireProtocolTasks> {
        let auth = self.auth;
        Ok(WireProtocolTasks::new("listener", move |listener| {
            Box::pin(listener::run_listener(listener, engine, auth))
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nimbus_core::TenantId;

    fn registry() -> CredentialRegistry {
        CredentialRegistry::new().bind("user-a", TenantId::new("tenant-a").unwrap(), "secret-a")
    }

    #[test]
    fn unbound_config_exposes_its_credential_and_is_not_tenant_bound() {
        let config =
            MongoDbConfig::localhost(27017, AuthConfig::new("ops".into(), "secret".into()));
        assert!(!config.is_tenant_bound());
        assert_eq!(
            config.auth_config().expect("unbound exposes auth").username,
            "ops"
        );
    }

    #[test]
    fn bound_config_is_tenant_bound_and_hides_a_single_credential() {
        let config = MongoDbConfig::bound("0.0.0.0:27017".parse().unwrap(), registry());
        assert!(config.is_tenant_bound());
        assert!(
            config.auth_config().is_none(),
            "bound mode has no single tenant-agnostic credential to expose"
        );
    }

    // Guard-flip precision, exercised through the real `WireProtocolAdapter::guard`
    // seam (the bind seam that aborts boot before serving a byte).

    #[test]
    fn guard_seam_permits_non_loopback_only_when_bound() {
        let routable: SocketAddr = "0.0.0.0:27017".parse().unwrap();

        let bound = MongoDbConfig::bound(routable, registry());
        bound
            .guard(routable)
            .expect("a bound config must permit a non-loopback bind");

        let unbound =
            MongoDbConfig::localhost(27017, AuthConfig::new("ops".into(), "secret".into()));
        let error = unbound
            .guard(routable)
            .expect_err("an unbound config must still refuse a non-loopback bind (M9b intact)");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("non-loopback"));
    }

    #[test]
    fn guard_seam_permits_loopback_in_either_mode() {
        let loopback: SocketAddr = "127.0.0.1:27017".parse().unwrap();
        MongoDbConfig::bound(loopback, registry())
            .guard(loopback)
            .expect("bound loopback bind must be permitted");
        MongoDbConfig::localhost(27017, AuthConfig::new("ops".into(), "secret".into()))
            .guard(loopback)
            .expect("unbound loopback bind must be permitted");
    }
}
