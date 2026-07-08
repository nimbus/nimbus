//! Blob lifecycle and garbage-collection seam.
//!
//! `BlobStore::release` only drops one tenant-local claim. `BlobGc` is the
//! explicit lifecycle protocol that compares local byte-plane entries against
//! metadata/snapshot roots, applies a grace window for in-flight writes, and
//! then compacts packs.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use nimbus_core::{Clock, Result, SystemClock};

use crate::pins::BlobPinRegistry;
use crate::store::BlobStore;
use crate::{BlobHash, CompactionStats, LocalPackStore};

/// Provides the live blob roots for one tenant.
#[async_trait]
pub trait BlobGcRoots: Send + Sync {
    async fn live_blob_hashes(&self) -> Result<BTreeSet<BlobHash>>;
}

/// Static root provider used by tests and by early single-process callers.
#[derive(Clone, Debug, Default)]
pub struct StaticBlobRoots {
    live: BTreeSet<BlobHash>,
}

impl StaticBlobRoots {
    pub fn new(live: impl IntoIterator<Item = BlobHash>) -> Self {
        Self {
            live: live.into_iter().collect(),
        }
    }
}

#[async_trait]
impl BlobGcRoots for StaticBlobRoots {
    async fn live_blob_hashes(&self) -> Result<BTreeSet<BlobHash>> {
        Ok(self.live.clone())
    }
}

/// Result of a mark-and-sweep pass.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlobGcReport {
    pub referenced_retained: usize,
    pub grace_retained: usize,
    /// Retained by an active [`BlobPinRegistry`] write-intent hold (GR9),
    /// past grace and unrooted. The fallback the pin arm covers for is
    /// exactly the put→pin ordering window; see `pins` module docs.
    pub intent_retained: usize,
    pub swept: usize,
    pub compaction: CompactionStats,
}

/// Mark-and-sweep lifecycle coordinator for local packs.
pub struct BlobGc<R> {
    store: LocalPackStore,
    roots: R,
    grace_window: Duration,
    clock: Arc<dyn Clock>,
    pins: BlobPinRegistry,
}

impl<R> BlobGc<R>
where
    R: BlobGcRoots,
{
    pub fn new(store: LocalPackStore, roots: R, grace_window: Duration) -> Self {
        Self {
            store,
            roots,
            grace_window,
            clock: Arc::new(SystemClock),
            pins: BlobPinRegistry::new(),
        }
    }

    /// Overrides the sweep-cutoff clock (e.g. `ManualClock` for
    /// deterministic tests). Defaults to the real system clock.
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Shares a [`BlobPinRegistry`] with this GC so `sweep` honors holds
    /// taken by other components (e.g. a sandbox volume snapshotting onto
    /// the same byte plane). Defaults to a private, always-empty registry,
    /// so `sweep`'s pin check costs one cheap map lookup per entry and
    /// changes nothing when no caller ever pins through this instance.
    pub fn with_pins(mut self, pins: BlobPinRegistry) -> Self {
        self.pins = pins;
        self
    }

    /// Runs mark-and-sweep over local blobs and then compacts dead pack bytes.
    pub async fn sweep(&self) -> Result<BlobGcReport> {
        let roots = self.roots.live_blob_hashes().await?;
        let cutoff = self
            .clock
            .now_millis()
            .saturating_sub(self.grace_window.as_millis() as u64);
        let mut report = BlobGcReport::default();

        for entry in self.store.live_entries()? {
            if roots.contains(&entry.hash) {
                report.referenced_retained += 1;
                continue;
            }
            if self.pins.is_held(&entry.hash) {
                report.intent_retained += 1;
                continue;
            }
            if entry.written_at_millis > cutoff {
                report.grace_retained += 1;
                continue;
            }
            self.store.release(&entry.hash).await?;
            report.swept += 1;
        }

        report.compaction = self.store.compact().await?;
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use nimbus_core::{ManualClock, Timestamp};

    use super::*;
    use crate::BlobStore;

    fn open_temp(target: u64) -> (tempfile::TempDir, LocalPackStore) {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let store =
            LocalPackStore::open_with_pack_target(dir.path(), target).expect("store should open");
        (dir, store)
    }

    #[tokio::test]
    async fn referenced_blob_is_never_swept() {
        let (_dir, store) = open_temp(128);
        let hash = store.put(Bytes::from_static(b"referenced")).await.unwrap();
        let gc = BlobGc::new(store.clone(), StaticBlobRoots::new([hash]), Duration::ZERO);

        let report = gc.sweep().await.unwrap();

        assert_eq!(report.referenced_retained, 1);
        assert_eq!(report.swept, 0);
        assert!(store.has(&hash).await.unwrap());
    }

    #[tokio::test]
    async fn partial_upload_without_manifest_is_reclaimed_after_grace_window() {
        let (_dir, store) = open_temp(128);
        let hash = store
            .put(Bytes::from_static(b"partial upload"))
            .await
            .unwrap();
        let gc = BlobGc::new(store.clone(), StaticBlobRoots::default(), Duration::ZERO);

        let report = gc.sweep().await.unwrap();

        assert_eq!(report.swept, 1);
        assert!(!store.has(&hash).await.unwrap());
    }

    #[tokio::test]
    async fn grace_window_retains_recent_unreferenced_blob() {
        let (_dir, store) = open_temp(128);
        let hash = store.put(Bytes::from_static(b"recent")).await.unwrap();
        let gc = BlobGc::new(
            store.clone(),
            StaticBlobRoots::default(),
            Duration::from_secs(60),
        );

        let report = gc.sweep().await.unwrap();

        assert_eq!(report.grace_retained, 1);
        assert_eq!(report.swept, 0);
        assert!(store.has(&hash).await.unwrap());
    }

    #[tokio::test]
    async fn sweep_runs_pack_compaction_after_releasing_dead_blobs() {
        let (dir, store) = open_temp(96);
        let keep = store.put(Bytes::from_static(b"keep this")).await.unwrap();
        let drop_hash = store.put(Bytes::from_static(b"drop this")).await.unwrap();
        let gc = BlobGc::new(store.clone(), StaticBlobRoots::new([keep]), Duration::ZERO);

        let report = gc.sweep().await.unwrap();

        assert_eq!(report.swept, 1);
        assert!(store.has(&keep).await.unwrap());
        assert!(!store.has(&drop_hash).await.unwrap());
        assert!(
            report.compaction.packs_removed >= 1,
            "sweep should compact released pack bytes"
        );
        let pack_count = std::fs::read_dir(dir.path().join("packs"))
            .unwrap()
            .filter_map(std::result::Result::ok)
            .count();
        assert_eq!(pack_count, 1);
    }

    #[tokio::test]
    async fn pinned_blob_past_grace_is_retained_as_intent_not_swept() {
        let (_dir, store) = open_temp(128);
        let clock = Arc::new(ManualClock::new(Timestamp(0)));
        let store = store.with_clock(clock.clone());
        let hash = store.put(Bytes::from_static(b"pinned")).await.unwrap();

        let pins = BlobPinRegistry::new();
        let pin = pins.pin(hash);
        clock.advance(Duration::from_secs(120));

        let gc = BlobGc::new(
            store.clone(),
            StaticBlobRoots::default(),
            Duration::from_secs(60),
        )
        .with_clock(clock.clone())
        .with_pins(pins);

        let report = gc.sweep().await.unwrap();

        assert_eq!(report.intent_retained, 1);
        assert_eq!(report.grace_retained, 0);
        assert_eq!(report.swept, 0);
        assert!(store.has(&hash).await.unwrap());

        drop(pin);
    }

    #[tokio::test]
    async fn dropped_pin_past_grace_and_unrooted_is_swept() {
        let (_dir, store) = open_temp(128);
        let clock = Arc::new(ManualClock::new(Timestamp(0)));
        let store = store.with_clock(clock.clone());
        let hash = store.put(Bytes::from_static(b"was-pinned")).await.unwrap();

        let pins = BlobPinRegistry::new();
        drop(pins.pin(hash));
        clock.advance(Duration::from_secs(120));

        let gc = BlobGc::new(
            store.clone(),
            StaticBlobRoots::default(),
            Duration::from_secs(60),
        )
        .with_clock(clock)
        .with_pins(pins);

        let report = gc.sweep().await.unwrap();

        assert_eq!(report.intent_retained, 0);
        assert_eq!(report.swept, 1);
        assert!(!store.has(&hash).await.unwrap());
    }

    #[tokio::test]
    async fn refcounted_pin_keeps_blob_retained_until_last_guard_drops() {
        let (_dir, store) = open_temp(128);
        let clock = Arc::new(ManualClock::new(Timestamp(0)));
        let store = store.with_clock(clock.clone());
        let hash = store
            .put(Bytes::from_static(b"double-pinned"))
            .await
            .unwrap();

        let pins = BlobPinRegistry::new();
        let first = pins.pin(hash);
        let second = pins.pin(hash);
        clock.advance(Duration::from_secs(120));

        let gc = BlobGc::new(
            store.clone(),
            StaticBlobRoots::default(),
            Duration::from_secs(60),
        )
        .with_clock(clock)
        .with_pins(pins);

        drop(first);
        let report = gc.sweep().await.unwrap();
        assert_eq!(
            report.intent_retained, 1,
            "second guard should still hold the pin"
        );
        assert!(store.has(&hash).await.unwrap());

        drop(second);
        let report = gc.sweep().await.unwrap();
        assert_eq!(report.swept, 1);
        assert!(!store.has(&hash).await.unwrap());
    }
}
