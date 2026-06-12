//! Registration seam for sibling wire-protocol listeners.
//!
//! Adapters that speak their own protocol on their own port (MongoDB wire
//! protocol, DynamoDB HTTP) run beside the main HTTP server and share its
//! `Arc<Engine>`. `serve` drives every registered adapter through one uniform
//! bind -> guard -> record -> spawn sequence, so adding an adapter means
//! implementing this trait — not growing another bespoke block in
//! `construction.rs`.

use std::net::SocketAddr;
use std::sync::Arc;

use nimbus_engine::Engine;

/// A sibling wire-protocol listener serving an adapter surface beside the
/// main HTTP server.
///
/// For each adapter, `serve` binds [`bind_addr`](Self::bind_addr), runs
/// [`guard`](Self::guard) against the post-bind local address (fail-closed: a
/// guard error aborts boot before the listener serves a single byte), records
/// the listener in the system tenant, then hands the listener to
/// [`spawn`](Self::spawn).
pub(crate) trait WireProtocolAdapter: Send {
    /// Stable adapter name recorded in the system tenant listener registry.
    fn name(&self) -> &'static str;

    /// Transport label recorded beside the listener address ("tcp", "http").
    fn protocol(&self) -> &'static str;

    /// Address to bind before guarding.
    fn bind_addr(&self) -> SocketAddr;

    /// Adapter-specific refusal gate, run against the post-bind local address
    /// so OS-assigned ports are already resolved.
    fn guard(&self, addr: SocketAddr) -> std::io::Result<()>;

    /// Spawn the listener task plus any adapter-owned background tasks,
    /// consuming the adapter's config. Every returned handle is aborted when
    /// the main HTTP server exits.
    fn spawn(
        self: Box<Self>,
        listener: tokio::net::TcpListener,
        engine: Arc<Engine>,
    ) -> Vec<tokio::task::JoinHandle<()>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::dynamodb::DynamoDbConfig;
    use crate::adapters::mongodb::{AuthConfig, MongoDbConfig};

    fn mongodb_adapter(port: u16) -> Box<dyn WireProtocolAdapter> {
        Box::new(MongoDbConfig::localhost(
            port,
            AuthConfig::new("test-user".into(), "test-password".into()),
        ))
    }

    #[test]
    fn mongodb_adapter_reports_identity_and_bind_addr() {
        let adapter = mongodb_adapter(27017);
        assert_eq!(adapter.name(), "mongodb");
        assert_eq!(adapter.protocol(), "tcp");
        assert_eq!(adapter.bind_addr(), "127.0.0.1:27017".parse().unwrap());
    }

    #[test]
    fn mongodb_adapter_guard_refuses_routable_addresses_through_the_seam() {
        let adapter = mongodb_adapter(27017);
        let routable: SocketAddr = "0.0.0.0:27017".parse().unwrap();
        let error = adapter
            .guard(routable)
            .expect_err("network-reachable MongoDB listener must be refused");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(adapter.guard(adapter.bind_addr()).is_ok());
    }

    #[test]
    fn dynamodb_adapter_reports_identity_and_bind_addr() {
        let adapter: Box<dyn WireProtocolAdapter> = Box::new(DynamoDbConfig::new(8000));
        assert_eq!(adapter.name(), "dynamodb");
        assert_eq!(adapter.protocol(), "http");
        assert_eq!(adapter.bind_addr(), "127.0.0.1:8000".parse().unwrap());
    }

    #[test]
    fn dynamodb_adapter_guard_refuses_routable_lookup_mode_through_the_seam() {
        let routable: SocketAddr = "0.0.0.0:8000".parse().unwrap();
        let lookup: Box<dyn WireProtocolAdapter> =
            Box::new(DynamoDbConfig::new(8000).insecure_dev_auth());
        let error = lookup
            .guard(routable)
            .expect_err("signature-skipping lookup mode must stay loopback-only");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        // Loopback lookup and Strict-anywhere both stay allowed.
        assert!(lookup.guard(lookup.bind_addr()).is_ok());
        let strict: Box<dyn WireProtocolAdapter> = Box::new(DynamoDbConfig::new(8000));
        assert!(strict.guard(routable).is_ok());
    }
}
