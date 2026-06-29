//! `nimbus-blob` — content-addressed, per-tenant byte storage.
//!
//! This crate is the byte plane of the Nimbus object-storage stack. It owns the
//! object-safe [`BlobStore`] seam (spec §16b, seam A), its cluster extension
//! [`ReplicatingBlobStore`] (feature `cluster`), and the composable decorators
//! that layer encryption ([`EncryptedBlobStore`]) and placement
//! ([`PlacementBlobStore`]) over a backing store ([`MemoryBlobStore`]).
//!
//! ## Per-tenant store
//!
//! One store instance serves one tenant — the store **is** the tenant, matching
//! the per-tenant capability traits in `nimbus-storage/src/traits/mod.rs` and
//! spec §19. No [`BlobStore`] method takes a `tenant` argument; a tenant is
//! provisioned by constructing its own store (its own [`MemoryBlobStore`], its
//! own [`EncryptedBlobStore`] holding that tenant's DEK).
//!
//! Blobs are opaque content-addressed bytes (BLAKE3); the *named* object plane
//! (manifests, metadata, the S3 surface) lives in `nimbus-s3` over this byte
//! plane. `ChunkRead` is **not** defined here — it belongs to `nimbus-fs`; the
//! streaming read side is served by [`BlobStore::get_stream`].
//!
//! Composition order is **encrypt below placement**, so every placement leg
//! stores identical ciphertext under the same content address:
//!
//! ```text
//! PlacementBlobStore { local: EncryptedBlobStore<LocalPackStore>, mode: ... }
//! ```
//!
//! The shipped backing adapters are [`MemoryBlobStore`] for deterministic seam
//! tests, [`LocalPackStore`] for the durable local append-only pack, and
//! [`ObjectStoreBlobStore`] for cloud/object_store legs. [`ObjectBackup`] emits
//! the single-file structural backup bundle that can restore into any placement.

mod backup;
mod encrypted;
mod gc;
mod hash;
mod local;
mod memory;
mod object_store;
mod placement;
mod store;

pub use backup::{
    BackupBundle, BackupChunk, BackupRequest, BackupRestoreReport, KeyEscrow, ObjectBackup,
};
pub use encrypted::EncryptedBlobStore;
pub use gc::{BlobGc, BlobGcReport, BlobGcRoots, StaticBlobRoots};
pub use hash::{BLAKE3_HASH_LEN, BlobHash};
#[cfg(feature = "cluster")]
pub use hash::{BlobTicket, PeerAddr};
pub use local::{CompactionStats, LocalBlobEntry, LocalPackStore};
pub use memory::MemoryBlobStore;
pub use nimbus_crypto::{FRAME_PLAINTEXT_LEN, FramedBlobKey, KEY_SEED_LEN, NONCE_LEN};
pub use object_store::ObjectStoreBlobStore;
pub use placement::{PlacementBlobStore, PlacementMode};
#[cfg(feature = "cluster")]
pub use store::ReplicatingBlobStore;
pub use store::{BlobStore, ByteStream};

#[cfg(test)]
mod object_safety_tests {
    use std::sync::Arc;

    use bytes::Bytes;

    use super::*;

    /// A deterministic per-tenant framed key for tests.
    fn key() -> FramedBlobKey {
        FramedBlobKey::new(nimbus_crypto::DataEncryptionKey::new(
            *blake3::hash(b"tenant").as_bytes(),
        ))
    }

    /// `BlobStore` is object-safe: each impl coerces to `Arc<dyn BlobStore>`.
    /// Each store instance is one tenant's byte plane.
    #[tokio::test]
    async fn every_impl_is_object_safe() {
        let local: Arc<dyn BlobStore> = Arc::new(MemoryBlobStore::new());
        let encrypted: Arc<dyn BlobStore> =
            Arc::new(EncryptedBlobStore::new(MemoryBlobStore::new(), key()));
        let placement: Arc<dyn BlobStore> = Arc::new(PlacementBlobStore::local_only(Arc::new(
            MemoryBlobStore::new(),
        )));
        let object_store: Arc<dyn BlobStore> = Arc::new(ObjectStoreBlobStore::at_root(Arc::new(
            ::object_store::memory::InMemory::new(),
        )));

        for store in [local, encrypted, placement, object_store] {
            let hash = store.put(Bytes::from_static(b"object-safe")).await.unwrap();
            assert_eq!(
                store.get(&hash).await.unwrap(),
                Bytes::from_static(b"object-safe")
            );
        }
    }
}

#[cfg(all(test, feature = "cluster"))]
mod cluster_tests {
    use std::ops::Range;
    use std::sync::Arc;

    use async_trait::async_trait;
    use bytes::Bytes;
    use nimbus_core::{Error, Result, StorageErrorKind};

    use super::*;

    /// A minimal `ReplicatingBlobStore` for the upcast test.
    struct ReplicatingStub {
        inner: MemoryBlobStore,
    }

    #[async_trait]
    impl BlobStore for ReplicatingStub {
        async fn put(&self, bytes: Bytes) -> Result<BlobHash> {
            self.inner.put(bytes).await
        }
        async fn put_stream(&self, src: ByteStream) -> Result<BlobHash> {
            self.inner.put_stream(src).await
        }
        async fn get(&self, hash: &BlobHash) -> Result<Bytes> {
            self.inner.get(hash).await
        }
        async fn get_stream(&self, hash: &BlobHash) -> Result<ByteStream> {
            self.inner.get_stream(hash).await
        }
        async fn get_range(&self, hash: &BlobHash, range: Range<u64>) -> Result<Bytes> {
            self.inner.get_range(hash, range).await
        }
        async fn has(&self, hash: &BlobHash) -> Result<bool> {
            self.inner.has(hash).await
        }
        async fn release(&self, hash: &BlobHash) -> Result<()> {
            self.inner.release(hash).await
        }
    }

    #[async_trait]
    impl ReplicatingBlobStore for ReplicatingStub {
        async fn announce(&self, hash: &BlobHash) -> Result<BlobTicket> {
            Ok(BlobTicket::new(*hash, b"local-ticket".to_vec()))
        }
        async fn fetch_from(&self, _peer: &PeerAddr, ticket: &BlobTicket) -> Result<BlobHash> {
            // Fetch the bytes the ticket points at, re-hash them, and verify the
            // content address before admitting the blob (BLOB-5).
            let bytes = self.inner.get(&ticket.hash).await?;
            let actual = BlobHash::of(&bytes);
            if actual != ticket.hash {
                return Err(Error::storage(
                    StorageErrorKind::Corruption,
                    "fetched blob does not match ticket hash",
                ));
            }
            Ok(actual)
        }
    }

    /// A peer impl whose stored bytes do **not** match the announced hash, to
    /// prove `fetch_from` rejects a mismatch (BLOB-5).
    struct LyingPeer;

    #[async_trait]
    impl BlobStore for LyingPeer {
        async fn put(&self, bytes: Bytes) -> Result<BlobHash> {
            Ok(BlobHash::of(&bytes))
        }
        async fn put_stream(&self, _src: ByteStream) -> Result<BlobHash> {
            Err(Error::storage(
                StorageErrorKind::Unavailable,
                "lying peer does not accept writes",
            ))
        }
        async fn get(&self, _hash: &BlobHash) -> Result<Bytes> {
            // Always returns bytes that hash to something else.
            Ok(Bytes::from_static(b"tampered payload"))
        }
        async fn get_stream(&self, _hash: &BlobHash) -> Result<ByteStream> {
            Err(Error::storage(
                StorageErrorKind::Unavailable,
                "lying peer does not serve streams",
            ))
        }
        async fn get_range(&self, _hash: &BlobHash, _range: Range<u64>) -> Result<Bytes> {
            Err(Error::storage(
                StorageErrorKind::Unavailable,
                "lying peer does not serve ranges",
            ))
        }
        async fn has(&self, _hash: &BlobHash) -> Result<bool> {
            Ok(true)
        }
        async fn release(&self, _hash: &BlobHash) -> Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl ReplicatingBlobStore for LyingPeer {
        async fn announce(&self, hash: &BlobHash) -> Result<BlobTicket> {
            Ok(BlobTicket::new(*hash, b"lying-ticket".to_vec()))
        }
        async fn fetch_from(&self, _peer: &PeerAddr, ticket: &BlobTicket) -> Result<BlobHash> {
            let bytes = self.get(&ticket.hash).await?;
            let actual = BlobHash::of(&bytes);
            if actual != ticket.hash {
                return Err(Error::storage(
                    StorageErrorKind::Corruption,
                    "fetched blob does not match ticket hash",
                ));
            }
            Ok(actual)
        }
    }

    /// `Arc<dyn ReplicatingBlobStore>` → `Arc<dyn BlobStore>` native upcast
    /// (spec §17 D2). Exercises both the upcast and the upcasted handle.
    #[tokio::test]
    async fn replicating_upcasts_to_blob_store() {
        let replicating: Arc<dyn ReplicatingBlobStore> = Arc::new(ReplicatingStub {
            inner: MemoryBlobStore::new(),
        });

        // Use the cluster-leg methods through the subtrait handle.
        let hash = replicating
            .put(Bytes::from_static(b"replicate me"))
            .await
            .unwrap();
        let ticket = replicating.announce(&hash).await.unwrap();
        assert_eq!(ticket.hash, hash);
        let fetched = replicating
            .fetch_from(&PeerAddr::new("peer-1"), &ticket)
            .await
            .unwrap();
        assert_eq!(fetched, hash);

        // Native trait upcast: ReplicatingBlobStore -> BlobStore (D2, no shim).
        let as_blob: Arc<dyn BlobStore> = replicating;
        let got = as_blob.get(&hash).await.unwrap();
        assert_eq!(got, Bytes::from_static(b"replicate me"));
    }

    /// BLOB-5: a fetch whose bytes re-hash to the ticket hash is accepted, and a
    /// mismatch is rejected.
    #[tokio::test]
    async fn fetch_from_verifies_content_address() {
        // Honest peer: bytes match the announced hash.
        let honest = ReplicatingStub {
            inner: MemoryBlobStore::new(),
        };
        let hash = honest
            .put(Bytes::from_static(b"honest bytes"))
            .await
            .unwrap();
        let ticket = honest.announce(&hash).await.unwrap();
        let fetched = honest
            .fetch_from(&PeerAddr::new("peer"), &ticket)
            .await
            .unwrap();
        assert_eq!(fetched, hash, "matching bytes re-hash to ticket.hash");

        // Lying peer: announces one hash but serves bytes that hash to another.
        let liar = LyingPeer;
        let announced = BlobHash::of(b"what the ticket claims");
        let bad_ticket = liar.announce(&announced).await.unwrap();
        let err = liar
            .fetch_from(&PeerAddr::new("peer"), &bad_ticket)
            .await
            .unwrap_err();
        assert_eq!(
            err.storage_kind(),
            Some(StorageErrorKind::Corruption),
            "a hash mismatch on fetch is rejected"
        );
    }
}
