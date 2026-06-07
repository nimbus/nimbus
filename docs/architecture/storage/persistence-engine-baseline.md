# Persistence Engine Baseline

This document extends [ARCHITECTURE.md](../../../ARCHITECTURE.md) with the
current persistence-engine baseline. Keep the high-level crate map and repo
invariants in `ARCHITECTURE.md`; use this reference when work needs the
current backend layouts, MVCC versioning contract, durable-journal contract,
serving-snapshot direction, diagnostics, or the settled persistence-specific
design decisions.

## Current Baseline

- SQLite is the default embedded tenant provider.
- redb remains a supported embedded tenant provider during the provider-model
  transition.
- Postgres, MySQL, and replica-connected SQLite preserve the same
  engine-visible behavior behind provider-owned seams.
- The cross-tenant usage and control database remains local and redb-backed
  today.
- The tenant event journal is the authoritative per-tenant ordered history.
  `TenantEventRecord` carries document, schema, table lifecycle, index
  lifecycle, scheduler, trigger-delivery, and barrier events.
- Current serving reads still come from applied materialized state rather than
  from a journal-overlay path.
- Historical reads use versioned document rows, versioned index intervals, and
  a resolved `HistoricalReadShape` that carries table, schema, index, and
  read-policy identity as of the requested snapshot.
- CDC/changefeed and PITR use the same tenant event journal instead of a
  second log.
- Retention GC prunes only past safe per-resource watermarks and preserves
  required document anchors and active pins.

## Backend Layouts

### SQLite tenant layout

Each SQLite tenant database keeps documents as JSON at rest, durable journal
rows as serialized `TenantEventRecord` blobs, and scheduler or metadata
state in relational tables:

| Table | Columns | Purpose |
| --- | --- | --- |
| `table_catalog` | `namespace`, `table_name`, `table_id` | Stable logical table identity catalog |
| `documents` | `table_id`, `id`, `data_json`, `typed_fields_json`, `creation_time`, `update_time` | Primary document store with JSON-at-rest payloads keyed by stable table identity |
| `document_versions` | `table_id`, `id`, `visible_at_sequence`, `visible_at_timestamp`, `data_json`, `tombstone`, `storage_format` | Versioned document history for latest-at-or-before historical point reads |
| `index_versions` | `table_id`, `index_name`, encoded tuple/range keys, `document_id`, `visible_from`, `visible_until`, `storage_format` | Versioned index intervals for historical equality, range, prefix, and cursor-bound pages |
| `schemas` | `table_name`, `schema_json` | Per-table schema definitions |
| `scheduled_jobs` | `id`, `data_json` | Pending scheduled mutations |
| `running_scheduled_jobs` | `id`, `data_json` | In-flight jobs for crash recovery |
| `scheduled_job_results` | `job_id`, `data_json` | Execution outcomes |
| `scheduled_job_executions` | `execution_id` | Dedup guard for scheduled execution ids |
| `cron_jobs` | `name`, `data_json` | Recurring job definitions |
| `commit_log` | `sequence`, `record_blob` | Append-only tenant event journal |
| `metadata` | `key`, `value_blob` | Applied head and related per-tenant metadata |

SQLite expression indexes are derived from table schema definitions and own the
physical indexed-read path.

### redb tenant layout

The retained embedded redb tenant file keeps key-value tables for documents,
indexes, schemas, the durable journal, scheduler state, and metadata:

| Table | Key | Value | Purpose |
| --- | --- | --- | --- |
| `TABLE_CATALOG` | `namespace\0table_name` | `table_id` | Stable logical table identity catalog |
| `DOCUMENTS` | `table_id\0doc_id` | msgpack(Document) | Primary document store keyed by stable table identity |
| `DOCUMENT_VERSIONS` | `table_id\0doc_id\0sequence` | msgpack(version row) | Versioned document history for historical point reads |
| `INDEXES` | `table_id\0idx\0encoded_val+doc_id` | empty | Secondary index entries keyed by stable table identity |
| `INDEX_VERSIONS` | `table_id\0idx\0encoded_tuple\0doc_id\0sequence` | msgpack(visibility interval) | Versioned secondary-index history for historical index reads and pagination |
| `SCHEMAS` | `table_name` | msgpack(TableSchema) | Per-table schema definitions |
| `COMMIT_LOG` | `sequence (u64)` | msgpack(TenantEventRecord) | Append-only tenant event journal |
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

### External provider history layout

Postgres, MySQL, and libSQL own the same logical history families behind
provider-specific DDL:

- latest `documents` rows remain the current-read fast path
- `document_versions` rows store insert, update, and tombstone visibility with
  durable format gates
- `index_versions` rows store open and closed visibility intervals for
  historical index scans
- `commit_log` remains the tenant event journal used for durable recovery,
  CDC/changefeed, PITR, and materializer catch-up

The SQL-family implementations keep the same Nimbus-visible semantics while
choosing backend-appropriate keys and indexes. MySQL uses hash-assisted encoded
tuple keys where needed for ordered/indexed lookup constraints. libSQL keeps
remote primary rows and refreshes the local SQLite replica cache before
historical index reads.

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

Historical planning starts by resolving a `HistoricalReadShape` from the
versioned registry as of the requested snapshot. Point reads then use
`document_versions` latest-at-or-before lookup. Indexed historical reads use
`index_versions` visibility intervals plus `document_versions` to reconstruct
the document visible at the same retained sequence. Historical cursors are
bound to the read shape, index identity, bounds, and retained sequence; a cursor
from another shape fails closed instead of being reinterpreted.

## Tenant Event Journal Baseline

### Why the tenant event journal is Nimbus-owned

Nimbus does not treat the tenant event journal as a generic storage-engine WAL
substitute. The authoritative journal is a Nimbus-defined logical ordered
history built above backend internals because the reactive architecture needs:

- logical tenant event records rather than page-level recovery entries
- the same ordered history for replay, dependency-aware invalidation, CDC, and
  future replica consumers
- freedom to change materializers later without redefining the
  application-level durability contract

Document, index, schema, table lifecycle, scheduler, trigger-delivery, and
diagnostic metadata tables remain applied materialized views maintained from
that history. `applied_sequence` defines the serving boundary between what is
already materialized and what still lives only in the journal tail.

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

### Retention floor and destructive cleanup

Hard delete is retention-gated. `RetentionFloor` tracks participants that pin a
table identity or event sequence, including transaction sessions, exported
snapshots, journal consumers, embedded replicas, shadow materializers, and
CDC/subscription consumers. A backend may physically remove a deleting table
only after the retention floor proves no retained reader or consumer can still
reference that table identity.

### Read visibility

Committed does not immediately mean read-visible. The durable journal defines
commit order and durability, while serving reads still come from applied
materialized state. Async mutations acknowledge after the durable append, but
reads, subscriptions, and cache publication wait for
`applied_sequence >= required_sequence` instead of overlaying journal-only
records into point reads, scans, subscriptions, or cache lookups.

`ReadVisibility`, `RequiredSequence`, `PinnedServingSnapshot`, and
`PinnedServingReadSnapshot` are the typed boundary for this current latest-row
posture.

### MVCC, PITR, CDC, and retention contract

Nimbus keeps latest-row storage for the serving path and adds explicit
version-history storage for enterprise history features. A committed write
records the document write, index effects, version rows, and tenant event in
one storage transaction. A backend must not expose a document write without the
matching index/version/journal effects.

Historical reads are admitted only when the requested sequence or timestamp is
inside the retained history window and the backend can validate the relevant
storage-format markers. Expired history, unsupported adapters, unsupported
backends, format mismatches, missing policy snapshots, unavailable serving
snapshots, and cursor mismatches surface as typed `HistoricalReadErrorKind`
fail-closed errors.

PITR export/import uses a typed `PointInTimeRestoreArchive` with canonical
fingerprints and sequence or timestamp targets. Restore imports validate the
archive, require an empty materialized base where appropriate, replay through
the durable journal path, and compare restored fingerprints.

CDC/changefeed uses typed `ChangefeedHandle`, `ChangefeedCursor`,
`ChangefeedBootstrap`, `ChangefeedPage`, and `ChangefeedEvent` over the same
tenant event journal. The initial snapshot cut and journal handoff are explicit
so consumers do not miss or duplicate events.

Retention GC computes separate safe watermarks for document versions, index
versions, registry metadata, read-policy metadata, CDC, PITR exports, shadow
materializers, embedded replicas, and transaction sessions. Document pruning
preserves the latest anchor at or before the safe floor for each document;
index pruning removes only closed intervals whose `visible_until` is safe.

### Diagnostics and format gates

Every backend exposes `StorageCapabilities` and `StorageHealthDiagnostic`.
The diagnostic reports backend layout, event-log head, applied head, retention
floor, storage format version, document-version and index-version counts and
ranges, MVCC operator state, historical-query admission, retention pressure,
backend capability profile, backend feature support, adapter capability
profiles, adapter support, backend-parity state, encryption posture, freshness
lag, last recovery status, and exact-summary support. The profile is derived
from the detailed feature matrix (`latest_only`, `historical_reads`,
`historical_reads_pitr`, `historical_reads_pitr_cdc`, or
`enterprise_complete`) and never replaces typed per-feature unsupported errors.

`StorageFormatVersion` is explicit and unknown future versions fail closed
through startup validation rather than being treated as best-effort metadata.
Document-version and index-version storage formats are separately marked and
validated, so a backend can reject unknown old or future history layouts before
serving historical reads.

`storage_health_diagnostic_with_retention_config(...)` lets operators inspect
pressure under a proposed retention window without mutating storage. Backend
parity diagnostics compare operator-visible heads, ranges, and version counts
so regression checks can report divergence without comparing physical database
files byte-for-byte.

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

The serving manager now exposes `PinnedServingReadSnapshot` for read-shape
aware historical serving checks. The pinned handle carries the resolved
`HistoricalReadShape`, stable table/index/read-snapshot identity, and coverage
validation; if the serving snapshot does not cover the requested retained
sequence or table, reads fail closed with `SnapshotUnavailable`.

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

- [ARCHITECTURE.md](../../../ARCHITECTURE.md)
- [Storage backends operating guide](../../operating/storage-backends.md)
- [Provider topology reference](provider-topologies.md)
- [Versioned serving snapshot design note](../../plans/research/versioned-serving-snapshot-design-note.md)
