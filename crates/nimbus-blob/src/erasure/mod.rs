//! Erasure-coded local blob storage over per-drive pack stores.
//!
//! [`ErasureBlobStore`] is a multi-drive [`crate::BlobStore`] leg. It splits
//! each blob into stripes, stores data and parity shards as ordinary blobs in
//! per-drive [`crate::LocalPackStore`] roots, and makes the blob visible only
//! by publishing a replicated manifest to every drive root. The manifest is
//! the commit point; shards written before a manifest publish are inert orphan
//! records until the later GC-root phase reclaims them.
//!
//! The store sits below encryption in the placement stack, so shard bytes are
//! opaque ciphertext whenever the caller composes it under
//! [`crate::EncryptedBlobStore`].

mod config;
mod heal;
mod manifest;
mod roots;
mod stats;
mod store;
mod stripe;

pub use config::ErasureConfig;
pub use heal::{ErasureHealer, HealPacing, HealReport, HealSummary};
pub use stats::ErasureStats;
pub use store::ErasureBlobStore;

#[cfg(test)]
mod tests;
