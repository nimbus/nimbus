use std::collections::BTreeMap;

use nimbus_core::Result;

/// Tenant-scoped flat key/value capability used by Redis/Valkey-compatible
/// surfaces and future Workers KV adapters.
pub trait TenantKvStore {
    fn kv_get(&self, key: &[u8], now_ms: i64) -> Result<Option<KvEntry>>;
    fn kv_put(&self, put: KvPut) -> Result<()>;
    fn kv_delete(&self, key: &[u8]) -> Result<bool>;
    fn kv_scan(
        &self,
        prefix: &[u8],
        cursor: Option<&[u8]>,
        limit: usize,
        now_ms: i64,
    ) -> Result<KvScanPage>;
    fn kv_apply_batch(&self, ops: &[KvBatchOp]) -> Result<KvBatchOutcome>;
    fn kv_update(
        &self,
        key: &[u8],
        now_ms: i64,
        update: &mut dyn FnMut(Option<KvEntry>) -> Result<KvMutation>,
    ) -> Result<Option<KvEntry>>;
    fn kv_sweep_expired(&self, now_ms: i64, limit: usize) -> Result<KvSweepOutcome>;
}

/// Swappable KV engine seam. The default implementation is redb, but F2 keeps
/// the storage API independent of any one engine family.
pub trait KvStorageEngine: TenantKvStore + Send + Sync {
    fn engine_name(&self) -> &'static str;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KvEntry {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub metadata: BTreeMap<String, Vec<u8>>,
    pub expire_at_ms: Option<i64>,
}

impl KvEntry {
    pub fn is_expired_at(&self, now_ms: i64) -> bool {
        self.expire_at_ms
            .is_some_and(|expire_at_ms| expire_at_ms <= now_ms)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KvPut {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub metadata: BTreeMap<String, Vec<u8>>,
    pub expire_at_ms: Option<i64>,
}

impl KvPut {
    pub fn new(key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            metadata: BTreeMap::new(),
            expire_at_ms: None,
        }
    }

    pub fn with_expire_at_ms(mut self, expire_at_ms: i64) -> Self {
        self.expire_at_ms = Some(expire_at_ms);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KvBatchOp {
    Put(KvPut),
    Delete(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KvMutation {
    Put(KvPut),
    Delete,
    Keep,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KvScanPage {
    pub entries: Vec<KvEntry>,
    pub next_cursor: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KvBatchOutcome {
    pub puts: usize,
    pub deletes: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KvSweepOutcome {
    pub deleted: usize,
    pub stale_index_entries: usize,
}
