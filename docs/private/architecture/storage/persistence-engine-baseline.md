# Persistence Engine Baseline

This document extends [ARCHITECTURE.md](../../../../ARCHITECTURE.md) with the
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

## Mutation Commit And Publication Baseline

The PPSC closeout establishes one engine-owned sequence authority per loaded
tenant. The three public write shapes remain the queued journal path, the
direct `apply_mutation_with_mode*` path, and `MutationExecutionUnit`; every
production backend selects the immutable ordered-publisher arm when the tenant
runtime is constructed. The serial reference arm is test-only and exists for
byte/state differential checks, not as a production rollback path.

Provider-backed writes validate the held committer lease `(owner_id, epoch)`
and expected durable head in the same transaction as document, index, journal,
schema, restore, trigger-cursor, or projection effects. Sequence-adjacent
internal state is not allowed to bypass that authority:

- schedule-only execution units validate the provider lease atomically with
  the complete scheduler batch even though they allocate no journal sequence;
- scheduler acknowledgement loss is reconciled against captured exact pre- and
  intended post-state, with mixed or unreadable state forcing tenant recovery;
- trigger-invocation begin-attempt, takeover claim, retry, completion, and
  terminal transitions are idempotent complete-record replacements serialized
  through the committer and fenced at the current durable head;
- trigger handlers remain at-least-once across crash/takeover; after a handler
  returns, only its already-computed record may be retried.

A conditional write carries its expected state to the commit authority
instead of deciding it against its own pre-read. `ObjectExpectedState` and
`ObjectUploadExpectedState` travel with the write, the committer evaluates them
against its own read inside the actor and before sequence assignment, and a
refused write takes no sequence, appends no journal record, and publishes
nothing. Adapters keep wire policy — S3 ETag syntax and the RFC 9110
strong/weak reduction — and no adapter or provider receives a raw
compare-and-swap escape hatch. Multipart metadata is fenced on the revision the
writer observed, so a stale completion publishes nothing.

Every storage writer declares its commit effects explicitly rather than
inheriting them by omission. One checked matrix covers the complete
`SqlStoreCore` writer set, and each row names admission, lease, condition,
document, index, version, catalog, scheduler, trigger, journal, watermark, and
outcome as a closed enum variant, with no `Default`, no `Option`, and no opaque
callback. A gate reads the trait source and fails on an unowned writer, a stale
row, a declaration the source contradicts, an outcome that does not match the
return type, or a row that declares nothing.

Publication exposes only a contiguous applied prefix. Assigned, durable,
applied, and published heads are monotonic, and a durable acknowledgement whose
visibility is unreadable is classified as ambiguous and recovered by durable
reload rather than guessed. Identical replay is a no-op; different-content
reuse of an applied sequence is corruption.

The 2026-07-23 U8 performance closeout measured the production publisher rather
than a test arm. Embedded SQLite at concurrency 256 sustained **20,132
mutations/second** (95% CI **[19,734, 20,531]**), **143.25 writes per durable
append**, and **16.3% fsync share**. The provider proof retained separate
loopback and +5 ms/direction RTT measurements: PostgreSQL measured **20.12 ms**
and **643.84 ms** per 32-operation round, while MySQL measured **20.10 ms** and
**553.52 ms**. PostgreSQL observed real statement overlap depth 2; MySQL
deliberately retained ordered depth 1. The corresponding required-fixture
lanes executed PostgreSQL **72/72**, MySQL **45/45**, and libSQL **49/49**
without provider skips. Raw samples and hashes live in
`docs/private/plans/research/concurrent-write-throughput-benchmark.md`.

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

SQLite, Postgres, and MySQL share one pure historical index scan planner in
`crates/nimbus-storage/src/index/history_scan.rs` for query shape, encoded
tuple bounds, cursor validation, and page finalization. Backend modules own only
their physical `index_versions` lookup and `document_versions` hydration.
SQL-family production roots stay below the repo's 1,500-line review threshold by
moving stable table-id catalog operations to `mysql/table_catalog.rs`,
document/index filtering and range helpers to SQL-family `query_helpers.rs`
modules, and Postgres schema-cache event shaping to `postgres/write_schema_events.rs`.

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
archive and its opaque target position before the first destination write.
They require an empty materialized base where appropriate, replay through the
durable journal path, and compare restored fingerprints.

Storage owns one canonical position for every materialized artifact.
`MaterializedPosition` carries the state version, the applied sequence, and a
state digest. Version 2 streams a domain-tagged logical codec directly into
SHA-256. The codec sorts every map key itself, gives finite and non-finite
floating-point classes explicit encodings, and consumes the same adapter-lowered
`StoredValue` tree used by persistence, semantic equality, and index-key
derivation. Cargo features, provider layout, insertion order, and equivalent
`Json`, `Map`, or `List` spellings cannot change the result. Snapshot and
bootstrap fingerprints, the shadow materializer manifest, and every
point-in-time restore import route compare that position. `durable_head` is
compared as its own field because it is a durability fact about the journal,
not a property of the materialized state. A sequence alone is not an identity:
two artifacts at the same applied sequence with different state must compare
unequal.

Repeated consistency checks use a separate, derived
`VerificationPosition`: root format version, exact applied sequence, and a
deterministic Merkle root over the same canonical logical leaves. The root is
process-local and disposable. It is not stored in a provider schema and never
replaces `MaterializedPosition` in an artifact, recovery, bootstrap, or PITR
contract. A verification session retains independent authoritative, shadow,
and embedded-replica indexes. Each writer publishes exact post-apply deltas or
invalidates the index before a later fast result can pass.

An incremental result compares those three roots at one applied sequence. It
also names the `MaterializedPosition` from its last full scrub. The result has
incremental assurance only. It proves contiguous tracked change since that
anchor, not a new read of all provider state.

Nimbus runs a full scrub for
cold start, restart, cache clear, or five minutes of idle time. It also scrubs
for a 15-minute anchor age, sequence gap or rewind, retention loss, index
invalidation, root mismatch, or an operator request.
The full scrub reads actual materialized state, rebuilds the disposable roots,
and detects same-sequence provider tamper that journal replay alone cannot see.
The registry bounds admission to 64 tenants and 256 MiB of total root-index
memory. Exhaustion can refuse the optional check but cannot block tenant reads
or writes.

CDC/changefeed uses typed `ChangefeedHandle`, `ChangefeedCursor`,
`ChangefeedBootstrap`, `ChangefeedPage`, and `ChangefeedEvent` over the same
tenant event journal. The initial snapshot cut and journal handoff are explicit
so consumers do not miss or duplicate events.

Durable journal streams expose a retained `cursor_floor`; cursors before that
floor fail closed as expired history. Nimbus persists separate document,
index, and journal read floors on memory, redb, SQLite, Postgres, MySQL, and
libSQL. Each page validates the relevant authoritative floor before and after
its read. A concurrent prune can return `RetentionExpired`, but it cannot
return a partial page, a sequence gap, or a missing record as an empty logical
event.

Retention GC computes separate safe watermarks for document versions, index
versions, registry metadata, read-policy metadata, CDC, PITR exports, shadow
materializers, embedded replicas, and transaction sessions. Document pruning
preserves the latest anchor at or before the safe floor for each document;
index pruning removes only closed intervals whose `visible_until` is safe.

A `MaterializedRetentionCheckpoint` binds a materialized snapshot, applied and
durable heads, timestamp, `MaterializedPosition`, and checkpoint digest. The
maintenance transaction validates the prior checkpoint and current authority,
publishes the next checkpoint and read floors, prunes the journal through that
checkpoint, and compacts eligible MVCC rows atomically. Desired, confirmed,
and physical floors remain distinct operator-visible state. A fault can retain
extra history, but it cannot publish a floor without its rebuild base.

The Engine runs one bounded, single-flight controller for each loaded tenant.
The shipped profile retains 100,000 document-version, index-version, and PITR
sequences, 50,000 CDC sequences, and becomes eligible after 10,000 new applied
sequences. It prepares the checkpoint off the mutation path and finalizes it
through the existing ordered maintenance route. Embedded stores use the
process fence; provider stores validate the current committer lease inside the
maintenance transaction. `retain-all` is an explicit operator profile.

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

`StorageCapabilities` also publishes a `semantic_contract` profile per store
type, so an operator reads what a backend guarantees without probing a live
tenant. A closed matrix qualifies every provider against atomic effects,
committer fencing, conditional admission, journal progress, durable recovery,
write isolation, and position parity. Each cell either names the test that
qualifies it or declares the guarantee not owned, and "not owned" is checked
against the provider registrations rather than accepted. A lane whose cargo
feature is disabled or whose fixture is absent reports `UNVERIFIED`; it never
reports qualified, and the shared conformance scenarios run one body per
dimension across providers rather than one paraphrase each.

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
suffix and compacts only when explicit journal state crosses the configured
threshold. Its versioned manifest stores a `checkpoint_position`, not a
sequence alone, so recovery rejects a checkpoint that diverges from the state
the same sequence should carry.

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
- External epoch lineages, journal-format seals, and reader-first format
  rollout are not implemented here. They are binding prerequisites owned by
  `docs/private/plans/horizontal-scaling-plan.md`, and they activate when a
  second external artifact consumer does. Current point-in-time restore already
  fails closed on exact versions, so nothing in the local baseline depends on
  them.

## Related Docs

- [ARCHITECTURE.md](../../../../ARCHITECTURE.md)
- [Time and ordering](../time-and-ordering.md)
- [Verification runbook](../../operating/verification.md)
- [Storage integrity contracts archive](../../plans/archive/storage-integrity-contracts-plan.md)
