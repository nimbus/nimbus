//! [`EncryptedBlobStore`] - transparent at-rest encryption decorator.
//!
//! `nimbus-blob` owns the byte-plane decorator and content-addressed store
//! composition. The framed AEAD construction itself lives in `nimbus-crypto`.

use std::ops::Range;

use async_trait::async_trait;
use bytes::Bytes;
use nimbus_core::{Error, Result};
use nimbus_crypto::{
    FRAMED_HEADER_LEN, FramedBlobHeader, FramedBlobKey, FramedBlobSeed,
    framed_span_for_plaintext_range, open_framed_blob, open_framed_blob_range, open_framed_span,
    random_framed_salt, seal_framed_blob,
};
use tokio::io::AsyncReadExt;

use crate::hash::BlobHash;
use crate::store::{BlobStore, ByteStream};

/// Encrypts on `put`, decrypts on `get` - transparent at-rest encryption.
///
/// Holds one tenant framed-blob key. The inner store only sees framed
/// ciphertext; content addresses are over that ciphertext.
pub struct EncryptedBlobStore<S: BlobStore> {
    inner: S,
    key: FramedBlobKey,
}

impl<S: BlobStore> EncryptedBlobStore<S> {
    /// Wraps `inner` for one tenant under `key`.
    pub fn new(inner: S, key: FramedBlobKey) -> Self {
        Self { inner, key }
    }

    /// Borrows the inner store for inspecting stored ciphertext in tests.
    pub fn inner(&self) -> &S {
        &self.inner
    }

    fn seal_content(&self, plaintext: &[u8]) -> Result<Bytes> {
        seal_framed_blob(&self.key, FramedBlobSeed::Content, plaintext).map(Bytes::from)
    }

    fn seal_stream_buffer(&self, plaintext: &[u8]) -> Result<Bytes> {
        seal_framed_blob(
            &self.key,
            FramedBlobSeed::Salt(random_framed_salt()),
            plaintext,
        )
        .map(Bytes::from)
    }

    fn open(&self, framed: &[u8]) -> Result<Bytes> {
        open_framed_blob(&self.key, framed).map(Bytes::from)
    }

    fn open_range(&self, framed: &[u8], range: Range<u64>) -> Result<Bytes> {
        open_framed_blob_range(&self.key, framed, range).map(Bytes::from)
    }

    #[cfg(test)]
    fn parse_header_for_tests(framed: &[u8]) -> Result<nimbus_crypto::FramedBlobHeader> {
        nimbus_crypto::FramedBlobHeader::parse(framed).map(|(header, _body)| header)
    }
}

#[async_trait]
impl<S: BlobStore> BlobStore for EncryptedBlobStore<S> {
    async fn put(&self, bytes: Bytes) -> Result<BlobHash> {
        let framed = self.seal_content(&bytes)?;
        self.inner.put(framed).await
    }

    async fn put_stream(&self, mut src: ByteStream) -> Result<BlobHash> {
        let mut buf = Vec::new();
        src.read_to_end(&mut buf).await.map_err(|err| {
            Error::storage(
                nimbus_core::StorageErrorKind::Io,
                format!("read blob stream: {err}"),
            )
        })?;
        let framed = self.seal_stream_buffer(&buf)?;
        self.inner.put(framed).await
    }

    async fn get(&self, hash: &BlobHash) -> Result<Bytes> {
        let framed = self.inner.get(hash).await?;
        self.open(&framed)
    }

    async fn get_stream(&self, hash: &BlobHash) -> Result<ByteStream> {
        let plaintext = self.get(hash).await?;
        Ok(Box::new(std::io::Cursor::new(plaintext)))
    }

    async fn get_range(&self, hash: &BlobHash, range: Range<u64>) -> Result<Bytes> {
        if range.start > range.end {
            return Err(Error::InvalidInput(format!(
                "range {}..{} out of bounds: start after end",
                range.start, range.end
            )));
        }
        // Bounded probe: the header is a small fixed size, so learn the
        // plaintext length (and frame layout) without ever fetching the
        // whole ciphertext.
        let header_bytes = self
            .inner
            .get_range(hash, 0..FRAMED_HEADER_LEN as u64)
            .await?;
        let (header, _) = FramedBlobHeader::parse(&header_bytes)?;
        let len = header.plaintext_len as u64;
        if range.end > len {
            return Err(Error::InvalidInput(format!(
                "range {}..{} out of bounds for blob of {len} bytes",
                range.start, range.end
            )));
        }
        if range.start == range.end {
            return Ok(Bytes::new());
        }
        if range.start == 0 && range.end == len {
            // Full-blob range: keep the whole-fetch path so the trailing
            // ciphertext-body-length check in `open_framed_blob_range`
            // still runs (it only runs for the exact `0..len` request).
            let framed = self.inner.get(hash).await?;
            return self.open_range(&framed, range);
        }
        let span = framed_span_for_plaintext_range(&header, range.clone())?;
        let framed_span = self.inner.get_range(hash, span).await?;
        let plaintext = open_framed_span(&self.key, &header, &framed_span, range)?;
        Ok(Bytes::from(plaintext))
    }

    async fn has(&self, hash: &BlobHash) -> Result<bool> {
        self.inner.has(hash).await
    }

    async fn release(&self, hash: &BlobHash) -> Result<()> {
        self.inner.release(hash).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use nimbus_crypto::{DataEncryptionKey, FRAME_PLAINTEXT_LEN, FramedSeedKind, KEY_SEED_LEN};

    use super::*;
    use crate::memory::MemoryBlobStore;

    fn key(seed: &str) -> FramedBlobKey {
        FramedBlobKey::new(DataEncryptionKey::new(
            *blake3::hash(seed.as_bytes()).as_bytes(),
        ))
    }

    /// Test-only [`BlobStore`] wrapper that counts bytes served by `get` and
    /// `get_range`, so a `get_range` test on [`EncryptedBlobStore`] can prove
    /// the *inner* (ciphertext) transfer stayed bounded to a handful of
    /// frames instead of the whole framed blob.
    #[derive(Clone)]
    struct CountingBlobStore {
        inner: Arc<MemoryBlobStore>,
        bytes_served: Arc<AtomicU64>,
    }

    impl CountingBlobStore {
        fn new(inner: MemoryBlobStore) -> Self {
            Self {
                inner: Arc::new(inner),
                bytes_served: Arc::new(AtomicU64::new(0)),
            }
        }
    }

    #[async_trait]
    impl BlobStore for CountingBlobStore {
        async fn put(&self, bytes: Bytes) -> Result<BlobHash> {
            self.inner.put(bytes).await
        }

        async fn put_stream(&self, src: ByteStream) -> Result<BlobHash> {
            self.inner.put_stream(src).await
        }

        async fn get(&self, hash: &BlobHash) -> Result<Bytes> {
            let bytes = self.inner.get(hash).await?;
            self.bytes_served
                .fetch_add(bytes.len() as u64, Ordering::SeqCst);
            Ok(bytes)
        }

        async fn get_stream(&self, hash: &BlobHash) -> Result<ByteStream> {
            self.inner.get_stream(hash).await
        }

        async fn get_range(&self, hash: &BlobHash, range: Range<u64>) -> Result<Bytes> {
            let bytes = self.inner.get_range(hash, range).await?;
            self.bytes_served
                .fetch_add(bytes.len() as u64, Ordering::SeqCst);
            Ok(bytes)
        }

        async fn has(&self, hash: &BlobHash) -> Result<bool> {
            self.inner.has(hash).await
        }

        async fn release(&self, hash: &BlobHash) -> Result<()> {
            self.inner.release(hash).await
        }
    }

    #[tokio::test]
    async fn put_then_get_round_trips_through_crypto() {
        let store = EncryptedBlobStore::new(MemoryBlobStore::new(), key("acme"));
        let plaintext = Bytes::from_static(b"top secret payload");
        let hash = store.put(plaintext.clone()).await.unwrap();
        let got = store.get(&hash).await.unwrap();
        assert_eq!(got, plaintext);
    }

    #[tokio::test]
    async fn ciphertext_differs_from_plaintext() {
        let store = EncryptedBlobStore::new(MemoryBlobStore::new(), key("acme"));
        let plaintext = Bytes::from_static(b"this should be encrypted at rest");
        let hash = store.put(plaintext.clone()).await.unwrap();
        let stored = store.inner().get(&hash).await.unwrap();
        assert_ne!(stored, plaintext, "stored bytes must not be plaintext");
        assert_eq!(hash, BlobHash::of(&stored), "address is over ciphertext");
    }

    #[tokio::test]
    async fn identical_plaintext_and_key_yields_identical_ciphertext() {
        let store_a = EncryptedBlobStore::new(MemoryBlobStore::new(), key("acme"));
        let store_b = EncryptedBlobStore::new(MemoryBlobStore::new(), key("acme"));
        let plaintext = Bytes::from_static(b"dedup me please");

        let hash_a = store_a.put(plaintext.clone()).await.unwrap();
        let hash_b = store_b.put(plaintext.clone()).await.unwrap();
        assert_eq!(
            hash_a, hash_b,
            "same plaintext + DEK yields same content address"
        );

        let cipher_a = store_a.inner().get(&hash_a).await.unwrap();
        let cipher_b = store_b.inner().get(&hash_b).await.unwrap();
        assert_eq!(
            cipher_a, cipher_b,
            "same plaintext + DEK yields same ciphertext"
        );
    }

    #[tokio::test]
    async fn different_keys_yield_different_ciphertext() {
        let plaintext = Bytes::from_static(b"sensitive");
        let store_a = EncryptedBlobStore::new(MemoryBlobStore::new(), key("tenant-a"));
        let store_b = EncryptedBlobStore::new(MemoryBlobStore::new(), key("tenant-b"));
        let hash_a = store_a.put(plaintext.clone()).await.unwrap();
        let hash_b = store_b.put(plaintext.clone()).await.unwrap();
        assert_ne!(
            hash_a, hash_b,
            "different DEK yields different ciphertext/address"
        );
    }

    #[tokio::test]
    async fn empty_plaintext_round_trips() {
        let store = EncryptedBlobStore::new(MemoryBlobStore::new(), key("acme"));
        let hash = store.put(Bytes::new()).await.unwrap();
        assert_eq!(store.get(&hash).await.unwrap(), Bytes::new());
    }

    #[tokio::test]
    async fn multi_frame_plaintext_round_trips() {
        let store = EncryptedBlobStore::new(MemoryBlobStore::new(), key("acme"));
        let plaintext: Vec<u8> = (0..(FRAME_PLAINTEXT_LEN * 2 + 1234))
            .map(|i| (i % 251) as u8)
            .collect();
        let bytes = Bytes::from(plaintext.clone());
        let hash = store.put(bytes).await.unwrap();
        let got = store.get(&hash).await.unwrap();
        assert_eq!(got.len(), plaintext.len());
        assert_eq!(got, Bytes::from(plaintext));
    }

    #[tokio::test]
    async fn get_range_returns_plaintext_slice() {
        let store = EncryptedBlobStore::new(MemoryBlobStore::new(), key("acme"));
        let hash = store.put(Bytes::from_static(b"abcdefghij")).await.unwrap();
        let slice = store.get_range(&hash, 3..7).await.unwrap();
        assert_eq!(slice, Bytes::from_static(b"defg"));
    }

    #[tokio::test]
    async fn get_range_opens_only_overlapping_frames() {
        let store = EncryptedBlobStore::new(MemoryBlobStore::new(), key("acme"));
        let plaintext: Vec<u8> = (0..(FRAME_PLAINTEXT_LEN * 3 + 21))
            .map(|i| (i % 251) as u8)
            .collect();
        let hash = store.put(Bytes::from(plaintext.clone())).await.unwrap();
        let start = FRAME_PLAINTEXT_LEN as u64 + 11;
        let end = FRAME_PLAINTEXT_LEN as u64 * 2 + 19;
        let slice = store.get_range(&hash, start..end).await.unwrap();
        assert_eq!(
            slice,
            Bytes::copy_from_slice(&plaintext[start as usize..end as usize])
        );
    }

    #[tokio::test]
    async fn encrypted_range_read_transfers_only_inner_bytes_served() {
        let counting = CountingBlobStore::new(MemoryBlobStore::new());
        let bytes_served = counting.bytes_served.clone();
        let store = EncryptedBlobStore::new(counting, key("acme"));

        let big: Vec<u8> = (0..1_048_576usize).map(|i| (i % 251) as u8).collect();
        let hash = store.put(Bytes::from(big.clone())).await.unwrap();
        bytes_served.store(0, Ordering::SeqCst);

        let slice = store.get_range(&hash, 4096..8192).await.unwrap();

        assert_eq!(slice, Bytes::copy_from_slice(&big[4096..8192]));
        let served = bytes_served.load(Ordering::SeqCst);
        assert!(
            served < (FRAME_PLAINTEXT_LEN * 3) as u64,
            "range read should transfer only the overlapping frames (< 3 frames' worth), \
             not the whole 1MiB blob: served {served} bytes"
        );
    }

    #[tokio::test]
    async fn get_range_straddles_two_frames() {
        let store = EncryptedBlobStore::new(MemoryBlobStore::new(), key("acme"));
        let plaintext: Vec<u8> = (0..(FRAME_PLAINTEXT_LEN * 2))
            .map(|i| (i % 251) as u8)
            .collect();
        let hash = store.put(Bytes::from(plaintext.clone())).await.unwrap();
        let start = FRAME_PLAINTEXT_LEN as u64 - 10;
        let end = FRAME_PLAINTEXT_LEN as u64 + 10;
        let slice = store.get_range(&hash, start..end).await.unwrap();
        assert_eq!(
            slice,
            Bytes::copy_from_slice(&plaintext[start as usize..end as usize])
        );
    }

    #[tokio::test]
    async fn get_range_at_exact_frame_edge() {
        let store = EncryptedBlobStore::new(MemoryBlobStore::new(), key("acme"));
        let plaintext: Vec<u8> = (0..(FRAME_PLAINTEXT_LEN * 2))
            .map(|i| (i % 251) as u8)
            .collect();
        let hash = store.put(Bytes::from(plaintext.clone())).await.unwrap();
        let start = 0u64;
        let end = FRAME_PLAINTEXT_LEN as u64;
        let slice = store.get_range(&hash, start..end).await.unwrap();
        assert_eq!(
            slice,
            Bytes::copy_from_slice(&plaintext[start as usize..end as usize])
        );
    }

    #[tokio::test]
    async fn get_range_covers_final_partial_frame() {
        let store = EncryptedBlobStore::new(MemoryBlobStore::new(), key("acme"));
        let plaintext: Vec<u8> = (0..(FRAME_PLAINTEXT_LEN * 2 + 123))
            .map(|i| (i % 251) as u8)
            .collect();
        let hash = store.put(Bytes::from(plaintext.clone())).await.unwrap();
        let start = FRAME_PLAINTEXT_LEN as u64 * 2;
        let end = plaintext.len() as u64;
        let slice = store.get_range(&hash, start..end).await.unwrap();
        assert_eq!(
            slice,
            Bytes::copy_from_slice(&plaintext[start as usize..end as usize])
        );
    }

    #[tokio::test]
    async fn get_range_rejects_end_past_blob_length() {
        let store = EncryptedBlobStore::new(MemoryBlobStore::new(), key("acme"));
        let hash = store.put(Bytes::from_static(b"abcdefghij")).await.unwrap();
        let err = store.get_range(&hash, 3..100).await.unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)));
    }

    #[tokio::test]
    #[allow(clippy::reversed_empty_ranges)]
    async fn get_range_rejects_start_after_end() {
        let store = EncryptedBlobStore::new(MemoryBlobStore::new(), key("acme"));
        let hash = store.put(Bytes::from_static(b"abcdefghij")).await.unwrap();
        let err = store.get_range(&hash, 8..4).await.unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)));
    }

    #[tokio::test]
    async fn streamed_put_uses_crypto_salt_and_round_trips() {
        use std::io::Cursor;
        let store = EncryptedBlobStore::new(MemoryBlobStore::new(), key("acme"));
        let payload = Bytes::from_static(b"streamed via salt seed");
        let src: ByteStream = Box::new(Cursor::new(payload.clone()));
        let hash = store.put_stream(src).await.unwrap();
        let got = store.get(&hash).await.unwrap();
        assert_eq!(got, payload);

        let framed = store.inner().get(&hash).await.unwrap();
        let header =
            EncryptedBlobStore::<MemoryBlobStore>::parse_header_for_tests(&framed).unwrap();
        assert_eq!(
            header.seed_kind,
            FramedSeedKind::Salt,
            "streamed put seeds by crypto-grade random salt"
        );
    }

    #[tokio::test]
    async fn streamed_puts_do_not_dedup() {
        use std::io::Cursor;
        let store = EncryptedBlobStore::new(MemoryBlobStore::new(), key("acme"));
        let payload = Bytes::from_static(b"identical stream bytes");
        let h1 = store
            .put_stream(Box::new(Cursor::new(payload.clone())))
            .await
            .unwrap();
        let h2 = store
            .put_stream(Box::new(Cursor::new(payload.clone())))
            .await
            .unwrap();
        assert_ne!(h1, h2, "random per-object salt yields distinct ciphertext");
        assert_eq!(store.get(&h1).await.unwrap(), payload);
        assert_eq!(store.get(&h2).await.unwrap(), payload);
    }

    #[tokio::test]
    async fn tampered_ciphertext_fails_to_open() {
        let store = EncryptedBlobStore::new(MemoryBlobStore::new(), key("acme"));
        let hash = store.put(Bytes::from_static(b"authentic")).await.unwrap();

        let mut framed = store.inner().get(&hash).await.unwrap().to_vec();
        let body_byte = 4 + 1 + KEY_SEED_LEN + 8;
        framed[body_byte] ^= 0xff;
        let tampered_hash = store.inner().put(Bytes::from(framed)).await.unwrap();

        let err = store.get(&tampered_hash).await.unwrap_err();
        assert_eq!(
            err.storage_kind(),
            Some(nimbus_core::StorageErrorKind::Corruption)
        );
    }

    #[tokio::test]
    async fn tampered_header_len_fails_to_open() {
        let store = EncryptedBlobStore::new(MemoryBlobStore::new(), key("acme"));
        let hash = store.put(Bytes::from_static(b"len-bound")).await.unwrap();
        let mut framed = store.inner().get(&hash).await.unwrap().to_vec();
        framed[4 + 1 + KEY_SEED_LEN + 8 - 1] ^= 0x01;
        let tampered_hash = store.inner().put(Bytes::from(framed)).await.unwrap();
        let err = store.get(&tampered_hash).await.unwrap_err();
        assert_eq!(
            err.storage_kind(),
            Some(nimbus_core::StorageErrorKind::Corruption)
        );
    }

    #[tokio::test]
    async fn crypto_shred_by_losing_tenant_dek_makes_blob_unreadable() {
        let store = EncryptedBlobStore::new(MemoryBlobStore::new(), key("tenant-before-rm"));
        let hash = store
            .put(Bytes::from_static(b"tenant secret"))
            .await
            .unwrap();
        let framed = store.inner().get(&hash).await.unwrap();

        let raw_after_rm = MemoryBlobStore::new();
        let copied_hash = raw_after_rm.put(framed).await.unwrap();
        assert_eq!(copied_hash, hash);

        let after_shred = EncryptedBlobStore::new(raw_after_rm, key("tenant-after-rm"));
        let err = after_shred.get(&hash).await.unwrap_err();
        assert_eq!(
            err.storage_kind(),
            Some(nimbus_core::StorageErrorKind::Corruption)
        );
    }

    // Recipe 1: exercise the exact full-blob range (`0..len`) path, where
    // `get_range` fetches the whole framed blob and calls `open_range`. No
    // other counted test hits this branch, so a mutant that makes
    // `open_range` return empty bytes survives without this assertion.
    #[tokio::test]
    async fn get_range_full_span_equals_get() {
        let store = EncryptedBlobStore::new(MemoryBlobStore::new(), key("acme"));
        let plaintext = Bytes::from_static(b"full range must equal a whole get");
        let hash = store.put(plaintext.clone()).await.unwrap();
        let len = plaintext.len() as u64;

        let full = store.get_range(&hash, 0..len).await.unwrap();

        assert_eq!(
            full,
            store.get(&hash).await.unwrap(),
            "get_range(0..len) must return exactly what get() returns"
        );
        assert_eq!(
            full, plaintext,
            "get_range(0..len) must decode to the true plaintext, not empty bytes"
        );
    }

    // Recipe 2: an in-bounds empty range (`n..n`) inside a non-empty blob is a
    // valid empty slice, not an error. A mutant that narrows the `start > end`
    // guard to reject `start == end` would turn this into an error.
    #[tokio::test]
    async fn get_range_empty_point_in_bounds_returns_empty_slice() {
        let store = EncryptedBlobStore::new(MemoryBlobStore::new(), key("acme"));
        let hash = store.put(Bytes::from_static(b"abcdefghij")).await.unwrap();

        let point = store.get_range(&hash, 5..5).await.unwrap();

        assert_eq!(
            point,
            Bytes::new(),
            "an in-bounds empty range n..n yields an empty slice, not an error and not the whole blob"
        );
    }

    // Recipe 3: prove the exact `0..len` request takes the whole-blob fast
    // path (`self.inner.get` + `open_framed_blob_range`) whose trailing
    // ciphertext-body-length check runs, rather than the partial-span path
    // (`open_framed_span`) which sizes an exact per-frame span and never sees
    // trailing bytes. Appending garbage past the true framed body makes only
    // the whole-blob path reject it, so a mutant that flips the fast-path
    // predicate (routing `0..len` through the partial-span path) would wrongly
    // succeed and is caught here.
    #[tokio::test]
    async fn get_range_full_span_runs_whole_blob_body_length_check() {
        let store = EncryptedBlobStore::new(MemoryBlobStore::new(), key("acme"));
        let plaintext = Bytes::from_static(b"abcdefghij");
        let len = plaintext.len() as u64;
        let hash = store.put(plaintext.clone()).await.unwrap();

        // Sanity: an untouched full-blob read succeeds and equals the plaintext.
        assert_eq!(store.get_range(&hash, 0..len).await.unwrap(), plaintext);

        // Append trailing bytes past the true framed body. The partial-span
        // path computes a span of exactly [header .. header + one sealed
        // frame] and never fetches these bytes; only the whole-blob fast path
        // fetches the whole ciphertext and runs the body-length check.
        let mut framed = store.inner().get(&hash).await.unwrap().to_vec();
        framed.extend_from_slice(b"trailing garbage past the framed body");
        let tampered_hash = store.inner().put(Bytes::from(framed)).await.unwrap();

        let err = store.get_range(&tampered_hash, 0..len).await.unwrap_err();

        assert_eq!(
            err.storage_kind(),
            Some(nimbus_core::StorageErrorKind::Corruption),
            "the 0..len request must take the whole-blob path whose body-length check rejects \
             trailing bytes; the partial-span path would ignore them and wrongly succeed"
        );
    }

    // Recipe 4: `release` must delegate deletion to the inner
    // content-addressed store. Inspect the inner store directly (not just the
    // wrapper) so a mutant that makes `release` a no-op is caught.
    #[tokio::test]
    async fn release_delegates_deletion_to_inner_store() {
        let store = EncryptedBlobStore::new(MemoryBlobStore::new(), key("acme"));
        let hash = store
            .put(Bytes::from_static(b"ephemeral secret"))
            .await
            .unwrap();
        assert!(
            store.inner().has(&hash).await.unwrap(),
            "inner store holds the ciphertext before release"
        );

        store.release(&hash).await.unwrap();

        assert!(
            !store.inner().has(&hash).await.unwrap(),
            "release must remove the ciphertext from the inner content-addressed store"
        );
        let err = store.inner().get(&hash).await.unwrap_err();
        assert!(
            matches!(err, Error::NotFound(_)),
            "inner get after release is NotFound, matching the inner store's deletion semantics"
        );
        assert!(
            !store.has(&hash).await.unwrap(),
            "the encrypted wrapper also reports the blob gone after release"
        );
    }
}
