# NKV0 F3 Cache/Tiering Proof

Date: 2026-06-27
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/nkv-cloudflare-foundation`
Branch: `codex/nkv-cloudflare-foundation`
Commit: `b892ff9bc`

## Delivered Surface

- `nimbus-kv` now owns `NimbusKvStore`, `TieringConfig`, and `TieringMode`.
- Modes:
  - durable + cache by default through `NimbusKvStore::durable_at`.
  - `no-disk` through an in-memory redb transaction tier.
  - `no-cache` through durable redb reads/writes without the read-through cache.
- CLI surface:
  - `--data <path>` for the redb tenant file.
  - `--no-disk`.
  - `--no-cache`.
  - `--maxmemory <bytes>`.
- RESP commands route through the tiering layer: `GET`, `SET`, `DEL`, `EXPIRE`,
  `TTL`, and `INCR`.
- `INCR` and `EXPIRE` use `TenantKvStore::kv_update`, so read/modify/write
  behavior is linearized inside one storage transaction. Cache entries are
  updated from committed results.
- Cache entries carry the durable `expire_at_ms`; cache hits apply the same
  logical-expiry check as storage reads.

## Verification

Command:

```text
cargo fmt --all --check
```

Output: passed with no diff.

Command:

```text
cargo test -p nimbus-kv
```

Output:

```text
running 5 tests
test cache_expire_at_coherency_prevents_logically_expired_cache_hit ... ok
test no_cache_mode_reads_straight_through ... ok
test no_disk_mode_is_volatile ... ok
test durable_cache_round_trip_survives_restart ... ok
test concurrent_incr_cache_coherency_disk_backed_and_no_disk ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.51s

running 6 tests
test listener_rejects_non_loopback_bind ... ok
test tenant_a_credential_cannot_read_tenant_b_keys ... ok
test resp_get_set_del_expire_ttl_incr_round_trip ... ok
test redis_rs_client_connects_and_ping_echo_round_trip ... ok
test unauthenticated_command_is_rejected ... ok
test hello_3_negotiates_resp3 ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.19s
```

Command:

```text
cargo test -p nimbus-storage kv::tests
```

Output:

```text
running 6 tests
test kv::tests::ttl_sweep_deletes_expired_key_with_its_expired_index_entry ... ok
test kv::tests::kv_update_performs_read_modify_write_inside_one_transaction ... ok
test kv::tests::kv_apply_batch_is_atomic_for_multiple_keys ... ok
test kv::tests::ttl_sweep_compare_and_delete_preserves_key_extended_by_racing_set_ex ... ok
test kv::tests::redb_kv_round_trips_put_get_delete_and_scan ... ok
test kv::tests::skip_on_read_hides_expired_entries_from_get_and_scan ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 304 filtered out; finished in 0.21s
```

Command:

```text
cargo check -p nimbus-bin
```

Output:

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 12.12s
```

Command:

```text
bash scripts/verify-nimbus-kv-foundation.sh
```

Output:

```text
[6] F3: cache/tiering modes and coherency tests
  PASS  cache/tiering config + concurrent INCR + expiry-coherency tests

7 passed, 2 failed
```

The remaining verifier failures are expected F4/F5 deliverables.
