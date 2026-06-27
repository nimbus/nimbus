pub mod listener;

use std::net::SocketAddr;
use std::sync::Arc;

use nimbus_engine::Engine;

use super::wire::WireProtocolAdapter;

pub use nimbus_mongodb::AuthConfig;
pub use nimbus_mongodb::{bson_bridge, wire};

#[derive(Debug, Clone)]
pub struct MongoDbConfig {
    pub bind_addr: SocketAddr,
    pub auth: Arc<AuthConfig>,
}

impl MongoDbConfig {
    pub fn new(bind_addr: SocketAddr, auth: AuthConfig) -> Self {
        Self {
            bind_addr,
            auth: Arc::new(auth),
        }
    }

    pub fn localhost(port: u16, auth: AuthConfig) -> Self {
        Self::new(SocketAddr::from(([127, 0, 0, 1], port)), auth)
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
        listener::guard_bind_address(addr, &self.auth)
    }

    fn spawn(
        self: Box<Self>,
        listener: tokio::net::TcpListener,
        engine: Arc<Engine>,
    ) -> Vec<tokio::task::JoinHandle<()>> {
        let auth = self.auth;
        vec![tokio::spawn(async move {
            listener::run_listener(listener, engine, auth).await;
        })]
    }
}
