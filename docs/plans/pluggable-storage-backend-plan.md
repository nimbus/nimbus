# Plan: Pluggable Storage Backend

Canonical plan for abstracting the storage layer behind a backend-agnostic
trait boundary, implementing SQLite as the primary embedded backend, and
establishing the architecture for additional storage backends (Postgres,
MySQL) and user-facing database bindings (D1, Hyperdrive-style proxy).

---

## Status

- **Status:** `deferred`
- **Primary owner:** this plan
- **Activation gate:** developer approval; no code dependency on other plans

## How To Use This Plan

- Read this before starting any storage backend work.
- Treat the current git worktree plus this plan's ledger as progress state.
- Resume any `in_progress` phase before starting a new one.
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

Neovex is pre-launch with zero production data. The current redb storage
layer works but locks the system into a single embedded key-value backend
with no query engine, no SQL, no replication ecosystem, no encryption at
rest, and no path to user-facing D1/Postgres/MySQL database bindings.

The market requires:
- **D1 compatibility** (SQLite) for Workers/Vinext apps
- **Postgres** for enterprise and the Next.js ORM ecosystem (Prisma, Drizzle)
- **MySQL** for enterprise legacy and Aurora
- **Reactive document database** capabilities across all backends

Convex Cloud runs on Postgres internally, and self-hosted Convex supports
SQLite + Postgres. This validates the multi-backend approach.

### Role of redb going forward

redb is not the long-term production backend. SQLite replaces it as the
default embedded backend for production deployments. However, redb is
retained for three reasons:

1. **Reference implementation.** A trait boundary validated against only one
   backend is not a real abstraction. redb as a second implementation forces
   the seam to be honest — if the traits only work with SQLite, they are
   SQLite traits wearing a trait coat.

2. **Test harness.** redb's `InMemoryBackend` provides fast in-memory
   storage for unit tests that don't need SQL semantics. Until SQLite
   in-memory equivalents are proven equally fast and reliable in the test
   suite, redb remains a useful test backend.

3. **Benchmark baseline.** Real-world performance comparisons between redb
   and SQLite on the same trait boundary produce honest numbers. These
   benchmarks validate that the abstraction overhead is acceptable and
   identify any SQLite regressions relative to the known-working redb path.

redb may be retired after SQLite is proven in production and the trait
boundary is validated by a third backend (Postgres). Until then, it earns
its keep as the reference that keeps the abstraction honest.

### Current state

redb is **entirely encapsulated** within `neovex-storage`. The engine and
server crates never import redb directly. The existing seams:

| Seam | Status |
|------|--------|
| Async boundary traits (`StorageEngine`, `TenantReadStorage`, `TenantWriteStorage`, `UsageStorage`) | **Good** — already trait-based |
| Sync store types (`TenantStore`, `TenantReadSnapshot`, `TenantWriteTransaction`) | **Needs work** — concrete types held by engine |
| Error mapping | **Good** — all redb errors map to `Error::Storage` |
| Serialization | **Needs work** — MessagePack is baked into storage code |
| Engine references | **Needs work** — engine holds `Arc<RedbStorageEngine>`, `Arc<RedbTenantStorage>`, `Arc<TenantStore>` |

### Two distinct use cases for "database support"

1. **Neovex's own storage backend** — what stores tenant data, powers
   subscriptions, and runs the reactive document model. This is the storage
   engine (currently redb, future SQLite, Postgres, MySQL).

2. **User-facing database bindings** — what user code (Workers functions,
   Vinext apps) can query via `env.DB` (D1), `env.HYPERDRIVE` (Postgres/MySQL
   proxy), or ORMs. This is a service binding in the Workers API surface.

This plan covers #1. User-facing bindings (#2) depend on the Workers API
surface design and are out of scope here.

---

## Architecture

### Backend trait hierarchy

The goal is a trait boundary that lets the engine work with any storage
backend without knowing which one is active.

```
StorageBackend (new top-level trait)
├── TenantBackend (per-tenant operations)
│   ├── read: snapshot-based reads
│   ├── write: transactional writes with commit hooks
│   └── change_notifications: hook into reactive system
├── UsageBackend (cross-tenant metering)
└── BackendConfig (construction + configuration)
```

The key addition vs the current traits: **change notification hooks** as a
first-class part of the backend contract, so the reactive engine can
subscribe to changes regardless of whether the backend is redb, SQLite, or
Postgres.

### Backend kinds

| Backend | Embedding model | Primary use case | Change notifications |
|---------|----------------|-----------------|---------------------|
| **redb** (current) | Embedded, pure Rust | Existing behavior, lightweight | Custom journal sequence model |
| **SQLite** (rusqlite) | Embedded, bundled C | D1 compat, primary production | `update_hook` + `preupdate_hook` |
| **Postgres** (future) | External connection | Enterprise, existing infra | `LISTEN`/`NOTIFY` or logical replication |
| **MySQL** (future) | External connection | Enterprise legacy, Aurora | Binlog or polling |

### Embedded vs external backends

Embedded backends (redb, SQLite) run in-process with per-tenant file
isolation. External backends (Postgres, MySQL) connect over the network.

The trait hierarchy must accommodate both:
- Embedded: `BackendConfig::open(path)` → one file per tenant
- External: `BackendConfig::connect(url)` → one schema/database per tenant

The async boundary pattern differs:
- Embedded: `spawn_blocking` for sync I/O (current pattern)
- External: native async via connection pool (tokio-postgres, sqlx)

### Configuration

```toml
# neovex.toml or CLI flags
[storage]
backend = "sqlite"  # "redb" | "sqlite" | "postgres" | "mysql"

# Embedded backends
data_dir = "./data"

# External backends (future)
# database_url = "postgres://user:pass@host/db"
```

---

## Phase 1: Abstract the Storage Seam

**Goal:** Make the engine backend-agnostic without changing behavior. redb
remains the only implementation. All existing tests pass unchanged.

### SB1: Define backend traits

Define the new trait hierarchy in `neovex-storage/src/backend/traits.rs`:

```rust
/// Top-level backend factory.
pub trait StorageBackendFactory: Send + Sync + 'static {
    type Tenant: TenantBackend;
    type Usage: UsageBackend;

    async fn open_tenant(&self, tenant_id: &TenantId, path: &Path) -> Result<Self::Tenant>;
    async fn list_tenants(&self, data_dir: &Path) -> Result<Vec<TenantId>>;
    async fn open_usage(&self, path: &Path) -> Result<Self::Usage>;
    async fn delete_tenant(&self, tenant_id: &TenantId, path: &Path) -> Result<()>;
}

/// Per-tenant storage operations.
pub trait TenantBackend: Send + Sync + 'static {
    type ReadSnapshot: TenantReadOps + Send + 'static;
    type WriteTransaction: TenantWriteOps + Send + 'static;

    fn read_snapshot(&self) -> Result<Self::ReadSnapshot>;
    fn begin_write(&self) -> Result<Self::WriteTransaction>;
}

/// Read operations on a consistent snapshot.
pub trait TenantReadOps: Send + 'static {
    fn get_document(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>>;
    fn scan_table(&self, table: &TableName) -> Result<Vec<Document>>;
    fn scan_index(&self, table: &TableName, index: &IndexDefinition, bounds: &IndexBounds) -> Result<Vec<Document>>;
    fn get_schema(&self, table: &TableName) -> Result<Option<TableSchema>>;
    fn get_all_schemas(&self) -> Result<HashMap<TableName, TableSchema>>;
    fn list_tables(&self) -> Result<Vec<TableName>>;
    fn get_sequence(&self) -> Result<u64>;
    // ... scheduled job reads, journal reads, etc.
}

/// Write operations within a transaction.
pub trait TenantWriteOps: Send + 'static {
    fn insert_document(&mut self, table: &TableName, doc: &Document) -> Result<()>;
    fn update_document(&mut self, table: &TableName, doc: &Document) -> Result<()>;
    fn delete_document(&mut self, table: &TableName, id: &DocumentId) -> Result<()>;
    fn set_schema(&mut self, table: &TableName, schema: &TableSchema) -> Result<()>;
    fn commit(self) -> Result<CommitResult>;
    // ... scheduled job writes, index maintenance, etc.
}

/// Commit result with change information for reactive notifications.
pub struct CommitResult {
    pub sequence: u64,
    pub changes: Vec<ChangeRecord>,
}

pub struct ChangeRecord {
    pub table: TableName,
    pub document_id: DocumentId,
    pub kind: ChangeKind,
}

pub enum ChangeKind {
    Insert,
    Update,
    Delete,
}
```

### SB2: Implement redb backend behind new traits

Move current `TenantStore`, `TenantReadSnapshot`, `TenantWriteTransaction`
into `neovex-storage/src/backend/redb/` and implement the new traits. All
existing behavior preserved. The current async boundary
(`RedbStorageEngine`, `RedbTenantStorage`) wraps the new trait
implementations.

Directory structure after SB2:

```
neovex-storage/src/
├── backend/
│   ├── mod.rs          (trait definitions, re-exports)
│   ├── traits.rs       (StorageBackendFactory, TenantBackend, etc.)
│   └── redb/
│       ├── mod.rs      (RedbBackendFactory)
│       ├── store.rs    (RedbTenantBackend — wraps current TenantStore)
│       ├── read.rs     (RedbReadSnapshot — wraps current TenantReadSnapshot)
│       ├── write.rs    (RedbWriteTransaction — wraps current TenantWriteTransaction)
│       ├── index/      (moved from current index/)
│       ├── keys.rs     (moved from current keys.rs)
│       └── ...         (other redb-specific modules)
├── async_storage/      (unchanged — wraps backend traits)
├── lib.rs              (re-exports backend traits + redb default)
└── ...
```

### SB3: Remove concrete redb types from engine

Replace all `Arc<RedbStorageEngine>`, `Arc<RedbTenantStorage>`,
`Arc<TenantStore>` references in `neovex-engine` with trait-based
alternatives. The engine becomes generic over the backend or uses trait
objects.

**Approach:** Use trait objects (`Arc<dyn TenantBackend>`) rather than
generics to avoid monomorphization of the entire engine. The storage
backend is selected once at startup, not per-call.

### SB4: Verification

- All existing tests pass with zero behavior change
- No `redb` import exists outside `neovex-storage/src/backend/redb/`
- Engine compiles against traits, not concrete types
- `cargo check` with redb feature disabled compiles the engine (but not the
  binary, which needs at least one backend)

---

## Phase 2: SQLite Backend

**Goal:** Implement a fully functional SQLite backend using rusqlite that
passes all existing tests and adds reactive change notifications via
SQLite hooks.

### SB5: SQLite schema design

```sql
-- Per-tenant SQLite database (one .sqlite file per tenant)
-- Created by SqliteTenantBackend::open()

-- Document storage
CREATE TABLE IF NOT EXISTS documents (
    table_name TEXT NOT NULL,
    id TEXT NOT NULL,
    data TEXT NOT NULL,           -- JSON (via JSON1 extension)
    creation_time REAL NOT NULL,
    PRIMARY KEY (table_name, id)
);

CREATE INDEX IF NOT EXISTS idx_documents_table
    ON documents(table_name);

-- Dynamic indexes (created by set_schema)
-- CREATE INDEX idx_{table}_{field} ON documents(table_name, json_extract(data, '$.{field}'))

-- Schemas
CREATE TABLE IF NOT EXISTS schemas (
    table_name TEXT NOT NULL PRIMARY KEY,
    schema_data TEXT NOT NULL     -- JSON serialized TableSchema
);

-- Scheduled jobs
CREATE TABLE IF NOT EXISTS scheduled_jobs (
    id TEXT NOT NULL PRIMARY KEY,
    data TEXT NOT NULL            -- JSON serialized ScheduledJob
);

CREATE TABLE IF NOT EXISTS running_scheduled_jobs (
    id TEXT NOT NULL PRIMARY KEY,
    data TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS scheduled_job_results (
    job_id TEXT NOT NULL PRIMARY KEY,
    data TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS scheduled_job_executions (
    function_name TEXT NOT NULL PRIMARY KEY,
    data TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS cron_jobs (
    name TEXT NOT NULL PRIMARY KEY,
    data TEXT NOT NULL
);

-- Durable journal (replaces custom journal layer)
CREATE TABLE IF NOT EXISTS journal (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    mutation_data TEXT NOT NULL   -- JSON serialized DurableMutationRecord
);

-- Metadata
CREATE TABLE IF NOT EXISTS metadata (
    key TEXT NOT NULL PRIMARY KEY,
    value TEXT NOT NULL
);

-- Pragmas set on open
-- PRAGMA journal_mode = WAL;
-- PRAGMA synchronous = FULL;
-- PRAGMA busy_timeout = 5000;
-- PRAGMA foreign_keys = OFF;
-- PRAGMA cache_size = -8000;   -- 8MB cache
```

### SB6: Implement SqliteTenantBackend

Implement `TenantBackend`, `TenantReadOps`, `TenantWriteOps` for SQLite
using rusqlite in `neovex-storage/src/backend/sqlite/`.

Key implementation details:
- `SqliteTenantBackend` holds `rusqlite::Connection` (single connection per
  tenant, WAL mode allows concurrent reads via separate connections)
- Read snapshots use `BEGIN DEFERRED` transactions for consistent reads
- Write transactions use `BEGIN IMMEDIATE` for exclusive write access
- JSON1 extension for document storage and querying
- `json_extract()` for field-level index support

### SB7: SQLite change notifications

Wire `sqlite3_update_hook` and `sqlite3_preupdate_hook` into the
`CommitResult` returned by `TenantWriteOps::commit()`. This replaces the
custom journal-based change tracking:

```rust
// On SqliteTenantBackend::open():
conn.update_hook(Some(|action, db_name, table_name, rowid| {
    // Record change: table_name, rowid, INSERT/UPDATE/DELETE
    // Pushed to a channel that the engine subscribes to
}));

conn.preupdate_hook(Some(|action, db_name, table_name, ...| {
    // Record old/new values for fine-grained diff delivery
}));
```

The reactive engine (`neovex-engine`) receives `CommitResult` with change
records and fans out to subscriptions exactly as it does today with the
custom journal model.

### SB8: SQLite index management

Map the existing `IndexDefinition` model to SQLite indexes:

- `set_schema()` creates SQLite indexes via
  `CREATE INDEX IF NOT EXISTS idx_{table}_{field} ON documents(table_name, json_extract(data, '$.{field}'))`
- `scan_index()` generates `SELECT ... WHERE table_name = ? AND json_extract(data, '$.{field}') BETWEEN ? AND ?`
- Drop/recreate on schema change

This replaces `~800 lines` of custom index encoding, keyspace, bounds,
scan, and maintenance code with SQLite's native index engine.

### SB9: SQLite verification

- All existing engine and server tests pass against the SQLite backend
- Subscription fan-out works via `update_hook` (not custom journal)
- Index-backed queries use SQLite indexes (visible via `EXPLAIN QUERY PLAN`)
- fsync durability verified with crash-recovery test
- Performance comparison: SQLite vs redb on the existing benchmark harness

---

## Phase 3: Backend Configuration and Selection

**Goal:** Make the backend selectable at startup and establish the
architecture for future external backends.

### SB10: Backend selection

Add `--storage-backend` CLI flag and `NEOVEX_STORAGE_BACKEND` env var:

```
neovex --storage-backend sqlite --data-dir ./data
neovex --storage-backend redb --data-dir ./data
```

Default: `sqlite` (changed from redb after SB9 verification).

redb remains available for testing, benchmarking, and as the reference
implementation that keeps the trait boundary honest.

Implementation: `neovex-bin` constructs the appropriate
`StorageBackendFactory` at startup and passes it to `Service::new()`.

### SB11: Feature flags

```toml
[features]
default = ["backend-sqlite", "backend-redb"]
backend-redb = ["dep:redb"]
backend-sqlite = ["dep:rusqlite"]
# Future:
# backend-postgres = ["dep:tokio-postgres"]
# backend-mysql = ["dep:sqlx"]
```

Both backends are included by default during development. Production
builds may exclude redb once SQLite is proven. The test suite runs against
both backends to validate the trait boundary.

### SB12: Cross-backend benchmark harness

Extend the existing benchmark infrastructure to run the same workloads
against both backends on the same hardware:

- Document CRUD throughput (insert/update/delete per second)
- Point read latency (p50, p99)
- Index scan latency (p50, p99)
- Subscription fan-out latency (mutation to WebSocket push)
- Concurrent tenant load (50 active tenants, mixed read/write)

These benchmarks produce the real-world numbers that justify the SQLite
default and identify any regressions. They also validate that the trait
boundary abstraction overhead (vtable dispatch) is not measurable relative
to I/O costs.

### SB13: Data migration utility

Since Neovex is pre-launch, migration between backends is a
nice-to-have, not a blocker. But for development and any early adopters:

```
neovex migrate --from redb --to sqlite --data-dir ./data
```

Reads all tenants from the source backend, writes to the target. Uses the
backend trait boundary — the migration tool is backend-agnostic.

---

## Phase 4: External Backend Architecture (Future)

**Goal:** Establish the pattern for Postgres and MySQL backends. These are
deferred until post-launch but the trait design from Phase 1 must
accommodate them.

### Design considerations for external backends

| Concern | Embedded (redb, SQLite) | External (Postgres, MySQL) |
|---------|------------------------|---------------------------|
| Tenant isolation | One file per tenant | One schema or database per tenant |
| Async model | `spawn_blocking` for sync I/O | Native async (`tokio-postgres`, `sqlx`) |
| Connection management | Open file handle | Connection pool per tenant |
| Change notifications | `update_hook` / custom journal | `LISTEN`/`NOTIFY` (Postgres) or polling (MySQL) |
| Transactions | Local ACID | Network-round-trip ACID |
| Latency | Microseconds (local NVMe) | Milliseconds (network) |
| Single binary | Yes | Requires external database server |

The `StorageBackendFactory` trait already accommodates this — `open_tenant`
can open a file or establish a connection pool. The async boundary differs
(embedded uses `spawn_blocking`, external uses native async) but both
return the same trait objects.

### Postgres backend (SB14, future)

- `tokio-postgres` or `sqlx` for async Postgres
- One Postgres schema per tenant (`CREATE SCHEMA tenant_{id}`)
- Same table structure as SQLite but with Postgres-native types
- `LISTEN`/`NOTIFY` for change notifications
- Connection pooling via `deadpool-postgres` or `bb8`
- Enables the Convex Cloud-compatible deployment model

### MySQL backend (SB15, future)

- `sqlx` with MySQL driver
- One MySQL database per tenant
- Binlog-based change detection or polling fallback
- Enables enterprise Aurora/PlanetScale deployments

### User-facing database bindings (separate plan)

User-accessible `env.DB` (D1) and `env.HYPERDRIVE` (Postgres/MySQL proxy)
are **Workers API surface features**, not storage backend features. They
belong in the Workers compatibility plan, not here. The storage backend is
what Neovex uses internally; user-facing bindings are what user code
accesses through the runtime.

When the SQLite backend is active, `env.DB` can expose the tenant's SQLite
database directly — this is D1 compatibility for free.

---

## Phase Status Ledger

| Phase | Status | Summary | Hard Dependencies | Gate Note |
|-------|--------|---------|-------------------|-----------|
| SB1 | `todo` | Define backend traits | none | ~200 lines in new `backend/traits.rs` |
| SB2 | `todo` | Implement redb behind new traits | SB1 | Move + wrap existing code; zero behavior change |
| SB3 | `todo` | Remove concrete redb types from engine | SB2 | Replace `Arc<RedbStorageEngine>` etc. with trait objects |
| SB4 | `todo` | Verification: all tests pass, no redb leakage | SB3 | Gate before proceeding to Phase 2 |
| SB5 | `todo` | SQLite schema design | SB4 | Design doc + CREATE TABLE statements |
| SB6 | `todo` | Implement SqliteTenantBackend | SB5 | `TenantBackend` + `TenantReadOps` + `TenantWriteOps` for rusqlite |
| SB7 | `todo` | SQLite change notifications via hooks | SB6 | `update_hook` + `preupdate_hook` wired to `CommitResult` |
| SB8 | `todo` | SQLite index management | SB6 | `json_extract`-based indexes, schema-driven creation |
| SB9 | `todo` | Verification: all tests pass on SQLite | SB6, SB7, SB8 | Performance comparison vs redb |
| SB10 | `todo` | Backend selection (CLI + env var) | SB4, SB9 | Default changed to SQLite; redb retained as reference backend |
| SB11 | `todo` | Feature flags for compile-time backend selection | SB10 | Both backends included by default; test suite runs against both |
| SB12 | `todo` | Cross-backend benchmark harness | SB9, SB10 | Real-world performance comparison: SQLite vs redb on same traits |
| SB13 | `todo` | Data migration utility | SB10 | Nice-to-have, not a blocker |
| SB14 | `deferred` | Postgres backend | SB4 | Post-launch; enterprise feature |
| SB15 | `deferred` | MySQL backend | SB4 | Post-launch; enterprise feature |

## Recommended Delivery Order

1. **SB1-SB4** (Phase 1) — Abstract the seam. Zero behavior change. All
   tests pass. This is a pure refactor.
2. **SB5-SB9** (Phase 2) — SQLite backend. New capability. Verified against
   existing test suite plus SQLite-specific tests.
3. **SB10-SB11** (Phase 3) — Make it configurable. Change the default.
4. **SB12** — Cross-backend benchmarks. Validates SQLite default with real
   numbers.
5. **SB13** — Migration utility. Nice-to-have.
6. **SB14-SB15** — External backends. Post-launch, driven by enterprise
   demand.

## Verification Contract

| Phase | Required verification |
|-------|---------------------|
| SB1 | Trait definitions compile; existing code unchanged |
| SB2 | All existing tests pass; redb code moved but not modified |
| SB3 | Engine compiles against trait objects; no `use redb` outside backend/redb/ |
| SB4 | Full `make test`, `make clippy`, `cargo fmt --check` green |
| SB5 | SQL schema reviewed; CREATE TABLE statements verified in sqlite3 CLI |
| SB6 | Document CRUD round-trips through SQLite backend |
| SB7 | `update_hook` fires for all mutation types; `CommitResult` contains correct `ChangeRecord`s; subscription fan-out verified end-to-end |
| SB8 | Indexed query uses SQLite index (verified via EXPLAIN QUERY PLAN); schema change rebuilds index |
| SB9 | Full test suite passes on SQLite; latency comparison vs redb within 2x for reads, writes; fsync durability verified |
| SB10 | `--storage-backend sqlite` and `--storage-backend redb` both work; default is sqlite |
| SB11 | `cargo check --no-default-features --features backend-sqlite` compiles without redb; full test suite passes against both backends |
| SB12 | Benchmark report: SQLite vs redb for CRUD throughput, point read latency, index scan latency, subscription fan-out, concurrent tenant load; trait abstraction overhead is not measurable relative to I/O |
| SB13 | `neovex migrate --from redb --to sqlite` completes without data loss |

## Known Risks

| Risk | Severity | Mitigation |
|------|----------|-----------|
| Trait abstraction adds runtime overhead (vtable dispatch) | LOW | Storage I/O dominates; vtable cost is nanoseconds vs microsecond-millisecond I/O |
| SQLite JSON1 query performance for document model | MEDIUM | Benchmark in SB9; `json_extract` indexes mitigate; SQLite JSON performance is well-studied |
| SQLite WAL checkpoint starvation under sustained writes | LOW | `PRAGMA wal_autocheckpoint` handles this; well-documented SQLite behavior |
| `preupdate_hook` API stability in rusqlite | LOW | Feature has existed since rusqlite 0.28; SQLite API is stable since 3.18 (2017) |
| Large refactor touches many files in Phase 1 | MEDIUM | SB2 is a pure move+wrap, no logic change; existing tests are the safety net |
| Engine generic/trait-object boundary design | MEDIUM | Use `Arc<dyn TenantBackend>` not generics; avoids monomorphization explosion |

## Implementation Checkpoints

| Phase | Checkpoint | Next Step |
|-------|-----------|-----------|
| SB1 | none yet | define `StorageBackendFactory`, `TenantBackend`, `TenantReadOps`, `TenantWriteOps` in new module |
| SB2 | none yet | create `backend/redb/` directory, move existing store code, implement traits |
| SB3 | none yet | replace `Arc<RedbStorageEngine>` with `Arc<dyn StorageBackendFactory>` in Service |
| SB4 | none yet | run full test suite, verify no redb imports outside backend/redb/ |
| SB5 | none yet | write and verify SQL schema in sqlite3 CLI |
| SB6 | none yet | implement SqliteTenantBackend with read/write operations |
| SB7 | none yet | wire update_hook + preupdate_hook to CommitResult |
| SB8 | none yet | implement json_extract-based index creation and querying |
| SB9 | none yet | run full test suite on SQLite backend, benchmark comparison |
| SB10 | none yet | add CLI flag and env var for backend selection |
| SB11 | none yet | add Cargo feature flags |
| SB12 | none yet | extend benchmark harness to run same workloads against both backends |
| SB13 | none yet | implement backend-agnostic migration tool |

## Execution Log

| Date | Phase | Outcome | Summary | Verification | Next Step |
|------|-------|---------|---------|--------------|-----------|
| 2026-04-06 | meta | documented | Initial plan authored based on architecture discussion covering redb limitations, SQLite advantages for reactive document DB, D1/Workers compatibility, and enterprise database requirements. Convex Cloud confirmed to use Postgres internally with self-hosted supporting SQLite + Postgres. Current redb seam mapped: fully encapsulated in neovex-storage, async traits exist, sync types need abstraction, engine holds concrete types. | review of neovex-storage crate structure, async_storage traits, engine Service struct, and external market research | activate when developer approves |
