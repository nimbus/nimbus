//! Cloudflare-compatible adapter bootstrap.
//!
//! The bootstrap owns adapter registration and binding configuration. KV and
//! Durable Object data-plane behavior lives in concept-owned child modules over
//! Nimbus storage and service primitives.
//!
//! Cloudflare non-loopback binds are refused during CLI startup unless the
//! operator opts in with the shared network-bind guard.
//!
//! `CloudflareConfig` and its wrangler-config parsing are axum-free (CP2) and
//! live in `nimbus_compute::cloudflare_config`; this module re-exports them so
//! existing `crate::adapters::cloudflare::CloudflareConfig` paths keep
//! resolving. Router construction stays here because it needs `axum::Router`
//! and `AppState`.

use std::sync::Arc;

use axum::Router;

use crate::state::AppState;

pub mod durable_objects;
pub mod host_bridge;
pub mod kv;

pub use nimbus_compute::cloudflare_config::{
    CloudflareBindingRegistry, CloudflareConfig, D1DatabaseBinding, DurableObjectBinding,
    KvNamespaceBinding, R2BucketBinding, WranglerConfigError,
};

pub(crate) fn build_cloudflare_router(config: Arc<CloudflareConfig>) -> Router<Arc<AppState>> {
    kv::router(config)
}
