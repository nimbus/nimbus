//! [`ObjectStoreBlobStore`] - cloud/object_store backing leg for [`BlobStore`].
//!
//! The adapter is intentionally only a byte-plane leg. Placement policy stays in
//! [`crate::PlacementBlobStore`], and provider construction stays outside this
//! crate so operator config can inject S3/GCS/Azure/local object stores later.

use std::ops::Range;
use std::sync::Arc;

use ::object_store::path::Path as ObjectPath;
use ::object_store::{ObjectStore, ObjectStoreExt};
use async_trait::async_trait;
use bytes::Bytes;
use nimbus_core::{Error, Result, StorageErrorKind};
use tokio::io::AsyncReadExt;

use crate::hash::BlobHash;
use crate::store::{BlobStore, ByteStream};

/// A [`BlobStore`] implementation backed by an `object_store::ObjectStore`.
///
/// Each instance is still one tenant's byte plane. The wrapped object store
/// should already point at the tenant/provider prefix that config selected; the
/// optional `prefix` here is an additional Nimbus-private namespace below it.
#[derive(Clone)]
pub struct ObjectStoreBlobStore {
    store: Arc<dyn ObjectStore>,
    prefix_parts: Arc<[String]>,
}

impl ObjectStoreBlobStore {
    /// Creates a cloud/object-store blob leg under `prefix`.
    pub fn new(store: Arc<dyn ObjectStore>, prefix: impl AsRef<str>) -> Self {
        let prefix_parts = prefix
            .as_ref()
            .split('/')
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>()
            .into();
        Self {
            store,
            prefix_parts,
        }
    }

    /// Creates a cloud/object-store blob leg at the store root.
    pub fn at_root(store: Arc<dyn ObjectStore>) -> Self {
        Self::new(store, "")
    }

    fn object_path(&self, hash: &BlobHash) -> ObjectPath {
        let hex = hash.to_hex();
        let shard = &hex[..2];
        let mut parts = self
            .prefix_parts
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        parts.extend(["blobs", shard, hex.as_str()]);
        ObjectPath::from_iter(parts)
    }
}

#[async_trait]
impl BlobStore for ObjectStoreBlobStore {
    async fn put(&self, bytes: Bytes) -> Result<BlobHash> {
        let hash = BlobHash::of(&bytes);
        let path = self.object_path(&hash);
        self.store
            .put(&path, bytes.into())
            .await
            .map_err(|err| map_object_error(err, "put", &path))?;
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
        let path = self.object_path(hash);
        let bytes = self
            .store
            .get(&path)
            .await
            .map_err(|err| map_object_error(err, "get", &path))?
            .bytes()
            .await
            .map_err(|err| map_object_error(err, "read", &path))?;
        verify_content_address(hash, bytes)
    }

    async fn get_stream(&self, hash: &BlobHash) -> Result<ByteStream> {
        Ok(Box::new(std::io::Cursor::new(self.get(hash).await?)))
    }

    async fn get_range(&self, hash: &BlobHash, range: Range<u64>) -> Result<Bytes> {
        if range.start > range.end {
            return Err(Error::InvalidInput(format!(
                "range {}..{} out of bounds: start after end",
                range.start, range.end
            )));
        }
        let path = self.object_path(hash);
        if range.start == range.end {
            // `object_store`'s own `GetRange::Bounded` treats a zero-length
            // range as invalid input rather than a trivially satisfiable
            // request, so short-circuit here. Still confirm the blob exists
            // (a metadata-only HEAD, no body bytes) so a missing blob keeps
            // failing NotFound instead of silently returning empty bytes.
            self.store
                .head(&path)
                .await
                .map_err(|err| map_object_error(err, "head", &path))?;
            return Ok(Bytes::new());
        }
        let requested_len = range.end - range.start;
        match self.store.get_range(&path, range.clone()).await {
            Ok(bytes) => {
                if bytes.len() as u64 != requested_len {
                    // `object_store`'s own `GetRange::Bounded` semantics clamp
                    // `range.end` to the object's actual length instead of
                    // erroring when only the end overflows (HTTP Range
                    // semantics: "if the range ends after the end of the
                    // object, the entire remainder... will be returned").
                    // Normalize that into the same out-of-bounds
                    // `InvalidInput` shape the other two `BlobStore` legs use.
                    let actual_len = self
                        .store
                        .head(&path)
                        .await
                        .map(|meta| meta.size)
                        .unwrap_or(range.start + bytes.len() as u64);
                    return Err(Error::InvalidInput(format!(
                        "range {}..{} out of bounds for blob of {actual_len} bytes",
                        range.start, range.end
                    )));
                }
                Ok(bytes)
            }
            Err(err) => {
                // `object_store` backends collapse their internal range-validation
                // errors (`InvalidGetRange`) into an opaque `Error::Generic`
                // regardless of backend (confirmed for both `memory` and `local`),
                // so `err` alone can't distinguish "genuinely out of bounds" from
                // any other backend failure. Ask the backend for the object's true
                // size (a metadata-only HEAD, no body bytes) to disambiguate and
                // produce the same `InvalidInput` shape the other two `BlobStore`
                // legs use for out-of-bounds ranges.
                if let Ok(meta) = self.store.head(&path).await {
                    if range.end > meta.size {
                        return Err(Error::InvalidInput(format!(
                            "range {}..{} out of bounds for blob of {} bytes",
                            range.start, range.end, meta.size
                        )));
                    }
                }
                Err(map_object_error(err, "get_range", &path))
            }
        }
    }

    async fn has(&self, hash: &BlobHash) -> Result<bool> {
        let path = self.object_path(hash);
        match self.store.head(&path).await {
            Ok(_) => Ok(true),
            Err(::object_store::Error::NotFound { .. }) => Ok(false),
            Err(err) => Err(map_object_error(err, "head", &path)),
        }
    }

    async fn release(&self, hash: &BlobHash) -> Result<()> {
        let path = self.object_path(hash);
        match self.store.delete(&path).await {
            Ok(()) | Err(::object_store::Error::NotFound { .. }) => Ok(()),
            Err(err) => Err(map_object_error(err, "delete", &path)),
        }
    }
}

fn verify_content_address(expected: &BlobHash, bytes: Bytes) -> Result<Bytes> {
    let actual = BlobHash::of(&bytes);
    if &actual != expected {
        return Err(Error::storage(
            StorageErrorKind::Corruption,
            format!("object_store blob {expected} content-address mismatch: read {actual}"),
        ));
    }
    Ok(bytes)
}

fn map_object_error(err: ::object_store::Error, operation: &str, path: &ObjectPath) -> Error {
    match err {
        ::object_store::Error::NotFound { .. } => Error::NotFound(format!("blob object {path}")),
        ::object_store::Error::AlreadyExists { .. } => {
            Error::AlreadyExists(format!("blob object {path}"))
        }
        ::object_store::Error::InvalidPath { .. } => {
            Error::InvalidInput(format!("invalid object_store path {path}: {err}"))
        }
        ::object_store::Error::Precondition { .. } | ::object_store::Error::NotModified { .. } => {
            Error::PreconditionFailed(format!("{operation} object_store path {path}: {err}"))
        }
        ::object_store::Error::NotSupported { .. }
        | ::object_store::Error::NotImplemented { .. } => Error::storage(
            StorageErrorKind::Unavailable,
            format!("{operation} object_store path {path}: {err}"),
        ),
        other => Error::storage(
            StorageErrorKind::Unavailable,
            format!("{operation} object_store path {path}: {other}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::fmt;
    use std::sync::atomic::{AtomicU64, Ordering};

    use ::object_store::memory::InMemory;
    use ::object_store::path::Path;
    use ::object_store::{
        CopyOptions, GetOptions, GetResult, GetResultPayload, ListResult, MultipartUpload,
        ObjectMeta, PutMultipartOptions, PutOptions, PutPayload, PutResult,
    };
    use futures::stream::BoxStream;

    use super::*;

    /// Test-only [`ObjectStore`] wrapper that counts the body bytes actually
    /// served by `get_opts`, so a `get_range` test can prove the underlying
    /// transfer stayed bounded to the requested window instead of the whole
    /// object.
    struct CountingObjectStore {
        inner: Arc<dyn ObjectStore>,
        bytes_served: Arc<AtomicU64>,
    }

    impl fmt::Debug for CountingObjectStore {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("CountingObjectStore").finish()
        }
    }

    impl fmt::Display for CountingObjectStore {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "CountingObjectStore({})", self.inner)
        }
    }

    #[async_trait]
    impl ObjectStore for CountingObjectStore {
        async fn put_opts(
            &self,
            location: &Path,
            payload: PutPayload,
            opts: PutOptions,
        ) -> ::object_store::Result<PutResult> {
            self.inner.put_opts(location, payload, opts).await
        }

        async fn put_multipart_opts(
            &self,
            location: &Path,
            opts: PutMultipartOptions,
        ) -> ::object_store::Result<Box<dyn MultipartUpload>> {
            self.inner.put_multipart_opts(location, opts).await
        }

        async fn get_opts(
            &self,
            location: &Path,
            options: GetOptions,
        ) -> ::object_store::Result<GetResult> {
            let result = self.inner.get_opts(location, options).await?;
            let meta = result.meta.clone();
            let range = result.range.clone();
            let attributes = result.attributes.clone();
            let bytes = result.bytes().await?;
            self.bytes_served
                .fetch_add(bytes.len() as u64, Ordering::SeqCst);
            Ok(GetResult {
                payload: GetResultPayload::Stream(Box::pin(futures::stream::once(async move {
                    Ok(bytes)
                }))),
                meta,
                range,
                attributes,
                extensions: Default::default(),
            })
        }

        fn delete_stream(
            &self,
            locations: BoxStream<'static, ::object_store::Result<Path>>,
        ) -> BoxStream<'static, ::object_store::Result<Path>> {
            self.inner.delete_stream(locations)
        }

        fn list(
            &self,
            prefix: Option<&Path>,
        ) -> BoxStream<'static, ::object_store::Result<ObjectMeta>> {
            self.inner.list(prefix)
        }

        async fn list_with_delimiter(
            &self,
            prefix: Option<&Path>,
        ) -> ::object_store::Result<ListResult> {
            self.inner.list_with_delimiter(prefix).await
        }

        async fn copy_opts(
            &self,
            from: &Path,
            to: &Path,
            options: CopyOptions,
        ) -> ::object_store::Result<()> {
            self.inner.copy_opts(from, to, options).await
        }
    }

    #[tokio::test]
    async fn object_store_blob_store_round_trips_through_memory_cloud() {
        let cloud: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let store = ObjectStoreBlobStore::new(cloud, "tenant-a");

        let hash = store.put(Bytes::from_static(b"cloud bytes")).await.unwrap();

        assert!(store.has(&hash).await.unwrap());
        assert_eq!(
            store.get(&hash).await.unwrap(),
            Bytes::from_static(b"cloud bytes")
        );
        assert_eq!(
            store.get_range(&hash, 6..11).await.unwrap(),
            Bytes::from_static(b"bytes")
        );
        store.release(&hash).await.unwrap();
        assert!(!store.has(&hash).await.unwrap());
    }

    #[tokio::test]
    async fn object_store_blob_store_rejects_corrupted_cloud_bytes() {
        let cloud: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let store = ObjectStoreBlobStore::new(cloud.clone(), "tenant-a");
        let hash = store.put(Bytes::from_static(b"original")).await.unwrap();
        let path = store.object_path(&hash);

        cloud
            .put(&path, Bytes::from_static(b"tampered").into())
            .await
            .unwrap();

        let err = store.get(&hash).await.unwrap_err();
        assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
    }

    #[tokio::test]
    async fn object_store_range_read_transfers_only_underlying_bytes_served() {
        let bytes_served = Arc::new(AtomicU64::new(0));
        let cloud: Arc<dyn ObjectStore> = Arc::new(CountingObjectStore {
            inner: Arc::new(InMemory::new()),
            bytes_served: bytes_served.clone(),
        });
        let store = ObjectStoreBlobStore::new(cloud, "tenant-a");

        let big: Vec<u8> = (0..1_048_576usize).map(|i| (i % 251) as u8).collect();
        let hash = store.put(Bytes::from(big.clone())).await.unwrap();
        // `put`/`has` go through the counting wrapper too; only the `get_range`
        // transfer below matters for this assertion.
        bytes_served.store(0, Ordering::SeqCst);

        let slice = store.get_range(&hash, 4096..8192).await.unwrap();

        assert_eq!(slice, Bytes::copy_from_slice(&big[4096..8192]));
        assert_eq!(
            bytes_served.load(Ordering::SeqCst),
            4096,
            "range read should transfer exactly the requested window, not the whole 1MiB blob"
        );
    }

    #[tokio::test]
    async fn object_store_get_range_rejects_end_past_blob_length() {
        let cloud: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let store = ObjectStoreBlobStore::new(cloud, "tenant-a");
        let hash = store.put(Bytes::from_static(b"cloud bytes")).await.unwrap();

        let err = store.get_range(&hash, 6..100).await.unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)));
    }

    #[tokio::test]
    async fn object_store_get_range_empty_range_returns_empty_bytes() {
        let cloud: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let store = ObjectStoreBlobStore::new(cloud, "tenant-a");
        let hash = store.put(Bytes::from_static(b"cloud bytes")).await.unwrap();

        let slice = store.get_range(&hash, 4..4).await.unwrap();
        assert_eq!(slice, Bytes::new());
    }

    #[tokio::test]
    #[allow(clippy::reversed_empty_ranges)]
    async fn object_store_get_range_rejects_start_after_end() {
        let cloud: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let store = ObjectStoreBlobStore::new(cloud, "tenant-a");
        let hash = store.put(Bytes::from_static(b"cloud bytes")).await.unwrap();

        let err = store.get_range(&hash, 8..4).await.unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)));
    }

    #[tokio::test]
    async fn object_store_get_range_missing_blob_stays_not_found() {
        let cloud: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let store = ObjectStoreBlobStore::new(cloud, "tenant-a");
        let hash = BlobHash::of(b"never stored");

        let err = store.get_range(&hash, 0..4).await.unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));
    }
}
