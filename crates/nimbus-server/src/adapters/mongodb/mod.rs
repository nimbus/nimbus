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
    pub fn new(port: u16) -> Self {
        Self {
            bind_addr: SocketAddr::from(([127, 0, 0, 1], port)),
            auth: Arc::new(AuthConfig::default()),
        }
    }

    pub fn with_auth(mut self, username: String, password: String) -> Self {
        self.auth = Arc::new(AuthConfig::new(username, password));
        self
    }
}

impl Default for MongoDbConfig {
    fn default() -> Self {
        Self::new(27017)
    }
}
