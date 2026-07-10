use std::collections::HashMap;
use std::fs;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex, OnceLock, Weak};

use tokio::sync::Mutex as AsyncMutex;

use async_trait::async_trait;
use bytes::Bytes;
use nimbus_core::{Error, Result, StorageErrorKind};
use tokio::io::AsyncReadExt;

use crate::disk::{self, NoopSyncObserver, SyncObserver};
use crate::hash::BlobHash;
use crate::local::LocalPackStore;
use crate::pins::BlobPinRegistry;
use crate::root_guard::LocalPackStoreOptions;
use crate::store::{BlobStore, ByteStream};

use super::config::ErasureConfig;
use super::heal::HealSummary;
use super::manifest::{self, ErasureManifest, ShardRef};
use super::stripe;

#[derive(Clone)]
pub struct ErasureBlobStore {
    pub(super) config: ErasureConfig,
    pub(super) drive_roots: Vec<PathBuf>,
    pub(super) stores: Vec<LocalPackStore>,
    observer: Arc<dyn SyncObserver>,
    shared: Arc<LegSharedState>,
    read_only: bool,
    /// Serializes manifest mutations (put's read-modify-publish, release's
    /// multi-file removal) for the LEG, shared process-wide via a canonical
    /// drive-0 registry — same-process instances over the same roots alias
    /// one lock, mirroring LocalPackStore's shared-state semantics. Reads
    /// stay lock-free: a loaded manifest keeps serving (shards outlive
    /// release until Phase B GC), and quorum keeps partial states invisible.
    pub(super) mutation: Arc<AsyncMutex<()>>,
    /// Leg-wide pin registry: heal pins the blob it is repairing, and put
    /// pins its in-flight shards until the manifest publish resolves —
    /// both compose into every drive's shard GC so a sweep never reclaims
    /// bytes an active operation depends on.
    pub(super) leg_pins: BlobPinRegistry,
    /// Fail-stop poison (RFS3 fsyncgate idiom): set when a publish rollback
    /// could not be made durable while below quorum — the failed put's
    /// replicas may resurface after a crash, so no further operation on
    /// this leg is allowed to observe the ambiguous state. A restart
    /// re-resolves via the quorum rule (both post-crash states are safe:
    /// the unlinks held, or a complete durable blob reappeared whole).
    poisoned: Arc<std::sync::atomic::AtomicBool>,
    #[cfg(test)]
    force_nondurable_rollback: Arc<std::sync::atomic::AtomicBool>,
}

impl std::fmt::Debug for ErasureBlobStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ErasureBlobStore")
            .field("drive_roots", &self.drive_roots)
            .field("data_shards", &self.config.data_shards)
            .field("parity_shards", &self.config.parity_shards)
            .field("stripe_width", &self.config.stripe_width)
            .finish()
    }
}

impl ErasureBlobStore {
    pub fn open(config: ErasureConfig) -> Result<Self> {
        Self::open_inner(config, false)
    }

    /// Opens a lock-free, read-only inspection handle over every drive.
    ///
    /// The handle takes no per-drive flock and never mutates roots, so it can
    /// inspect an erasure leg while its writable owner is running. Mutations
    /// fail with [`StorageErrorKind::Busy`], matching
    /// [`LocalPackStore::open_read_only`].
    pub fn open_read_only(config: ErasureConfig) -> Result<Self> {
        Self::open_inner(config, true)
    }

    fn open_inner(config: ErasureConfig, read_only: bool) -> Result<Self> {
        let mut stores = Vec::with_capacity(config.drives.len());
        let mut drive_roots = Vec::with_capacity(config.drives.len());
        let observer: Arc<dyn SyncObserver> = Arc::new(NoopSyncObserver);

        for (index, root) in config.drives.iter().enumerate() {
            let store = if read_only {
                // Identity still validated read-only: inspecting a foreign
                // leg's roots (or the wrong drive order) fails closed
                // instead of serving that leg's blobs under our leg id.
                LocalPackStore::open_read_only_with_identity(
                    root,
                    Some(drive_identity(&config.leg_id, index)),
                )?
            } else {
                LocalPackStore::open_with_options(
                    root,
                    LocalPackStoreOptions {
                        identity: Some(drive_identity(&config.leg_id, index)),
                        ..LocalPackStoreOptions::default()
                    },
                )?
            };
            let canonical = root.canonicalize().map_err(|err| {
                Error::storage(
                    StorageErrorKind::Io,
                    format!("canonicalize erasure drive root {}: {err}", root.display()),
                )
            })?;
            if !read_only {
                let manifest_dir = manifest::manifest_dir(&canonical);
                disk::create_dir_all_durable(&manifest_dir, &*observer).map_err(|err| {
                    Error::storage(
                        StorageErrorKind::Io,
                        format!(
                            "create erasure manifest dir {}: {err}",
                            manifest_dir.display()
                        ),
                    )
                })?;
            }
            stores.push(store);
            drive_roots.push(canonical);
        }

        let shared = shared_state_for(&drive_roots[0]);
        Ok(Self {
            config,
            drive_roots,
            stores,
            observer,
            shared: Arc::clone(&shared),
            read_only,
            mutation: Arc::clone(&shared.mutation),
            leg_pins: shared.leg_pins.clone(),
            poisoned: Arc::clone(&shared.poisoned),
            #[cfg(test)]
            force_nondurable_rollback: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        })
    }

    pub(super) fn ensure_live(&self) -> Result<()> {
        if !self.read_only && self.poisoned.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(Error::storage(
                StorageErrorKind::Io,
                "erasure store poisoned: a publish rollback could not be made durable; \
                 restart to re-resolve manifest state via the quorum rule",
            ));
        }
        Ok(())
    }

    pub(super) fn ensure_writable(&self, operation: &str) -> Result<()> {
        if self.read_only {
            return Err(Error::storage(
                StorageErrorKind::Busy,
                format!("read-only inspection handle refuses {operation}"),
            ));
        }
        Ok(())
    }

    pub(super) fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub(super) async fn load_manifest(&self, hash: &BlobHash) -> Result<Option<ErasureManifest>> {
        let hash = *hash;
        let drive_roots = self.drive_roots.clone();
        // Visibility quorum: min(parity+1, data) replicas. The parity+1 arm
        // keeps interrupted-put/release minorities and forged single copies
        // invisible; the data-shards ceiling keeps the manifest plane AT
        // LEAST as loss-tolerant as the data plane — a committed publish
        // writes all k+m replicas, so quorum <= k means the blob stays
        // visible through the same m drive losses the shards tolerate
        // (relevant for high-parity layouts like k=2,m=4, where parity+1
        // would demand 5 of 6 manifests and lose visibility after only two
        // drive failures while the data is still recoverable).
        //
        // A CRASH after quorum but before full replication leaves the blob
        // visible although put never returned. That is deliberate and safe:
        // shards are durably written before any manifest, so visibility
        // implies the blob is complete, durable, and hash-verified —
        // "unacknowledged but durable", the same semantics the baseline
        // LocalPackStore has at its index-append commit point (a crash
        // after the append, before put returns, leaves the blob visible on
        // restart). No coordinator-free protocol can close the
        // ack-vs-durability gap; the guarantees that matter are that an
        // ERRORED put is invisible (publish rollback + quorum) and that
        // anything visible is completely readable.
        let quorum = self.visibility_quorum();
        blocking(move || manifest::load_newest(&hash, &drive_roots, quorum)).await
    }

    async fn manifest_for_read(&self, hash: &BlobHash) -> Result<ErasureManifest> {
        let manifest = self
            .load_manifest(hash)
            .await?
            .ok_or_else(|| Error::NotFound(format!("blob {hash}")))?;
        self.validate_manifest(hash, &manifest)?;
        Ok(manifest)
    }

    pub(super) fn validate_manifest(
        &self,
        hash: &BlobHash,
        manifest: &ErasureManifest,
    ) -> Result<()> {
        if manifest.blob_hash != *hash {
            return Err(corruption(format!(
                "manifest file for {hash} names blob {}",
                manifest.blob_hash
            )));
        }
        if manifest.data_shards != self.config.data_shards
            || manifest.parity_shards != self.config.parity_shards
            || manifest.stripe_width != self.config.stripe_width
        {
            return Err(corruption(
                "erasure manifest layout does not match this store",
            ));
        }
        if manifest.data_shards + manifest.parity_shards != self.stores.len() {
            return Err(corruption(
                "erasure manifest shard count does not match drive count",
            ));
        }
        if manifest.stripe_hashes.len() != manifest.stripes.len() {
            return Err(corruption(
                "erasure manifest stripe hash count does not match stripe count",
            ));
        }
        Ok(())
    }

    pub(super) async fn publish_manifest(&self, manifest: ErasureManifest) -> Result<()> {
        let drive_roots = self.drive_roots.clone();
        let observer = Arc::clone(&self.observer);
        let quorum = self.visibility_quorum();
        #[cfg(test)]
        let force_nondurable = self
            .force_nondurable_rollback
            .load(std::sync::atomic::Ordering::SeqCst);
        let outcome = blocking(move || {
            Ok(manifest::publish(
                &manifest,
                &drive_roots,
                &*observer,
                quorum,
            ))
        })
        .await?;
        match outcome {
            Ok(()) => Ok(()),
            Err(publish_err) => {
                #[cfg(test)]
                let rollback_durable = publish_err.rollback_durable && !force_nondurable;
                #[cfg(not(test))]
                let rollback_durable = publish_err.rollback_durable;
                if !rollback_durable {
                    self.poisoned
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                }
                Err(publish_err.error)
            }
        }
    }

    pub(super) fn poison_flag(&self) -> Arc<std::sync::atomic::AtomicBool> {
        Arc::clone(&self.poisoned)
    }

    pub(super) fn visibility_quorum(&self) -> usize {
        (self.config.parity_shards + 1).min(self.config.data_shards)
    }

    pub(super) fn set_last_heal(&self, summary: HealSummary) -> Result<()> {
        *self.shared.last_heal.lock().map_err(|_| {
            Error::storage(StorageErrorKind::Other, "erasure last-heal lock poisoned")
        })? = Some(summary);
        Ok(())
    }

    pub(super) fn last_heal(&self) -> Result<Option<HealSummary>> {
        Ok(*self.shared.last_heal.lock().map_err(|_| {
            Error::storage(StorageErrorKind::Other, "erasure last-heal lock poisoned")
        })?)
    }

    /// Reads, reassembles, and VERIFIES one stripe against its manifest
    /// payload hash. Every read path goes through this, so a manifest whose
    /// shard refs drifted (wrong-but-valid shard) fails closed here instead
    /// of serving wrong bytes — including range reads that never see the
    /// whole blob.
    async fn read_stripe_verified(
        &self,
        manifest: &ErasureManifest,
        stripe_index: usize,
    ) -> Result<Bytes> {
        let shards = self.read_stripe_data(manifest, stripe_index).await?;
        let true_len = Self::stripe_true_len(manifest, stripe_index)?;
        let stripe = stripe::reassemble_stripe(&shards, true_len)?;
        let expected = manifest.stripe_hashes.get(stripe_index).ok_or_else(|| {
            corruption(format!(
                "erasure manifest missing stripe hash for stripe {stripe_index}"
            ))
        })?;
        let actual = BlobHash::of(&stripe);
        if actual != *expected {
            return Err(corruption(format!(
                "erasure stripe {stripe_index} reassembled to {actual}, manifest expects {expected}"
            )));
        }
        Ok(stripe)
    }

    async fn read_stripe_data(
        &self,
        manifest: &ErasureManifest,
        stripe_index: usize,
    ) -> Result<Vec<Bytes>> {
        let mut present = Vec::new();
        let mut degraded = false;

        for shard_index in 0..manifest.data_shards {
            match self.read_shard(manifest, stripe_index, shard_index).await {
                Ok(bytes) => present.push((shard_index, bytes)),
                Err(_) => degraded = true,
            }
        }

        if !degraded {
            present.sort_by_key(|(index, _)| *index);
            return Ok(present.into_iter().map(|(_, bytes)| bytes).collect());
        }

        for shard_index in manifest.data_shards..manifest.data_shards + manifest.parity_shards {
            if let Ok(bytes) = self.read_shard(manifest, stripe_index, shard_index).await {
                present.push((shard_index, bytes));
            }
        }

        if present.len() < manifest.data_shards {
            return Err(corruption(format!(
                "erasure stripe {stripe_index} has {} healthy shards, need {}",
                present.len(),
                manifest.data_shards
            )));
        }
        stripe::decode_stripe(manifest.data_shards, manifest.parity_shards, &present)
    }

    pub(super) async fn read_shard(
        &self,
        manifest: &ErasureManifest,
        stripe_index: usize,
        shard_index: usize,
    ) -> Result<Bytes> {
        let shard = manifest.stripes[stripe_index]
            .iter()
            .find(|shard| shard.shard_index as usize == shard_index)
            .ok_or_else(|| {
                corruption(format!(
                    "erasure manifest missing shard {shard_index} for stripe {stripe_index}"
                ))
            })?;
        let drive = stripe::drive_for(shard_index, stripe_index, self.stores.len());
        self.stores[drive].get(&shard.shard_hash).await
    }

    pub(super) fn stripe_true_len(
        manifest: &ErasureManifest,
        stripe_index: usize,
    ) -> Result<usize> {
        let start = (stripe_index as u64)
            .checked_mul(manifest.stripe_width as u64)
            .ok_or_else(|| corruption("erasure stripe offset overflow"))?;
        let remaining = manifest.blob_len.checked_sub(start).ok_or_else(|| {
            corruption(format!(
                "erasure stripe {stripe_index} starts beyond blob length {}",
                manifest.blob_len
            ))
        })?;
        usize::try_from(remaining.min(manifest.stripe_width as u64))
            .map_err(|_| corruption("erasure stripe length overflows usize"))
    }

    #[cfg(test)]
    pub(crate) fn drive_store(&self, index: usize) -> LocalPackStore {
        self.stores[index].clone()
    }

    #[cfg(test)]
    pub(crate) fn drive_root(&self, index: usize) -> PathBuf {
        self.drive_roots[index].clone()
    }

    #[cfg(test)]
    pub(crate) fn drive_roots(&self) -> Vec<PathBuf> {
        self.drive_roots.clone()
    }

    #[cfg(test)]
    pub(crate) fn mutation_lock(&self) -> Arc<AsyncMutex<()>> {
        Arc::clone(&self.shared.mutation)
    }

    #[cfg(test)]
    pub(crate) fn heal_pin_registry(&self) -> BlobPinRegistry {
        self.leg_pins.clone()
    }

    #[cfg(test)]
    pub(crate) fn arm_nondurable_rollback(&self) {
        self.force_nondurable_rollback
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Sets the leg poison directly (tests): lets interleaving tests
    /// trip the fail-stop deterministically while holding the mutation
    /// lock, with no scheduler-order dependence.
    #[cfg(test)]
    pub(crate) fn poison_for_test(&self) {
        self.poisoned
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn is_poisoned(&self) -> bool {
        self.poisoned.load(std::sync::atomic::Ordering::SeqCst)
    }

    #[cfg(test)]
    pub(crate) async fn load_manifest_for_test(&self, hash: &BlobHash) -> Result<ErasureManifest> {
        self.manifest_for_read(hash).await
    }

    #[cfg(test)]
    pub(crate) async fn publish_manifest_for_test(&self, manifest: ErasureManifest) -> Result<()> {
        self.publish_manifest(manifest).await
    }
}

#[async_trait]
impl BlobStore for ErasureBlobStore {
    async fn put(&self, bytes: Bytes) -> Result<BlobHash> {
        self.ensure_writable("put")?;
        self.ensure_live()?;
        let hash = BlobHash::of(&bytes);
        // Mutations are serialized per leg: without this, an idempotent put
        // could observe an existing quorum, skip identical replicas, and
        // race a concurrent release into returning Ok with the manifests
        // gone (LocalPackStore gets the equivalent from its state lock).
        let _mutation = self.mutation.lock().await;
        // Recheck the fail-stop UNDER the lock: a mutation queued ahead of
        // us can poison the leg while we waited (heal and sweep_drive do
        // the same).
        self.ensure_live()?;
        if let Some(existing) = self.load_manifest(&hash).await? {
            // Idempotent path REPAIRS replication: a crash mid-publish or a
            // partially completed release can leave the manifest on a subset
            // of drives, and treating one surviving copy as success would
            // leave the commit point permanently under-replicated. Publish is
            // an atomic replace per drive, so re-publishing is safe.
            self.validate_manifest(&hash, &existing)?;
            self.publish_manifest(existing).await?;
            return Ok(hash);
        }

        let mut manifest = ErasureManifest {
            generation: 1,
            blob_hash: hash,
            blob_len: bytes.len() as u64,
            data_shards: self.config.data_shards,
            parity_shards: self.config.parity_shards,
            stripe_width: self.config.stripe_width,
            stripe_hashes: Vec::new(),
            stripes: Vec::new(),
        };

        // Pin every shard hash BEFORE its bytes land: pre-publish shards are
        // unrooted (ManifestShardRoots only sees visible manifests), so a
        // concurrent sweep with a short grace could otherwise reclaim live
        // shards mid-put and the acknowledged manifest would point at
        // missing bytes. The RAII pins drop after publish resolves — on
        // failure the orphans become sweepable again, which is correct.
        let mut inflight_pins = Vec::new();
        for (stripe_index, chunk) in bytes.chunks(self.config.stripe_width).enumerate() {
            manifest.stripe_hashes.push(BlobHash::of(chunk));
            let shards =
                stripe::encode_stripe(chunk, self.config.data_shards, self.config.parity_shards)?;
            let mut refs = Vec::with_capacity(shards.len());
            for (shard_index, shard_bytes) in shards.into_iter().enumerate() {
                let drive =
                    stripe::drive_for(shard_index, stripe_index, self.config.total_shards());
                let shard_hash = BlobHash::of(&shard_bytes);
                inflight_pins.push(self.leg_pins.pin(shard_hash));
                let written = self.stores[drive].put(shard_bytes).await?;
                debug_assert_eq!(written, shard_hash, "pack stores hash their input");
                refs.push(ShardRef {
                    shard_index: shard_index as u16,
                    shard_hash,
                });
            }
            refs.sort_by_key(|shard| shard.shard_index);
            manifest.stripes.push(refs);
        }

        // Phase B's root-based GC owns reclaiming shards left behind if this
        // put fails after shard writes but before or during manifest publish.
        self.publish_manifest(manifest).await?;
        Ok(hash)
    }

    async fn put_stream(&self, mut src: ByteStream) -> Result<BlobHash> {
        self.ensure_writable("put_stream")?;
        let mut buf = Vec::new();
        src.read_to_end(&mut buf).await.map_err(|err| {
            Error::storage(StorageErrorKind::Io, format!("read blob stream: {err}"))
        })?;
        self.put(Bytes::from(buf)).await
    }

    async fn get(&self, hash: &BlobHash) -> Result<Bytes> {
        self.ensure_live()?;
        let manifest = self.manifest_for_read(hash).await?;
        // Grow from verified stripe bytes only — blob_len is manifest data
        // and a checksum-valid forgery with a huge claim must fail closed on
        // its (missing) shards, not reserve memory upfront.
        let mut out = Vec::new();
        for stripe_index in 0..manifest.stripes.len() {
            let stripe = self.read_stripe_verified(&manifest, stripe_index).await?;
            out.extend_from_slice(&stripe);
        }
        out.truncate(manifest.blob_len as usize);
        let bytes = Bytes::from(out);
        let actual = BlobHash::of(&bytes);
        if actual != *hash {
            return Err(corruption(format!(
                "erasure blob {hash} reassembled to content address {actual}"
            )));
        }
        Ok(bytes)
    }

    async fn get_stream(&self, hash: &BlobHash) -> Result<ByteStream> {
        Ok(Box::new(std::io::Cursor::new(self.get(hash).await?)))
    }

    async fn get_range(&self, hash: &BlobHash, range: Range<u64>) -> Result<Bytes> {
        self.ensure_live()?;
        let manifest = self.manifest_for_read(hash).await?;
        if range.start > range.end {
            return Err(Error::InvalidInput(format!(
                "range {}..{} out of bounds: start after end",
                range.start, range.end
            )));
        }
        if range.end > manifest.blob_len {
            return Err(Error::InvalidInput(format!(
                "range {}..{} out of bounds for blob of {} bytes",
                range.start, range.end, manifest.blob_len
            )));
        }
        if range.start == range.end {
            return Ok(Bytes::new());
        }

        let first_stripe = usize::try_from(range.start / manifest.stripe_width as u64)
            .map_err(|_| Error::InvalidInput("range start overflows usize".to_string()))?;
        let last_stripe = usize::try_from((range.end - 1) / manifest.stripe_width as u64)
            .map_err(|_| Error::InvalidInput("range end overflows usize".to_string()))?;
        // Grow from verified stripe bytes only (see get: manifest-supplied
        // lengths must not drive upfront reservations).
        let mut out = Vec::new();
        for stripe_index in first_stripe..=last_stripe {
            let stripe = self.read_stripe_verified(&manifest, stripe_index).await?;
            let stripe_start = stripe_index as u64 * manifest.stripe_width as u64;
            let copy_start = range.start.max(stripe_start) - stripe_start;
            let copy_end = range.end.min(stripe_start + stripe.len() as u64) - stripe_start;
            out.extend_from_slice(&stripe[copy_start as usize..copy_end as usize]);
        }
        // Range reads are bounded to covering stripes; integrity comes from
        // per-shard pack verification PLUS the per-stripe payload hash
        // checked in read_stripe_verified. This intentionally does NOT bind
        // the window to the whole-blob content address (that would require
        // reading every stripe or a merkle-proof scheme): it matches — and
        // exceeds — the repo-wide BlobStore range-read contract, where
        // LocalPackStore serves the requested window without a whole-record
        // re-hash and at-rest integrity is owned by verified-at-write
        // structures, the scrubber, and AEAD authentication at the
        // encryption layer in the shipped composition.
        Ok(Bytes::from(out))
    }

    async fn has(&self, hash: &BlobHash) -> Result<bool> {
        self.ensure_live()?;
        Ok(self.load_manifest(hash).await?.is_some())
    }

    async fn release(&self, hash: &BlobHash) -> Result<()> {
        self.ensure_writable("release")?;
        self.ensure_live()?;
        let _mutation = self.mutation.lock().await;
        // Same post-lock recheck as put/heal/sweep_drive.
        self.ensure_live()?;
        let hash = *hash;
        let drive_roots = self.drive_roots.clone();
        let observer = Arc::clone(&self.observer);
        blocking(move || {
            for root in &drive_roots {
                let path = manifest::manifest_path(root, &hash);
                match fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(err) => {
                        return Err(Error::storage(
                            StorageErrorKind::Io,
                            format!("remove erasure manifest {}: {err}", path.display()),
                        ));
                    }
                }
                let dir = manifest::manifest_dir(root);
                disk::fsync_dir(&dir, &*observer).map_err(|err| {
                    Error::storage(
                        StorageErrorKind::Io,
                        format!("sync erasure manifest dir {}: {err}", dir.display()),
                    )
                })?;
            }
            // Phase B's manifest-root GC owns shard reclamation. Releasing
            // shard blobs here would be unsafe because shard hashes may be
            // shared across blobs and crash windows need root reconstruction.
            Ok(())
        })
        .await
    }
}

async fn blocking<T>(op: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(op)
        .await
        .map_err(|err| Error::storage(StorageErrorKind::Other, format!("erasure task: {err}")))?
}

struct LegSharedState {
    mutation: Arc<AsyncMutex<()>>,
    leg_pins: BlobPinRegistry,
    last_heal: Arc<StdMutex<Option<HealSummary>>>,
    /// Poison is LEG state, not handle state: a nondurable rollback makes
    /// the on-disk manifest view ambiguous for every same-process handle
    /// over these roots, so all of them must fail-stop together.
    poisoned: Arc<std::sync::atomic::AtomicBool>,
}

/// Process-wide registry of per-leg state, keyed by the canonical drive-0 root
/// (unique per leg: identity binding prevents two legs from sharing any root).
/// Weak entries let dropped stores free their slot.
fn shared_state_for(drive0: &PathBuf) -> Arc<LegSharedState> {
    static REGISTRY: OnceLock<StdMutex<HashMap<PathBuf, Weak<LegSharedState>>>> = OnceLock::new();
    let registry = REGISTRY.get_or_init(|| StdMutex::new(HashMap::new()));
    let mut map = registry.lock().expect("erasure mutation registry poisoned");
    map.retain(|_, weak| weak.strong_count() > 0);
    if let Some(existing) = map.get(drive0).and_then(Weak::upgrade) {
        return existing;
    }
    let fresh = Arc::new(LegSharedState {
        mutation: Arc::new(AsyncMutex::new(())),
        leg_pins: BlobPinRegistry::new(),
        last_heal: Arc::new(StdMutex::new(None)),
        poisoned: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    });
    map.insert(drive0.clone(), Arc::downgrade(&fresh));
    fresh
}

/// Binds a drive root to BOTH the leg instance and its drive index: a root
/// provisioned for another leg (even at the same index) or for another index
/// of this leg refuses to open (RFS2 fail-closed identity semantics).
fn drive_identity(leg_id: &str, index: usize) -> [u8; crate::BLAKE3_HASH_LEN] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"nimbus-erasure-leg");
    hasher.update(&(leg_id.len() as u64).to_le_bytes());
    hasher.update(leg_id.as_bytes());
    hasher.update(&(index as u64).to_le_bytes());
    *hasher.finalize().as_bytes()
}

fn corruption(message: impl Into<String>) -> Error {
    Error::storage(StorageErrorKind::Corruption, message)
}
