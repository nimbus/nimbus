//! Server-side composition shim for the S3 adapter.
//!
//! The `nimbus-s3` crate owns protocol behavior and public configuration. This
//! module owns the dedicated listener and the Engine-backed byte/metadata
//! backend that binds that protocol surface into the Nimbus server process.

pub mod listener;

use std::net::SocketAddr;
use std::sync::Arc;

use nimbus_engine::Engine;
pub use nimbus_s3::S3Config;

use super::wire::WireProtocolAdapter;

impl WireProtocolAdapter for S3Config {
    fn name(&self) -> &'static str {
        "s3"
    }

    fn protocol(&self) -> &'static str {
        "http"
    }

    fn bind_addr(&self) -> SocketAddr {
        self.bind_addr
    }

    fn guard(&self, _addr: SocketAddr) -> std::io::Result<()> {
        listener::guard_has_access_keys(&self.access_keys)
    }

    fn spawn(
        self: Box<Self>,
        listener: tokio::net::TcpListener,
        engine: Arc<Engine>,
    ) -> Vec<tokio::task::JoinHandle<()>> {
        let S3Config { access_keys, .. } = *self;
        vec![tokio::spawn(async move {
            listener::run_listener(listener, engine, access_keys).await;
        })]
    }
}
