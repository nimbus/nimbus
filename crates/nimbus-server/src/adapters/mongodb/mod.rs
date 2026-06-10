pub mod listener;

use std::net::SocketAddr;
use std::sync::Arc;

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
