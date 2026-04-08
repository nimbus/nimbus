# Plan: SQLite Storage Backend Migration

This file keeps its historical filename, but its scope is now SQLite-only:
move Neovex storage from redb to SQLite, benchmark SQLite against redb before
cutover, then remove redb. Postgres and MySQL now belong to a later plan.

---

## Status

- **Status:** `deferred`
- **Primary owner:** this plan
- **Activation gate:** developer approval plus an explicit sequencing decision
  with `docs/plans/encryption-at-rest-plan.md`, which still assumes a redb
  `StorageBackend` seam

## How To Use This Plan

- Read this before starting any SQLite storage migration work.
- Treat the current git worktree plus this plan's ledger as progress state.
- Resume any `in_progress` item before starting a new one.
- Checkpoint state here before stopping, handing off, or likely context loss.

## Control Plan Rules

### Status model

- `todo`: not started; eligible when hard dependencies and gate notes are met
- `in_progress`: actively being implemented; keep exactly one phase in this
  state per autonomous execution run
- `blocked`: cannot proceed until the recorded blocker is resolved
- `done`: acceptance criteria are met and verification has been recorded
- `deferred`: intentionally parked behind a product or benchmarking gate

---

## Context

### Why now

Neovex is pre-launch with zero production data. That gives us room to make a
clean backend replacement instead of carrying long-lived compatibility layers.

SQLite is now the target embedded backend because it gives us:

- a well-understood storage engine with mature tooling
- expression indexes and SQL query planning for the document model
- a practical path toward D1-adjacent storage semantics
- a simpler operational story than keeping a custom redb-only storage layer

redb remains relevant only during the migration window:

- as the baseline for correctness parity checks
- as the baseline for benchmark comparisons before removal
- as the source implementation we are replacing, not a permanent co-equal
  backend

Once the benchmark and parity gates are satisfied, this plan removes redb.

### Current verified state

The current engine/storage contract is narrower and more concrete than the
previous draft plan assumed:

- the async boundary already exists via `StorageEngine`,
  `TenantReadStorage`, `TenantWriteStorage`, and `UsageStorage`
- that async boundary still leaks concrete sync types through closures:
  `Arc<TenantStore>` and `&mut TenantWriteTransaction`
- the write path depends on `TenantWriteCommit<T>` and
  `TenantWriteOutcome<T>`, including cancellable-before-commit semantics
- direct mutations use validation callbacks
  (`update_document_validated`, `delete_document_validated`)
- execution units commit via `apply_resolved_write_batch(...)` and
  `apply_execution_unit_batch(...)` over `ResolvedWrite` and
  `ResolvedScheduleOp`
- query planning depends on cancellable point, scan, and composite index
  operations on both `TenantStore` and `TenantReadSnapshot`
- the reactive system is driven by engine-owned `CommitEntry` values, not by
  backend hooks
- durable journal streaming, bootstrap export, materialized snapshot
  export/restore, and durable/applied head tracking are first-class storage
  responsibilities
- scheduled execution deduplication is keyed by execution id
  (`scheduled:{job.id}`), not by function name

### Scope

This plan covers:

- SQLite as the only target replacement backend for Neovex internal storage
- a temporary redb-vs-SQLite migration window for parity tests and benchmarks
- a benchmark gate before changing the default and before deleting redb
- preserving the current engine-facing storage contract while swapping the
  backend implementation
- final removal of redb and migration-only backend-selection scaffolding

This plan does not cover:

- a general backend abstraction for future external databases
- Postgres or MySQL internal storage
- user-facing `env.DB` / `env.HYPERDRIVE` bindings
- a long-lived migration or compatibility layer for launched users

Because Neovex is pre-launch, this plan prefers a clean replacement over a
permanent dual-backend architecture. A redb-to-SQLite import tool is optional
and only worth adding if local developer migration friction becomes real.

### Cross-Plan Note

`docs/plans/encryption-at-rest-plan.md` is currently redb-specific. If this
SQLite migration activates, that plan cannot proceed unchanged. Before starting
implementation, explicitly choose one of:

- rewrite encryption-at-rest around SQLite
- defer encryption-at-rest until after SQLite lands
- retire the redb-specific parts of the current encryption plan

### Follow-On Ownership

- `docs/plans/external-sql-storage-backends-plan.md` owns future Postgres and
  MySQL internal storage work after this SQLite migration is stable

---

## Architecture

### Guiding decisions

1. Preserve the current engine/storage contract before changing backend
   internals.
2. Keep `CommitEntry` and the durable journal as the canonical reactive and
   replication model.
3. Use temporary migration scaffolding for redb vs SQLite instead of designing
   a fully general object-safe backend abstraction now.
4. Remove redb after parity and benchmark gates clear.

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

### Engine-facing contract to preserve

The SQLite path must support the behavior the engine already depends on today:

- async read execution via `execute(...)` and `execute_cancellable(...)`
- async write execution via `execute_write(...)` and
  `execute_write_cancellable(...)`
- `TenantWriteCommit<T>` and `TenantWriteOutcome<T>` semantics, including the
  current pre-commit cancellation behavior
- direct validated writes for insert/update/delete flows
- scheduled execution dedupe keyed by execution id
- execution-unit batch application over `ResolvedWrite` and
  `ResolvedScheduleOp`
- cancellable table scans and index scans, including composite exact-prefix and
  range scans
- `CommitEntry` generation during writes so the engine can continue calling
  `process_commit(...)`
- durable journal reads, streaming, bootstrap export, materialized snapshot
  export/restore, and journal progress tracking

This plan should not reframe the migration around:

- CRUD-only traits that omit batch operations, validation callbacks, or commit
  wrappers
- backend hooks as the primary reactive contract
- object-safety or factory abstractions designed around hypothetical future
  external backends

### Temporary migration selection

During the migration window, temporary backend selection is acceptable because
it is bounded and short-lived. The goal is to compare redb and SQLite on the
same engine contract, not to ship a permanent pluggable-backend architecture.

Use an enum-backed or otherwise explicitly temporary selection layer rather
than `Arc<dyn ...>` traits that force object-safety compromises up front.

```rust
enum StorageBackendSelection {
    Redb(...),
    Sqlite(...),
}
```

That temporary selection layer may exist in `neovex-storage`, `neovex-engine`,
or both, but it must preserve the current call surface and be deleted once
SQLite fully replaces redb.

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
- `metadata` continues to own sequence and journal-progress state

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

## Phase 1: Codify the Migration Contract

**Goal:** Define the SQLite migration around the actual current engine/storage
surface instead of a greenfield CRUD abstraction.

### SB1: Document the preserved contract from current call sites

- inventory the current engine call sites that depend on `TenantStore`,
  `TenantReadSnapshot`, `TenantWriteTransaction`, `TenantWriteCommit<T>`, and
  `TenantWriteOutcome<T>`
- define the minimal migration contract around those operations
- make journal/snapshot APIs explicit in the plan and in any migration-facing
  interfaces
- make scheduled execution dedupe and cancellation semantics explicit

### SB2: Introduce temporary redb-vs-SQLite selection scaffolding

- add temporary startup/backend selection that can route the engine to redb or
  SQLite during the migration window
- keep this scaffolding intentionally temporary and local to the migration
- do not introduce a permanent `StorageBackendFactory` abstraction designed for
  Postgres/MySQL in this phase

### SB3: Verification

- migration scaffolding compiles without changing behavior
- the plan and code agree on the preserved contract
- no object-safety workaround is required for this phase

---

## Phase 2: Implement SQLite Storage Parity

**Goal:** Implement SQLite with the same engine-visible behavior that redb
currently provides.

### SB4: Implement SQLite store foundation and async boundary

- add SQLite store open/create paths and the usage-store equivalent
- mirror the current async read/write execution boundary for SQLite
- configure WAL mode, synchronous level, busy timeout, and cache settings
- preserve the current write parallelism and cancellation behavior at the async
  boundary

### SB5: Implement SQLite read path and query/index parity

- implement point reads and cancellable table scans
- implement planner-facing index scans for:
  - exact equality
  - prefix scans
  - range scans
  - composite prefix-plus-range scans
- add parity tests that compare redb and SQLite results for the same query
  corpora

### SB6: Implement SQLite write path, scheduler, journal, and snapshot parity

- implement validated direct writes
- implement execution-unit batch apply using `ResolvedWrite` and
  `ResolvedScheduleOp`
- preserve scheduled execution dedupe keyed by execution id
- build `CommitEntry` during SQLite writes and keep engine reactivity
  commit-driven
- implement durable journal append/read/stream/bootstrap
- implement materialized journal snapshot export/restore/rebuild
- preserve durable/applied head tracking and recovery semantics

### SB7: Verification

- SQLite passes targeted storage tests for CRUD, scheduler, journal, recovery,
  and query planning
- commit-driven subscription fan-out still works end-to-end
- redb and SQLite agree on composite index and journal behavior for parity
  corpora

---

## Phase 3: Validate, Benchmark, and Cut Over

**Goal:** Prove SQLite is correct, measure it against redb, then make SQLite
the default backend.

### SB8: Wire engine, tests, and harnesses through SQLite

- update `Service`, `TenantRuntime`, and related helpers to work through the
  migration selection layer
- migrate tests and fixtures that currently depend on concrete redb-backed
  types
- run the full workspace verification suite with SQLite enabled

### SB9: Benchmark gate against redb

Run the same workloads against redb and SQLite on the same machine before
removing redb:

- document CRUD throughput
- point read latency
- indexed query latency, including composite index paths
- durable journal stream/bootstrap latency
- subscription fan-out latency
- concurrent multi-tenant mixed read/write load

This phase produces a checked-in benchmark report and an explicit go/no-go
decision for redb removal.

### SB10: Switch the default backend to SQLite

- make SQLite the default runtime backend during the final migration window
- keep redb available only as a temporary explicit benchmark/parity fallback if
  still needed
- do not add a permanent dual-backend promise to operator-facing docs

### SB11: Verification

- full `make test`, `make clippy`, `make check`, and `cargo fmt --check` are
  green with SQLite as the default
- benchmark report is recorded and reviewed
- any material SQLite regressions are either fixed or explicitly signed off
  before redb removal proceeds

---

## Phase 4: Remove redb

**Goal:** Delete the old backend and the temporary migration scaffolding once
SQLite is proven.

### SB12: Remove redb and migration-only selection code

- delete redb storage code and redb-specific feature flags
- delete temporary redb-vs-SQLite selection scaffolding
- delete redb-only tests and fixtures that no longer make sense post-migration
- simplify `Service`, `TenantRuntime`, and storage entry points back to a
  single-backend model

### SB13: Final cleanup and close-out

- update docs and verification guidance so SQLite is the only internal backend
- record any intentionally deferred follow-up work in
  `docs/plans/external-sql-storage-backends-plan.md`
- close out or revise redb-specific cross-plan assumptions, especially
  encryption-at-rest

### SB14: Verification

- workspace compiles and tests without redb
- no redb imports remain in the active codepath
- documentation no longer promises redb support
- the plan ledger records the benchmark gate that justified removal

---

## Phase Status Ledger

| Phase | Status | Summary | Hard Dependencies | Gate Note |
|-------|--------|---------|-------------------|-----------|
| SB1 | `todo` | Document preserved engine/storage contract | none | Must derive from actual call sites, not CRUD ideals |
| SB2 | `todo` | Add temporary migration selection scaffolding | SB1 | Temporary only; no permanent external-backend abstraction |
| SB3 | `todo` | Phase 1 verification | SB1, SB2 | Confirms contract and migration setup before SQLite work |
| SB4 | `todo` | SQLite store foundation and async boundary | SB3 | Preserve current cancellation and async semantics |
| SB5 | `todo` | SQLite read/query/index parity | SB4 | Must include composite indexes and cancellable scans |
| SB6 | `todo` | SQLite write/journal/scheduler parity | SB4 | Must preserve `CommitEntry`, journal, and snapshot behavior |
| SB7 | `todo` | Phase 2 verification | SB5, SB6 | Gate before engine-wide cutover |
| SB8 | `todo` | Engine/tests/harness integration | SB7 | Broad migration surface across engine and test helpers |
| SB9 | `todo` | Benchmark gate vs redb | SB8 | Required before removing redb |
| SB10 | `todo` | Switch default to SQLite | SB9 | redb may remain only as a temporary fallback |
| SB11 | `todo` | Phase 3 verification | SB10 | Benchmarks and full verification must be recorded |
| SB12 | `todo` | Remove redb and temporary scaffolding | SB11 | Delete redb once benchmark gate is satisfied |
| SB13 | `todo` | Final cleanup and cross-plan follow-up | SB12 | Close out docs and deferred follow-ons |
| SB14 | `todo` | Final verification | SB13 | Confirms SQLite-only state |

## Recommended Delivery Order

1. **SB1-SB3** — Codify the real current contract and add only temporary
   migration scaffolding.
2. **SB4-SB7** — Implement SQLite parity for reads, writes, journal, scheduler,
   and snapshots.
3. **SB8-SB11** — Integrate broadly, benchmark against redb, then make SQLite
   the default.
4. **SB12-SB14** — Remove redb and close out the migration cleanly.

## Verification Contract

| Phase | Required verification |
|-------|---------------------|
| SB1 | Contract inventory reviewed against current engine/storage call sites |
| SB2 | Temporary migration selection compiles without behavior change |
| SB3 | Phase 1 review recorded; no permanent object-safety abstraction introduced |
| SB4 | SQLite open/create paths work; async read/write boundary preserves cancellation semantics |
| SB5 | Point reads, scans, exact scans, prefix scans, range scans, and composite scans pass parity tests |
| SB6 | Direct writes, execution-unit batches, scheduler flows, durable journal, and snapshot export/restore pass targeted tests |
| SB7 | SQLite targeted suite green; subscription and journal parity verified end-to-end |
| SB8 | Full workspace verification green with SQLite wired through the engine and tests |
| SB9 | Checked-in benchmark report comparing redb vs SQLite on agreed workloads |
| SB10 | SQLite is the default backend; any temporary redb fallback is explicit and migration-only |
| SB11 | Full `make test`, `make clippy`, `make check`, and `cargo fmt --check` green with SQLite default |
| SB12 | Workspace compiles and tests without redb-specific runtime support |
| SB13 | Docs and plan index updated; cross-plan follow-ups recorded |
| SB14 | No active redb imports or operator-facing redb promises remain |

## Known Risks

| Risk | Severity | Mitigation |
|------|----------|-----------|
| SQLite JSON comparison and index ordering differ from current redb index semantics | HIGH | Add explicit parity corpora for exact/prefix/range/composite scans before cutover |
| Journal/snapshot parity is broader than a CRUD swap | HIGH | Keep journal/bootstrap/snapshot APIs explicit and verify them in targeted tests before engine-wide cutover |
| Migration touches many engine tests and fixtures that currently name concrete redb types | MEDIUM | Budget an explicit engine/test integration phase instead of calling the work a pure refactor |
| Benchmark gate may show unacceptable regressions and delay redb removal | MEDIUM | Require a checked-in benchmark report before default switch and before deletion |
| Encryption-at-rest plan still assumes redb | MEDIUM | Resolve sequencing with `docs/plans/encryption-at-rest-plan.md` before activation |
| Temporary migration scaffolding could become permanent by accident | MEDIUM | Mark it as migration-only in code and remove it in SB12 |

## Implementation Checkpoints

| Phase | Checkpoint | Next Step |
|-------|-----------|-----------|
| SB1 | none yet | inventory current engine/storage contract and record preserved operations |
| SB2 | none yet | add temporary redb-vs-SQLite selection scaffolding |
| SB3 | none yet | review Phase 1 contract and scaffolding before SQLite implementation |
| SB4 | none yet | implement SQLite store open/create paths and async boundary |
| SB5 | none yet | implement planner-facing SQLite reads and composite indexes |
| SB6 | none yet | implement SQLite writes, scheduler state, journal, and snapshot flows |
| SB7 | none yet | run targeted SQLite parity suite |
| SB8 | none yet | wire engine/tests/harnesses through SQLite |
| SB9 | none yet | run and record benchmark comparison vs redb |
| SB10 | none yet | switch default backend to SQLite |
| SB11 | none yet | run full workspace verification with SQLite default |
| SB12 | none yet | remove redb code and temporary migration selection |
| SB13 | none yet | update docs and follow-on plans |
| SB14 | none yet | record final SQLite-only verification |

## Execution Log

| Date | Phase | Outcome | Summary | Verification | Next Step |
|------|-------|---------|---------|--------------|-----------|
| 2026-04-06 | meta | documented | Initial version of this file was authored as a pluggable multi-backend plan with redb retained long-term. | doc review | revise scope before activation |
| 2026-04-08 | meta | revised | Re-scoped this file to a SQLite-only migration plan. Fixed the contract sketch to match current engine/storage behavior, removed hook-driven reactivity as the core model, made journal/snapshot APIs explicit, required composite SQLite indexes, added a benchmark gate before redb removal, moved Postgres/MySQL to a separate later plan, and added an explicit rubric for what SQLite should replace versus what remains Neovex-owned product semantics. | review against current `neovex-engine` and `neovex-storage` call sites | activate only after developer approval and encryption-plan sequencing decision |
