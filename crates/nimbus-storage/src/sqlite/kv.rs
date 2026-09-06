use std::collections::BTreeMap;

use nimbus_core::TenantId;
use rusqlite::OptionalExtension;

use super::*;
use crate::{
    KvBatchOp, KvBatchOutcome, KvEntry, KvMutation, KvPut, KvScanPage, KvStorageEngine,
    KvSweepOutcome, TenantKvStore,
};

impl TenantKvStore for SqliteTenantStore {
    fn kv_get(&self, tenant: &TenantId, key: &[u8], now_ms: i64) -> Result<Option<KvEntry>> {
        let scoped_key = tenant_key(tenant, key);
        let conn = self.acquire_read_connection()?;
        let row = conn
            .query_row(
                "SELECT value, metadata_blob, expire_at_ms
                 FROM tenant_kv_values
                 WHERE key = ?1 AND (expire_at_ms IS NULL OR expire_at_ms > ?2)",
                params![scoped_key, now_ms],
                row_to_kv_record,
            )
            .optional()
            .map_err(map_sqlite_error)?;
        row.map(|record| record.into_entry(key.to_vec()))
            .transpose()
    }

    fn kv_put(&self, tenant: &TenantId, put: KvPut) -> Result<()> {
        let put = scope_put(tenant, put);
        self.execute_kv_write(|conn| apply_put(conn, &put))
    }

    fn kv_delete(&self, tenant: &TenantId, key: &[u8]) -> Result<bool> {
        let scoped_key = tenant_key(tenant, key);
        self.execute_kv_write(|conn| {
            conn.execute(
                "DELETE FROM tenant_kv_values WHERE key = ?1",
                params![scoped_key],
            )
            .map(|deleted| deleted > 0)
            .map_err(map_sqlite_error)
        })
    }

    fn kv_scan(
        &self,
        tenant: &TenantId,
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

        let scoped_prefix = tenant_key(tenant, prefix);
        let scoped_cursor = cursor.map(|cursor| tenant_key(tenant, cursor));
        let effective_cursor = scoped_cursor
            .as_deref()
            .filter(|cursor| cursor.starts_with(scoped_prefix.as_slice()));
        let upper_bound = crate::keys::prefix_end(scoped_prefix.as_slice());
        let conn = self.acquire_read_connection()?;
        let mut entries = Vec::new();

        match (effective_cursor, upper_bound.as_deref()) {
            (Some(cursor), Some(end)) => {
                collect_scan_rows(
                    &conn,
                    "SELECT key, value, metadata_blob, expire_at_ms
                     FROM tenant_kv_values
                     WHERE key > ?1 AND key < ?2
                       AND (expire_at_ms IS NULL OR expire_at_ms > ?3)
                     ORDER BY key LIMIT ?4",
                    params![cursor, end, now_ms, limit_as_i64(limit)?],
                    &mut entries,
                )?;
            }
            (Some(cursor), None) => {
                collect_scan_rows(
                    &conn,
                    "SELECT key, value, metadata_blob, expire_at_ms
                     FROM tenant_kv_values
                     WHERE key > ?1
                       AND (expire_at_ms IS NULL OR expire_at_ms > ?2)
                     ORDER BY key LIMIT ?3",
                    params![cursor, now_ms, limit_as_i64(limit)?],
                    &mut entries,
                )?;
            }
            (None, Some(end)) => {
                collect_scan_rows(
                    &conn,
                    "SELECT key, value, metadata_blob, expire_at_ms
                     FROM tenant_kv_values
                     WHERE key >= ?1 AND key < ?2
                       AND (expire_at_ms IS NULL OR expire_at_ms > ?3)
                     ORDER BY key LIMIT ?4",
                    params![scoped_prefix, end, now_ms, limit_as_i64(limit)?],
                    &mut entries,
                )?;
            }
            (None, None) => {
                collect_scan_rows(
                    &conn,
                    "SELECT key, value, metadata_blob, expire_at_ms
                     FROM tenant_kv_values
                     WHERE key >= ?1
                       AND (expire_at_ms IS NULL OR expire_at_ms > ?2)
                     ORDER BY key LIMIT ?3",
                    params![scoped_prefix, now_ms, limit_as_i64(limit)?],
                    &mut entries,
                )?;
            }
        }

        let next_cursor = (entries.len() == limit)
            .then(|| entries.last().map(|entry| entry.key.clone()))
            .flatten();
        let entries = entries
            .into_iter()
            .map(|entry| untenant_entry(tenant, entry))
            .collect::<Result<Vec<_>>>()?;
        let next_cursor = next_cursor
            .map(|cursor| untenant_key(tenant, cursor.as_slice()))
            .transpose()?;
        Ok(KvScanPage {
            entries,
            next_cursor,
        })
    }

    fn kv_apply_batch(&self, tenant: &TenantId, ops: &[KvBatchOp]) -> Result<KvBatchOutcome> {
        let scoped_ops = ops
            .iter()
            .cloned()
            .map(|op| scope_op(tenant, op))
            .collect::<Vec<_>>();
        self.execute_kv_write(|conn| {
            let mut outcome = KvBatchOutcome::default();
            for op in &scoped_ops {
                match op {
                    KvBatchOp::Put(put) => {
                        apply_put(conn, put)?;
                        outcome.puts += 1;
                    }
                    KvBatchOp::Delete(key) => {
                        outcome.deletes += conn
                            .execute("DELETE FROM tenant_kv_values WHERE key = ?1", params![key])
                            .map_err(map_sqlite_error)?;
                    }
                }
            }
            Ok(outcome)
        })
    }

    fn kv_update(
        &self,
        tenant: &TenantId,
        key: &[u8],
        now_ms: i64,
        update: &mut dyn FnMut(Option<KvEntry>) -> Result<KvMutation>,
    ) -> Result<Option<KvEntry>> {
        let scoped_key = tenant_key(tenant, key);
        self.execute_kv_write(|conn| {
            let previous = conn
                .query_row(
                    "SELECT value, metadata_blob, expire_at_ms
                     FROM tenant_kv_values WHERE key = ?1",
                    params![scoped_key],
                    row_to_kv_record,
                )
                .optional()
                .map_err(map_sqlite_error)?;
            let visible_previous = match previous {
                Some(record) if record.is_expired_at(now_ms) => {
                    conn.execute(
                        "DELETE FROM tenant_kv_values WHERE key = ?1",
                        params![scoped_key],
                    )
                    .map_err(map_sqlite_error)?;
                    None
                }
                Some(record) => Some(record.into_entry(key.to_vec())?),
                None => None,
            };

            match update(visible_previous.clone())? {
                KvMutation::Keep => Ok(visible_previous),
                KvMutation::Delete => {
                    conn.execute(
                        "DELETE FROM tenant_kv_values WHERE key = ?1",
                        params![scoped_key],
                    )
                    .map_err(map_sqlite_error)?;
                    Ok(None)
                }
                KvMutation::Put(put) if put.key != key => Err(Error::InvalidInput(
                    "kv_update put key must match update key".to_string(),
                )),
                KvMutation::Put(put) => {
                    let entry = KvEntry {
                        key: put.key.clone(),
                        value: put.value.clone(),
                        metadata: put.metadata.clone(),
                        expire_at_ms: put.expire_at_ms,
                    };
                    apply_put(conn, &scope_put(tenant, put))?;
                    Ok(Some(entry))
                }
            }
        })
    }

    fn kv_sweep_expired(&self, now_ms: i64, limit: usize) -> Result<KvSweepOutcome> {
        if limit == 0 {
            return Ok(KvSweepOutcome::default());
        }
        self.execute_kv_write(|conn| {
            let deleted = conn
                .execute(
                    "DELETE FROM tenant_kv_values
                     WHERE key IN (
                         SELECT key FROM tenant_kv_values
                         WHERE expire_at_ms IS NOT NULL AND expire_at_ms <= ?1
                         ORDER BY expire_at_ms, key
                         LIMIT ?2
                     )",
                    params![now_ms, limit_as_i64(limit)?],
                )
                .map_err(map_sqlite_error)?;
            Ok(KvSweepOutcome {
                deleted,
                stale_index_entries: 0,
            })
        })
    }
}

impl KvStorageEngine for SqliteTenantStore {
    fn engine_name(&self) -> &'static str {
        "sqlite"
    }
}

impl SqliteTenantStore {
    fn execute_kv_write<T>(&self, task: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.acquire_writer_connection()?;
        // A connection returns to the resident slot only after a confirmed
        // transaction end. BEGIN, COMMIT, or ROLLBACK failure drops the
        // suspect handle; acquire_writer_connection opens a fully initialized
        // replacement when the slot is empty.
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(map_sqlite_error)?;
        let value = match task(&conn) {
            Ok(value) => value,
            Err(error) => {
                if conn.execute_batch("ROLLBACK").is_ok() {
                    self.release_writer_connection(conn);
                }
                return Err(error);
            }
        };
        conn.execute_batch("COMMIT").map_err(map_sqlite_error)?;
        self.release_writer_connection(conn);
        Ok(value)
    }
}

#[derive(Debug)]
struct SqliteKvRecord {
    value: Vec<u8>,
    metadata: BTreeMap<String, Vec<u8>>,
    expire_at_ms: Option<i64>,
}

impl SqliteKvRecord {
    fn into_entry(self, key: Vec<u8>) -> Result<KvEntry> {
        Ok(KvEntry {
            key,
            value: self.value,
            metadata: self.metadata,
            expire_at_ms: self.expire_at_ms,
        })
    }

    fn is_expired_at(&self, now_ms: i64) -> bool {
        self.expire_at_ms
            .is_some_and(|expire_at_ms| expire_at_ms <= now_ms)
    }
}

fn row_to_kv_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<SqliteKvRecord> {
    let metadata_blob: Vec<u8> = row.get(1)?;
    let metadata = rmp_serde::from_slice(&metadata_blob).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            metadata_blob.len(),
            rusqlite::types::Type::Blob,
            Box::new(error),
        )
    })?;
    Ok(SqliteKvRecord {
        value: row.get(0)?,
        metadata,
        expire_at_ms: row.get(2)?,
    })
}

fn collect_scan_rows<P>(
    conn: &Connection,
    sql: &str,
    params: P,
    entries: &mut Vec<KvEntry>,
) -> Result<()>
where
    P: rusqlite::Params,
{
    let mut statement = conn.prepare_cached(sql).map_err(map_sqlite_error)?;
    let rows = statement
        .query_map(params, |row| {
            let key: Vec<u8> = row.get(0)?;
            let metadata_blob: Vec<u8> = row.get(2)?;
            let metadata = rmp_serde::from_slice(&metadata_blob).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    metadata_blob.len(),
                    rusqlite::types::Type::Blob,
                    Box::new(error),
                )
            })?;
            Ok(KvEntry {
                key,
                value: row.get(1)?,
                metadata,
                expire_at_ms: row.get(3)?,
            })
        })
        .map_err(map_sqlite_error)?;
    for row in rows {
        entries.push(row.map_err(map_sqlite_error)?);
    }
    Ok(())
}

fn apply_put(conn: &Connection, put: &KvPut) -> Result<()> {
    let metadata_blob = rmp_serde::to_vec_named(&put.metadata)
        .map_err(|error| Error::Serialization(error.to_string()))?;
    conn.execute(
        "INSERT INTO tenant_kv_values (key, value, metadata_blob, expire_at_ms)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(key) DO UPDATE SET
             value = excluded.value,
             metadata_blob = excluded.metadata_blob,
             expire_at_ms = excluded.expire_at_ms",
        params![put.key, put.value, metadata_blob, put.expire_at_ms],
    )
    .map_err(map_sqlite_error)?;
    Ok(())
}

fn tenant_key(tenant: &TenantId, key: &[u8]) -> Vec<u8> {
    let tenant = tenant.as_str().as_bytes();
    let mut scoped = Vec::with_capacity(tenant.len() + 1 + key.len());
    scoped.extend_from_slice(tenant);
    scoped.push(0);
    scoped.extend_from_slice(key);
    scoped
}

fn untenant_key(tenant: &TenantId, key: &[u8]) -> Result<Vec<u8>> {
    let prefix = tenant_key(tenant, &[]);
    key.strip_prefix(prefix.as_slice())
        .map(|key| key.to_vec())
        .ok_or_else(|| Error::Internal("kv entry escaped tenant scope".to_string()))
}

fn scope_put(tenant: &TenantId, mut put: KvPut) -> KvPut {
    put.key = tenant_key(tenant, put.key.as_slice());
    put
}

fn scope_op(tenant: &TenantId, op: KvBatchOp) -> KvBatchOp {
    match op {
        KvBatchOp::Put(put) => KvBatchOp::Put(scope_put(tenant, put)),
        KvBatchOp::Delete(key) => KvBatchOp::Delete(tenant_key(tenant, key.as_slice())),
    }
}

fn untenant_entry(tenant: &TenantId, mut entry: KvEntry) -> Result<KvEntry> {
    entry.key = untenant_key(tenant, entry.key.as_slice())?;
    Ok(entry)
}

fn limit_as_i64(limit: usize) -> Result<i64> {
    i64::try_from(limit)
        .map_err(|_| Error::InvalidInput("kv scan limit exceeds SQLite range".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant(id: &str) -> TenantId {
        TenantId::new(id).expect("tenant id should be valid")
    }

    #[test]
    fn sqlite_kv_round_trips_metadata_pagination_expiry_and_restart() {
        let dir = tempfile::tempdir().expect("temp dir should build");
        let path = dir.path().join("tenant.sqlite3");
        let tenant_a = tenant("tenant-a");
        let tenant_b = tenant("tenant-b");
        {
            let store = SqliteTenantStore::open(&path).expect("store should open");
            let mut first = KvPut::new("alpha:one", "1");
            first.metadata.insert("kind".to_string(), b"first".to_vec());
            store.kv_put(&tenant_a, first).expect("put should succeed");
            store
                .kv_put(
                    &tenant_a,
                    KvPut::new("alpha:two", "2").with_expire_at_ms(200),
                )
                .expect("expiring put should succeed");
            store
                .kv_put(&tenant_b, KvPut::new("alpha:one", "other"))
                .expect("other tenant put should succeed");

            let first_page = store
                .kv_scan(&tenant_a, b"alpha:", None, 1, 100)
                .expect("first page should scan");
            assert_eq!(first_page.entries[0].key, b"alpha:one");
            assert_eq!(first_page.next_cursor, Some(b"alpha:one".to_vec()));
            let second_page = store
                .kv_scan(
                    &tenant_a,
                    b"alpha:",
                    first_page.next_cursor.as_deref(),
                    1,
                    100,
                )
                .expect("second page should scan");
            assert_eq!(second_page.entries[0].key, b"alpha:two");
            assert_eq!(
                store
                    .kv_get(&tenant_a, b"alpha:two", 200)
                    .expect("expired get should succeed"),
                None
            );
            assert_eq!(
                store
                    .kv_sweep_expired(200, 10)
                    .expect("expiry sweep should succeed")
                    .deleted,
                1
            );
        }

        let reopened = SqliteTenantStore::open(&path).expect("store should reopen");
        let entry = reopened
            .kv_get(&tenant_a, b"alpha:one", 300)
            .expect("restarted get should succeed")
            .expect("durable key should exist");
        assert_eq!(entry.value, b"1");
        assert_eq!(entry.metadata["kind"], b"first");
        assert_eq!(
            reopened
                .kv_get(&tenant_b, b"alpha:one", 300)
                .expect("other tenant get should succeed")
                .expect("other tenant key should exist")
                .value,
            b"other"
        );
    }

    #[test]
    fn sqlite_kv_update_and_batch_are_atomic() {
        let dir = tempfile::tempdir().expect("temp dir should build");
        let store =
            SqliteTenantStore::open(dir.path().join("tenant.sqlite3")).expect("store should open");
        let tenant = tenant("tenant-a");
        store
            .kv_put(&tenant, KvPut::new("counter", "1"))
            .expect("initial put should succeed");
        let updated = store
            .kv_update(&tenant, b"counter", 0, &mut |previous| {
                assert_eq!(previous.expect("counter should exist").value, b"1");
                Ok(KvMutation::Put(KvPut::new("counter", "2")))
            })
            .expect("update should succeed")
            .expect("updated entry should exist");
        assert_eq!(updated.value, b"2");

        let error = store.kv_update(&tenant, b"counter", 0, &mut |_| {
            Err(Error::InvalidInput("reject update".to_string()))
        });
        assert!(matches!(error, Err(Error::InvalidInput(message)) if message == "reject update"));
        assert_eq!(
            store
                .kv_get(&tenant, b"counter", 0)
                .expect("get after rollback should succeed")
                .expect("rolled-back counter should remain")
                .value,
            b"2"
        );

        let outcome = store
            .kv_apply_batch(
                &tenant,
                &[
                    KvBatchOp::Put(KvPut::new("a", "a")),
                    KvBatchOp::Put(KvPut::new("b", "b")),
                    KvBatchOp::Delete(b"counter".to_vec()),
                ],
            )
            .expect("batch should succeed");
        assert_eq!(outcome.puts, 2);
        assert_eq!(outcome.deletes, 1);
        assert_eq!(
            store
                .kv_get(&tenant, b"counter", 0)
                .expect("get should succeed"),
            None
        );
    }
}
