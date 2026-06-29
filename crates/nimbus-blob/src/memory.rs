//! [`MemoryBlobStore`] — an in-memory backing [`BlobStore`].
//!
//! This adapter backs blobs with an in-memory, content-addressed map keyed by
//! [`BlobHash`]. One [`MemoryBlobStore`] instance serves one tenant (the store
//! *is* the tenant — see the tenancy note on [`BlobStore`]), so there is no
//! tenant component in the key. It is a real, correct [`BlobStore`]: addressing
//! is BLAKE3 over the stored bytes, `put` is idempotent, and reads verify the
//! content address before returning. The production local adapter is
//! `LocalPackStore`, owned by NOS-A1; that adapter seals blobs into a small set
//! of append-only encrypted pack files with a `HashSeq`-style manifest.

use std::collections::HashMap;
use std::io::Cursor;
use std::ops::Range;
use std::sync::Mutex;

use async_trait::async_trait;
use bytes::Bytes;
use nimbus_core::{Error, Result, StorageErrorKind};
use tokio::io::AsyncReadExt;

use crate::hash::BlobHash;
use crate::store::{BlobStore, ByteStream};

/// In-memory, per-tenant, content-addressed blob store.
#[derive(Default)]
pub struct MemoryBlobStore {
    blobs: Mutex<HashMap<BlobHash, Bytes>>,
}

impl MemoryBlobStore {
    /// Creates an empty store for one tenant.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of distinct blobs currently stored.
    ///
    /// Exposed for tests asserting dedup (idempotent `put` stores once).
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Whether the store holds no blobs.
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<BlobHash, Bytes>> {
        // A poisoned lock means a prior holder panicked mid-mutation; surface
        // the bytes anyway rather than cascading the panic — the map values are
        // immutable `Bytes`, so there is no torn state to recover.
        self.blobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn fetch(&self, hash: &BlobHash) -> Result<Bytes> {
        let bytes = self
            .lock()
            .get(hash)
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("blob {hash}")))?;
        // Verify-on-read: detect silent corruption of the backing store.
        let actual = BlobHash::of(&bytes);
        if &actual != hash {
            return Err(Error::storage(
                StorageErrorKind::Corruption,
                format!("blob {hash} content address mismatch (stored bytes hash to {actual})"),
            ));
        }
        Ok(bytes)
    }
}

#[async_trait]
impl BlobStore for MemoryBlobStore {
    async fn put(&self, bytes: Bytes) -> Result<BlobHash> {
        let hash = BlobHash::of(&bytes);
        // Idempotent: identical bytes hash identically, so re-inserting under
        // the same key is a no-op store-once.
        self.lock().entry(hash).or_insert(bytes);
        Ok(hash)
    }

    async fn put_stream(&self, mut src: ByteStream) -> Result<BlobHash> {
        let mut buf = Vec::new();
        src.read_to_end(&mut buf).await.map_err(|err| {
            Error::storage(StorageErrorKind::Io, format!("read blob stream: {err}"))
        })?;
        self.put(Bytes::from(buf)).await
    }

    async fn get(&self, hash: &BlobHash) -> Result<Bytes> {
        self.fetch(hash)
    }

    async fn get_stream(&self, hash: &BlobHash) -> Result<ByteStream> {
        let bytes = self.fetch(hash)?;
        Ok(Box::new(Cursor::new(bytes)))
    }

    async fn get_range(&self, hash: &BlobHash, range: Range<u64>) -> Result<Bytes> {
        let bytes = self.fetch(hash)?;
        let len = bytes.len() as u64;
        if range.start > range.end || range.end > len {
            return Err(Error::InvalidInput(format!(
                "range {}..{} out of bounds for blob of {len} bytes",
                range.start, range.end
            )));
        }
        Ok(bytes.slice(range.start as usize..range.end as usize))
    }

    async fn has(&self, hash: &BlobHash) -> Result<bool> {
        Ok(self.lock().contains_key(hash))
    }

    async fn release(&self, hash: &BlobHash) -> Result<()> {
        // MemoryBlobStore has single-reference semantics; the durable store's
        // mark-and-sweep GC is owned by NOS-A2.
        self.lock().remove(hash);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn put_then_get_round_trips() {
        let store = MemoryBlobStore::new();
        let hash = store.put(Bytes::from_static(b"payload")).await.unwrap();
        let got = store.get(&hash).await.unwrap();
        assert_eq!(got, Bytes::from_static(b"payload"));
        assert_eq!(hash, BlobHash::of(b"payload"));
    }

    #[tokio::test]
    async fn put_is_idempotent_and_stores_once() {
        let store = MemoryBlobStore::new();
        let first = store.put(Bytes::from_static(b"dup")).await.unwrap();
        let second = store.put(Bytes::from_static(b"dup")).await.unwrap();
        assert_eq!(first, second);
        assert_eq!(store.len(), 1, "identical bytes stored exactly once");
    }

    #[tokio::test]
    async fn distinct_tenant_stores_are_independent() {
        // One store per tenant: the store *is* the tenant. Two tenants are two
        // separate instances and share nothing.
        let tenant_a = MemoryBlobStore::new();
        let tenant_b = MemoryBlobStore::new();
        let hash = tenant_a.put(Bytes::from_static(b"shared")).await.unwrap();
        assert!(tenant_a.has(&hash).await.unwrap());
        assert!(
            !tenant_b.has(&hash).await.unwrap(),
            "tenant b's store sees nothing tenant a stored"
        );
        assert_eq!(tenant_a.len(), 1);
        assert_eq!(tenant_b.len(), 0);
    }

    #[tokio::test]
    async fn get_range_slices_stored_bytes() {
        let store = MemoryBlobStore::new();
        let hash = store.put(Bytes::from_static(b"0123456789")).await.unwrap();
        let mid = store.get_range(&hash, 2..5).await.unwrap();
        assert_eq!(mid, Bytes::from_static(b"234"));
    }

    #[tokio::test]
    async fn get_range_rejects_out_of_bounds() {
        let store = MemoryBlobStore::new();
        let hash = store.put(Bytes::from_static(b"short")).await.unwrap();
        let err = store.get_range(&hash, 0..99).await.unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)));
    }

    #[tokio::test]
    async fn get_missing_is_not_found() {
        let store = MemoryBlobStore::new();
        let err = store.get(&BlobHash::of(b"absent")).await.unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));
    }

    #[tokio::test]
    async fn release_removes_the_blob() {
        let store = MemoryBlobStore::new();
        let hash = store.put(Bytes::from_static(b"temp")).await.unwrap();
        store.release(&hash).await.unwrap();
        assert!(!store.has(&hash).await.unwrap());
    }

    #[tokio::test]
    async fn put_stream_and_get_stream_round_trip() {
        use tokio::io::AsyncReadExt as _;

        let store = MemoryBlobStore::new();
        let src: ByteStream = Box::new(Cursor::new(Bytes::from_static(b"streamed")));
        let hash = store.put_stream(src).await.unwrap();

        let mut reader = store.get_stream(&hash).await.unwrap();
        let mut out = Vec::new();
        reader.read_to_end(&mut out).await.unwrap();
        assert_eq!(out, b"streamed");
    }
}
