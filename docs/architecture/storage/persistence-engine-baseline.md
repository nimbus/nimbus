# Persistence Engine Baseline

This document extends [ARCHITECTURE.md](../../ARCHITECTURE.md) with the
current persistence-engine baseline. Keep the high-level crate map and repo
invariants in `ARCHITECTURE.md`; use this reference when work needs the
current backend layouts, durable-journal contract, serving-snapshot direction,
or the settled persistence-specific design decisions.

## Current Baseline

- SQLite is the default embedded tenant provider.
- redb remains a supported embedded tenant provider during the provider-model
  transition.
- Postgres, MySQL, and replica-connected SQLite preserve the same
  engine-visible behavior behind provider-owned seams.
- The cross-tenant usage and control database remains local and redb-backed
  today.
- `DurableMutationRecord` is the authoritative per-tenant ordered history.
- Serving reads still come from applied materialized state rather than from a
  journal-overlay path.

## Backend Layouts

### SQLite tenant layout

Each SQLite tenant database keeps documents as JSON at rest, durable journal
rows as serialized `DurableMutationRecord` blobs, and scheduler or metadata
state in relational tables:

| Table | Columns | Purpose |
| --- | --- | --- |
| `documents` | `table_name`, `id`, `data_json`, `creation_time` | Primary document store with JSON-at-rest payloads |
| `schemas` | `table_name`, `schema_json` | Per-table schema definitions |
| `scheduled_jobs` | `id`, `data_json` | Pending scheduled mutations |
| `running_scheduled_jobs` | `id`, `data_json` | In-flight jobs for crash recovery |
| `scheduled_job_results` | `job_id`, `data_json` | Execution outcomes |
| `scheduled_job_executions` | `execution_id` | Dedup guard for scheduled execution ids |
| `cron_jobs` | `name`, `data_json` | Recurring job definitions |
| `commit_log` | `sequence`, `record_blob` | Append-only durable mutation journal |
| `metadata` | `key`, `value_blob` | Applied head and related per-tenant metadata |

SQLite expression indexes are derived from table schema definitions and own the
physical indexed-read path.

### redb tenant layout

The retained embedded redb tenant file keeps key-value tables for documents,
indexes, schemas, the durable journal, scheduler state, and metadata:

| Table | Key | Value | Purpose |
| --- | --- | --- | --- |
| `DOCUMENTS` | `table\0doc_id` | msgpack(Document) | Primary document store |
| `INDEXES` | `table\0idx\0encoded_val+doc_id` | empty | Secondary index entries |
| `SCHEMAS` | `table_name` | msgpack(TableSchema) | Per-table schema definitions |
| `COMMIT_LOG` | `sequence (u64)` | msgpack(DurableMutationRecord) | Append-only durable mutation journal |
| `METADATA` | `"next_sequence"` / `"applied_sequence"` | `u64` | Durable-sequence and applied-head tracking |
| `SCHEDULED_JOBS` | `run_at(8B)+job_id(16B)` | msgpack(ScheduledJob) | Pending scheduled mutations |
| `RUNNING_SCHEDULED_JOBS` | `job_id(16B)` | msgpack(ScheduledJob) | In-flight jobs for crash recovery |
| `SCHEDULED_JOB_RESULTS` | `job_id(16B)` | msgpack(Result) | Execution outcomes |
| `SCHEDULED_JOB_EXECUTIONS` | `job_id(16B)` | empty | Dedup guard for crash-replayed jobs |
| `CRON_JOBS` | `cron_name` | msgpack(CronJob) | Recurring job definitions |

The global `nimbus-control.db` remains redb-backed and local today and contains
three tables for MAU tracking:

| Table | Key | Value | Purpose |
| --- | --- | --- | --- |
| `monthly_active_identities` | `month_prefix\0token_id` | empty | Per-identity dedup |
| `monthly_active_counts` | `month_start_unix_ms (u64)` | msgpack(count) | Monthly counters |
| `monthly_active_last_recorded` | `month_start_unix_ms (u64)` | msgpack(timestamp) | Last-seen timestamps |

The first Postgres-first non-local activation stays tenant-scoped, so this
cross-tenant usage and control database remains local and redb-backed for that
slice.

## Query Planning

The engine planner chooses the semantic path, then hands physical execution to
the backend-specific read layer:

1. Exact equality on an indexed field uses the exact-index path with residual
   filters.
2. Range filters on an indexed field use the range-index path with residual
   filters.
3. Everything else falls back to a full table scan.

SQLite executes the physical read path through parameterized SQL plus
expression indexes. redb executes the physical read path through encoded
secondary-index key scans. Residual semantics, auth, and final query meaning
stay in Nimbus.

## Durable Journal Baseline

### Why the durable journal is Nimbus-owned

Nimbus does not treat the durable journal as a generic storage-engine WAL
substitute. The authoritative journal is a Nimbus-defined logical ordered
history built above backend internals because the reactive architecture needs:

- logical mutation records rather than page-level recovery entries
- the same ordered history for replay, dependency-aware invalidation, CDC, and
  future replica consumers
- freedom to change materializers later without redefining the
  application-level durability contract

Document and index tables remain an applied materialized view maintained from
that history, with `applied_sequence` defining the serving boundary between
what is already materialized and what still lives only in the journal tail.

### Bootstrap and replay contract

Bootstrap is snapshot plus the same ordered stream, not a separate export
format. A downstream consumer restores a materialized snapshot, resumes after
the snapshot's applied sequence, and replays journal records through the
bootstrap cut. If newer writes arrive during catch-up, they remain part of the
same ordered stream.

Materialized snapshot boundaries also record the applied sequence they include
and the durable head observed at export time so rebuild can reject an
incomplete journal tail loudly instead of silently reconstructing only the
applied prefix.

### Read visibility

Committed does not immediately mean read-visible. The durable journal defines
commit order and durability, while serving reads still come from applied
materialized state. Async mutations acknowledge after the durable append, but
reads, subscriptions, and cache publication wait for
`applied_sequence >= required_sequence` instead of overlaying journal-only
records into point reads, scans, subscriptions, or cache lookups.

## Replica and Serving Baseline

### Embedded replica scope

`EmbeddedReplica` is a validated architectural path, but it is not the default
serving path. It bootstraps from the same snapshot-plus-stream contract,
applies the authoritative journal into a local materialized store, and
evaluates queries or pagination locally against that store.

Replica catch-up also refreshes schema state even when there are no new
durable mutation records, and replica-local evaluation reuses the same
schema- and principal-aware planning helpers as the live service.

### Server-side serving promotion

The near-term production path still keeps writes, subscription re-evaluation,
and pushed results on the main server. Promoted serving reads now reuse an
explicit serving layer for warmed full-scan tables and the read shapes that can
prove parity against the authoritative path.

The canonical next abstraction is a versioned `ServingSnapshotManager`, not a
bigger cache. The current in-memory warmed-table implementation is treated as
the first backend for that abstraction, and future serving backends should
reuse the same manager-facing contract instead of growing new read paths ad
hoc.

### Shadow materializer posture

The first custom materializer remains shadow-only and checkpoint-driven. It
rebuilds from an explicit `MaterializedJournalSnapshot` plus a durable-journal
suffix, tracks checkpoint and current sequence in a versioned manifest, and
compacts only when explicit journal state crosses the configured threshold.

redb remains the serving oracle while the materializer proves parity. Promotion
onto any live serving path requires replay, corruption, interruption, and
shadow-parity evidence rather than benchmark-only confidence.

### Format guidance

The current measured guidance is to promote materialized reads before inventing
a new binary format. If Nimbus needs another major read-path gain, it should
first promote more serving paths onto existing materialized-document surfaces
such as the serving snapshot layer or embedded replica. A new on-disk or
zero-copy format should only be revisited if those promotions still leave
MessagePack decode as the dominant measured cost.

## Persistence-Specific Design Decisions

### Why SQLite is the default embedded backend

SQLite provides transactions, WAL durability, physical query execution,
JSON-at-rest documents, and expression indexes without forcing the engine to
keep redb-specific physical scan or key-encoding machinery as the default
shape. redb can remain supported as long as the engine-visible seam is no
longer redb-shaped.

### Why usage and control state stays separate

MAU tracking and other cross-tenant usage or control data are global rather
than tenant-scoped, so they remain in a dedicated local `nimbus-control.db`
managed separately from tenant lifecycle. That is also why the first
Postgres-first non-local activation remains tenant-scoped: the cross-tenant
usage and control path keeps its own design and rollout boundary.

### Explicit non-decisions

- OpenRaft is not the local journal implementation.
- Fjall, RocksDB, or another LSM engine are not substitutions for the current
  durable-journal contract.
- A thin generic append-only log crate is not enough on its own because Nimbus
  needs logical replay payloads, dependency metadata, visibility rules, and
  tenant-scoped recovery semantics.

## Related Docs

- [ARCHITECTURE.md](../../ARCHITECTURE.md)
- [Provider topology reference](provider-topologies.md)
- [Versioned serving snapshot design note](../research/versioned-serving-snapshot-design-note.md)
