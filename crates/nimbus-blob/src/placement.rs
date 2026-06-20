//! [`PlacementBlobStore`] — composite that routes blob ops across legs.
//!
//! The spec §16b composite holds the primary `local` leg plus a
//! [`PlacementMode`] describing the secondary leg and routing policy.
//! Encryption is composed *below* placement (see [`crate::EncryptedBlobStore`])
//! so every leg stores identical ciphertext under the same content address.

use std::ops::Range;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use nimbus_core::Result;
use tokio::io::AsyncReadExt;

use crate::hash::BlobHash;
use crate::store::{BlobStore, ByteStream};

/// Where a blob's secondary copy lives and how reads/writes route.
pub enum PlacementMode {
    /// Single local leg; no secondary copy.
    LocalOnly,
    /// Write through to `mirror`; `require_ack` gates whether the mirror write
    /// must succeed for the overall write to succeed.
    Mirror {
        /// The mirror leg.
        mirror: Arc<dyn BlobStore>,
        /// If true, a mirror-write failure fails the whole `put`.
        require_ack: bool,
    },
    /// Local is a hot cache over a `cold` backing tier.
    Tier {
        /// The cold backing tier.
        cold: Arc<dyn BlobStore>,
    },
    /// `cloud` is authoritative; local is a cache. Reads fall through to cloud
    /// on a local miss.
    CloudPrimary {
        /// The authoritative cloud leg.
        cloud: Arc<dyn BlobStore>,
    },
}

/// Routes blob operations across a local leg and a mode-defined secondary leg.
///
/// Per-tenant: every leg is itself a per-tenant [`BlobStore`] (the placement
/// store *is* the tenant — see the tenancy note on [`BlobStore`]), so no method
/// carries a `tenant` argument.
pub struct PlacementBlobStore {
    local: Arc<dyn BlobStore>,
    mode: PlacementMode,
}

impl PlacementBlobStore {
    /// Builds a placement store over `local` with routing policy `mode`.
    pub fn new(local: Arc<dyn BlobStore>, mode: PlacementMode) -> Self {
        Self { local, mode }
    }

    /// Convenience constructor for a single-leg ([`PlacementMode::LocalOnly`])
    /// placement.
    pub fn local_only(local: Arc<dyn BlobStore>) -> Self {
        Self::new(local, PlacementMode::LocalOnly)
    }
}

#[async_trait]
impl BlobStore for PlacementBlobStore {
    async fn put(&self, bytes: Bytes) -> Result<BlobHash> {
        match &self.mode {
            PlacementMode::LocalOnly => self.local.put(bytes).await,
            PlacementMode::Mirror {
                mirror,
                require_ack,
            } => {
                let hash = self.local.put(bytes.clone()).await?;
                // Write through to the mirror. With require_ack, a mirror
                // failure fails the whole write; otherwise it is best-effort.
                let mirrored = mirror.put(bytes).await;
                if *require_ack {
                    let mirror_hash = mirrored?;
                    debug_assert_eq!(mirror_hash, hash, "legs are content-addressed identically");
                }
                Ok(hash)
            }
            PlacementMode::Tier { cold } => {
                // Persist to the cold tier before warming the local cache so a
                // failed write never leaves the cache ahead of the durable leg.
                let durable_hash = cold.put(bytes.clone()).await?;
                let local_hash = self.local.put(bytes).await?;
                debug_assert_eq!(
                    local_hash, durable_hash,
                    "legs are content-addressed identically"
                );
                Ok(durable_hash)
            }
            PlacementMode::CloudPrimary { cloud } => {
                // Cloud is authoritative; write it before warming the local
                // cache so local never gets ahead of the source of truth.
                let durable_hash = cloud.put(bytes.clone()).await?;
                let local_hash = self.local.put(bytes).await?;
                debug_assert_eq!(
                    local_hash, durable_hash,
                    "legs are content-addressed identically"
                );
                Ok(durable_hash)
            }
        }
    }

    async fn put_stream(&self, mut src: ByteStream) -> Result<BlobHash> {
        // Buffer to fan out the same bytes to every leg.
        let mut buf = Vec::new();
        src.read_to_end(&mut buf).await.map_err(|err| {
            nimbus_core::Error::storage(
                nimbus_core::StorageErrorKind::Io,
                format!("read blob stream: {err}"),
            )
        })?;
        self.put(Bytes::from(buf)).await
    }

    async fn get(&self, hash: &BlobHash) -> Result<Bytes> {
        match self.local.get(hash).await {
            Ok(bytes) => Ok(bytes),
            Err(local_err) => match &self.mode {
                // CloudPrimary: on a local miss, fall through to the cloud leg.
                PlacementMode::CloudPrimary { cloud } => cloud.get(hash).await,
                // Tier: on a local (cache) miss, read from the cold tier.
                PlacementMode::Tier { cold } => cold.get(hash).await,
                _ => Err(local_err),
            },
        }
    }

    async fn get_stream(&self, hash: &BlobHash) -> Result<ByteStream> {
        match self.local.get_stream(hash).await {
            Ok(stream) => Ok(stream),
            Err(local_err) => match &self.mode {
                PlacementMode::CloudPrimary { cloud } => cloud.get_stream(hash).await,
                PlacementMode::Tier { cold } => cold.get_stream(hash).await,
                _ => Err(local_err),
            },
        }
    }

    async fn get_range(&self, hash: &BlobHash, range: Range<u64>) -> Result<Bytes> {
        match self.local.get_range(hash, range.clone()).await {
            Ok(bytes) => Ok(bytes),
            Err(local_err) => match &self.mode {
                PlacementMode::CloudPrimary { cloud } => cloud.get_range(hash, range).await,
                PlacementMode::Tier { cold } => cold.get_range(hash, range).await,
                _ => Err(local_err),
            },
        }
    }

    async fn has(&self, hash: &BlobHash) -> Result<bool> {
        if self.local.has(hash).await? {
            return Ok(true);
        }
        match &self.mode {
            PlacementMode::CloudPrimary { cloud } => cloud.has(hash).await,
            PlacementMode::Tier { cold } => cold.has(hash).await,
            _ => Ok(false),
        }
    }

    async fn release(&self, hash: &BlobHash) -> Result<()> {
        // Release the local cache first and durable legs last. A crash between
        // legs can leave an extra durable copy, but it will not leave the cache
        // pointing at a blob the durable leg has already dropped.
        self.local.release(hash).await?;
        match &self.mode {
            PlacementMode::LocalOnly => {}
            PlacementMode::Mirror { mirror, .. } => {
                mirror.release(hash).await?;
            }
            PlacementMode::Tier { cold } => {
                cold.release(hash).await?;
            }
            PlacementMode::CloudPrimary { cloud } => {
                cloud.release(hash).await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::LocalPackStore;
    use nimbus_core::{Error, StorageErrorKind};

    struct FailsPut;

    #[async_trait]
    impl BlobStore for FailsPut {
        async fn put(&self, _: Bytes) -> Result<BlobHash> {
            Err(Error::storage(StorageErrorKind::Unavailable, "leg down"))
        }

        async fn put_stream(&self, _: ByteStream) -> Result<BlobHash> {
            Err(Error::storage(StorageErrorKind::Unavailable, "leg down"))
        }

        async fn get(&self, hash: &BlobHash) -> Result<Bytes> {
            Err(Error::NotFound(format!("blob {hash}")))
        }

        async fn get_stream(&self, hash: &BlobHash) -> Result<ByteStream> {
            Err(Error::NotFound(format!("blob {hash}")))
        }

        async fn get_range(&self, hash: &BlobHash, _: Range<u64>) -> Result<Bytes> {
            Err(Error::NotFound(format!("blob {hash}")))
        }

        async fn has(&self, _: &BlobHash) -> Result<bool> {
            Ok(false)
        }

        async fn release(&self, _: &BlobHash) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn mirror_writes_both_legs() {
        let local: Arc<dyn BlobStore> = Arc::new(LocalPackStore::new());
        let mirror_inner = Arc::new(LocalPackStore::new());
        let mirror: Arc<dyn BlobStore> = mirror_inner.clone();
        let store = PlacementBlobStore::new(
            local.clone(),
            PlacementMode::Mirror {
                mirror,
                require_ack: true,
            },
        );
        let hash = store.put(Bytes::from_static(b"mirrored")).await.unwrap();

        assert!(local.has(&hash).await.unwrap(), "primary leg has blob");
        assert!(
            mirror_inner.has(&hash).await.unwrap(),
            "mirror leg has blob"
        );
    }

    #[tokio::test]
    async fn cloud_primary_get_falls_through_on_local_miss() {
        // Seed the cloud leg directly; the local cache is empty.
        let cloud_inner = Arc::new(LocalPackStore::new());
        let hash = cloud_inner
            .put(Bytes::from_static(b"in the cloud"))
            .await
            .unwrap();

        let local: Arc<dyn BlobStore> = Arc::new(LocalPackStore::new());
        let cloud: Arc<dyn BlobStore> = cloud_inner.clone();
        let store = PlacementBlobStore::new(local.clone(), PlacementMode::CloudPrimary { cloud });

        assert!(!local.has(&hash).await.unwrap(), "local starts empty");
        let got = store.get(&hash).await.unwrap();
        assert_eq!(got, Bytes::from_static(b"in the cloud"));
    }

    #[tokio::test]
    async fn local_only_does_not_fall_through() {
        let store = PlacementBlobStore::local_only(Arc::new(LocalPackStore::new()));
        let err = store.get(&BlobHash::of(b"absent")).await.unwrap_err();
        assert!(matches!(err, nimbus_core::Error::NotFound(_)));
    }

    #[tokio::test]
    async fn tier_write_lands_in_cold_and_reads_back_on_cache_miss() {
        let local: Arc<dyn BlobStore> = Arc::new(LocalPackStore::new());
        let cold_inner = Arc::new(LocalPackStore::new());
        let cold: Arc<dyn BlobStore> = cold_inner.clone();
        let store = PlacementBlobStore::new(local.clone(), PlacementMode::Tier { cold });
        let hash = store.put(Bytes::from_static(b"cold data")).await.unwrap();

        assert!(cold_inner.has(&hash).await.unwrap(), "cold tier persisted");
        // Evict the cache copy, then read should fall through to cold.
        local.release(&hash).await.unwrap();
        assert!(!local.has(&hash).await.unwrap());
        let got = store.get(&hash).await.unwrap();
        assert_eq!(got, Bytes::from_static(b"cold data"));
    }

    #[tokio::test]
    async fn tier_write_failure_does_not_warm_local_cache() {
        let local_inner = Arc::new(LocalPackStore::new());
        let local: Arc<dyn BlobStore> = local_inner.clone();
        let cold: Arc<dyn BlobStore> = Arc::new(FailsPut);
        let store = PlacementBlobStore::new(local, PlacementMode::Tier { cold });

        let err = store
            .put(Bytes::from_static(b"cold data"))
            .await
            .unwrap_err();

        assert_eq!(err.storage_kind(), Some(StorageErrorKind::Unavailable));
        assert!(
            local_inner.is_empty(),
            "failed durable write must not populate local cache"
        );
    }

    #[tokio::test]
    async fn cloud_primary_write_failure_does_not_warm_local_cache() {
        let local_inner = Arc::new(LocalPackStore::new());
        let local: Arc<dyn BlobStore> = local_inner.clone();
        let cloud: Arc<dyn BlobStore> = Arc::new(FailsPut);
        let store = PlacementBlobStore::new(local, PlacementMode::CloudPrimary { cloud });

        let err = store
            .put(Bytes::from_static(b"cloud data"))
            .await
            .unwrap_err();

        assert_eq!(err.storage_kind(), Some(StorageErrorKind::Unavailable));
        assert!(
            local_inner.is_empty(),
            "failed cloud write must not populate local cache"
        );
    }

    #[tokio::test]
    async fn mirror_without_ack_tolerates_unreachable_mirror() {
        // A mirror that always errors should not fail a best-effort write.
        let local: Arc<dyn BlobStore> = Arc::new(LocalPackStore::new());
        let store = PlacementBlobStore::new(
            local.clone(),
            PlacementMode::Mirror {
                mirror: Arc::new(FailsPut),
                require_ack: false,
            },
        );
        // Best-effort mirror: the put still succeeds on the local leg.
        let hash = store.put(Bytes::from_static(b"best effort")).await.unwrap();
        assert!(local.has(&hash).await.unwrap());
    }
}
