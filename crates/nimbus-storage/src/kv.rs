use std::path::Path;
use std::sync::Arc;

use nimbus_core::{Error, Result};
use redb::{ReadableTable, TableDefinition, TableError};
use serde::{Deserialize, Serialize};

use crate::keys::prefix_end;
use crate::store::{TenantStore, map_redb_error};
use crate::traits::{
    KvBatchOp, KvBatchOutcome, KvEntry, KvMutation, KvPut, KvScanPage, KvStorageEngine,
    KvSweepOutcome, TenantKvStore,
};

const KV_VALUES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("kv_values");
const KV_EXPIRY: TableDefinition<&[u8], &[u8]> = TableDefinition::new("kv_expiry");
const EMPTY_EXPIRY_VALUE: &[u8] = &[];

#[derive(Clone)]
pub struct RedbTenantKvStore {
    inner: Arc<TenantStore>,
}

impl RedbTenantKvStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self::from_store(Arc::new(TenantStore::open(path)?)))
    }

    pub fn create_in_memory() -> Result<Self> {
        Ok(Self::from_store(Arc::new(TenantStore::create_in_memory()?)))
    }

    pub fn from_store(inner: Arc<TenantStore>) -> Self {
        Self { inner }
    }
}

impl TenantKvStore for RedbTenantKvStore {
    fn kv_get(&self, key: &[u8], now_ms: i64) -> Result<Option<KvEntry>> {
        self.inner.kv_get(key, now_ms)
    }

    fn kv_put(&self, put: KvPut) -> Result<()> {
        self.inner.kv_put(put)
    }

    fn kv_delete(&self, key: &[u8]) -> Result<bool> {
        self.inner.kv_delete(key)
    }

    fn kv_scan(
        &self,
        prefix: &[u8],
        cursor: Option<&[u8]>,
        limit: usize,
        now_ms: i64,
    ) -> Result<KvScanPage> {
        self.inner.kv_scan(prefix, cursor, limit, now_ms)
    }

    fn kv_apply_batch(&self, ops: &[KvBatchOp]) -> Result<KvBatchOutcome> {
        self.inner.kv_apply_batch(ops)
    }

    fn kv_update(
        &self,
        key: &[u8],
        now_ms: i64,
        update: &mut dyn FnMut(Option<KvEntry>) -> Result<KvMutation>,
    ) -> Result<Option<KvEntry>> {
        self.inner.kv_update(key, now_ms, update)
    }

    fn kv_sweep_expired(&self, now_ms: i64, limit: usize) -> Result<KvSweepOutcome> {
        self.inner.kv_sweep_expired(now_ms, limit)
    }
}

impl KvStorageEngine for RedbTenantKvStore {
    fn engine_name(&self) -> &'static str {
        "redb"
    }
}

impl TenantKvStore for TenantStore {
    fn kv_get(&self, key: &[u8], now_ms: i64) -> Result<Option<KvEntry>> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        let values = match read_txn.open_table(KV_VALUES) {
            Ok(values) => values,
            Err(TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(error) => return Err(map_redb_error(error)),
        };
        let Some(record) = values
            .get(key)
            .map_err(map_redb_error)?
            .map(|value| decode_record(value.value()))
            .transpose()?
        else {
            return Ok(None);
        };
        if record.is_expired_at(now_ms) {
            return Ok(None);
        }
        Ok(Some(record.into_entry(key.to_vec())))
    }

    fn kv_put(&self, put: KvPut) -> Result<()> {
        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        {
            let mut values = write_txn.open_table(KV_VALUES).map_err(map_redb_error)?;
            let mut expiry = write_txn.open_table(KV_EXPIRY).map_err(map_redb_error)?;
            apply_put(&mut values, &mut expiry, put)?;
        }
        write_txn.commit().map_err(map_redb_error)?;
        Ok(())
    }

    fn kv_delete(&self, key: &[u8]) -> Result<bool> {
        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        let removed = {
            let mut values = write_txn.open_table(KV_VALUES).map_err(map_redb_error)?;
            let mut expiry = write_txn.open_table(KV_EXPIRY).map_err(map_redb_error)?;
            apply_delete(&mut values, &mut expiry, key)?
        };
        write_txn.commit().map_err(map_redb_error)?;
        Ok(removed)
    }

    fn kv_scan(
        &self,
        prefix: &[u8],
        cursor: Option<&[u8]>,
        limit: usize,
        now_ms: i64,
    ) -> Result<KvScanPage> {
        if limit == 0 {
            return Ok(KvScanPage {
                entries: Vec::new(),
                next_cursor: None,
            });
        }

        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        let values = match read_txn.open_table(KV_VALUES) {
            Ok(values) => values,
            Err(TableError::TableDoesNotExist(_)) => {
                return Ok(KvScanPage {
                    entries: Vec::new(),
                    next_cursor: None,
                });
            }
            Err(error) => return Err(map_redb_error(error)),
        };

        let effective_cursor = cursor.filter(|cursor| cursor.starts_with(prefix));
        let start = effective_cursor.unwrap_or(prefix);
        let mut entries = Vec::new();
        let mut next_cursor = None;

        match prefix_end(prefix) {
            Some(end) => {
                for item in values
                    .range(start..end.as_slice())
                    .map_err(map_redb_error)?
                {
                    let (key, value) = item.map_err(map_redb_error)?;
                    scan_entry(
                        &mut entries,
                        &mut next_cursor,
                        key.value(),
                        value.value(),
                        prefix,
                        effective_cursor,
                        limit,
                        now_ms,
                    )?;
                    if next_cursor.is_some() {
                        break;
                    }
                }
            }
            None => {
                for item in values.range(start..).map_err(map_redb_error)? {
                    let (key, value) = item.map_err(map_redb_error)?;
                    if !key.value().starts_with(prefix) {
                        break;
                    }
                    scan_entry(
                        &mut entries,
                        &mut next_cursor,
                        key.value(),
                        value.value(),
                        prefix,
                        effective_cursor,
                        limit,
                        now_ms,
                    )?;
                    if next_cursor.is_some() {
                        break;
                    }
                }
            }
        }

        Ok(KvScanPage {
            entries,
            next_cursor,
        })
    }

    fn kv_apply_batch(&self, ops: &[KvBatchOp]) -> Result<KvBatchOutcome> {
        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        let mut outcome = KvBatchOutcome::default();
        {
            let mut values = write_txn.open_table(KV_VALUES).map_err(map_redb_error)?;
            let mut expiry = write_txn.open_table(KV_EXPIRY).map_err(map_redb_error)?;
            for op in ops {
                match op {
                    KvBatchOp::Put(put) => {
                        apply_put(&mut values, &mut expiry, put.clone())?;
                        outcome.puts += 1;
                    }
                    KvBatchOp::Delete(key) => {
                        if apply_delete(&mut values, &mut expiry, key)? {
                            outcome.deletes += 1;
                        }
                    }
                }
            }
        }
        write_txn.commit().map_err(map_redb_error)?;
        Ok(outcome)
    }

    fn kv_update(
        &self,
        key: &[u8],
        now_ms: i64,
        update: &mut dyn FnMut(Option<KvEntry>) -> Result<KvMutation>,
    ) -> Result<Option<KvEntry>> {
        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        let updated = {
            let mut values = write_txn.open_table(KV_VALUES).map_err(map_redb_error)?;
            let mut expiry = write_txn.open_table(KV_EXPIRY).map_err(map_redb_error)?;
            apply_update(&mut values, &mut expiry, key, now_ms, update)?
        };
        write_txn.commit().map_err(map_redb_error)?;
        Ok(updated)
    }

    fn kv_sweep_expired(&self, now_ms: i64, limit: usize) -> Result<KvSweepOutcome> {
        if limit == 0 {
            return Ok(KvSweepOutcome::default());
        }

        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        let mut outcome = KvSweepOutcome::default();
        {
            let mut expiry = match write_txn.open_table(KV_EXPIRY) {
                Ok(expiry) => expiry,
                Err(TableError::TableDoesNotExist(_)) => {
                    return Ok(KvSweepOutcome::default());
                }
                Err(error) => return Err(map_redb_error(error)),
            };
            let mut values = write_txn.open_table(KV_VALUES).map_err(map_redb_error)?;
            let mut candidates = Vec::new();
            match expiry_index_exclusive_upper_bound(now_ms) {
                Some(upper_bound) => {
                    for item in expiry
                        .range::<&[u8]>(..upper_bound.as_slice())
                        .map_err(map_redb_error)?
                    {
                        let (expiry_key, _) = item.map_err(map_redb_error)?;
                        candidates.push(expiry_key.value().to_vec());
                        if candidates.len() >= limit {
                            break;
                        }
                    }
                }
                None => {
                    for item in expiry.iter().map_err(map_redb_error)? {
                        let (expiry_key, _) = item.map_err(map_redb_error)?;
                        candidates.push(expiry_key.value().to_vec());
                        if candidates.len() >= limit {
                            break;
                        }
                    }
                }
            }

            for expiry_key in candidates {
                let (indexed_expire_at_ms, key) = decode_expiry_index_key(&expiry_key)?;
                let current = values
                    .get(key.as_slice())
                    .map_err(map_redb_error)?
                    .map(|value| decode_record(value.value()))
                    .transpose()?;

                match current {
                    Some(record)
                        if record.expire_at_ms == Some(indexed_expire_at_ms)
                            && record.is_expired_at(now_ms) =>
                    {
                        values.remove(key.as_slice()).map_err(map_redb_error)?;
                        expiry
                            .remove(expiry_key.as_slice())
                            .map_err(map_redb_error)?;
                        outcome.deleted += 1;
                    }
                    _ => {
                        expiry
                            .remove(expiry_key.as_slice())
                            .map_err(map_redb_error)?;
                        outcome.stale_index_entries += 1;
                    }
                }
            }
        }
        write_txn.commit().map_err(map_redb_error)?;
        Ok(outcome)
    }
}

impl KvStorageEngine for TenantStore {
    fn engine_name(&self) -> &'static str {
        "redb"
    }
}

pub const FJALL_KV_ENGINE_NAME: &str = "fjall";

pub fn fjall_kv_engine_type_marker() -> &'static str {
    std::any::type_name::<fjall::SingleWriterTxDatabase>()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RedbKvRecord {
    value: Vec<u8>,
    metadata: std::collections::BTreeMap<String, Vec<u8>>,
    expire_at_ms: Option<i64>,
}

impl RedbKvRecord {
    fn into_entry(self, key: Vec<u8>) -> KvEntry {
        KvEntry {
            key,
            value: self.value,
            metadata: self.metadata,
            expire_at_ms: self.expire_at_ms,
        }
    }

    fn is_expired_at(&self, now_ms: i64) -> bool {
        self.expire_at_ms
            .is_some_and(|expire_at_ms| expire_at_ms <= now_ms)
    }
}

fn apply_put(
    values: &mut redb::Table<&[u8], &[u8]>,
    expiry: &mut redb::Table<&[u8], &[u8]>,
    put: KvPut,
) -> Result<()> {
    let KvPut {
        key,
        value,
        metadata,
        expire_at_ms,
    } = put;
    let record = RedbKvRecord {
        value,
        metadata,
        expire_at_ms,
    };
    if let Some(previous) = values
        .get(key.as_slice())
        .map_err(map_redb_error)?
        .map(|value| decode_record(value.value()))
        .transpose()?
    {
        remove_expiry_index(expiry, key.as_slice(), previous.expire_at_ms)?;
    }

    let encoded = encode_record(&record)?;
    values
        .insert(key.as_slice(), encoded.as_slice())
        .map_err(map_redb_error)?;
    if let Some(expire_at_ms) = record.expire_at_ms {
        let expiry_key = expiry_index_key(expire_at_ms, key.as_slice());
        expiry
            .insert(expiry_key.as_slice(), EMPTY_EXPIRY_VALUE)
            .map_err(map_redb_error)?;
    }
    Ok(())
}

fn apply_delete(
    values: &mut redb::Table<&[u8], &[u8]>,
    expiry: &mut redb::Table<&[u8], &[u8]>,
    key: &[u8],
) -> Result<bool> {
    let removed = values.remove(key).map_err(map_redb_error)?;
    let Some(value) = removed else {
        return Ok(false);
    };
    let previous = decode_record(value.value())?;
    remove_expiry_index(expiry, key, previous.expire_at_ms)?;
    Ok(true)
}

fn apply_update(
    values: &mut redb::Table<&[u8], &[u8]>,
    expiry: &mut redb::Table<&[u8], &[u8]>,
    key: &[u8],
    now_ms: i64,
    update: &mut dyn FnMut(Option<KvEntry>) -> Result<KvMutation>,
) -> Result<Option<KvEntry>> {
    let previous = values
        .get(key)
        .map_err(map_redb_error)?
        .map(|value| decode_record(value.value()))
        .transpose()?;
    let visible_previous = match previous {
        Some(record) if record.is_expired_at(now_ms) => {
            remove_expiry_index(expiry, key, record.expire_at_ms)?;
            values.remove(key).map_err(map_redb_error)?;
            None
        }
        Some(record) => Some(record.into_entry(key.to_vec())),
        None => None,
    };

    match update(visible_previous.clone())? {
        KvMutation::Keep => Ok(visible_previous),
        KvMutation::Delete => {
            if let Some(previous) = visible_previous {
                remove_expiry_index(expiry, key, previous.expire_at_ms)?;
                values.remove(key).map_err(map_redb_error)?;
            }
            Ok(None)
        }
        KvMutation::Put(put) => {
            if put.key != key {
                return Err(Error::InvalidInput(
                    "kv_update put key must match update key".to_string(),
                ));
            }
            let entry = KvEntry {
                key: put.key.clone(),
                value: put.value.clone(),
                metadata: put.metadata.clone(),
                expire_at_ms: put.expire_at_ms,
            };
            apply_put(values, expiry, put)?;
            Ok(Some(entry))
        }
    }
}

fn remove_expiry_index(
    expiry: &mut redb::Table<&[u8], &[u8]>,
    key: &[u8],
    expire_at_ms: Option<i64>,
) -> Result<()> {
    if let Some(expire_at_ms) = expire_at_ms {
        let expiry_key = expiry_index_key(expire_at_ms, key);
        expiry
            .remove(expiry_key.as_slice())
            .map_err(map_redb_error)?;
    }
    Ok(())
}

fn scan_entry(
    entries: &mut Vec<KvEntry>,
    next_cursor: &mut Option<Vec<u8>>,
    key: &[u8],
    value: &[u8],
    prefix: &[u8],
    cursor: Option<&[u8]>,
    limit: usize,
    now_ms: i64,
) -> Result<()> {
    if !key.starts_with(prefix) {
        return Ok(());
    }
    if cursor.is_some_and(|cursor| key <= cursor) {
        return Ok(());
    }
    let record = decode_record(value)?;
    if record.is_expired_at(now_ms) {
        return Ok(());
    }
    entries.push(record.into_entry(key.to_vec()));
    if entries.len() == limit {
        *next_cursor = Some(key.to_vec());
    }
    Ok(())
}

fn encode_record(record: &RedbKvRecord) -> Result<Vec<u8>> {
    rmp_serde::to_vec_named(record).map_err(|error| Error::Serialization(error.to_string()))
}

fn decode_record(bytes: &[u8]) -> Result<RedbKvRecord> {
    rmp_serde::from_slice(bytes).map_err(|error| Error::Serialization(error.to_string()))
}

fn expiry_index_key(expire_at_ms: i64, key: &[u8]) -> Vec<u8> {
    let mut index_key = Vec::with_capacity(8 + key.len());
    index_key.extend_from_slice(&encode_sortable_i64(expire_at_ms));
    index_key.extend_from_slice(key);
    index_key
}

fn expiry_index_exclusive_upper_bound(expire_at_ms: i64) -> Option<Vec<u8>> {
    let next_expire_at = expire_at_ms.checked_add(1)?;
    let mut bound = Vec::with_capacity(8);
    bound.extend_from_slice(&encode_sortable_i64(next_expire_at));
    Some(bound)
}

fn decode_expiry_index_key(index_key: &[u8]) -> Result<(i64, Vec<u8>)> {
    let Some(encoded_expire_at) = index_key.get(..8) else {
        return Err(Error::Serialization(
            "kv expiry index key is shorter than timestamp prefix".to_string(),
        ));
    };
    let mut timestamp = [0_u8; 8];
    timestamp.copy_from_slice(encoded_expire_at);
    Ok((
        decode_sortable_i64(timestamp),
        index_key.get(8..).unwrap_or_default().to_vec(),
    ))
}

fn encode_sortable_i64(value: i64) -> [u8; 8] {
    ((value as u64) ^ (1_u64 << 63)).to_be_bytes()
}

fn decode_sortable_i64(bytes: [u8; 8]) -> i64 {
    (u64::from_be_bytes(bytes) ^ (1_u64 << 63)) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redb_kv_round_trips_put_get_delete_and_scan() {
        let store = TenantStore::create_in_memory().expect("store should open");
        store
            .kv_put(KvPut::new("alpha:one", "1"))
            .expect("put should succeed");
        store
            .kv_put(KvPut::new("alpha:two", "2"))
            .expect("put should succeed");
        store
            .kv_put(KvPut::new("beta:one", "3"))
            .expect("put should succeed");

        let entry = store
            .kv_get(b"alpha:one", 0)
            .expect("get should succeed")
            .expect("entry should exist");
        assert_eq!(entry.value, b"1");

        let page = store
            .kv_scan(b"alpha:", None, 10, 0)
            .expect("scan should succeed");
        let keys: Vec<Vec<u8>> = page.entries.into_iter().map(|entry| entry.key).collect();
        assert_eq!(keys, vec![b"alpha:one".to_vec(), b"alpha:two".to_vec()]);
        assert_eq!(page.next_cursor, None);

        assert!(store.kv_delete(b"alpha:one").expect("delete succeeds"));
        assert_eq!(store.kv_get(b"alpha:one", 0).expect("get succeeds"), None);
    }

    #[test]
    fn kv_apply_batch_is_atomic_for_multiple_keys() {
        let store = TenantStore::create_in_memory().expect("store should open");
        let outcome = store
            .kv_apply_batch(&[
                KvBatchOp::Put(KvPut::new("counter:a", "1")),
                KvBatchOp::Put(KvPut::new("counter:b", "2")),
                KvBatchOp::Delete(b"missing".to_vec()),
            ])
            .expect("batch should commit");

        assert_eq!(
            outcome,
            KvBatchOutcome {
                puts: 2,
                deletes: 0
            }
        );
        assert_eq!(
            store
                .kv_get(b"counter:a", 0)
                .expect("get succeeds")
                .expect("entry exists")
                .value,
            b"1"
        );
        assert_eq!(
            store
                .kv_get(b"counter:b", 0)
                .expect("get succeeds")
                .expect("entry exists")
                .value,
            b"2"
        );
    }

    #[test]
    fn skip_on_read_hides_expired_entries_from_get_and_scan() {
        let store = TenantStore::create_in_memory().expect("store should open");
        store
            .kv_put(KvPut::new("ttl:dead", "old").with_expire_at_ms(100))
            .expect("expired put should succeed");
        store
            .kv_put(KvPut::new("ttl:live", "new").with_expire_at_ms(1_000))
            .expect("live put should succeed");

        assert_eq!(store.kv_get(b"ttl:dead", 200).expect("get succeeds"), None);
        let page = store
            .kv_scan(b"ttl:", None, 10, 200)
            .expect("scan should succeed");
        let entries: Vec<(Vec<u8>, Vec<u8>)> = page
            .entries
            .into_iter()
            .map(|entry| (entry.key, entry.value))
            .collect();
        assert_eq!(entries, vec![(b"ttl:live".to_vec(), b"new".to_vec())]);
    }

    #[test]
    fn kv_update_performs_read_modify_write_inside_one_transaction() {
        let store = TenantStore::create_in_memory().expect("store should open");
        store
            .kv_put(KvPut::new("counter", "41"))
            .expect("put should succeed");

        let mut increment = |previous: Option<KvEntry>| {
            let previous = previous.expect("counter should exist");
            let value = std::str::from_utf8(&previous.value)
                .expect("counter is utf8")
                .parse::<i64>()
                .expect("counter parses");
            Ok(KvMutation::Put(KvPut::new(
                previous.key,
                (value + 1).to_string(),
            )))
        };
        let updated = store
            .kv_update(b"counter", 0, &mut increment)
            .expect("update should commit")
            .expect("updated entry should exist");

        assert_eq!(updated.value, b"42");
        assert_eq!(
            store
                .kv_get(b"counter", 0)
                .expect("get succeeds")
                .expect("entry exists")
                .value,
            b"42"
        );
    }

    #[test]
    fn ttl_sweep_compare_and_delete_preserves_key_extended_by_racing_set_ex() {
        let store = TenantStore::create_in_memory().expect("store should open");
        store
            .kv_put(KvPut::new("lease", "old").with_expire_at_ms(100))
            .expect("initial put should succeed");
        store
            .kv_put(KvPut::new("lease", "new").with_expire_at_ms(1_000))
            .expect("racing SET EX extension should commit");
        insert_stale_expiry_index_for_test(&store, 100, b"lease");

        let sweep = store
            .kv_sweep_expired(200, 10)
            .expect("sweep should succeed");
        assert_eq!(
            sweep,
            KvSweepOutcome {
                deleted: 0,
                stale_index_entries: 1
            }
        );
        let entry = store
            .kv_get(b"lease", 200)
            .expect("get should succeed")
            .expect("extended key must survive stale expiry index sweep");
        assert_eq!(entry.value, b"new");
        assert_eq!(entry.expire_at_ms, Some(1_000));
    }

    #[test]
    fn ttl_sweep_deletes_expired_key_with_its_expired_index_entry() {
        let store = TenantStore::create_in_memory().expect("store should open");
        store
            .kv_put(KvPut::new("session", "dead").with_expire_at_ms(100))
            .expect("put should succeed");

        let sweep = store
            .kv_sweep_expired(200, 10)
            .expect("sweep should succeed");
        assert_eq!(
            sweep,
            KvSweepOutcome {
                deleted: 1,
                stale_index_entries: 0
            }
        );
        assert_eq!(store.kv_get(b"session", 200).expect("get succeeds"), None);

        let second_sweep = store
            .kv_sweep_expired(200, 10)
            .expect("second sweep should succeed");
        assert_eq!(second_sweep, KvSweepOutcome::default());
    }

    fn insert_stale_expiry_index_for_test(store: &TenantStore, expire_at_ms: i64, key: &[u8]) {
        let write_txn = store.db.begin_write().expect("write txn should open");
        {
            let mut expiry = write_txn
                .open_table(KV_EXPIRY)
                .expect("expiry table should open");
            let expiry_key = expiry_index_key(expire_at_ms, key);
            expiry
                .insert(expiry_key.as_slice(), EMPTY_EXPIRY_VALUE)
                .expect("stale expiry index insert should succeed");
        }
        write_txn.commit().expect("write txn should commit");
    }
}
