//! Write-intent pin registry — `BlobGc`'s third retention arm (GR9).
//!
//! `BlobGc::sweep` already retains blobs referenced by the root set (mark
//! phase) or written inside the grace window. Neither arm covers a write
//! that has completed (so it is past grace) but whose caller has not yet
//! registered the hash as a root — e.g. a sandbox volume snapshot: the
//! archive blob is `put` before its `SnapshotId` is durably recorded as a
//! root anywhere. [`BlobPinRegistry`] closes that hole: a caller takes an
//! RAII [`BlobPin`] for a hash it is about to depend on, and `sweep` treats
//! a held hash as retained regardless of grace.
//!
//! ## Ordering contract
//!
//! `pin()` is called with an already-known [`BlobHash`], i.e. *after*
//! `BlobStore::put` returns. There is necessarily a short window between a
//! put completing (which stamps `written_at_millis` and starts the grace
//! clock) and the caller acquiring the pin. This window is covered by the
//! existing grace arm, not by the pin arm — the two are complementary, not
//! redundant: grace covers "just written, root not registered yet" for
//! every write; the pin arm covers "written and past grace, root still not
//! registered" for the specific case where a caller wants a hash to survive
//! past grace before it becomes a durable root.
//!
//! A simpler `pin_pending()` shape (reserve a token before the hash is
//! known, so the write itself is covered by an intent hold rather than by
//! grace) was considered and rejected: it would need `sweep` to reason
//! about open-ended "a write is in flight" tokens with no associated hash,
//! which is a bigger surface for a window that is already microseconds
//! wide and already covered by grace. No concrete hole was found in the
//! simpler shape, so this module ships pin-after-put only.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::hash::BlobHash;

/// Ref-counted write-intent holds, keyed by [`BlobHash`].
///
/// Cheap to share: the mutable state lives behind one internal `Arc`, so
/// cloning a `BlobPinRegistry` shares the same holds (the registry is
/// itself the shared handle — callers do not need to wrap it in an
/// `Arc` themselves).
#[derive(Clone, Default)]
pub struct BlobPinRegistry {
    holds: Arc<Mutex<HashMap<BlobHash, usize>>>,
}

impl BlobPinRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Acquires a hold on `hash`. Sweep treats a held hash as retained
    /// until every [`BlobPin`] for it has dropped.
    pub fn pin(&self, hash: BlobHash) -> BlobPin {
        let mut holds = self.lock();
        *holds.entry(hash).or_insert(0) += 1;
        drop(holds);
        BlobPin {
            holds: Arc::clone(&self.holds),
            hash,
        }
    }

    /// Whether `hash` currently has at least one live pin.
    pub fn is_held(&self, hash: &BlobHash) -> bool {
        self.lock().contains_key(hash)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<BlobHash, usize>> {
        self.holds
            .lock()
            .expect("blob pin registry lock should not be poisoned")
    }
}

/// RAII write-intent hold on one [`BlobHash`]. Dropping it releases the
/// hold; the registry entry is removed once the last pin for a hash drops.
pub struct BlobPin {
    holds: Arc<Mutex<HashMap<BlobHash, usize>>>,
    hash: BlobHash,
}

impl Drop for BlobPin {
    fn drop(&mut self) {
        let mut holds = self
            .holds
            .lock()
            .expect("blob pin registry lock should not be poisoned");
        if let std::collections::hash_map::Entry::Occupied(mut entry) = holds.entry(self.hash) {
            let count = entry.get_mut();
            *count -= 1;
            if *count == 0 {
                entry.remove();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(seed: u8) -> BlobHash {
        BlobHash::from_bytes([seed; crate::hash::BLAKE3_HASH_LEN])
    }

    #[test]
    fn unheld_hash_is_not_held() {
        let registry = BlobPinRegistry::new();
        assert!(!registry.is_held(&hash(1)));
    }

    #[test]
    fn pin_holds_until_dropped() {
        let registry = BlobPinRegistry::new();
        let h = hash(2);
        let pin = registry.pin(h);
        assert!(registry.is_held(&h));
        drop(pin);
        assert!(!registry.is_held(&h));
    }

    #[test]
    fn refcounted_pins_require_every_guard_to_drop() {
        let registry = BlobPinRegistry::new();
        let h = hash(3);
        let first = registry.pin(h);
        let second = registry.pin(h);
        assert!(registry.is_held(&h));

        drop(first);
        assert!(
            registry.is_held(&h),
            "one remaining pin should keep the hash held"
        );

        drop(second);
        assert!(
            !registry.is_held(&h),
            "dropping the last pin should release the hold"
        );
    }

    #[test]
    fn cloned_registry_shares_the_same_holds() {
        let registry = BlobPinRegistry::new();
        let shared = registry.clone();
        let h = hash(4);
        let pin = registry.pin(h);
        assert!(shared.is_held(&h), "clone should observe the same holds");
        drop(pin);
        assert!(!shared.is_held(&h));
    }
}
