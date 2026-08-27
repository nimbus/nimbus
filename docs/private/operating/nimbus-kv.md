# Nimbus KV Operator Notes

`nimbus kv` is the NKV0 RESP-native foundation service. It is Redis/Valkey
wire-compatible for the NKV0 string surface only: `GET`, `SET`, `DEL`,
`EXPIRE`, `TTL`, `INCR`, `PING`, `ECHO`, `HELLO`, `AUTH`, `CLIENT SETINFO`,
`FLUSHALL`, `FUNCTION FLUSH`, `NIMBUS.READY`, and `NIMBUS.METRICS`.

## Startup

Example:

```text
nimbus kv --bind 127.0.0.1:6380 --tenant demo --username demo --password local-secret
```

Options:

| Option | Meaning |
| --- | --- |
| `--bind <addr>` | RESP listener address. Must be loopback; non-loopback binds fail closed. |
| `--tenant <id>` | Tenant bound to the single dev credential. Defaults to `demo`. |
| `--username <name>` | AUTH username. Defaults to the tenant id. |
| `--password <secret>` | AUTH password. If omitted, `NIMBUS_KV_PASSWORD` is used; if that is missing, a generated dev password is printed. |
| `--data <path>` | redb tenant file for durable mode. Defaults to `.nimbus/kv/<tenant>.redb`. |
| `--no-disk` | In-memory redb tier only; state is volatile. |
| `--no-cache` | Durable redb reads/writes without the read-through cache. Cannot combine with `--no-disk`. |
| `--maxmemory <bytes>` | Approximate cache byte budget for NKV0's simple bounded cache. |

## Auth Posture

NKV0 credentials are loopback-only development credentials. Each credential maps
to exactly one `TenantId`; `SELECT` may acknowledge `0` or the credential-bound
tenant id, but cannot switch tenants. Production credential provisioning,
rotation, revocation, and non-loopback plaintext AUTH/TLS posture are follow-on
work in the service-identity / secret-management lane.

## Tiering

Default mode is durable plus cache:

- Writes go through `TenantKvStore` and update the read-through cache from the
  committed result.
- Atomic RMW commands such as `INCR` and `EXPIRE` execute through
  `TenantKvStore::kv_update`, not against cached state.
- Cache entries carry the durable `expire_at_ms`; a logically expired value is
  never served from cache.

`--no-disk` keeps the same transactional source of truth, but the redb tier is
in-memory and volatile. `--no-cache` disables read-through caching and reads
directly from the durable tier.

## Durability And Recovery Boundary

NKV0 durable mode means local redb transaction durability for the selected
tenant file. `SET`, `DEL`, expiry changes, `INCR`, and batches commit directly
through `TenantKvStore`; the value and expiry-index effects share one redb write
transaction. A successful command does not mean that Nimbus appended a tenant
event or assigned a document commit sequence.

There are two current compositions:

- Standalone `nimbus kv --data <path>` opens `RedbTenantKvStore` directly.
- An embedder that calls `Engine::tenant_kv_*` reaches the loaded tenant
  runtime's flat-KV tables. Embedded redb and SQLite tenants implement this
  bridge. Each backend commits a value, metadata, and expiry change in one
  local transaction.

Neither composition enters the document mutation committer. Tenant-KV writes
do not acquire a committer lease, receive a document commit sequence, or append
to the tenant event journal. The local Engine process fence and redb's own
transaction serialization still protect their respective files; those guards
do not make the KV plane journal-visible.

Consequences:

- document PITR exports and journal replay do not contain tenant-KV writes;
- current replica and changefeed paths do not reproduce tenant-KV state;
- libSQL, PostgreSQL, MySQL, and memory tenant providers do not implement the
  Engine tenant-KV bridge;
- NKV0 has no supported backup/restore operator contract. A closed-file copy is
  not a substitute for the future NKV-DR contract;
- `--no-disk` remains intentionally volatile.

NKV-DR owns supported backup, restore, and point-in-time recovery. NKV6 owns
replication, distributed fencing, and cross-node ordering. Routing tenant-KV
writes through the document committer would change latency, ordering, recovery,
and provider semantics, so it requires an explicit NKV owner decision and a new
contract; do not infer that behavior from the shared `TenantStore` file.

## Conformance

Focused local commands:

```text
cargo test -p nimbus-kv
cargo build -p nimbus-bin --bin nimbus
REDISRS_SERVER_BIN=target/debug/nimbus cargo test -p nimbus-kv --test spawn_harness -- --ignored --nocapture
REDISRS_SERVER_BIN="$(pwd)/target/debug/nimbus" bash scripts/nimbus-kv-conformance.sh
```

The Valkey conformance runner pins Valkey to
`c9e8005e9d0ec817e26c7db318861cb821409249` (`9.1.0`) and runs
`unit/type/nimbus_kv_smoke` in external mode under RESP2 and RESP3. The skipfile
is `tests/nimbus-kv-skip.txt`; the NKV0 behavioral skip budget is 0. Later
phases add more `unit/type/<t>` slices by extending `NIMBUS_KV_VALKEY_SLICE` (or
the script default) and adding only issue-cited behavioral skips.

## Observability

`NIMBUS.READY` is the readiness probe. It is authenticated and distinct from
`PING`; a successful reply is:

```text
+READY
```

`NIMBUS.METRICS` returns a text snapshot with:

- `connected_clients`
- `cache_hits`
- `cache_misses`
- `cache_hit_ratio_ppm`
- `durable_writes_started`
- `durable_writes_completed`
- `durable_writes_in_flight`
- `durable_write_latency_us_total`
- per-command `command.<NAME>.calls`, `.errors`, and `.latency_us_total`

`durable_writes_in_flight` is the NKV0 write-depth signal. NKV0 still has a
single-writer durable path, so a rising in-flight count or write-latency total is
operator evidence of the known durable-tier ceiling rather than Redis cache-class
throughput.

## Compatibility Boundary

NKV0 is strings + expiry only. Collections, streams, transactions, Lua/functions,
ACLs, persistence administration, cluster, replication, eviction policy, and
Redis/Valkey command coverage outside the smoke slice are later NKV phases.
