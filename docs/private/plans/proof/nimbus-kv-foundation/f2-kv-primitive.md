# NKV0 F2 KV Primitive Proof

Date: 2026-06-27
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/nkv-cloudflare-foundation`
Branch: `codex/nkv-cloudflare-foundation`
Commit: `3222babfc`

## Delivered Surface

- `crates/nimbus-storage/src/traits/kv.rs` defines `TenantKvStore` with `kv_get`,
  `kv_put`, `kv_delete`, `kv_scan`, `kv_apply_batch`, and `kv_sweep_expired`.
- `KvStorageEngine` is the swappable engine seam. The default implementation is
  redb via `TenantStore` and `RedbTenantKvStore`.
- `fjall = 3.1.5` is a compiled workspace dependency and the
  `kv-engine-write-throughput` bench exercises a real
  `SingleWriterTxDatabase` write path.
- TTL state is value-record truth plus an expiry-ordered secondary index in the
  same redb transaction as the value write. Reads perform skip-on-read logical
  expiry.
- The expiry sweep is transactional compare-and-delete: the deleting write
  transaction re-reads the current value record and deletes only when its
  `expire_at_ms` still matches the indexed expiry and is `<= now`.

## Atomicity Notes

`kv_apply_batch` executes every put/delete in one redb write transaction. This is
the F2 RMW foundation for later `INCR`, conditional `SET`, and CAS behavior:
command code must execute the read/modify/write inside the storage-engine
transaction instead of reading from cache and writing afterward.

The durable write path is intentionally serialized. The three serialization
points that matter for throughput claims are:

- redb `begin_write`, a single-writer transaction lock.
- the per-tenant engine `sequence_gate`, which serializes engine mutation order.
- the storage write semaphore, `TENANT_WRITE_PARALLELISM = 1`.

Nimbus KV durable mode is therefore a correctness-first durable tier, not Redis
cache throughput parity. Cache-class latency belongs to no-disk/cache modes.

## TTL Safety Tests

Command:

```text
cargo test -p nimbus-storage kv::tests
```

Output:

```text
running 5 tests
test kv::tests::ttl_sweep_compare_and_delete_preserves_key_extended_by_racing_set_ex ... ok
test kv::tests::kv_apply_batch_is_atomic_for_multiple_keys ... ok
test kv::tests::skip_on_read_hides_expired_entries_from_get_and_scan ... ok
test kv::tests::redb_kv_round_trips_put_get_delete_and_scan ... ok
test kv::tests::ttl_sweep_deletes_expired_key_with_its_expired_index_entry ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 304 filtered out; finished in 0.21s
```

## redb vs fjall Durable Write/TTL Microbench

This redb vs fjall write throughput benchmark measures the F2 durable
write-plus-expiry-index workload.

Command:

```text
NIMBUS_KV_BENCH_WRITES=500 cargo bench -p nimbus-storage --bench kv-engine-write-throughput
```

Output:

```text
nimbus-kv F2 durable write+TTL microbench
writes=500 value_bytes=64 expire_at_ms=3600000
redb elapsed_ms=2590 writes_per_sec=193.00
fjall elapsed_ms=2366 writes_per_sec=211.24
```

This is a local macOS/APFS laptop measurement of one durable transaction per key
plus one expiry-index write. It is a gate artifact for engine choice and
serialization honesty, not a production capacity number.
