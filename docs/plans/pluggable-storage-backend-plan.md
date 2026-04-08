# SQLite Storage Backend Migration Control Plan

This file keeps its historical filename, but it now owns a SQLite-only
migration: move Neovex internal storage from redb to SQLite, benchmark SQLite
against redb before cutover, then remove redb. Future Postgres/MySQL internal
storage work belongs to `docs/plans/external-sql-storage-backends-plan.md`.

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
- the current repo still uses redb as the only implemented internal backend
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
workstream is not to create a permanent multi-backend architecture. The goal is
to preserve Neovex's current product semantics while replacing redb-specific
storage mechanics with SQLite-native ones.

This plan exists to keep that migration disciplined. It should prevent two
failure modes:

- preserving redb-era code by wrapping it behind new interfaces instead of
  deleting it
- treating Neovex-owned product behavior as if SQLite already solved it just
  because SQLite has lower-level storage hooks or replication features

---

## Relationship To Other Plans

- Use `docs/plans/README.md` as the owning plan index.
- This plan is separate from `docs/plans/encryption-at-rest-plan.md`, which is
  currently redb-specific and must be sequenced explicitly before activation.
- Future Postgres/MySQL internal storage work belongs to
  `docs/plans/external-sql-storage-backends-plan.md`, not this plan.
- If work turns into a product-level redesign of journaling, replication, or
  reactivity semantics, stop and move that scope into the owning follow-on plan
  instead of stretching this migration plan across multiple workstreams.

---

## Scope

This plan covers:

- SQLite as the only target replacement backend for Neovex internal storage
- a temporary redb-vs-SQLite migration window for parity tests and benchmarks
- a benchmark gate before changing the default backend and before deleting redb
- preserving the current engine-facing storage contract while swapping the
  backend implementation
- final removal of redb and migration-only backend-selection scaffolding

This plan does not cover:

- a permanent generalized backend abstraction for future external databases
- Postgres or MySQL internal storage
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

4. Prefer deletion over wrapping.
   When SQLite replaces a redb-era mechanism, delete the old module instead of
   preserving it behind a new compatibility interface.

5. Keep temporary migration scaffolding temporary.
   Any redb-vs-SQLite selection layer exists only to bridge parity and
   benchmarking. It must be removed before this plan is closed.

6. Benchmark before default-switch and before deletion.
   redb removal requires a checked-in benchmark report and an explicit go/no-go
   decision.

7. Treat the current git worktree as baseline reality.
   Resume from the code and this plan's ledger, not from chat memory.

---

## Current Assessed State

- Neovex is pre-launch with zero production users and zero production data, so
  breaking changes and direct replacements are preferred over compatibility
  layers.
- redb is still the only implemented internal backend today.
- The async storage boundary already exists via `StorageEngine`,
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

4. SQLite should delete large redb-era storage code instead of wrapping it.
   The entire `crates/neovex-storage/src/index/` tree, `keys.rs`,
   `store/scan.rs`, and `store/schema_rewrite.rs` are expected deletions, while
   `schema_store.rs` and the engine planner should shrink materially.

5. The query planner should be simplified toward SQL generation and residual
   Neovex semantics, not preserved as a redb scan-shape chooser with SQLite
   adapters underneath.

6. Documents should move to JSON at rest in SQLite, while durable journal blobs
   stay serialized `DurableMutationRecord` values.
   SQLite sequence allocation should replace redb-style `next_sequence`
   bookkeeping unless parity proves otherwise.

7. redb stays only long enough to provide parity and benchmark baselines.
   Postgres/MySQL belong to a separate follow-on plan, and
   encryption-at-rest sequencing still needs an explicit decision before
   implementation starts.

---

## Success Criteria

This plan is successful only when all of the following are true:

- SQLite matches the current engine-visible storage contract across reads,
  writes, scheduler flows, `CommitEntry`, journal APIs, snapshot/rebuild flows,
  and recovery semantics.
- Composite indexed query behavior remains intact, but the physical execution
  path is SQLite-native rather than redb-key encoded.
- The benchmark report is checked in and justifies both the default switch and
  eventual redb removal.
- The deletion inventory lands: redb-era index, key, scan, and schema-rewrite
  code are removed instead of hidden behind compatibility layers.
- Docs, plan index ownership, and cross-plan notes reflect SQLite as the only
  internal backend once the workstream closes.

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

This plan should aggressively delete redb-specific implementation code, but it
should not treat current Neovex product guarantees as accidental storage
details just because SQLite has lower-level hooks or replication features.

### Explicit deletion inventory

The migration should prefer deletion over wrapping. When the SQLite path lands,
the following redb-era implementation modules should be deleted instead of
hidden behind new interfaces:

- `crates/neovex-storage/src/index/`
  - SQLite expression indexes and SQL predicates replace custom key encoding,
    bounds computation, scan adapters, and index maintenance
- `crates/neovex-storage/src/keys.rs`
  - SQLite `PRIMARY KEY (table_name, id)` replaces custom flat-key encoding
- `crates/neovex-storage/src/store/scan.rs`
  - SQLite `WHERE` clauses and indexed predicates replace MessagePack byte-level
    filter pushdown
- `crates/neovex-storage/src/store/schema_rewrite.rs`
  - SQLite index maintenance during replay is automatic once document writes hit
    indexed columns

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

---

## Verification Contract

Always run the focused verification listed on the active item before marking it
`done`.

Always run:

- `cargo fmt --all --check`
- `cargo check --workspace`

Run, as appropriate:

- `bash scripts/cargo-isolated.sh -- test -p neovex-storage`
- `bash scripts/cargo-isolated.sh -- test -p neovex-engine`
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

---

## Known Risks

| Risk | Severity | Mitigation |
| --- | --- | --- |
| SQLite JSON comparison and index ordering differ from current redb index semantics | high | add explicit parity corpora for exact, prefix, range, and composite scans before cutover |
| Journal/snapshot parity is broader than a CRUD swap | high | keep journal/bootstrap/snapshot APIs explicit and verify them in targeted tests before engine-wide cutover |
| Migration accidentally wraps dead redb-era modules instead of deleting them | high | treat the deletion inventory in this plan as a required outcome, not an optional cleanup pass |
| Planner simplification leaves too much redb scan-shape logic in place | high | generate SQL directly and delete `planner/loading.rs` instead of re-implementing scan adapters on SQLite |
| JSON-at-rest document storage leaves MessagePack assumptions in SQLite paths | high | make the document codec transition explicit and keep MessagePack only where intentionally retained, such as durable journal blobs |
| Migration touches many engine tests and fixtures that currently name concrete redb types | medium | budget an explicit engine/test integration phase instead of calling the work a pure refactor |
| Benchmark gate may show unacceptable regressions and delay redb removal | medium | require a checked-in benchmark report before default switch and before deletion |
| `docs/plans/encryption-at-rest-plan.md` still assumes redb | medium | resolve sequencing with the encryption plan before `SB1+` starts |
| Temporary migration scaffolding could become permanent by accident | medium | mark it as migration-only in code and remove it in `SB12` |

---

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies | Gate Note |
| --- | --- | --- | --- | --- |
| SB0 | `done` | rewrote the historical pluggable-backend document into a SQLite-only control plan, moved future Postgres/MySQL work into a follow-on plan, and recorded the canonical deletion, planner, codec, sequence, and benchmark decisions | none | docs-only planning pass completed 2026-04-08 |
| SB1 | `todo` | document the preserved engine/storage contract from actual call sites | SB0 | activation gate must be resolved before implementation starts |
| SB2 | `todo` | add temporary redb-vs-SQLite migration selection scaffolding | SB1 | temporary only; no permanent external-backend abstraction |
| SB3 | `todo` | verify that the codified contract and migration scaffolding preserve the current seam | SB1, SB2 | no object-safety workaround should be required here |
| SB4 | `todo` | implement SQLite store foundation and the async read/write boundary | SB3 | preserve current cancellation and async semantics |
| SB5 | `todo` | implement SQLite read path and query/index parity | SB4 | must include composite indexes and deletion of redb-era scan/index code |
| SB6 | `todo` | implement SQLite write path, scheduler, journal, and snapshot parity | SB4 | must preserve `CommitEntry`, journal, snapshot, and dedupe behavior while simplifying schema/index rewrite logic |
| SB7 | `todo` | run targeted parity verification for the SQLite implementation | SB5, SB6 | gate before engine-wide cutover |
| SB8 | `todo` | wire engine, tests, and harnesses through SQLite | SB7 | broad migration surface across engine and test helpers |
| SB9 | `todo` | benchmark SQLite against redb and record the go/no-go decision | SB8 | required before default switch and before redb removal |
| SB10 | `todo` | switch the default backend to SQLite during the final migration window | SB9 | redb may remain only as an explicit temporary fallback |
| SB11 | `todo` | run full verification with SQLite as the default | SB10 | benchmark report and full verification must be recorded |
| SB12 | `todo` | remove redb and temporary migration-only selection code | SB11 | delete redb only after the benchmark gate is satisfied |
| SB13 | `todo` | update docs and close out cross-plan follow-up after redb removal | SB12 | external SQL follow-on and encryption sequencing must be recorded cleanly |
| SB14 | `todo` | run final SQLite-only verification and close the workstream | SB13 | no active redb codepath or operator-facing redb promise may remain |

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
- `SB9` gates both `SB10` and eventual redb deletion.
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
   make SQLite the default
4. `SB12` through `SB14` - remove redb and close the migration cleanly

---

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| SB0 | done; rewrote this document into the durable SQLite migration control plane, split future external SQL work into `docs/plans/external-sql-storage-backends-plan.md`, and recorded the SQLite-vs-Neovex boundary, deletion inventory, planner direction, storage model, codec/sequence decisions, and benchmark gate | resolve the plan-level activation gate, then start `SB1` with a call-site inventory of the current engine/storage contract |
| SB1 | none yet | inventory current engine/storage call sites and record the preserved contract |
| SB2 | none yet | add temporary redb-vs-SQLite selection scaffolding |
| SB3 | none yet | verify the codified contract and migration setup before SQLite implementation starts |
| SB4 | none yet | implement SQLite store open/create paths and async boundary |
| SB5 | none yet | implement SQL-backed reads, delete redb-era index/key/scan modules, and simplify planner loading |
| SB6 | none yet | implement SQLite writes, scheduler state, journal, snapshot flows, and schema/index reconciliation cleanup |
| SB7 | none yet | run the targeted SQLite parity suite |
| SB8 | none yet | wire engine/tests/harnesses through SQLite |
| SB9 | none yet | run and record the benchmark comparison against redb |
| SB10 | none yet | switch the default backend to SQLite |
| SB11 | none yet | run full workspace verification with SQLite as the default |
| SB12 | none yet | remove redb code and temporary migration selection |
| SB13 | none yet | update docs and follow-on plans after redb removal |
| SB14 | none yet | record final SQLite-only verification and close the workstream |

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
   read/write execution boundary, including cancellation behavior.
2. Implement SQLite point reads, scans, and query generation with composite
   index support, and delete the redb-era index/key/scan modules once SQLite
   replaces them.
3. Implement validated direct writes, execution-unit batch apply,
   scheduler-state flows, `CommitEntry` generation, durable journal
   append/read/stream/bootstrap, and materialized snapshot export/restore/
   rebuild.
4. Simplify schema/index reconciliation so SQLite DDL and automatic index
   maintenance replace manual redb-era rewrite logic.
5. Run targeted parity verification before broader integration begins.

#### Focused verification

- `bash scripts/cargo-isolated.sh -- test -p neovex-storage`
- `bash scripts/cargo-isolated.sh -- test -p neovex-engine`
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
4. Make SQLite the default backend only after the benchmark gate clears, then
   rerun the full verification sweep with SQLite as the default.

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
- SQLite becomes the default backend only after parity and benchmark gates pass

### SB12-SB14. Remove redb and close out the migration

#### Implementation plan

1. Delete redb storage code, redb-specific flags, and the temporary
   redb-vs-SQLite selection scaffolding.
2. Simplify engine/runtime/storage entry points back to a single-backend model.
3. Update docs and plan ownership so SQLite is the only internal backend and
   deferred follow-on work lives in the external SQL plan.
4. Run the final SQLite-only verification sweep and close the workstream
   cleanly.

#### Focused verification

- `make check`
- `make test`
- `make clippy`
- `make ci` if practical

#### Acceptance criteria

- no active redb codepath or operator-facing redb promise remains
- docs and follow-on plan ownership match the landed architecture
- the benchmark gate that justified redb removal is recorded in the plan

---

## Execution Log

| Date | Item | Outcome | Summary | Verification | Next Step |
| --- | --- | --- | --- | --- | --- |
| 2026-04-06 | meta | documented | Authored the initial version of this file as a pluggable multi-backend plan with redb retained long-term. | doc review only | revise scope before activation |
| 2026-04-08 | SB0 | done | Re-scoped the historical pluggable-backend plan into a SQLite-only migration control plane. Moved future Postgres/MySQL work into `docs/plans/external-sql-storage-backends-plan.md`, kept `CommitEntry` and journal/snapshot semantics explicit, rejected hook-driven reactivity as the core contract, required composite SQLite indexes, added a benchmark gate before redb removal, clarified the SQLite-vs-Neovex boundary, and recorded the explicit deletion inventory plus planner, codec, and sequence decisions so implementers delete redb-era code instead of wrapping it. | docs-only review against the files listed in `Reviewed against`; no new code verification claimed | resolve the plan-level activation gate, then start `SB1` with a call-site inventory of the current engine/storage contract |
| 2026-04-08 | meta | activated | Promoted this control plane into active execution ownership by updating the plan index and `AGENTS.md`, and recorded the activation decision so fresh agent contexts start from the SQLite migration rather than treating it as a deferred design note. | docs-only update; no new code verification claimed | start `SB1` from the live call sites and mark it `in_progress` when implementation begins |
