use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex};

use nimbus_core::Error;
use nimbus_storage::{KvBatchOp, KvEntry, KvMutation, KvPut, RedbTenantKvStore, TenantKvStore};

use crate::KvError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TieringMode {
    Durable,
    NoDisk,
    NoCache,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TieringConfig {
    pub mode: TieringMode,
    pub maxmemory: Option<usize>,
}

impl TieringConfig {
    #[must_use]
    pub fn durable() -> Self {
        Self {
            mode: TieringMode::Durable,
            maxmemory: None,
        }
    }

    #[must_use]
    pub fn no_disk() -> Self {
        Self {
            mode: TieringMode::NoDisk,
            maxmemory: None,
        }
    }

    #[must_use]
    pub fn no_cache() -> Self {
        Self {
            mode: TieringMode::NoCache,
            maxmemory: None,
        }
    }

    #[must_use]
    pub fn with_maxmemory(mut self, maxmemory: usize) -> Self {
        self.maxmemory = Some(maxmemory);
        self
    }
}

impl Default for TieringConfig {
    fn default() -> Self {
        Self::durable()
    }
}

#[derive(Clone)]
pub struct NimbusKvStore {
    inner: Arc<NimbusKvStoreInner>,
}

struct NimbusKvStoreInner {
    engine: Arc<dyn TenantKvStore + Send + Sync>,
    tiering: TieringConfig,
    cache: Mutex<BTreeMap<Vec<u8>, CacheEntry>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CacheEntry {
    value: Vec<u8>,
    expire_at_ms: Option<i64>,
}

impl NimbusKvStore {
    pub fn durable_at(path: impl AsRef<Path>, tiering: TieringConfig) -> Result<Self, KvError> {
        let tiering = TieringConfig {
            mode: if tiering.mode == TieringMode::NoDisk {
                TieringMode::Durable
            } else {
                tiering.mode
            },
            ..tiering
        };
        let engine = Arc::new(RedbTenantKvStore::open(path)?);
        Ok(Self::from_engine(engine, tiering))
    }

    pub fn no_disk(tiering: TieringConfig) -> Result<Self, KvError> {
        let tiering = TieringConfig {
            mode: TieringMode::NoDisk,
            ..tiering
        };
        let engine = Arc::new(RedbTenantKvStore::create_in_memory()?);
        Ok(Self::from_engine(engine, tiering))
    }

    pub fn no_cache_at(path: impl AsRef<Path>) -> Result<Self, KvError> {
        Self::durable_at(path, TieringConfig::no_cache())
    }

    pub fn from_engine(
        engine: Arc<dyn TenantKvStore + Send + Sync>,
        tiering: TieringConfig,
    ) -> Self {
        Self {
            inner: Arc::new(NimbusKvStoreInner {
                engine,
                tiering,
                cache: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    #[must_use]
    pub fn tiering(&self) -> &TieringConfig {
        &self.inner.tiering
    }

    pub fn get(&self, key: &[u8], now_ms: i64) -> Result<Option<Vec<u8>>, KvError> {
        Ok(self.get_entry(key, now_ms)?.map(|entry| entry.value))
    }

    pub fn set(
        &self,
        key: impl Into<Vec<u8>>,
        value: impl Into<Vec<u8>>,
        expire_at_ms: Option<i64>,
    ) -> Result<(), KvError> {
        let put = KvPut {
            key: key.into(),
            value: value.into(),
            metadata: BTreeMap::new(),
            expire_at_ms,
        };
        self.inner.engine.kv_put(put.clone())?;
        self.cache_put(&KvEntry {
            key: put.key,
            value: put.value,
            metadata: put.metadata,
            expire_at_ms: put.expire_at_ms,
        });
        Ok(())
    }

    pub fn delete(&self, key: &[u8]) -> Result<bool, KvError> {
        let deleted = self.inner.engine.kv_delete(key)?;
        self.cache_remove(key);
        Ok(deleted)
    }

    pub fn flush_all(&self, now_ms: i64) -> Result<usize, KvError> {
        const BATCH_LIMIT: usize = 256;

        let mut cursor = None;
        let mut deleted = 0_usize;
        loop {
            let outcome = self.inner.engine.kv_sweep_expired(now_ms, BATCH_LIMIT)?;
            deleted += outcome.deleted;
            if outcome.deleted == 0 && outcome.stale_index_entries == 0 {
                break;
            }
        }

        loop {
            let page = self
                .inner
                .engine
                .kv_scan(b"", cursor.as_deref(), BATCH_LIMIT, now_ms)?;
            if page.entries.is_empty() {
                break;
            }

            let ops = page
                .entries
                .iter()
                .map(|entry| KvBatchOp::Delete(entry.key.clone()))
                .collect::<Vec<_>>();
            deleted += self.inner.engine.kv_apply_batch(&ops)?.deletes;
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }

        if self.cache_enabled() {
            self.cache_lock()?.clear();
        }
        Ok(deleted)
    }

    pub fn expire(&self, key: &[u8], expire_at_ms: i64, now_ms: i64) -> Result<bool, KvError> {
        let mut found = false;
        let mut update = |previous: Option<KvEntry>| {
            let Some(previous) = previous else {
                return Ok(KvMutation::Keep);
            };
            found = true;
            Ok(KvMutation::Put(KvPut {
                key: previous.key,
                value: previous.value,
                metadata: previous.metadata,
                expire_at_ms: Some(expire_at_ms),
            }))
        };
        let updated = self.inner.engine.kv_update(key, now_ms, &mut update)?;
        match updated {
            Some(entry) => self.cache_put(&entry),
            None => self.cache_remove(key),
        }
        Ok(found)
    }

    pub fn ttl(&self, key: &[u8], now_ms: i64) -> Result<i64, KvError> {
        let Some(entry) = self.get_entry(key, now_ms)? else {
            return Ok(-2);
        };
        let Some(expire_at_ms) = entry.expire_at_ms else {
            return Ok(-1);
        };
        Ok(((expire_at_ms - now_ms).max(0) + 999) / 1_000)
    }

    pub fn incr(&self, key: &[u8], now_ms: i64) -> Result<i64, KvError> {
        let mut next = None;
        let mut update = |previous: Option<KvEntry>| {
            let (previous_value, expire_at_ms, metadata) = match previous {
                Some(previous) => {
                    let value = parse_i64(&previous.value)?;
                    (value, previous.expire_at_ms, previous.metadata)
                }
                None => (0, None, BTreeMap::new()),
            };
            let value = previous_value
                .checked_add(1)
                .ok_or_else(|| Error::InvalidInput("increment would overflow".to_string()))?;
            next = Some(value);
            Ok(KvMutation::Put(KvPut {
                key: key.to_vec(),
                value: value.to_string().into_bytes(),
                metadata,
                expire_at_ms,
            }))
        };
        let updated = self.inner.engine.kv_update(key, now_ms, &mut update)?;
        if let Some(entry) = updated {
            self.cache_put(&entry);
        }
        next.ok_or_else(|| {
            KvError::Core(Error::Internal("INCR did not produce a value".to_string()))
        })
    }

    fn get_entry(&self, key: &[u8], now_ms: i64) -> Result<Option<KvEntry>, KvError> {
        if self.cache_enabled() {
            let mut cache = self.cache_lock()?;
            if let Some(entry) = cache.get(key) {
                if !cache_entry_expired(entry, now_ms) {
                    return Ok(Some(KvEntry {
                        key: key.to_vec(),
                        value: entry.value.clone(),
                        metadata: BTreeMap::new(),
                        expire_at_ms: entry.expire_at_ms,
                    }));
                }
            }
            cache.remove(key);
        }

        let entry = self.inner.engine.kv_get(key, now_ms)?;
        if let Some(entry) = &entry {
            self.cache_put(entry);
        }
        Ok(entry)
    }

    fn cache_enabled(&self) -> bool {
        self.inner.tiering.mode != TieringMode::NoCache
    }

    fn cache_put(&self, entry: &KvEntry) {
        if !self.cache_enabled() {
            return;
        }
        let Ok(mut cache) = self.cache_lock() else {
            return;
        };
        cache.insert(
            entry.key.clone(),
            CacheEntry {
                value: entry.value.clone(),
                expire_at_ms: entry.expire_at_ms,
            },
        );
        if let Some(maxmemory) = self.inner.tiering.maxmemory {
            while cache_footprint(&cache) > maxmemory {
                let Some(first_key) = cache.keys().next().cloned() else {
                    break;
                };
                cache.remove(&first_key);
            }
        }
    }

    fn cache_remove(&self, key: &[u8]) {
        if !self.cache_enabled() {
            return;
        }
        if let Ok(mut cache) = self.cache_lock() {
            cache.remove(key);
        }
    }

    fn cache_lock(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, BTreeMap<Vec<u8>, CacheEntry>>, KvError> {
        self.inner.cache.lock().map_err(|_| {
            KvError::Core(Error::Internal("nimbus-kv cache lock poisoned".to_string()))
        })
    }
}

impl fmt::Debug for NimbusKvStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NimbusKvStore")
            .field("tiering", &self.inner.tiering)
            .finish_non_exhaustive()
    }
}

fn cache_entry_expired(entry: &CacheEntry, now_ms: i64) -> bool {
    entry
        .expire_at_ms
        .is_some_and(|expire_at_ms| expire_at_ms <= now_ms)
}

fn cache_footprint(cache: &BTreeMap<Vec<u8>, CacheEntry>) -> usize {
    cache
        .iter()
        .map(|(key, value)| key.len() + value.value.len() + 16)
        .sum()
}

fn parse_i64(value: &[u8]) -> Result<i64, Error> {
    let value = std::str::from_utf8(value)
        .map_err(|_| Error::InvalidInput("value is not an integer".to_string()))?;
    value
        .parse::<i64>()
        .map_err(|_| Error::InvalidInput("value is not an integer".to_string()))
}
