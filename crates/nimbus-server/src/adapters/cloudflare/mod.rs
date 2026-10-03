//! Cloudflare-compatible adapter bootstrap.
//!
//! The bootstrap owns the Workers KV REST router. The binding configuration,
//! KV policy, Worker KV host bridge, and Durable Object substrate live in
//! `nimbus-cloudflare` over Nimbus storage and service primitives.
//!
//! Cloudflare non-loopback binds are refused during CLI startup unless the
//! operator opts in with the shared network-bind guard.
//!
//! Router construction stays here because it needs `axum::Router` and
//! `AppState`.

use std::sync::Arc;

use axum::Router;
use nimbus_cloudflare::CloudflareConfig;

use crate::state::AppState;

mod kv;

pub(crate) fn build_cloudflare_router(config: Arc<CloudflareConfig>) -> Router<Arc<AppState>> {
    kv::router(config)
}
