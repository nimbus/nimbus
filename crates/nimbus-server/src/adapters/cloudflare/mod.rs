//! Cloudflare-compatible adapter bootstrap.
//!
//! CFA1 only establishes the adapter surface and binding registry. Data-plane
//! behavior lands in the later KV and Durable Object phases.
//!
//! Cloudflare non-loopback binds are refused during CLI startup unless the
//! operator opts in with the shared network-bind guard.

use std::sync::Arc;

use axum::Router;
use nimbus_core::TenantId;
use nimbus_dynamodb::AccessKeyRegistry;

use crate::state::AppState;

pub mod config;
pub mod durable_objects;
pub mod host_bridge;
pub mod kv;

pub use config::{
    CloudflareBindingRegistry, D1DatabaseBinding, DurableObjectBinding, KvNamespaceBinding,
    R2BucketBinding, WranglerConfigError,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CloudflareConfig {
    bindings: CloudflareBindingRegistry,
    access_keys: AccessKeyRegistry,
}

impl CloudflareConfig {
    pub fn new(bindings: CloudflareBindingRegistry) -> Self {
        Self {
            bindings,
            access_keys: AccessKeyRegistry::new(),
        }
    }

    pub fn bindings(&self) -> &CloudflareBindingRegistry {
        &self.bindings
    }

    pub fn access_keys(&self) -> &AccessKeyRegistry {
        &self.access_keys
    }

    #[must_use]
    pub fn with_access_keys(mut self, access_keys: AccessKeyRegistry) -> Self {
        self.access_keys = access_keys;
        self
    }

    #[must_use]
    pub fn with_signed_access_key(
        mut self,
        access_key_id: impl Into<String>,
        tenant: TenantId,
        secret: impl Into<String>,
    ) -> Self {
        self.access_keys =
            self.access_keys
                .bind_signed(access_key_id.into(), tenant, secret.into());
        self
    }
}

pub(crate) fn build_cloudflare_router(config: Arc<CloudflareConfig>) -> Router<Arc<AppState>> {
    kv::router(config)
}
