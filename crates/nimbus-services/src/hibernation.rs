//! CB3: hibernation persistence over `TenantKvStore`.
//!
//! When an instance hibernates, only its SERIALIZED state crosses the seam —
//! the live socket (CB1) never persists. This module persists a hibernation
//! **attachment** (the instance's opaque state) to the durable, tenant-scoped
//! `nimbus_storage::TenantKvStore` (redb by default), prefix-namespaced by
//! `(tenant, namespace, instance)`, and rehydrates it on cold wake. Two
//! invariants from the plan:
//!
//! - **16 KiB attachment cap** — a hibernatable attachment must stay small
//!   (it is durable, per-DO, and read on every wake); an oversized attachment
//!   is refused, never silently truncated.
//! - **Epoch fence** — every persisted attachment carries the broker's
//!   per-instance epoch (CB1). Rehydrate refuses an attachment stamped with a
//!   lower epoch than the caller's current activation, so a stale activation
//!   (e.g. after placement moved) can never resurrect old state under a newer
//!   one. Single-activation, enforced at the storage boundary.

use nimbus_storage::{KvPut, TenantKvStore};

use crate::broker::InstanceKey;

/// The plan's hard cap on a hibernation attachment.
pub const MAX_ATTACHMENT_BYTES: usize = 16 * 1024;

/// A persisted hibernation attachment: the instance's serialized state plus
/// the epoch it was stamped under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HibernationAttachment {
    pub state: Vec<u8>,
    pub epoch: u64,
}

/// Errors persisting/rehydrating hibernation state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HibernationError {
    /// Attachment exceeds the 16 KiB cap.
    TooLarge { bytes: usize },
    /// The persisted record is corrupt (bad length prefix).
    Corrupt(String),
    /// Underlying KV error.
    Kv(String),
}

impl std::fmt::Display for HibernationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge { bytes } => write!(
                f,
                "hibernation attachment is {bytes} bytes, over the {MAX_ATTACHMENT_BYTES}-byte cap"
            ),
            Self::Corrupt(m) => write!(f, "corrupt hibernation record: {m}"),
            Self::Kv(m) => write!(f, "hibernation KV error: {m}"),
        }
    }
}

impl std::error::Error for HibernationError {}

/// Persists/rehydrates hibernation attachments over an injected
/// `TenantKvStore` (the production binding is redb; tests inject an in-memory
/// store). The broker owns the epoch; this store owns the durable boundary.
pub struct HibernationStore<'a> {
    kv: &'a dyn TenantKvStore,
}

impl<'a> HibernationStore<'a> {
    pub fn new(kv: &'a dyn TenantKvStore) -> Self {
        Self { kv }
    }

    /// The tenant-scoped KV key for an instance's hibernation attachment.
    fn attachment_key(key: &InstanceKey) -> Vec<u8> {
        format!("cb-hib/{}/{}", key.namespace(), key.instance_id()).into_bytes()
    }

    /// Persist `attachment` for `key` (epoch-fenced, 16 KiB-capped).
    ///
    /// Wire format: `epoch (u64 LE) || state`. `expire_at_ms` (optional TTL)
    /// threads the plan's idle-hibernation timeout when set.
    pub fn persist_hibernation(
        &self,
        key: &InstanceKey,
        attachment: &HibernationAttachment,
        expire_at_ms: Option<i64>,
    ) -> Result<(), HibernationError> {
        if attachment.state.len() > MAX_ATTACHMENT_BYTES {
            return Err(HibernationError::TooLarge {
                bytes: attachment.state.len(),
            });
        }
        let mut record = Vec::with_capacity(8 + attachment.state.len());
        record.extend_from_slice(&attachment.epoch.to_le_bytes());
        record.extend_from_slice(&attachment.state);

        let mut put = KvPut::new(Self::attachment_key(key), record);
        if let Some(expire_at_ms) = expire_at_ms {
            put = put.with_expire_at_ms(expire_at_ms);
        }
        self.kv
            .kv_put(key.tenant(), put)
            .map_err(|error| HibernationError::Kv(error.to_string()))
    }

    /// Rehydrate `key`'s attachment, fencing against a stale epoch.
    ///
    /// Returns `Ok(None)` if nothing is persisted OR if the persisted epoch is
    /// LOWER than `min_epoch` (the caller's current activation) — a stale
    /// activation must not resurrect old state. Returns the attachment only
    /// when its epoch is `>= min_epoch`.
    pub fn rehydrate_from_kv(
        &self,
        key: &InstanceKey,
        min_epoch: u64,
        now_ms: i64,
    ) -> Result<Option<HibernationAttachment>, HibernationError> {
        let entry = self
            .kv
            .kv_get(key.tenant(), &Self::attachment_key(key), now_ms)
            .map_err(|error| HibernationError::Kv(error.to_string()))?;
        let Some(entry) = entry else {
            return Ok(None);
        };
        if entry.value.len() < 8 {
            return Err(HibernationError::Corrupt(format!(
                "record is {} bytes, need >= 8 for the epoch prefix",
                entry.value.len()
            )));
        }
        let epoch = u64::from_le_bytes(entry.value[..8].try_into().unwrap());
        if epoch < min_epoch {
            // Epoch fence: a stale activation cannot rehydrate older state.
            return Ok(None);
        }
        Ok(Some(HibernationAttachment {
            state: entry.value[8..].to_vec(),
            epoch,
        }))
    }

    /// Drop an instance's persisted attachment (permanent teardown).
    pub fn drop_hibernation(&self, key: &InstanceKey) -> Result<bool, HibernationError> {
        self.kv
            .kv_delete(key.tenant(), &Self::attachment_key(key))
            .map_err(|error| HibernationError::Kv(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nimbus_core::TenantId;
    use nimbus_storage::{
        KvBatchOp, KvBatchOutcome, KvEntry, KvMutation, KvScanPage, KvSweepOutcome,
    };
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Minimal in-memory TenantKvStore for hermetic tests (put/get/delete are
    /// real; the scan/batch/update/sweep surface the HibernationStore never
    /// calls returns empty defaults).
    #[derive(Default)]
    struct MemKv {
        map: Mutex<HashMap<(String, Vec<u8>), KvEntry>>,
    }

    impl TenantKvStore for MemKv {
        fn kv_get(
            &self,
            tenant: &TenantId,
            key: &[u8],
            now_ms: i64,
        ) -> nimbus_core::Result<Option<KvEntry>> {
            let map = self.map.lock().unwrap();
            Ok(map
                .get(&(tenant.as_str().to_owned(), key.to_vec()))
                .filter(|e| !e.is_expired_at(now_ms))
                .cloned())
        }
        fn kv_put(&self, tenant: &TenantId, put: KvPut) -> nimbus_core::Result<()> {
            let entry = KvEntry {
                key: put.key.clone(),
                value: put.value,
                metadata: put.metadata,
                expire_at_ms: put.expire_at_ms,
            };
            self.map
                .lock()
                .unwrap()
                .insert((tenant.as_str().to_owned(), put.key), entry);
            Ok(())
        }
        fn kv_delete(&self, tenant: &TenantId, key: &[u8]) -> nimbus_core::Result<bool> {
            Ok(self
                .map
                .lock()
                .unwrap()
                .remove(&(tenant.as_str().to_owned(), key.to_vec()))
                .is_some())
        }
        // HibernationStore only exercises get/put/delete; the rest are not
        // reachable from these tests.
        fn kv_scan(
            &self,
            _t: &TenantId,
            _p: &[u8],
            _c: Option<&[u8]>,
            _l: usize,
            _n: i64,
        ) -> nimbus_core::Result<KvScanPage> {
            unreachable!("kv_scan is not used by HibernationStore")
        }
        fn kv_apply_batch(
            &self,
            _t: &TenantId,
            _o: &[KvBatchOp],
        ) -> nimbus_core::Result<KvBatchOutcome> {
            unreachable!("kv_apply_batch is not used by HibernationStore")
        }
        fn kv_update(
            &self,
            _t: &TenantId,
            _k: &[u8],
            _n: i64,
            _u: &mut dyn FnMut(Option<KvEntry>) -> nimbus_core::Result<KvMutation>,
        ) -> nimbus_core::Result<Option<KvEntry>> {
            unreachable!("kv_update is not used by HibernationStore")
        }
        fn kv_sweep_expired(&self, _n: i64, _l: usize) -> nimbus_core::Result<KvSweepOutcome> {
            unreachable!("kv_sweep_expired is not used by HibernationStore")
        }
    }

    fn key(instance: &str) -> InstanceKey {
        InstanceKey::new(TenantId::new("tenant-a").unwrap(), "chat", instance)
    }

    #[test]
    fn persist_then_rehydrate_round_trips_within_epoch() {
        let kv = MemKv::default();
        let store = HibernationStore::new(&kv);
        let k = key("room-1");

        store
            .persist_hibernation(
                &k,
                &HibernationAttachment {
                    state: b"session".to_vec(),
                    epoch: 3,
                },
                None,
            )
            .expect("persist");

        let got = store
            .rehydrate_from_kv(&k, 3, 0)
            .expect("rehydrate")
            .expect("present");
        assert_eq!(got.state, b"session");
        assert_eq!(got.epoch, 3);

        // A lower-or-equal min_epoch still rehydrates.
        assert!(store.rehydrate_from_kv(&k, 2, 0).unwrap().is_some());
    }

    #[test]
    fn epoch_fence_refuses_stale_activation() {
        let kv = MemKv::default();
        let store = HibernationStore::new(&kv);
        let k = key("room-1");
        store
            .persist_hibernation(
                &k,
                &HibernationAttachment {
                    state: b"old".to_vec(),
                    epoch: 5,
                },
                None,
            )
            .unwrap();

        // A newer activation (min_epoch 7) must NOT rehydrate epoch-5 state.
        assert!(
            store.rehydrate_from_kv(&k, 7, 0).unwrap().is_none(),
            "epoch fence: a stale attachment cannot resurrect under a newer activation"
        );
    }

    #[test]
    fn attachment_over_16kib_is_refused_not_truncated() {
        let kv = MemKv::default();
        let store = HibernationStore::new(&kv);
        let oversized = HibernationAttachment {
            state: vec![0u8; MAX_ATTACHMENT_BYTES + 1],
            epoch: 1,
        };
        let err = store
            .persist_hibernation(&key("room-1"), &oversized, None)
            .expect_err("over cap");
        assert!(matches!(err, HibernationError::TooLarge { .. }));
        // Nothing was written.
        assert!(
            store
                .rehydrate_from_kv(&key("room-1"), 0, 0)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn drop_removes_the_attachment() {
        let kv = MemKv::default();
        let store = HibernationStore::new(&kv);
        let k = key("room-1");
        store
            .persist_hibernation(
                &k,
                &HibernationAttachment {
                    state: b"x".to_vec(),
                    epoch: 1,
                },
                None,
            )
            .unwrap();
        assert!(store.drop_hibernation(&k).unwrap());
        assert!(store.rehydrate_from_kv(&k, 0, 0).unwrap().is_none());
    }
}
