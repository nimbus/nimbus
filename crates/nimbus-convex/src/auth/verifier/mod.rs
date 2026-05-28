use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use reqwest::Client;
use serde_json::Value;

use super::config::{ConvexAuthConfig, ConvexAuthProvider};

mod identity;
mod metadata;

const AUTH_METADATA_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const AUTH_METADATA_CACHE_TTL: Duration = Duration::from_secs(300);

#[derive(Clone)]
struct MetadataCacheEntry {
    expires_at: Instant,
    value: Value,
}

#[derive(Clone)]
pub struct ConvexAuthVerifier {
    client: Client,
    metadata_cache: Arc<RwLock<HashMap<String, MetadataCacheEntry>>>,
    metadata_cache_ttl: Duration,
    providers: Arc<Vec<ConvexAuthProvider>>,
}

impl std::fmt::Debug for ConvexAuthVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConvexAuthVerifier")
            .field("providers", &self.providers.len())
            .finish_non_exhaustive()
    }
}

impl ConvexAuthVerifier {
    pub fn empty() -> Self {
        Self::new(ConvexAuthConfig::default())
    }

    pub fn new(config: ConvexAuthConfig) -> Self {
        let client = Client::builder()
            .timeout(AUTH_METADATA_REQUEST_TIMEOUT)
            .build()
            .expect("bounded auth metadata HTTP client should build");
        Self {
            client,
            metadata_cache: Arc::new(RwLock::new(HashMap::new())),
            metadata_cache_ttl: AUTH_METADATA_CACHE_TTL,
            providers: Arc::new(config.providers),
        }
    }

    pub fn from_config(config: ConvexAuthConfig) -> Self {
        Self::new(config)
    }
}
