//! Native Nimbus object-storage control-plane resolver.
//!
//! This crate is deliberately not an S3 protocol crate. It turns persisted
//! placement policy plus operator configuration into byte-plane [`BlobStore`]
//! compositions that S3, Convex `_storage`, backup/restore, R2 compatibility,
//! and the future filesystem binder can share.

mod backup;
mod config;
mod credentials;
mod gc;
mod migrate;
mod resolver;

pub use backup::object_backup_roots;
pub use config::{ErasureLegConfig, LocalLeg, ObjectStorageConfig, ObjectStorageEnv};
pub use credentials::{ObjectStoreCredentialResolver, ObjectStoreSecret};
pub use gc::{object_gc_roots, object_gc_roots_provider};
pub use migrate::{MigrationLeg, migration_leg};
pub use resolver::{
    ObjectStorageResolver, object_blob_key_path, object_blob_root, object_master_key_path,
    tenant_root_identity,
};

#[cfg(test)]
mod tests;
