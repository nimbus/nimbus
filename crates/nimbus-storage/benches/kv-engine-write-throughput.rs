use std::env;
use std::error::Error;
use std::time::{Duration, Instant};

use fjall::{KeyspaceCreateOptions, PersistMode, SingleWriterTxDatabase};
use nimbus_storage::{KvPut, RedbTenantKvStore, TenantKvStore};

const DEFAULT_WRITES: usize = 2_000;
const VALUE_SIZE: usize = 64;
const EXPIRE_AT_MS: i64 = 3_600_000;

fn main() -> Result<(), Box<dyn Error>> {
    let writes = env::var("NIMBUS_KV_BENCH_WRITES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_WRITES);
    let value = vec![b'x'; VALUE_SIZE];

    let redb = measure_redb(writes, &value)?;
    let fjall = measure_fjall(writes, &value)?;

    println!("nimbus-kv F2 durable write+TTL microbench");
    println!("writes={writes} value_bytes={VALUE_SIZE} expire_at_ms={EXPIRE_AT_MS}");
    println!(
        "redb elapsed_ms={} writes_per_sec={:.2}",
        redb.as_millis(),
        writes_per_second(writes, redb)
    );
    println!(
        "fjall elapsed_ms={} writes_per_sec={:.2}",
        fjall.as_millis(),
        writes_per_second(writes, fjall)
    );

    Ok(())
}

fn measure_redb(writes: usize, value: &[u8]) -> Result<Duration, Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let store = RedbTenantKvStore::open(dir.path().join("tenant.redb"))?;
    let started = Instant::now();
    for index in 0..writes {
        store.kv_put(KvPut {
            key: bench_key(index),
            value: value.to_vec(),
            metadata: Default::default(),
            expire_at_ms: Some(EXPIRE_AT_MS + i64::try_from(index)?),
        })?;
    }
    Ok(started.elapsed())
}

fn measure_fjall(writes: usize, value: &[u8]) -> Result<Duration, Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let db = SingleWriterTxDatabase::builder(dir.path()).open()?;
    let values = db.keyspace("kv_values", KeyspaceCreateOptions::default)?;
    let expiry = db.keyspace("kv_expiry", KeyspaceCreateOptions::default)?;

    let started = Instant::now();
    for index in 0..writes {
        let key = bench_key(index);
        let expire_at_ms = EXPIRE_AT_MS + i64::try_from(index)?;
        let mut tx = db.write_tx().durability(Some(PersistMode::SyncAll));
        tx.insert(&values, key.clone(), value);
        tx.insert(
            &expiry,
            expiry_index_key(expire_at_ms, &key),
            Vec::<u8>::new(),
        );
        tx.commit()?;
    }
    Ok(started.elapsed())
}

fn bench_key(index: usize) -> Vec<u8> {
    format!("tenant-a:key:{index:08}").into_bytes()
}

fn expiry_index_key(expire_at_ms: i64, key: &[u8]) -> Vec<u8> {
    let mut index_key = Vec::with_capacity(8 + key.len());
    index_key.extend_from_slice(&encode_sortable_i64(expire_at_ms));
    index_key.extend_from_slice(key);
    index_key
}

fn encode_sortable_i64(value: i64) -> [u8; 8] {
    ((value as u64) ^ (1_u64 << 63)).to_be_bytes()
}

fn writes_per_second(writes: usize, elapsed: Duration) -> f64 {
    writes as f64 / elapsed.as_secs_f64()
}
