//! Registration seam for sibling wire-protocol listeners.
//!
//! Adapters that speak their own protocol on their own port (MongoDB wire
//! protocol, DynamoDB HTTP) run beside the main HTTP server and share its
//! `Arc<Engine>`. `serve` drives every registered adapter through one uniform
//! bind -> guard -> record -> prepare sequence, then activates and supervises
//! the complete group. Adding an adapter means implementing this trait, not
//! growing another bespoke block in `construction.rs`.

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use nimbus_engine::Engine;

pub(crate) type WireProtocolTaskFuture =
    Pin<Box<dyn Future<Output = io::Result<()>> + Send + 'static>>;

/// Unspawned tasks for one sibling wire adapter.
///
/// The required listener factory keeps the concrete socket in the structured
/// listener group until task construction succeeds. Optional background tasks
/// share the adapter lifecycle but never own the listening socket.
pub(crate) struct WireProtocolTasks {
    listener_name: &'static str,
    listener: Box<dyn FnOnce(tokio::net::TcpListener) -> WireProtocolTaskFuture + Send + 'static>,
    background: Vec<WireProtocolTask>,
}

impl WireProtocolTasks {
    pub(crate) fn new(
        listener_name: &'static str,
        listener: impl FnOnce(tokio::net::TcpListener) -> WireProtocolTaskFuture + Send + 'static,
    ) -> Self {
        Self {
            listener_name,
            listener: Box::new(listener),
            background: Vec::new(),
        }
    }

    pub(crate) fn with_background(
        mut self,
        name: &'static str,
        future: WireProtocolTaskFuture,
    ) -> Self {
        self.background.push(WireProtocolTask { name, future });
        self
    }

    pub(crate) fn bind_listener(self, listener: tokio::net::TcpListener) -> Vec<WireProtocolTask> {
        let Self {
            listener_name,
            listener: listener_task,
            mut background,
        } = self;
        let mut tasks = Vec::with_capacity(background.len() + 1);
        tasks.push(WireProtocolTask {
            name: listener_name,
            future: listener_task(listener),
        });
        tasks.append(&mut background);
        tasks
    }
}

pub(crate) struct WireProtocolTask {
    pub(crate) name: &'static str,
    pub(crate) future: WireProtocolTaskFuture,
}

/// A sibling wire-protocol listener serving an adapter surface beside the
/// main HTTP server.
///
/// For each adapter, `serve` binds [`bind_addr`](Self::bind_addr), runs
/// [`guard`](Self::guard) against the post-bind local address (fail-closed: a
/// guard error aborts boot before the listener serves a single byte), records
/// the listener in the system tenant, and prepares its unspawned task set.
/// The complete listener group activates only after every adapter succeeds.
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

    /// Construct, but do not spawn, the listener task and any adapter-owned
    /// background tasks. The server-owned listener group spawns and supervises
    /// the complete set only after bind, guard, and projection succeed.
    fn build_tasks(self: Box<Self>, engine: Arc<Engine>) -> io::Result<WireProtocolTasks>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::dynamodb::DynamoDbConfig;
    use crate::adapters::mongodb::{AuthConfig, MongoDbConfig};
    use crate::adapters::s3::S3Config;
    use nimbus_core::TenantId;

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

    #[test]
    fn s3_adapter_reports_identity_and_bind_addr() {
        let adapter: Box<dyn WireProtocolAdapter> = Box::new(S3Config::new(9000));
        assert_eq!(adapter.name(), "s3");
        assert_eq!(adapter.protocol(), "http");
        assert_eq!(adapter.bind_addr(), "127.0.0.1:9000".parse().unwrap());
    }

    #[test]
    fn s3_adapter_guard_requires_explicit_signed_keys() {
        let adapter: Box<dyn WireProtocolAdapter> = Box::new(S3Config::new(9000));
        let error = adapter
            .guard(adapter.bind_addr())
            .expect_err("empty S3 credentials must fail closed");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);

        let tenant = TenantId::new("tenant-s3").expect("tenant id");
        let adapter: Box<dyn WireProtocolAdapter> =
            Box::new(S3Config::new(9000).with_signed_access_key("AKIATEST", tenant, "secret"));
        assert!(adapter.guard(adapter.bind_addr()).is_ok());
    }
}
