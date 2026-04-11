# SQLite Storage Backend Migration Control Plan

This file keeps its historical filename, but it now owns a SQLite-first
embedded-provider migration: move Neovex internal storage from a redb-only
implementation to a provider model with SQLite as the default embedded backend,
benchmark SQLite against redb before cutover, and retain redb as a supported
embedded provider. Future replica-connected SQLite, Postgres/MySQL, and other
non-local provider work belongs to
`docs/plans/archive/external-sql-storage-backends-plan.md`.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `docs/plans/encryption-at-rest-plan.md`
- `crates/neovex-storage/src/async_storage/traits.rs`
- `crates/neovex-storage/src/store/write/direct.rs`
- `crates/neovex-storage/src/store/write/batch.rs`
- `crates/neovex-storage/src/store/journal_stream.rs`
- `crates/neovex-storage/src/store/journal_snapshot.rs`
- `crates/neovex-storage/src/index/`
- `crates/neovex-storage/src/keys.rs`
- `crates/neovex-storage/src/store/scan.rs`
- `crates/neovex-storage/src/schema_store.rs`
- `crates/neovex-storage/src/store/schema_rewrite.rs`
- `crates/neovex-engine/src/service/execution_units/commit.rs`
- `crates/neovex-engine/src/service/mutations/commit_processing.rs`
- `crates/neovex-engine/src/scheduler.rs`
- `crates/neovex-engine/src/service/queries/planner/`

Baseline verification status for this plan:

- this is a docs-only review and planning pass authored on 2026-04-08
- the original planning baseline on 2026-04-08 was redb-only; the live
  worktree now includes SQLite through `SB9`
- no new workspace-wide verification is claimed by this planning pass
- every `SB*` item must record its own focused verification before it can be
  marked `done`

---

## Status

- **Status:** `active`
- **Primary owner:** this plan
- **Activation gate:** met on 2026-04-08; the SQLite migration may proceed
  while redb-specific encryption-at-rest work is deferred or rewritten as a
  follow-on

---

## Purpose

Neovex is still pre-launch with zero production data, which makes this the
right moment to replace the current redb-only storage implementation cleanly
instead of carrying long-lived compatibility layers. The goal of this
workstream is to preserve Neovex's current product semantics while making
SQLite the default embedded backend and retaining redb as a supported embedded
provider behind the same durable behavior seam.

This plan exists to keep that migration disciplined. It should prevent two
failure modes:

- preserving redb-shaped shared seams just because redb remains supported,
  instead of pushing backend-local mechanics behind the provider boundary
- treating Neovex-owned product behavior as if SQLite already solved it just
  because SQLite has lower-level storage hooks or replication features

---

## Relationship To Other Plans

- Use `docs/plans/README.md` as the owning plan index.
- This plan is separate from `docs/plans/encryption-at-rest-plan.md`, which is
  currently redb-specific and must be sequenced explicitly before activation.
- Future replica-connected SQLite, Postgres/MySQL, and broader networked or
  coordinated provider work belongs to
  `docs/plans/archive/external-sql-storage-backends-plan.md`, not this plan.
- This plan owns the durable embedded-provider seam only far enough to support
  retained local providers (`EmbeddedSqliteProvider` and
  `EmbeddedRedbProvider`) after cutover. Non-local provider modes and future
  replication or coordination designs must not reshape `SB10` through `SB14`
  here.
- If work turns into a product-level redesign of journaling, replication, or
  reactivity semantics, stop and move that scope into the owning follow-on plan
  instead of stretching this migration plan across multiple workstreams.

---

## Scope

This plan covers:

- SQLite as the default target embedded backend for Neovex internal storage
- redb retained as a supported embedded provider behind the durable provider
  seam
- a temporary redb-vs-SQLite migration window for parity tests and benchmarks
- a benchmark gate before changing the default embedded backend
- preserving the current engine-facing storage contract while swapping the
  backend implementation
- promoting migration-only backend-selection scaffolding into durable embedded
  provider naming and typed config

This plan does not cover:

- replica-connected SQLite or networked SQL deployments
- Postgres or MySQL internal storage
- concrete horizontal-replication or coordination implementations for embedded
  providers (`raft`, `etcd`, or similar)
- user-facing `env.DB` / `env.HYPERDRIVE` bindings
- a long-lived compatibility layer or migration shim for launched users

Because Neovex is pre-launch, this plan prefers clean replacement over
compatibility scaffolding. A redb-to-SQLite import tool is optional and only
worth adding if local developer migration friction becomes real.

---

## Migration Invariants

These rules are mandatory for every item in this plan.

1. Preserve engine-visible behavior by default.
   Async read/write execution semantics, `TenantWriteCommit<T>` /
   `TenantWriteOutcome<T>`, validated direct writes, execution-unit batch
   application, scheduled execution dedupe, `CommitEntry`, journal/bootstrap/
   snapshot behavior, and recovery semantics stay unchanged unless a specific
   item explicitly records otherwise.

2. Keep the core architecture invariants intact.
   `neovex-core` stays zero I/O.
   `neovex-runtime` stays zero workspace dependencies.
   All mutations still flow through `Service::apply_mutation`.
   Storage atomicity remains intact.

3. Use SQLite for storage mechanics; keep Neovex-owned product semantics in
   Neovex.
   SQLite should own transactions, WAL durability, table/index storage, JSON
   expressions, and physical query execution. Neovex should continue to own
   logical commits, durable journal semantics, subscription fan-out, scheduler
   dedupe semantics, validation/policy rules, and execution-unit semantics.

4. Prefer deleting shared redb-shaped seams over wrapping them.
   When SQLite replaces a redb-era shared mechanism, delete that shared module
   or push the remaining redb-specific mechanics behind a provider-local
   boundary instead of preserving it as the canonical seam.

5. Keep temporary migration scaffolding temporary, but promote the durable
   provider seam.
   Any redb-vs-SQLite selection layer that only exists to bridge parity and
   benchmarking must be removed or renamed into the durable provider model
   before this plan is closed.

6. Do not stabilize a filesystem-shaped backend factory seam.
   Any temporary selection or construction seam used during the migration must
   stay explicitly temporary and must not define a durable API around tenant or
   usage filesystem paths. Embedded SQLite path handling is acceptable for this
   migration, but it is not the long-term cross-backend construction contract.

7. Benchmark before default-switch.
   The SQLite default switch requires a checked-in benchmark report and an
   explicit go/no-go decision.
   Before `SB10` resumes, the benchmark gate must also include alternating
   backend order, a larger measured sample set, separate steady-state versus
   cold-start lanes, and SQLite query-plan evidence for the indexed workloads
   so the cutover decision is not resting on a too-thin microbenchmark pass.

8. Treat the current git worktree as baseline reality.
   Resume from the code and this plan's ledger, not from chat memory.

9. Distinguish provider dialect from deployment mode.
   The durable provider seam must not assume that `sqlite` always means a local
   file or that `redb` must remain single-node forever. Local embedded,
   replica-connected, and future coordinated-provider modes must fit as typed
   provider configuration without changing `TenantPersistence`.

---

## Current Assessed State

- Neovex is pre-launch with zero production users and zero production data, so
  breaking changes and direct replacements are preferred over compatibility
  layers.
- SQLite is now the default embedded provider, redb is retained as an explicit
  embedded provider, and the stable engine-facing seam is expressed through
  `TenantPersistence` / `PersistenceProvider` rather than migration-only
  `Backend*` vocabulary.
- The async storage boundary already exists via `EmbeddedPersistenceProvider`,
  `TenantReadStorage`, `TenantWriteStorage`, and `UsageStorage`, but it still
  leaks concrete sync types through closures.
- The write path depends on `TenantWriteCommit<T>` and `TenantWriteOutcome<T>`,
  including pre-commit cancellation behavior.
- Direct mutation flows depend on validated write callbacks rather than simple
  CRUD-only helpers.
- Execution units commit through `ResolvedWrite` and `ResolvedScheduleOp` batch
  application.
- Query planning and execution are still shaped around redb-specific point,
  prefix, range, and composite scan surfaces.
- The reactive system is driven by engine-owned `CommitEntry` values rather
  than backend hooks.
- Durable journal streaming, bootstrap export, materialized snapshot
  export/restore/rebuild, and durable/applied head tracking are already
  first-class storage responsibilities.
- Scheduled execution deduplication is keyed by execution id
  (`scheduled:{job.id}`), not by function name.
- redb-specific implementation code still occupies a large surface area in
  custom index, key-encoding, filter-pushdown, schema-rewrite, and planner
  plumbing modules.
- `docs/plans/encryption-at-rest-plan.md` is still written around redb's
  `StorageBackend` seam and must be reconciled before this migration can start.

---

## Current Review Findings

1. The previous CRUD-style trait sketch did not preserve the real current
   engine/storage contract.
   The migration boundary must be derived from actual call sites, not from
   idealized CRUD interfaces or premature object-safe abstractions.

2. Backend-hook-driven reactivity is the wrong canonical seam.
   The engine already owns logical commit processing through `CommitEntry`, and
   SQLite hooks are lower-level observability signals rather than the product
   contract.

3. Journal streaming, bootstrap, snapshot export/restore/rebuild, durable-head
   tracking, and scheduled execution dedupe remain required after SQLite.
   They are Neovex semantics, not redb implementation accidents.

4. SQLite should delete or backend-localize large redb-era shared storage code
   instead of wrapping it.
   `crates/neovex-storage/src/index/`, `keys.rs`, `store/scan.rs`, and
   `store/schema_rewrite.rs` must stop defining the shared canonical seam,
   while `schema_store.rs` and the engine planner should shrink materially.

5. The query planner should be simplified toward SQL generation and residual
   Neovex semantics, not preserved as a redb scan-shape chooser with SQLite
   adapters underneath.

6. Documents should move to JSON at rest in SQLite, while durable journal blobs
   stay serialized `DurableMutationRecord` values.
   SQLite sequence allocation should replace redb-style `next_sequence`
   bookkeeping unless parity proves otherwise.

7. SQLite should become the default embedded provider, but redb may remain as a
   supported embedded provider if the durable seam is no longer redb-shaped.
   Postgres/MySQL, replica-connected SQLite, and broader coordination or
   replication designs belong to separate follow-on planning.

8. The migration seam must not become a path-only provider seam.
   A path-shaped constructor is acceptable for local embedded providers during
   the migration window, but the durable provider contract must distinguish
   backend dialect from deployment mode and must leave room for non-local and
   coordinated providers later.

---

## Success Criteria

This plan is successful only when all of the following are true:

- SQLite matches the current engine-visible storage contract across reads,
  writes, scheduler flows, `CommitEntry`, journal APIs, snapshot/rebuild flows,
  and recovery semantics.
- Composite indexed query behavior remains intact, but the physical execution
  path is SQLite-native rather than redb-key encoded.
- The benchmark report is checked in and justifies making SQLite the default
  embedded provider.
- The migration-only selection layer is promoted into durable provider naming
  and typed config, while redb remains available as a secondary embedded
  provider.
- The shared engine/storage seam is no longer redb-shaped even though redb
  remains supported underneath it.
- Docs, plan index ownership, and cross-plan notes reflect SQLite as the
  default embedded provider, redb as a retained embedded provider, and
  non-local provider work as deferred follow-on scope.

---

## Feature Preservation Matrix

- Async read execution via `execute(...)` and `execute_cancellable(...)` must
  remain unchanged.
- Async write execution via `execute_write(...)` and
  `execute_write_cancellable(...)` must remain unchanged.
- `TenantWriteCommit<T>` and `TenantWriteOutcome<T>` semantics, including
  pre-commit cancellation behavior, must remain unchanged.
- Direct validated writes for insert/update/delete flows must remain unchanged.
- Execution-unit batch application over `ResolvedWrite` and
  `ResolvedScheduleOp` must remain unchanged.
- Scheduled execution dedupe must remain keyed by execution id rather than
  function name.
- `CommitEntry` generation and engine `process_commit(...)` fan-out semantics
  must remain unchanged.
- Durable journal read/stream/bootstrap APIs and materialized snapshot
  export/restore/rebuild semantics must remain unchanged unless a later plan
  explicitly owns a product-level change.
- Storage atomicity must remain unchanged: document writes, supporting
  metadata/index effects, and durable commit recording still commit together.
- Existing query semantics, including exact, prefix, range, and composite index
  behavior, must remain unchanged even though SQLite takes over the physical
  execution strategy.

---

## Control Plane Rules

This document is the durable control plane for the SQLite migration
workstream. The source of truth is:

1. the current git worktree
2. this plan's `Roadmap Status Ledger`, `Implementation Checkpoints`, and
   `Execution Log`
3. `ARCHITECTURE.md` and `AGENTS.md` for architectural invariants
4. the referenced code, tests, and docs called out by the active item

Do not rely on prior chat transcripts as progress state.

### Status model

- `todo`: not started; eligible when hard dependencies and gate notes are met
- `in_progress`: actively being implemented; keep exactly one `SB*` item in
  this state per autonomous execution run
- `blocked`: cannot proceed until the recorded blocker is resolved
- `done`: acceptance criteria are met and verification has been recorded
- `deferred`: intentionally parked behind a recorded gate

### Recovery loop for every new session

1. Reread `Migration Invariants`, `Current Assessed State`,
   `Current Review Findings`, `Feature Preservation Matrix`,
   `Verification Contract`, `Roadmap Status Ledger`,
   `Implementation Checkpoints`, `Dependency Graph`,
   `Recommended Delivery Order`, and `Execution Log`.
2. Inspect the current git worktree and reconcile it against this plan before
   picking new scope.
3. If any item is already `in_progress`, resume that item first.
4. If the worktree is dirty, identify which item owns the changes and update
   that item's checkpoint or log entry before starting new work.
5. Implement exactly one `SB*` item by default.
6. Record verification in `Execution Log` before marking an item `done`.
7. If blocked, record the blocker here before stopping.

---

## Canonical Design Decisions

### Use SQLite vs Keep in Neovex

Use this rule throughout the migration:

- preserve Neovex semantics
- replace Neovex mechanics whenever SQLite already provides an equivalent or
  better primitive

SQLite should replace current implementation details where it already gives us
the right storage primitive:

- transactions and atomic commit
- WAL durability and concurrency behavior
- table storage for documents, schemas, scheduler state, metadata, and
  commit-log rows
- expression indexes, including composite indexes
- query planning for point lookups and indexed scans
- prepared statements and normal SQL read/write execution

Neovex should continue to own the parts that are product semantics rather than
storage mechanics:

- logical mutation commits via `CommitEntry`
- durable journal cursor, bootstrap, and rebuild semantics
- subscription fan-out behavior
- scheduled execution dedupe semantics
- schema validation, auth, and policy semantics
- execution-unit conflict and dependency behavior
- durable-head vs applied-head tracking while the engine still depends on it

This plan should aggressively delete redb-shaped shared implementation seams,
but it should not treat current Neovex product guarantees as accidental
storage details just because SQLite has lower-level hooks or replication
features.

### Temporary migration selection

During the migration window, temporary backend selection is acceptable because
it is bounded and short-lived. The goal is to compare redb and SQLite on the
same engine contract, then promote the surviving shape into a durable embedded
provider seam. The goal is not to ship a kitchen-sink generalized abstraction
for every future deployment mode in this workstream.

Use an explicitly temporary selection layer rather than a permanent
object-safety-first backend factory. If a constructor seam is needed during the
migration, it must not hard-code tenant or usage filesystem paths as the
durable cross-provider shape. By the end of this plan, the migration-only names
should give way to a durable local embedded provider seam. Future non-local or
replicated provider construction/config still belongs to
`docs/plans/archive/external-sql-storage-backends-plan.md`.

### Durable seam and naming

The long-term stable engine-facing seam should be named around persistence, not
around "backends" or filesystem paths.

- use `TenantPersistence` as the umbrella name for the tenant-scoped behavior
  contract the engine depends on
- keep that seam behavior-oriented, with capability groupings such as
  `TenantQueryRead`, `TenantMutationPersistence`,
  `TenantJournalPersistence`, `TenantSchedulerPersistence`,
  `TenantSnapshotPersistence`, and `TenantSchemaPersistence`
- use `PersistenceProvider` as the separate typed construction/config seam for
  backend selection, pools, URLs, credentials, and tenant routing
- reserve `store` names for backend-local physical adapters such as
  `SqliteTenantStore`
- reserve `backend` names for temporary migration switches or concrete
  implementation families, not for the durable architecture vocabulary

That means migration-only names such as `StorageBackendKind`,
`BackendStorageEngine`, `BackendTenantStore`, and `BackendTenantReadStorage`
were intentionally renamed or deleted in `SB12` and should not reappear as the
permanent interface vocabulary.

### Embedded vs external cost model

The benchmark results reinforce the intended seam placement.

- embedded backends mainly pay local CPU, memory, disk, and lock costs
- external SQL backends also pay network round trips, pool checkout, TLS,
  remote planning, and server-side concurrency costs

That makes the Neovex-owned pre-storage layer even more important for external
SQL, but only for semantic shaping and round-trip reduction:

- keep auth, validation, admission, batching, OCC, `CommitEntry`, and
  tenant-routing semantics in Neovex
- keep transactions, indexes, filtering, ordering, prepared statements,
  pooling, and physical query execution in the backend
- prefer coarse semantic operations at the seam over tiny CRUD or scan-shaped
  primitives that would force chatty remote interaction

### Dialect vs topology

The durable provider seam must distinguish storage dialect from deployment
topology.

- `sqlite` is not specific enough on its own because local-file SQLite and
  replica-connected SQLite have different latency, consistency, and config
  shapes
- `redb` remaining embedded today should not force the seam to assume redb can
  only ever exist as a single-node local-file deployment
- future coordination or replication designs for embedded providers (`raft`,
  `etcd`, or similar) should fit as provider/topology configuration without
  changing `TenantPersistence`

This plan therefore targets retained local providers such as
`EmbeddedSqliteProvider` and `EmbeddedRedbProvider`, while future
`SqliteReplicaProvider`, `PostgresProvider`, and `MySqlProvider` work stays
deferred.

### Explicit deletion inventory

The migration should prefer deleting shared redb-shaped seams over wrapping
them. When the SQLite path lands, redb-specific physical code may remain as a
provider-local implementation, but it must stop being the canonical
cross-provider machinery.

Delete or keep deleted:

- `crates/neovex-engine/src/service/queries/planner/loading.rs`
  - already deleted; do not restore a redb-shaped loading adapter seam

The following redb-era modules must either be deleted or clearly reduced to
redb-provider-local implementation code instead of remaining shared engine
plumbing:

- `crates/neovex-storage/src/index/`
  - SQLite expression indexes and SQL predicates replace the old shared custom
    key encoding, bounds computation, scan adapters, and index maintenance as
    the canonical seam; any remaining redb implementation must be provider-local
- `crates/neovex-storage/src/keys.rs`
  - SQLite `PRIMARY KEY (table_name, id)` replaces the old shared flat-key
    encoding model
- `crates/neovex-storage/src/store/scan.rs`
  - SQLite `WHERE` clauses and indexed predicates replace the old shared
    MessagePack byte-level filter-pushdown seam
- `crates/neovex-storage/src/store/schema_rewrite.rs`
  - SQLite index maintenance during replay is automatic once document writes hit
    indexed columns; any redb replay-specific rewrite logic must stay
    provider-local

The following modules should be significantly simplified, with the redb-specific
index-reconciliation and scan-plumbing logic removed:

- `crates/neovex-storage/src/schema_store.rs`
  - keep schema persistence
  - replace manual key collection and reconciliation with SQLite-backed schema
    persistence plus `DROP INDEX` / `CREATE INDEX`
- `crates/neovex-engine/src/service/queries/planner/`
  - keep only the Neovex-owned planning logic that still adds product value
  - delete storage-scan adapter plumbing and redb-oriented index candidate
    scoring once SQLite-native query generation exists

Do not preserve these modules just to keep old seams familiar. SQLite is
supposed to replace them.

### Query planner direction

The engine query planner should be simplified, not preserved verbatim.

- SQLite should own physical index choice and low-level scan execution
- Neovex should continue to own query semantics, auth/policy merge behavior,
  and any residual filtering for semantics not cleanly expressed in SQL

That means the intended direction is:

- keep a smaller Neovex planner/query-builder layer if it still adds value
- generate parameterized SQL instead of calling the current redb scan adapter
  surface
- delete `crates/neovex-engine/src/service/queries/planner/loading.rs`
  once SQLite-backed query generation replaces scan-adapter dispatch
- significantly simplify
  `crates/neovex-engine/src/service/queries/planner/mod.rs`
  and any exact/range/scoring modules that only exist to choose among
  redb-specific scan shapes

This plan should not keep the current planner intact and merely re-implement
`index_scan_eq`, `index_scan_prefix`, `index_scan_range`, and
`index_scan_composite_range` on top of SQLite.

### SQLite storage model

The SQLite backend should preserve the logical model while using SQLite-native
tables and indexes:

```sql
CREATE TABLE documents (
    table_name TEXT NOT NULL,
    id TEXT NOT NULL,
    data_json TEXT NOT NULL,
    creation_time INTEGER NOT NULL,
    PRIMARY KEY (table_name, id)
);

CREATE TABLE schemas (
    table_name TEXT NOT NULL PRIMARY KEY,
    schema_json TEXT NOT NULL
);

CREATE TABLE scheduled_jobs (
    id TEXT NOT NULL PRIMARY KEY,
    data_json TEXT NOT NULL
);

CREATE TABLE running_scheduled_jobs (
    id TEXT NOT NULL PRIMARY KEY,
    data_json TEXT NOT NULL
);

CREATE TABLE scheduled_job_results (
    job_id TEXT NOT NULL PRIMARY KEY,
    data_json TEXT NOT NULL
);

CREATE TABLE scheduled_job_executions (
    execution_id TEXT NOT NULL PRIMARY KEY
);

CREATE TABLE cron_jobs (
    name TEXT NOT NULL PRIMARY KEY,
    data_json TEXT NOT NULL
);

CREATE TABLE commit_log (
    sequence INTEGER NOT NULL PRIMARY KEY,
    record_blob BLOB NOT NULL
);

CREATE TABLE metadata (
    key TEXT NOT NULL PRIMARY KEY,
    value_blob BLOB NOT NULL
);
```

Notes:

- `documents.data_json` stores document fields in JSON text so SQLite JSON1
  expressions can drive indexes and scans
- `commit_log.record_blob` stores serialized `DurableMutationRecord` values so
  integrity hashing and replay semantics remain explicit
- `scheduled_job_executions` is keyed by `execution_id`, matching the current
  deduplication contract
- `metadata` continues to own applied-head and other journal-progress state
  that SQLite row storage does not model directly

### Document codec and sequence model

SQLite changes the at-rest document format:

- document rows in `documents.data_json` should use JSON text at rest
- durable journal rows in `commit_log.record_blob` should continue storing
  serialized `DurableMutationRecord` values so the current integrity and replay
  model stays explicit

As a consequence:

- remove `Document::to_msgpack()` / `Document::from_msgpack()` from SQLite
  document-storage paths
- delete MessagePack-specific scan/pushdown code from storage
- it is acceptable for MessagePack to remain only in durable-journal blobs and
  any remaining non-storage code paths that still need it

Sequence management should also simplify under SQLite:

- use SQLite row insertion on `commit_log(sequence INTEGER PRIMARY KEY, ...)`
  as the durable sequence allocator for new commits
- keep explicit metadata only for engine-visible progress that SQLite does not
  model itself, such as the current applied-head tracking if that contract
  remains
- do not carry over redb-style `next_sequence` bookkeeping unless a concrete
  SQLite parity test shows it is still needed

### SQLite index strategy

SQLite indexes must preserve the current planner capabilities, including
composite indexes. A single-field-only design is not sufficient.

For each `IndexDefinition { name, fields }`, SQLite should create an expression
index that matches the ordered field list:

```sql
CREATE INDEX idx_{table}_{index_name}
ON documents (
    table_name,
    json_extract(data_json, '$.field1'),
    json_extract(data_json, '$.field2'),
    id
);
```

Requirements:

- support exact-match scans
- support composite exact-prefix scans
- support single-field and composite range scans
- keep residual filtering in the engine where the current planner already does
  so
- add explicit parity tests for JSON comparison behavior, especially across
  composite ranges

### Reactive and journal model

The engine continues to own reactivity through `CommitEntry`.

- SQLite write transactions build `WriteOp` values and `CommitEntry` directly
- the engine continues to call `process_commit(...)`
- the durable journal remains explicit storage state, not an implementation
  detail hidden behind SQLite hooks
- `sqlite3_update_hook` / `preupdate_hook` may be explored later for debugging
  or observability, but they are not required for core correctness, journaling,
  or subscriptions in this plan

### Preserved storage contract from live call sites

`SB1` inventories the current seam from the actual engine and storage code
instead of projecting a new CRUD-oriented abstraction. The preserved contract
is:

- async reads still run through `TenantReadStorage::execute(...)` and
  `execute_cancellable(...)`, with closures over `Arc<TenantStore>`, because
  the engine currently depends on concrete snapshot, planner-loading, journal,
  and materialized-read helpers
  - reviewed call sites:
    `crates/neovex-engine/src/service/queries/documents.rs`,
    `crates/neovex-engine/src/service/queries/prepared.rs`,
    `crates/neovex-engine/src/service/queries/materialized.rs`,
    `crates/neovex-engine/src/service/queries/journal.rs`,
    `crates/neovex-engine/src/service/subscriptions/bootstrap.rs`,
    `crates/neovex-engine/src/service/scheduler/access.rs`
- async writes still run through `TenantWriteStorage::execute_write(...)` and
  `execute_write_cancellable(...)`, with closures over
  `TenantWriteTransaction`, because schema updates, scheduler transitions, and
  cancellation-sensitive writes rely on the current transaction lifecycle
  rather than document-only helpers
  - reviewed call sites:
    `crates/neovex-engine/src/service/schema.rs`,
    `crates/neovex-engine/src/service/scheduler/access.rs`,
    `crates/neovex-storage/src/tests/async_faults.rs`
- `TenantWriteCommit<T>` and `TenantWriteOutcome<T>` stay the write result
  contract, preserving the distinction between pre-commit cancellation and
  post-commit completion
- validated direct-write helpers remain part of the contract; they are not
  incidental redb convenience methods
  - reviewed call sites:
    `crates/neovex-storage/src/store/write/direct.rs`,
    `crates/neovex-engine/src/service/mutations/direct/execution.rs`,
    `crates/neovex-engine/src/service/mutations/direct/store.rs`
- execution-unit commits continue to hand storage fully resolved document and
  scheduler changes through `ResolvedWrite` and `ResolvedScheduleOp`, and
  storage continues to apply them atomically before the engine fans out the
  resulting `CommitEntry`
  - reviewed call sites:
    `crates/neovex-storage/src/store/write/batch.rs`,
    `crates/neovex-engine/src/service/execution_units/state.rs`,
    `crates/neovex-engine/src/service/execution_units/commit.rs`
- scheduled execution dedupe remains keyed by execution id and must continue to
  flow through storage-owned dedupe state rather than through function-level or
  hook-level shortcuts
  - reviewed call sites:
    `crates/neovex-engine/src/scheduler.rs`,
    `crates/neovex-storage/src/index/maintenance/writes.rs`,
    `crates/neovex-storage/src/store/journal.rs`
- durable journal read/stream/bootstrap plus materialized snapshot
  export/restore/rebuild remain first-class storage APIs, not incidental test
  helpers
  - reviewed call sites:
    `crates/neovex-storage/src/store/journal.rs`,
    `crates/neovex-storage/src/store/journal_stream.rs`,
    `crates/neovex-storage/src/store/journal_snapshot.rs`,
    `crates/neovex-engine/src/service/queries/journal.rs`,
    `crates/neovex-engine/src/service/queries/verification.rs`
- the current query planner still depends on storage-backed exact, prefix,
  range, and composite-range loading, which is why SQLite must eventually
  replace the physical execution path instead of wrapping the existing
  scan-shape API forever
  - reviewed call sites:
    `crates/neovex-engine/src/service/queries/planner/loading.rs`,
    `crates/neovex-engine/src/service/queries/planner/mod.rs`

---

## Verification Contract

Always run the focused verification listed on the active item before marking it
`done`.

Always run:

- `cargo fmt --all --check`
- `cargo check --workspace`

Run, as appropriate:

- `cargo test -p neovex-storage`
- `cargo test -p neovex-engine`
- `cargo test -p neovex-server`
- the checked-in benchmark workload commands defined by `SB9`

Before considering the whole workstream complete, run:

- `make check`
- `make test`
- `make clippy`
- `make ci` if practical

If sandbox or environment restrictions block a command, do not silently skip
it. Run the best focused alternative, record the limitation in `Execution Log`,
and continue only when the blocker is environmental rather than architectural.
Run focused Cargo verification serially against the shared workspace `target/`;
if contention appears, wait for the active Cargo process to finish or stop the
genuinely stale one instead of switching to alternate artifact directories.

---

## Known Risks

| Risk | Severity | Mitigation |
| --- | --- | --- |
| SQLite JSON comparison and index ordering differ from current redb index semantics | high | add explicit parity corpora for exact, prefix, range, and composite scans before cutover |
| Journal/snapshot parity is broader than a CRUD swap | high | keep journal/bootstrap/snapshot APIs explicit and verify them in targeted tests before engine-wide cutover |
| Migration accidentally preserves shared redb-shaped modules as the canonical seam instead of deleting or backend-localizing them | high | treat the deletion inventory in this plan as a required outcome, not an optional cleanup pass |
| Planner simplification leaves too much redb scan-shape logic in place | high | generate SQL directly and delete `planner/loading.rs` instead of re-implementing scan adapters on SQLite |
| JSON-at-rest document storage leaves MessagePack assumptions in SQLite paths | high | make the document codec transition explicit and keep MessagePack only where intentionally retained, such as durable journal blobs |
| Migration touches many engine tests and fixtures that currently name concrete redb types | medium | budget an explicit engine/test integration phase instead of calling the work a pure refactor |
| Benchmark gate may show unacceptable regressions and delay the SQLite default switch | medium | require a checked-in benchmark report before default switch |
| Benchmark methodology may be too weak to justify cutover confidence | medium | before `SB10`, rerun the gate with alternating backend order, 10+ measured samples, steady-state plus cold-start lanes, and SQLite `EXPLAIN QUERY PLAN` capture for indexed workloads |
| `docs/plans/encryption-at-rest-plan.md` still assumes redb | medium | resolve sequencing with the encryption plan before `SB1+` starts |
| Migration-only naming could fossilize instead of being promoted into the durable provider seam | medium | rename or delete migration-only `Backend*` and `StorageBackendKind` vocabulary in `SB12` |
| A constructor seam gets stabilized around embedded filesystem paths or collapses dialect with topology | medium | keep the durable provider config typed enough to distinguish local embedded providers from future replica-connected or coordinated deployments |

---

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies | Gate Note |
| --- | --- | --- | --- | --- |
| SB0 | `done` | rewrote the historical pluggable-backend document into a SQLite-only control plan, moved future Postgres/MySQL work into a follow-on plan, and recorded the canonical deletion, planner, codec, sequence, and benchmark decisions | none | docs-only planning pass completed 2026-04-08 |
| SB1 | `done` | documented the preserved engine/storage contract from actual call sites in code and in this plan | SB0 | completed 2026-04-08 after inventorying the live read/write, batch, journal, snapshot, scheduler, and planner seams |
| SB2 | `done` | added temporary redb-vs-SQLite migration selection scaffolding at the service/storage composition root while keeping redb as the active backend | SB1 | completed 2026-04-08; the switch is explicitly migration-only and SQLite stays gated until the store foundation lands |
| SB3 | `done` | verified that the codified contract and migration scaffolding preserve the current redb-backed seam | SB1, SB2 | completed 2026-04-08 with focused storage and engine seam-preservation tests plus workspace format/check verification |
| SB4 | `done` | implemented the SQLite store foundation and async read/write boundary in `neovex-storage` | SB3 | completed 2026-04-08 with concrete SQLite store/executor types, WAL initialization, and focused boundary verification |
| SB5 | `done` | implement SQLite read path and query/index parity | SB4 | completed 2026-04-08 with a planner-owned query read contract, SQLite-backed engine query coverage, and the first redb-era read-surface deletion |
| SB6 | `done` | implemented SQLite write path, scheduler, journal, and snapshot parity | SB4 | completed 2026-04-08 with SQLite-native batch writes, scheduler queues/results/crons, journal stream/bootstrap, snapshot export/restore/rebuild, and the concrete sequence-metadata fallback required once snapshot restore truncates the physical journal |
| SB7 | `done` | ran the broader parity verification sweep for the SQLite implementation | SB5, SB6 | completed 2026-04-08 with full storage and engine crate sweeps plus workspace format/check verification after the last SQLite schema-store additions |
| SB8 | `done` | wired engine, tests, and harnesses through SQLite | SB7 | completed 2026-04-08 with backend-aware tenant/runtime/service seams, scheduler discovery, consistency coverage, and repo-wide verification through the `make` entrypoints |
| SB9 | `done` | benchmark SQLite against redb and record the go/no-go decision | SB8 | completed 2026-04-09 with the strengthened checked-in report plus a final SQLite sort-elision tuning pass. SQLite now wins 6 of 8 steady-state lanes and all 8 cold-start lanes, including both indexed-query workloads; only steady-state durable journal stream and durable journal bootstrap remain redb-leading |
| SB10 | `done` | switch the default embedded backend to SQLite during the final migration window | SB9 | completed 2026-04-09 by making `EmbeddedProviderKind::Sqlite` the default service/provider selection while keeping explicit redb construction and tenant filenames supported |
| SB11 | `done` | run full verification with SQLite as the default and redb retained as a selectable embedded provider | SB10 | completed 2026-04-09 with full repo verification green after fixing a SQLite-exposed tempdir lifetime bug in the mutation-journal test helper |
| SB12 | `done` | replace migration-only backend-selection scaffolding with the durable embedded-provider seam and naming | SB11 | completed 2026-04-09 after promoting the engine/storage codebase to `TenantPersistence`, `PersistenceProvider`, `EmbeddedProviderKind`, `EmbeddedRedbProvider`, and `EmbeddedSqliteProvider`, and after renaming the retained embedded benchmark entrypoints accordingly |
| SB13 | `done` | update docs and follow-on plans after provider-seam promotion | SB12 | completed 2026-04-09 after aligning the live architecture docs, benchmark report instructions, and follow-on plan references with the retained embedded-provider model and deleting the stale duplicate SQLite plan file |
| SB14 | `done` | run final verification and close the workstream with SQLite default plus retained redb embedded-provider support | SB13 | completed 2026-04-09 after `make ci` passed from the settled provider-seam state and the repo entry docs were repointed away from this archived migration plan |

---

## Dependency Graph

- `SB0` is the docs-only anchor that makes this file the durable control plane.
- The plan-level activation gate must be resolved before `SB1+` begins.
- `SB1` must happen before `SB2` through `SB6` so the migration contract is
  derived from the real current call sites.
- `SB2` should land before broad SQLite implementation so parity and benchmark
  work can run against both backends during the migration window.
- `SB4` is the foundation for both `SB5` and `SB6`.
- `SB5` and `SB6` must both complete before `SB7`.
- `SB7` gates `SB8`.
- `SB8` gates `SB9`.
- `SB9` gates both `SB10` and the durable provider-seam promotion work.
- `SB10` gates `SB11`.
- `SB11` gates `SB12`.
- `SB12` gates `SB13`.
- `SB13` gates `SB14`.

---

## Recommended Delivery Order

1. `SB1` through `SB3` - codify the preserved contract and add temporary
   migration scaffolding
2. `SB4` through `SB7` - implement SQLite parity for reads, writes, scheduler,
   journal, and snapshots
3. `SB8` through `SB11` - integrate broadly, benchmark against redb, and then
   make SQLite the default embedded backend
4. `SB12` through `SB14` - promote the durable embedded-provider seam, keep
   redb as a supported provider, and close the migration cleanly

---

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| SB0 | done; rewrote this document into the durable SQLite migration control plane, split future external SQL work into `docs/plans/archive/external-sql-storage-backends-plan.md`, and recorded the SQLite-vs-Neovex boundary, deletion inventory, planner direction, storage model, codec/sequence decisions, and benchmark gate | resolve the plan-level activation gate, then start `SB1` with a call-site inventory of the current engine/storage contract |
| SB1 | done; documented the live contract on `async_storage` and `store` types, and recorded the reviewed engine/storage call-site inventory in this control plan so later SQLite work preserves the real seam instead of inventing a CRUD abstraction | start `SB2` by introducing temporary migration-only redb-vs-SQLite selection scaffolding around tenant lifecycle and async storage handles |
| SB2 | done; added the migration-only `StorageBackendKind`, threaded it through service construction and tenant path selection, preserved redb as the default path, and added focused tests covering both redb filename preservation and the temporary SQLite gate | start `SB3` by verifying the codified contract and migration scaffolding preserve the current seam without changing runtime behavior |
| SB3 | done; verified the preserved seam through async storage cancellation tests, focused engine `service_*` behavior tests, the new backend-selection tests, and fresh workspace format/check runs so SQLite foundation work can start from a proven redb baseline | start `SB4` by implementing the SQLite store foundation and async boundary |
| SB4 | done; added concrete SQLite tenant store/read snapshot/write transaction types, a SQLite tenant storage engine plus async executors, WAL-oriented initialization, metadata and journal-progress foundation helpers, and backend-specific async trait associated types so SQLite can advance without a permanent trait-object layer | start `SB5` by implementing SQLite read-path parity and beginning the redb-era read-surface deletions |
| SB5 | done; added a planner-owned `QueryReadStore` contract in `neovex-storage`, made the engine evaluator/planner/prepared query path generic over it, verified SQLite-backed store and snapshot query execution under engine tests, and deleted `crates/neovex-engine/src/service/queries/planner/loading.rs` so the redb-era read surface has started to come out instead of being wrapped | start `SB6` by replacing redb direct-write, execution-unit batch, scheduler dedupe, journal, and snapshot mechanics with SQLite-native implementations while preserving `CommitEntry` and durable replay semantics |
| SB6 | done; completed the SQLite storage-parity surface across execution-unit batch apply, scheduler queue/result/cron flows, durable journal stream/bootstrap, and materialized snapshot export/restore/rebuild. The SQLite path now keeps sequence progress in metadata when snapshot restore truncates the physical journal, uses SQLite DDL for schema/index restore, and preserves the same `CommitEntry`, execution-id dedupe, and durable/applied-head semantics under focused storage and engine tests | start `SB7` by running the broader crate-level parity sweep now that the focused SQLite contract tests are green |
| SB7 | done; reran the broader parity sweep after the last SQLite schema-store additions and kept the full storage crate, full engine crate, and workspace format/check verification green, which closes the SQLite parity gate before broad engine cutover begins | start `SB8` by wiring the engine tenant/runtime/service seams, scheduler discovery, and relevant tests/harnesses through SQLite-backed stores and executors |
| SB8 | done; introduced the migration-only backend wrapper in `neovex-engine`, moved tenant/runtime/service ownership onto backend-aware store/read-storage handles, updated scheduler discovery and consistency helpers for backend-specific tenant files, and proved the SQLite service path under full crate and workspace verification. The repo-wide sweep also flushed out a SQLite reopen race in the new backend-selection test, which was fixed by quiescing the first service before asserting the durable reopen path | start `SB9` by inventorying the existing benchmark/example surfaces, then add and run the redb-vs-SQLite workloads required for the migration gate |
| SB9 | done; the checked-in benchmark gate now includes the final SQLite sort-elision tuning pass in addition to the earlier hot-path, methodology, and `EXPLAIN QUERY PLAN` corrections. The SQLite read path now elides equality-constrained prefix fields from storage-level `ORDER BY`, the corrected benchmark plans no longer spill to a temp B-tree in the indexed-query lanes, and the refreshed full report shows SQLite leading 14 of 16 measured lanes overall. The remaining redb advantage is now limited to steady-state durable journal stream (0.98x SQLite) and durable journal bootstrap (0.74x SQLite) | remain paused here before `SB10`; if cutover resumes, decide whether the current journal-only steady-state caveats are acceptable or whether one more journal-focused tuning pass is worth it |
| SB10 | done; switched the actual default constructor path to `EmbeddedProviderKind::Sqlite`, aligned the enum default, and updated focused provider tests so `Service::new` now proves the SQLite default while explicit redb selection still preserves `.redb` tenant files | start `SB11` with the repo-level verification sweep now that the default-cutover code path is green under focused engine and server tests |
| SB11 | done; the repo-level verification sweep is green with SQLite as the default embedded backend. The only failure uncovered during `make test` was a test-helper lifetime bug that dropped a `TempDir` early; redb had masked it by keeping files open, while SQLite correctly reopens by path. The helper now retains the tempdir for the duration of the scenario, and the full suite plus clippy are green again | start `SB12` by renaming the migration-only `StorageBackendKind` / `Backend*` vocabulary into the durable embedded-provider seam and keeping explicit redb support intact |
| SB12 | done; promoted the migration-only backend-selection layer into the durable embedded-provider seam by renaming the live code to `TenantPersistence`, `PersistenceProvider`, `EmbeddedProviderKind`, `EmbeddedRedbProvider`, and `EmbeddedSqliteProvider`, and by renaming the retained benchmark entrypoints to `embedded-provider-benchmarks` / `make bench-embedded-providers` | start `SB13` by aligning the active plan, architecture docs, benchmark report instructions, and follow-on plan references with the landed provider vocabulary |
| SB13 | done; aligned the live architecture docs, benchmark report instructions, and follow-on plan references with the retained embedded-provider seam, and deleted the stale duplicate SQLite plan file so this historical filename remains the one durable migration record | start `SB14` with the final repo-wide verification sweep and the archive/index cleanup needed to retire this plan cleanly |
| SB14 | done; reran the final repo-wide verification sweep from the settled provider-seam state, then archived this completed migration plan and updated the repo entry docs so future work starts from the right follow-on plans instead of this finished control plane | archived complete; future provider-topology work belongs to `docs/plans/archive/external-sql-storage-backends-plan.md`, and retained redb encryption work remains in `docs/plans/encryption-at-rest-plan.md` |

---

## Work Items

### SB0. Baseline review and control-plane rewrite

#### Outcome

- Completed during this planning pass.

### SB1-SB3. Codify the migration contract

#### Implementation plan

1. Inventory the current engine/storage call sites that depend on
   `TenantStore`, `TenantReadSnapshot`, `TenantWriteTransaction`,
   `TenantWriteCommit<T>`, `TenantWriteOutcome<T>`, validated direct writes,
   execution-unit batch application, scheduled execution dedupe, and the
   journal/snapshot surfaces.
2. Define the minimal migration contract around those real operations instead
   of around idealized CRUD helpers or permanent object-safe abstractions.
3. Introduce only temporary redb-vs-SQLite selection scaffolding so parity and
   benchmark work can exercise both backends during the migration window.
4. Keep the plan and code aligned before SQLite implementation starts.

#### Focused verification

- review the contract inventory against the reviewed call sites listed above
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- the preserved contract is explicit in code and in this plan
- no permanent object-safety compromise is introduced
- temporary migration scaffolding is clearly marked as migration-only

### SB4-SB7. Implement SQLite storage parity

#### Implementation plan

1. Implement SQLite store open/create paths and mirror the current async
   read/write execution boundary, including cancellation behavior. The async
   storage traits may carry backend-specific concrete store and transaction
   associated types so SQLite can land alongside redb without forcing a
   permanent object-safe abstraction.
2. Implement SQLite point reads, scans, and query generation with composite
   index support, and delete or backend-localize the redb-era shared
   index/key/scan modules once SQLite stops depending on them as the canonical
   seam.
3. Implement validated direct writes, execution-unit batch apply,
   scheduler-state flows, `CommitEntry` generation, durable journal
   append/read/stream/bootstrap, and materialized snapshot export/restore/
   rebuild.
4. Simplify schema/index reconciliation so SQLite DDL and automatic index
   maintenance replace manual redb-era rewrite logic.
5. Run targeted parity verification before broader integration begins.

#### Focused verification

- `cargo test -p neovex-storage`
- `cargo test -p neovex-engine`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- SQLite matches the preserved contract for reads, writes, journal, scheduler,
  and snapshot flows
- composite indexed query behavior is covered by parity tests
- the explicit deletion inventory has started to land instead of being wrapped

### SB8-SB11. Integrate, benchmark, and cut over

#### Implementation plan

1. Wire `Service`, runtime helpers, tests, and harnesses through the temporary
   migration selection layer.
2. Run the broader workspace verification sweep with SQLite wired through the
   engine and test surfaces.
3. Benchmark SQLite against redb on the agreed workloads and check in the
   benchmark report plus go/no-go decision.
   Before resuming `SB10`, the benchmark gate must use alternating backend
   order, 10+ measured samples, separate steady-state and cold-start lanes, and
   SQLite query-plan capture for the indexed workloads.
4. Make SQLite the default backend only after the stronger benchmark gate
   clears, then rerun the full verification sweep with SQLite as the default
   while redb remains available as a retained embedded provider.

#### Focused verification

- targeted crate tests for every newly migrated engine or harness surface
- `make check`
- `make test`
- `make clippy`
- `cargo fmt --all --check`

#### Acceptance criteria

- SQLite is wired through the engine and tests without losing preserved
  behavior
- the benchmark report is recorded and reviewed before default switch
- the recorded benchmark report includes both steady-state and cold-start
  measurements, enough samples to expose variance, and SQLite plan evidence for
  indexed workloads
- SQLite becomes the default backend only after parity and benchmark gates pass
- the retained redb provider path remains intentionally supported after the
  default switch

### SB12-SB14. Promote the durable provider seam and close out the migration

#### Implementation plan

1. Replace migration-only backend-selection scaffolding and naming with the
   durable embedded-provider seam (`TenantPersistence` /
   `PersistenceProvider`) while keeping SQLite default and redb retained.
2. Ensure the shared engine/storage seam is no longer redb-shaped even though
   redb remains a supported provider-local implementation.
3. Update docs and follow-on plan ownership so retained embedded providers are
   explicit and future non-local or replicated provider work lives in the
   external SQL plan.
4. Run the final verification sweep and close the workstream cleanly.

#### Focused verification

- `make check`
- `make test`
- `make clippy`
- `make ci` if practical

#### Acceptance criteria

- no migration-only naming or path-only provider assumption remains in the
  durable architecture vocabulary
- SQLite is the default embedded backend and redb remains a supported embedded
  provider behind the durable provider seam
- docs and follow-on plan ownership match the landed architecture
- the benchmark gate that justified the SQLite default switch is recorded in
  the plan

---

## Execution Log

| Date | Item | Outcome | Summary | Verification | Next Step |
| --- | --- | --- | --- | --- | --- |
| 2026-04-06 | meta | documented | Authored the initial version of this file as a pluggable multi-backend plan with redb retained long-term. | doc review only | revise scope before activation |
| 2026-04-08 | SB0 | done | Re-scoped the historical pluggable-backend plan into a SQLite-only migration control plane. Moved future Postgres/MySQL work into `docs/plans/archive/external-sql-storage-backends-plan.md`, kept `CommitEntry` and journal/snapshot semantics explicit, rejected hook-driven reactivity as the core contract, required composite SQLite indexes, added a benchmark gate before redb removal, clarified the SQLite-vs-Neovex boundary, and recorded the explicit deletion inventory plus planner, codec, and sequence decisions so implementers delete redb-era code instead of wrapping it. | docs-only review against the files listed in `Reviewed against`; no new code verification claimed | resolve the plan-level activation gate, then start `SB1` with a call-site inventory of the current engine/storage contract |
| 2026-04-08 | meta | activated | Promoted this control plane into active execution ownership by updating the plan index and `AGENTS.md`, and recorded the activation decision so fresh agent contexts start from the SQLite migration rather than treating it as a deferred design note. | docs-only update; no new code verification claimed | start `SB1` from the live call sites and mark it `in_progress` when implementation begins |
| 2026-04-08 | SB1 | done | Inventoried the live engine/storage contract from the actual read, write, batch, scheduler, journal, snapshot, and planner call sites. Documented that preserved seam directly on the async storage and store types, and added a dedicated call-site inventory section here so later migration slices preserve `TenantWriteCommit<T>`, `TenantWriteOutcome<T>`, validated direct writes, `ResolvedWrite` / `ResolvedScheduleOp`, execution-id dedupe, engine-owned `CommitEntry`, and durable journal/bootstrap/snapshot semantics. | `cargo fmt --all --check`; `cargo check --workspace` | start `SB2` with temporary migration-only redb-vs-SQLite selection scaffolding |
| 2026-04-08 | SB2 | done | Added the migration-only `StorageBackendKind`, threaded it through service construction, tenant path selection, and storage-engine path helpers, and kept redb as the active backend while explicitly gating SQLite selection until the store foundation lands. Added focused engine tests that lock in both the preserved `.redb` tenant filename path and the temporary SQLite rejection behavior. | `cargo fmt --all --check`; `cargo check --workspace`; `cargo test -p neovex-engine storage_backend`; attempted `bash scripts/cargo-isolated.sh -- test -p neovex-engine storage_backend` but the isolated target hit sandbox DNS failures when `rusty_v8` tried to download its archive, so the focused test was rerun successfully against the existing workspace target | start `SB3` by verifying the contract docs plus migration scaffolding preserve the current seam and do not perturb current redb behavior |
| 2026-04-08 | SB3 | done | Verified that the codified contract and migration scaffolding preserve the current seam. Re-ran the storage cancellation/commit-boundary tests that define the async storage contract, exercised focused engine `service_*` behavior that covers tenant lifecycle and service-level redb baselines, and kept the new backend-selection seam under test before starting any SQLite implementation work. | `cargo test -p neovex-storage async_faults`; `cargo test -p neovex-engine storage_backend`; `cargo test -p neovex-engine service_`; `cargo fmt --all --check`; `cargo check --workspace` | start `SB4` with the SQLite store foundation and async boundary implementation |
| 2026-04-08 | SB4 | done | Landed the first concrete SQLite implementation slice inside `neovex-storage`: backend-specific associated types on the async storage traits, `SqliteTenantStore` / `SqliteReadSnapshot` / `SqliteWriteTransaction`, `SqliteTenantStorage` / `SqliteStorageEngine`, WAL-oriented initialization, and minimal metadata/journal-progress helpers that preserve the current pre-commit versus committed-write semantics at the boundary. | `cargo check --workspace`; `cargo test -p neovex-storage sqlite_`; `cargo test -p neovex-engine storage_backend`; `cargo fmt --all --check` | start `SB5` by implementing SQLite read-path parity and planner-facing query support |
| 2026-04-08 | SB5 | done | Finished the SQLite read-parity slice by introducing a planner-owned `QueryReadStore` contract in `neovex-storage`, making the engine evaluator/planner/prepared query path generic over that contract, adding SQLite-backed engine tests for planned store and snapshot query execution plus pagination, and deleting `crates/neovex-engine/src/service/queries/planner/loading.rs` so the redb-era read surface started landing in the deletion inventory instead of being wrapped. | `cargo test -p neovex-storage sqlite_`; `cargo check -p neovex-engine`; `cargo test -p neovex-engine sqlite_`; `cargo test -p neovex-engine evaluator`; `cargo test -p neovex-engine queries`; `cargo fmt --all --check`; `cargo check --workspace` | start `SB6` by replacing redb write/journal/snapshot mechanics with SQLite-native transaction, commit, and replay flows |
| 2026-04-08 | SB6 | in_progress | Re-read the live redb write-path ownership seams across direct writes, execution-unit batch apply, scheduler dedupe/state, durable journal append/apply/recovery, snapshot export/restore/rebuild, and schema/index reconciliation, then landed the first SQLite write slice: `SqliteWriteTransaction` now supports deduped direct insert/update/delete flows, emits `CommitEntry` values with SQLite-allocated sequences, writes durable journal blobs to `commit_log`, updates applied-head metadata, and exposes durable journal plus scheduled-execution reads under focused SQLite tests. | code review of `crates/neovex-storage/src/store/write/direct.rs`, `crates/neovex-storage/src/store/write/batch.rs`, `crates/neovex-storage/src/store/journal.rs`, `crates/neovex-storage/src/store/journal_snapshot.rs`, `crates/neovex-storage/src/schema_store.rs`, and `crates/neovex-engine/src/service/scheduler/access.rs`; `cargo test -p neovex-storage sqlite_`; `cargo fmt --all --check`; `cargo check --workspace` | extend SQLite write parity through execution-unit batch apply, durable replay/recovery, scheduler queues/results, snapshot export/restore/rebuild, and schema/index reconciliation cleanup |
| 2026-04-08 | SB6 | in_progress | Extended the SQLite write/journal slice beyond direct writes: durable append now enforces contiguous sequences, durable journal reads return serialized `DurableMutationRecord` blobs, recovery replays durable-but-unapplied records onto JSON-at-rest documents while preserving execution-id dedupe markers, and focused SQLite tests now cover both append-hole rejection and durable-head recovery semantics. | `cargo test -p neovex-storage sqlite_`; `cargo fmt --all --check`; `cargo check --workspace` | implement execution-unit batch apply plus scheduler queue/result/cron flows, then carry the same SQLite-native mechanics into snapshot export/restore/rebuild and schema/index cleanup |
| 2026-04-08 | SB6 | done | Finished the SQLite storage-parity implementation slice: `SqliteTenantStore` now covers execution-unit batch apply, scheduler queue/result/cron flows, durable journal stream/bootstrap, and materialized snapshot export/restore/rebuild, while preserving `CommitEntry`, execution-id dedupe, and durable/applied-head tracking. The SQLite path now also records next-sequence metadata once snapshot restore truncates the physical `commit_log`, which is the concrete parity case that proved a metadata fallback was still required. | `cargo fmt --all --check`; `cargo test -p neovex-storage sqlite_`; `cargo test -p neovex-engine sqlite_`; `cargo check --workspace` | start `SB7` with the broader crate-level parity sweep across storage and engine |
| 2026-04-08 | SB7 | done | Re-ran the broader parity sweep after the last SQLite schema-store additions and kept the full storage crate, full engine crate, and workspace format/check verification green. That closes the parity gate for the implemented SQLite storage surface and confirms the remaining work belongs to engine/test integration rather than missing storage semantics. | `cargo fmt --all --check`; `cargo test -p neovex-storage`; `cargo test -p neovex-engine`; `cargo check --workspace` | start `SB8` by wiring the engine tenant/runtime/service seams and test helpers through SQLite-backed stores and executors |
| 2026-04-08 | SB8 | done | Finished the engine/test integration slice for the temporary SQLite migration path. `Service`, tenant runtime ownership, scheduler access/discovery, direct mutation/query helpers, and the relevant consistency/backend-selection tests now run through backend-aware store and async executor wrappers instead of hard-coded redb types. The broader workspace verification sweep exposed one deterministic race in the new SQLite reopen test, which was fixed by quiescing the first service before reopening so the test asserts the durable path rather than executor teardown timing. | `cargo test -p neovex-engine`; `cargo test -p neovex-storage`; `cargo fmt --all --check`; `cargo check --workspace`; `make check`; `make test`; `make clippy` | start `SB9` by adding and running the redb-vs-SQLite benchmark workloads, then record the benchmark report and go/no-go decision |
| 2026-04-08 | SB9 | in_progress | Started the benchmark-gate slice by adding a checked-in redb-vs-SQLite harness in `crates/neovex-engine/benches/embedded-provider-benchmarks.rs` and a matching `make bench-embedded-providers` entrypoint. The harness compiles after a focused `cargo check`, and it is designed to write the checked-in markdown report directly, but the first release-mode benchmark run was interrupted cleanly when the user asked to pause before moving on to `SB10`, so no benchmark numbers are claimed yet. | `cargo check -p neovex-engine --bench embedded-provider-benchmarks`; `cargo fmt --all` | rerun `make bench-embedded-providers REPORT=docs/research/sqlite-storage-benchmark-report.md`, inspect the generated results, and only then record the benchmark gate decision |
| 2026-04-08 | SB9 | done | Completed the benchmark gate by running the checked-in redb-vs-SQLite harness to a captured markdown report at `docs/research/sqlite-storage-benchmark-report.md` and recording the go/no-go decision in this plan. The report shows SQLite ahead on document CRUD throughput (4.77x median ops/s), point reads (2.10x), durable journal streaming (1.63x), durable journal bootstrap (1.27x), subscription fan-out (3.84x), and concurrent multi-tenant mixed load (4.34x), while redb remains ahead on indexed-query latency (SQLite at 0.64x on the single-field query and 0.62x on the composite query). The migration gate is accepted because the broader write-heavy and mixed service-path workloads improved materially and the indexed-query regression is now explicitly documented before the default switch. | `make bench-embedded-providers REPORT=docs/research/sqlite-storage-benchmark-report.md`; `cargo fmt --all --check`; `cargo check --workspace`; `make check`; `make test`; `make clippy` | pause here per user request before starting `SB10` |
| 2026-04-08 | meta | documented | Folded the duplicate SQLite-plan guardrails back into this active control plane: the migration seam stays explicitly temporary, no durable filesystem-shaped backend factory seam should emerge from `SB10` through `SB14`, and the long-term Postgres/MySQL construction/config seam remains owned by `docs/plans/archive/external-sql-storage-backends-plan.md`. The stale duplicate SQLite plan file was retired so this historical filename remains the single live control plane. | docs-only review of the active SQLite plan, `docs/plans/README.md`, and `docs/plans/archive/external-sql-storage-backends-plan.md`; no code verification claimed | remain paused here before `SB10`; when work resumes, continue from this active plan rather than reviving duplicate SQLite plan docs |
| 2026-04-08 | SB9 | optimized | Tuned the SQLite indexed-read hot path before cutover by reusing pooled read connections, removing per-query schema reloads through a shared schema cache that refreshes on schema writes and snapshot restore, and reusing cached prepared statements on the pooled connections. The refreshed benchmark report at `docs/research/sqlite-storage-benchmark-report.md` now shows SQLite at 23.14x redb for CRUD throughput, 1.02x for point reads, 0.90x for the remaining single-field indexed-query caveat, 1.01x for composite indexed queries, 2.27x for durable journal streaming, 1.80x for durable journal bootstrap, 11.09x for subscription fan-out, and 15.20x for concurrent multi-tenant mixed load. | `cargo fmt --all --check`; `cargo test -p neovex-storage sqlite_`; `cargo test -p neovex-engine queries`; `cargo check -p neovex-engine --bench embedded-provider-benchmarks`; `make bench-embedded-providers REPORT=docs/research/sqlite-storage-benchmark-report.md`; `cargo check --workspace`; `make clippy` | pause here before `SB10`; when work resumes, switch the default backend to SQLite from this refreshed benchmark baseline |
| 2026-04-09 | SB9 | rebenchmarked | Hardened the benchmark gate before `SB10` by adding alternating backend order, 12 steady-state / 10 cold-start measured samples, 95% confidence intervals, workload filtering for surgical reruns, SQLite `EXPLAIN QUERY PLAN` capture, and a cloned seeded-dataset cold-start path that measures fresh opens without repeatedly rebuilding the same tenant data. The strengthened report shows SQLite still dominant on CRUD throughput (23.80x steady-state, 21.07x cold-start), subscription fan-out (6.05x steady-state, 2.09x cold-start), concurrent mixed load (13.34x steady-state, 12.99x cold-start), and every cold-start lane. redb still leads several steady-state read-only microbenchmarks, including point reads (1.02x), indexed queries (1.12x), composite indexed queries (1.18x), journal bootstrap (1.33x), and near-parity journal stream (1.01x). The SQLite `EXPLAIN QUERY PLAN` sections also show the indexed-query SQL using `sqlite_autoindex_documents_1 (table_name=?)` plus a temp B-tree for ordering instead of the intended expression indexes, which is now the main remaining cutover caveat. | `cargo check -p neovex-engine --bench embedded-provider-benchmarks`; workload-filtered release repros on `crud`, `point-read`, and `mixed-load` with 1 measured round per lane to validate the new harness shape; `make bench-embedded-providers REPORT=docs/research/sqlite-storage-benchmark-report.md`; `cargo fmt --all --check`; `cargo check --workspace`; `make clippy` | remain paused here before `SB10`; if work resumes, decide whether to accept the current benchmark trade-offs or do another SQLite indexed-query tuning pass first |
| 2026-04-09 | SB9 | corrected evidence | Replaced the benchmark-only duplicated SQLite `EXPLAIN QUERY PLAN` SQL with shared production-shaped statement builders from `neovex-storage`, added a regression test to keep the report aligned with the real read path, restored the checked-in winner scorecard directly in the benchmark renderer, and reran the checked-in benchmark gate. The corrected report now shows the intended SQLite expression indexes being used in both indexed-query lanes (`idx_tasks_by_status` and `idx_tasks_by_team_status_rank`), which means the earlier primary-key autoindex evidence was a benchmark-report artifact rather than a production query-planner failure. The remaining indexed-query caveat is narrower and real: both corrected plans still spill to a temp B-tree for `ORDER BY`, while the workload-level benchmark outcome remains the same broad shape with redb ahead on the steady-state read-only microbenchmarks and SQLite ahead on CRUD, subscriptions, mixed load, and every cold-start lane. | `cargo test -p neovex-storage sqlite_ -- --nocapture`; `cargo check -p neovex-engine --bench embedded-provider-benchmarks`; `cargo fmt --all --check`; `cargo check --workspace`; `make bench-embedded-providers REPORT=docs/research/sqlite-storage-benchmark-report.md`; `make clippy` | remain paused here before `SB10`; if work resumes, decide whether the current trade-off is acceptable or whether to do one more SQLite indexed-query sort-elision pass first |
| 2026-04-09 | SB9 | optimized | Landed the final pre-`SB10` SQLite sort-elision pass by trimming storage-level `ORDER BY` down to the non-constant index suffix plus `id`, and by routing both the production SQLite read path and the benchmark `EXPLAIN QUERY PLAN` capture through the same shared SQL builders. Added a regression test proving the indexed-query plans no longer use `USE TEMP B-TREE`, reran focused engine/storage verification, and refreshed the full benchmark report. On the updated report, SQLite now leads both indexed-query lanes in steady-state and cold-start, wins steady-state point reads by a hair, and moves the overall scorecard to 14 of 16 measured lanes, with redb still ahead only on steady-state durable journal stream and durable journal bootstrap. | sqlite probe via `/usr/bin/sqlite3` to confirm suffix-only `ORDER BY` removes temp sorts before code changes; `cargo test -p neovex-storage sqlite_ -- --nocapture`; `cargo test -p neovex-engine queries`; `cargo check -p neovex-engine --bench embedded-provider-benchmarks`; `cargo fmt --all --check`; `cargo check --workspace`; workload-filtered release reruns for `indexed-query` and `composite-indexed-query`; `make bench-embedded-providers REPORT=docs/research/sqlite-storage-benchmark-report.md`; `make clippy` | remain paused here before `SB10`; if work resumes, decide whether the remaining steady-state journal-only regressions merit another tuning pass or whether to proceed to the default-backend switch |
| 2026-04-09 | SB10 | done | Flipped the actual embedded default to SQLite by making `StorageBackendKind::Sqlite` the enum default and routing `Service::new` plus `Service::new_with_simulation` through that default, while keeping `Service::new_with_storage_backend(..., StorageBackendKind::Redb)` as the explicit retained redb path. Updated the focused backend tests so the default constructor proves `.sqlite3` tenant persistence and roundtrip loading, the retained redb constructor still preserves `.redb` tenant files, and the service fixture harness exercises the default path under SQLite. | `cargo test -p neovex-engine storage_backend`; `cargo test -p neovex-engine consistency`; `cargo test -p neovex-server core_http::tenants`; `cargo fmt --all --check`; `cargo check --workspace` | continue with `SB11` by running the full repo verification sweep under the new SQLite default while redb remains explicitly selectable |
| 2026-04-09 | SB11 | done | Ran the full verification sweep with SQLite as the default embedded backend and redb still explicitly selectable. `make test` initially surfaced three mutation-journal cancellation failures, but the root cause was not a SQLite engine regression: a shared test helper dropped its `TempDir` immediately after returning, which redb had masked by keeping the tenant file handle open while SQLite correctly reopened by path. The helper now retains the tempdir for the whole scenario, the focused cancellation cluster is green again, and the full repo verification sweep now passes cleanly under the SQLite default. | `make check`; `make test`; `cargo test -p neovex-engine waiting_for_applied_visibility`; `cargo fmt --all --check`; `make clippy` | start `SB12` by promoting migration-only backend-selection names into the durable embedded-provider seam and preserving explicit redb support behind that seam |
| 2026-04-09 | meta | documented | Updated `ARCHITECTURE.md`, this active plan, and the deferred external-SQL plan to codify the post-cutover storage direction from first principles: the durable engine-facing seam should be named `TenantPersistence`, the separate typed construction/config seam should be named `PersistenceProvider`, temporary `Backend*` and `StorageBackendKind` names must not survive `SB12`, and the embedded-vs-external cost model should drive a coarse semantic seam with backend-native physical execution below it. | docs-only review of `ARCHITECTURE.md`, this plan, and `docs/plans/archive/external-sql-storage-backends-plan.md`; attempted `npm run docs:validate-refs:strict`, but the root workspace currently has no such script | remain paused here before `SB10`; when work resumes, treat the persistence seam and provider naming above as canonical and continue with the default-backend switch from the refreshed SB9 benchmark baseline |
| 2026-04-09 | meta | amended | Revised the live control plane after the provider-seam review: SQLite remains the target default embedded backend, but redb is now retained as a supported embedded provider instead of being slated for deletion. `SB12` through `SB14` now promote the migration-only selector into the durable embedded-provider seam, future non-local or coordinated provider work is deferred, and the provider contract now explicitly distinguishes backend dialect from deployment topology so local-file SQLite, replica-connected SQLite, retained redb, and later coordinated providers can coexist without reshaping `TenantPersistence`. | docs-only review of this plan, `ARCHITECTURE.md`, `docs/plans/README.md`, and `docs/plans/archive/external-sql-storage-backends-plan.md` | remain paused here before `SB10`; when work resumes, cut over to SQLite as the default backend and then replace migration-only naming with the durable embedded-provider seam rather than removing redb |
| 2026-04-09 | SB12 | done | Promoted the migration-only backend-selection scaffolding into the durable embedded-provider seam. The live engine/storage code now uses `TenantPersistence` and `PersistenceProvider` at the seam, `EmbeddedProviderKind` at the local selector, `EmbeddedRedbProvider` / `EmbeddedSqliteProvider` for the retained embedded concrete providers, and `embedded-provider-benchmarks` / `make bench-embedded-providers` for the retained benchmark entrypoint. | `cargo check -p neovex-engine --bench embedded-provider-benchmarks`; `cargo fmt --all --check`; `make check`; `make test`; `make clippy` | start `SB13` by aligning the active docs and follow-on plans with the landed provider vocabulary |
| 2026-04-09 | SB13 | done | Aligned the live docs with the landed provider seam: updated `ARCHITECTURE.md`, the benchmark report instructions, the active plan, the plan index, and the redb encryption follow-on plan to use the retained embedded-provider vocabulary, then deleted the stale duplicate `docs/plans/sqlite-pluggable-storage-backend-plan.md` file so this historical filename remains the sole migration record. | docs-only review of the updated docs and plan files; no new code verification beyond the green SB12 sweep claimed | start `SB14` with the final repo-wide verification sweep and archive/index cleanup |
| 2026-04-09 | SB14 | done | Completed the final closure sweep from the settled provider-seam state. `make ci` passed after rerunning it with elevated filesystem access so `cargo deny` could take its advisory-db lock outside the workspace, and the repo entry docs now point future work at the right active/deferred plans instead of this completed migration control plane. | `make ci` (rerun with elevated filesystem access for the `cargo deny` advisory-db lock) | archived complete; use `docs/plans/archive/external-sql-storage-backends-plan.md` for future provider-topology work and `docs/plans/encryption-at-rest-plan.md` for retained redb encryption work |
