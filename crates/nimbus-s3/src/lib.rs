//! S3-compatible object surface over Nimbus blob and metadata planes.

mod auth;
mod backend;
mod checksum;
mod config;
pub mod convex;
mod object_io;
mod service;

pub use auth::{AccessKeyRegistry, KeyBinding, S3_ACCESS_KEY_SPEC};
pub use backend::{
    S3ObjectMeta, S3TenantBlobs, S3TenantObjects, S3TenantResolver, put_manifest_unconditional,
};
pub use config::{DEFAULT_S3_PORT, S3Config};
pub use service::NimbusS3;

#[cfg(test)]
mod tests;
