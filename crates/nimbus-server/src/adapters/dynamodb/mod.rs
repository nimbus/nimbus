//! Server-side composition shim for the DynamoDB adapter.
//!
//! Owns the listener router and the `POST /` route; the server listener group
//! owns task activation, supervision, and shutdown. All DynamoDB protocol logic
//! (X-Amz-Target dispatch, AttributeValue codec, expression bridging, SigV4)
//! lives in `nimbus-dynamodb`. `DynamoDbConfig` is re-exported from the adapter
//! crate, which owns its own config type.

pub mod listener;
pub mod ttl_sweeper;

use std::net::SocketAddr;
use std::sync::Arc;

use nimbus_engine::Engine;

use super::wire::{WireProtocolAdapter, WireProtocolTasks};

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

    fn build_tasks(self: Box<Self>, engine: Arc<Engine>) -> std::io::Result<WireProtocolTasks> {
        let DynamoDbConfig {
            access_keys,
            ttl_sweep_interval,
            ..
        } = *self;
        let listener_engine = Arc::clone(&engine);
        let listener_keys = access_keys.clone();
        let mut tasks = WireProtocolTasks::new("listener", move |listener| {
            Box::pin(listener::run_listener(
                listener,
                listener_engine,
                listener_keys,
            ))
        });
        if let Some(interval) = ttl_sweep_interval {
            let sweeper_engine = Arc::clone(&engine);
            let sweeper_keys = Arc::new(access_keys);
            tasks = tasks.with_background(
                "ttl-sweeper",
                Box::pin(async move {
                    ttl_sweeper::run_ttl_sweeper(sweeper_engine, sweeper_keys, interval).await;
                    Ok(())
                }),
            );
        }
        Ok(tasks)
    }
}
