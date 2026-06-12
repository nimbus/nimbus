//! Server-side composition shim for the DynamoDB adapter.
//!
//! Owns the listener bind/spawn/shutdown and the `POST /` route; all DynamoDB
//! protocol logic (X-Amz-Target dispatch, AttributeValue codec, expression
//! bridging, SigV4) lives in `nimbus-dynamodb`. `DynamoDbConfig` is re-exported
//! from the adapter crate, which owns its own config type.

pub mod listener;
pub mod ttl_sweeper;

use std::net::SocketAddr;
use std::sync::Arc;

use nimbus_engine::Engine;

use super::wire::WireProtocolAdapter;

pub use nimbus_dynamodb::DynamoDbConfig;

impl WireProtocolAdapter for DynamoDbConfig {
    fn name(&self) -> &'static str {
        "dynamodb"
    }

    fn protocol(&self) -> &'static str {
        "http"
    }

    fn bind_addr(&self) -> SocketAddr {
        self.bind_addr
    }

    fn guard(&self, addr: SocketAddr) -> std::io::Result<()> {
        // The signature-skipping lookup escape hatch is loopback-only: refuse
        // to expose an unauthenticated DynamoDB surface on a network-reachable
        // address. Production must use the default Strict mode with signed
        // keys.
        listener::guard_lookup_is_loopback_only(addr, &self.access_keys)
    }

    fn spawn(
        self: Box<Self>,
        listener: tokio::net::TcpListener,
        engine: Arc<Engine>,
    ) -> Vec<tokio::task::JoinHandle<()>> {
        let DynamoDbConfig {
            access_keys,
            ttl_sweep_interval,
            ..
        } = *self;
        let mut handles = Vec::new();
        // Spawn the background TTL sweeper before the access-key registry is
        // moved into the listener task (it shares the same registry + engine).
        if let Some(interval) = ttl_sweep_interval {
            let sweeper_engine = Arc::clone(&engine);
            let sweeper_keys = Arc::new(access_keys.clone());
            handles.push(tokio::spawn(ttl_sweeper::run_ttl_sweeper(
                sweeper_engine,
                sweeper_keys,
                interval,
            )));
        }
        handles.push(tokio::spawn(async move {
            listener::run_listener(listener, engine, access_keys).await;
        }));
        handles
    }
}
