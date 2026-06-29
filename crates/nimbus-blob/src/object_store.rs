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
        let bytes = self.get(hash).await?;
        Ok(Box::new(std::io::Cursor::new(bytes)))
    }

    async fn get_range(&self, hash: &BlobHash, range: Range<u64>) -> Result<Bytes> {
        let bytes = self.get(hash).await?;
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
    use super::*;
    use ::object_store::memory::InMemory;

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
}
