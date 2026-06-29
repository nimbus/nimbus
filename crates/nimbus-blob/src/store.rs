//! The object-safe blob-store seam (spec §16b, seam A) and its cluster
//! extension (spec §16b, `ReplicatingBlobStore`).
//!
//! ## Per-tenant store (tenancy decision)
//!
//! A `BlobStore` instance **is** one tenant's byte plane — the store carries
//! the tenant binding, so no method takes a `tenant` argument. This mirrors the
//! per-tenant capability traits in `nimbus-storage/src/traits/mod.rs` and spec
//! §19: a tenant is provisioned by constructing its own store (its own
//! `MemoryBlobStore`, its own `EncryptedBlobStore` holding that tenant's DEK).
//! Cross-tenant isolation is structural, not a per-call check.
//!
//! Both traits use `#[async_trait]` because they are consumed as
//! `Arc<dyn _>`. `BlobHash` is small but passed by reference for signature
//! uniformity with the spec.

use std::ops::Range;

use async_trait::async_trait;
use bytes::Bytes;
use nimbus_core::Result;
use tokio::io::AsyncRead;

use crate::hash::BlobHash;
#[cfg(feature = "cluster")]
use crate::hash::{BlobTicket, PeerAddr};

/// A boxed async byte source, used by the streaming read/write methods.
pub type ByteStream = Box<dyn AsyncRead + Send + Unpin + 'static>;

/// Content-addressed byte storage for a single tenant.
///
/// One instance serves one tenant (the store *is* the tenant), so no method
/// takes a `tenant` argument. Implementations store opaque bytes addressed by
/// their BLAKE3 digest. `put` is idempotent: storing the same bytes twice
/// yields the same [`BlobHash`] and stores them once. Reads verify the content
/// address before returning.
#[async_trait]
pub trait BlobStore: Send + Sync {
    /// Stores `bytes`, returning their content address.
    async fn put(&self, bytes: Bytes) -> Result<BlobHash>;

    /// Stores the full contents of `src`, returning the address.
    async fn put_stream(&self, src: ByteStream) -> Result<BlobHash>;

    /// Returns the bytes addressed by `hash`.
    async fn get(&self, hash: &BlobHash) -> Result<Bytes>;

    /// Returns a streaming reader over the bytes addressed by `hash`.
    ///
    /// Added vs. the sketch: the streaming read side (the CAS read-only
    /// backend consumes this; it replaces the informal `ChunkRead`). Without
    /// it, large reads buffer into `Bytes`.
    async fn get_stream(&self, hash: &BlobHash) -> Result<ByteStream>;

    /// Returns the half-open byte `range` of the blob addressed by `hash`.
    async fn get_range(&self, hash: &BlobHash, range: Range<u64>) -> Result<Bytes>;

    /// Reports whether this tenant has a blob addressed by `hash`.
    async fn has(&self, hash: &BlobHash) -> Result<bool>;

    /// Releases this tenant's reference to `hash` (may delete on last reference).
    async fn release(&self, hash: &BlobHash) -> Result<()>;
}

/// Cluster leg: a [`BlobStore`] that can announce blobs to and fetch blobs
/// from peers (spec §16b, feature `cluster`).
///
/// Per spec §17 D2, `Arc<dyn ReplicatingBlobStore>` upcasts natively to
/// `Arc<dyn BlobStore>` on the pinned toolchain (rustc ≥ 1.86); no
/// `as_blob_store` shim is added.
#[cfg(feature = "cluster")]
#[async_trait]
pub trait ReplicatingBlobStore: BlobStore {
    /// Announces `hash` to the cluster, returning a fetch ticket.
    async fn announce(&self, hash: &BlobHash) -> Result<BlobTicket>;

    /// Fetches the blob described by `ticket` from `peer`, returning its hash.
    ///
    /// The fetched bytes are re-hashed and verified against `ticket.hash`
    /// before the blob is admitted; a mismatch is rejected (untrusted peer).
    async fn fetch_from(&self, peer: &PeerAddr, ticket: &BlobTicket) -> Result<BlobHash>;
}
