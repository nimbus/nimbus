pub(crate) mod auth;
pub mod bson_bridge;
pub(crate) mod commands;
pub(crate) mod connection;
pub(crate) mod error;
pub mod listener;
pub mod wire;

use std::net::SocketAddr;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct MongoDbConfig {
    pub bind_addr: SocketAddr,
    pub auth: Arc<AuthConfig>,
}

#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub username: String,
    pub password: String,
    pub salt: [u8; 16],
    pub iterations: u32,
}

impl AuthConfig {
    pub fn new(username: String, password: String) -> Self {
        use std::hash::{Hash, Hasher};
        use std::time::SystemTime;

        let mut salt = [0u8; 16];
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .hash(&mut hasher);
        std::process::id().hash(&mut hasher);
        username.hash(&mut hasher);
        let h1 = hasher.finish();
        salt[..8].copy_from_slice(&h1.to_le_bytes());

        let mut hasher2 = std::collections::hash_map::DefaultHasher::new();
        h1.hash(&mut hasher2);
        std::thread::current().id().hash(&mut hasher2);
        let h2 = hasher2.finish();
        salt[8..].copy_from_slice(&h2.to_le_bytes());

        Self {
            username,
            password,
            salt,
            iterations: 4096,
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self::new("admin".into(), "admin".into())
    }
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
