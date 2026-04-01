use std::sync::Arc;

use reqwest::Client;

use super::config::{ConvexAuthConfig, ConvexAuthProvider};

mod headers;
mod identity;
mod metadata;

#[derive(Clone)]
pub(in crate::adapters::convex) struct ConvexAuthVerifier {
    client: Client,
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
    pub(in crate::adapters::convex) fn empty() -> Self {
        Self::new(ConvexAuthConfig::default())
    }

    pub(in crate::adapters::convex) fn new(config: ConvexAuthConfig) -> Self {
        Self {
            client: Client::new(),
            providers: Arc::new(config.providers),
        }
    }

    pub(in crate::adapters::convex) fn from_config(config: ConvexAuthConfig) -> Self {
        Self::new(config)
    }
}
