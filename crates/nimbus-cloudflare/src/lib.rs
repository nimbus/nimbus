//! Cloudflare-compatible adapter logic for Nimbus.
//!
//! This crate owns the wrangler binding configuration, the Workers KV policy,
//! the Worker KV host bridge, and the Durable Object substrate. It has no HTTP
//! transport. `nimbus-server` owns the KV REST router and maps [`kv::KvError`]
//! to the Cloudflare response envelope.

mod config;
pub mod durable_objects;
pub mod host_bridge;
pub mod kv;

pub use config::{
    CloudflareBindingRegistry, CloudflareConfig, D1DatabaseBinding, DurableObjectBinding,
    KvNamespaceBinding, R2BucketBinding, WranglerConfigError,
};
