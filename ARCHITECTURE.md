# Architecture

Neovex is a single-binary reactive document database. Clients subscribe to
queries over WebSocket and receive automatic pushes when data changes. Each
tenant gets an isolated persistence namespace: embedded deployments use local
per-tenant databases, while external providers preserve the same isolation
through provider-owned schemas or databases. Scaling is by distributing tenants
across nodes, not by sharding within one logical tenant database.

This document describes the stable architecture. It is intentionally kept at
the level of crates, key types, and data flows — not individual functions.
Keep it in sync with every commit.

---

## Tech Stack

```mermaid
flowchart TD
    T1["Client · TypeScript, React"]

    subgraph rust["Rust · single binary"]
        T2["Server · axum, WebSocket"]
        T3["Runtime · V8 (via deno_core)"]
        T4["Engine"]
        T5["Storage · SQLite / redb, JSON / blobs"]

        T2 --- T3 & T4
        T3 & T4 --- T5
    end

    T1 --- T2

    classDef client fill:#e1f5fe,stroke:#0288d1,color:#000
    classDef server fill:#fff3e0,stroke:#ef6c00,color:#000
    classDef runtime fill:#e0f2f1,stroke:#00796b,color:#000
    classDef logic fill:#e8f5e9,stroke:#2e7d32,color:#000
    classDef storage fill:#fce4ec,stroke:#c62828,color:#000

    class T1 client
    class T2 server
    class T3 runtime
    class T4 logic
    class T5 storage
```

Colors match the overview diagram below. The subgraph communicates the
language; each node lists only the technologies that define that layer. Engine
has no framework — it is pure Rust logic. Cross-cutting dependencies used
across all layers include `tokio` (async runtime), `serde` (serialization),
and `tracing` (observability). Additional dependencies include `ring`
(JWT/JWKS auth), `clap` (CLI), and `reqwest` (JWKS fetching).

---

## Overview

```mermaid
flowchart TD
    Client["Client · HTTP + WebSocket"]

    subgraph binary["neovex · single binary"]
        Server["neovex-server · transport"]
        Engine["neovex-engine · logic"]
        Runtime["neovex-runtime · V8 execution"]
        Sandbox["neovex-sandbox · isolation seam"]
        Storage["neovex-storage · persistence"]
        Core["neovex-core · types"]

        Server --> Engine
        Server --> Runtime
        Server --> Sandbox
        Runtime -.->|HostBridge| Engine
        Sandbox --> Core
        Engine --> Storage
        Engine & Storage --> Core
    end

    Disk[("Embedded tenant persistence")]

    Client <-->|HTTP / WS| Server
    Storage <-->|read / write| Disk

    classDef client fill:#e1f5fe,stroke:#0288d1,color:#000
    classDef transport fill:#fff3e0,stroke:#ef6c00,color:#000
    classDef runtime fill:#e0f2f1,stroke:#00796b,color:#000
    classDef logic fill:#e8f5e9,stroke:#2e7d32,color:#000
    classDef storage fill:#fce4ec,stroke:#c62828,color:#000
    classDef types fill:#f3e5f5,stroke:#7b1fa2,color:#000
    classDef disk fill:#eceff1,stroke:#546e6a,color:#000

    class Client client
    class Server transport
    class Runtime runtime
    class Engine logic
    class Storage storage
    class Core types
    class Disk disk
```

Solid arrows are Cargo dependencies. The dotted arrow is a runtime data flow:
V8 handler code makes host calls that the server's bridge implementation
routes to the engine. At the crate level, the runtime has zero workspace
dependencies.

**neovex-server** is the integration point. It owns all network I/O and
connects the two independent subsystems below it:

- **neovex-engine** is the central coordinator. Every read, write,
  subscription, and scheduled job flows through its `Service` struct. It
  depends on `neovex-storage` for persistence and `neovex-core` for shared
  types. This is the data path: server → engine → storage → core.

- **neovex-runtime** is a standalone V8 execution environment with zero
  workspace dependencies. It defines a `HostBridge` trait that declares what
  host operations a V8 handler can perform (`ctx.db.*`, `ctx.scheduler.*`,
  `ctx.run*`). The server implements that trait in
  `adapters/convex/host_bridge/` by calling into the engine's `Service` — so
  at runtime, V8 handler code reaches the engine, but at the crate level, the
  runtime knows nothing about it. This is dependency inversion: the runtime
  declares what it needs; the server provides it.

- **neovex-sandbox** is the generic isolation and sandbox-orchestration seam.
  It currently exposes backend-agnostic sandbox lifecycle contracts and the
  first server-facing catalog seam, while concrete krun-backed, Firecracker,
  and other backend implementations remain deferred.

The server has two request paths. **Native** requests (Neovex HTTP/WS API) go
directly to async engine methods; read and write paths now await an explicit
async storage boundary that owns backend blocking work. Durable writes cross
that boundary through `TenantWriteTransaction`, which defines an explicit
pre-commit versus post-commit split: cancellation may abort before durable
commit, but once the durable commit point is crossed the engine returns a
committed result even if the transport disconnects before observing it. Tenant
persistence now lowers through `ServicePersistenceConfig`: SQLite is the
default embedded provider, redb is retained as another embedded provider, and
Postgres, MySQL, plus replica-connected SQLite are opt-in external provider
families. The cross-tenant usage or control path lowers through a separate
control-plane provider seam and remains embedded redb-backed today. **Convex**
requests go to the runtime, which executes a V8 handler; async host operations
inside that handler now await the engine and storage futures directly instead
of bouncing through a Tokio `spawn_blocking(...)` adapter layer.

This leads to a deliberate two-tier logic model. V8 and `deno_core` remain a
first-class execution surface for Convex compatibility, JavaScript portability,
and the existing function-oriented developer model. At the same time, the
long-term Neovex-native surface should keep moving toward schema-driven CRUD,
planner-enforced policy, and, when needed, a database-native WASM plugin ABI
for tightly scoped extensions. WASM is therefore an additive path for Neovex,
not a planned replacement for the Convex compatibility runtime.

When that Neovex-native path lands, it should follow the same broad patterns
used by systems such as PostgREST, Hasura, and Wasmtime: a schema-owned public
API contract, planner-enforced policy, and a typed, capability-scoped plugin
ABI rather than an untyped general escape hatch.

The Convex surface also depends on a build-time pipeline: `packages/codegen`
(Node.js) reads application source and emits a function manifest
(`functions.json`), a runtime ESM bundle (`bundle.mjs`), and an integrity hash
(`bundle.sha256`). The server loads these at startup; the runtime verifies the
hash before every invocation.

The **neovex** facade crate re-exports the stable, embedder-oriented surface of
the workspace so embedders can usually depend on a single crate. Low-level
server-construction helpers such as localhost security internals, discovery or
token lifecycle records, and router-builder overloads remain owned by
**neovex-server** directly. The **neovex-bin** crate is the CLI entry point.

---

## Code Map

Each crate has a single responsibility. When looking for where something
lives, use this map. Search for type and function names rather than following
file links (links go stale; symbol search does not).

**`neovex-core`** — Shared types and validation. Zero I/O, zero external deps.

- `auth/` — auth and access-policy composition root. `mod.rs` owns
  `PrincipalContext`, `PrincipalSnapshot`, `PrincipalClaimSource`, and policy
  revision fingerprinting; `access.rs` owns table access-policy structures,
  predicate evaluation, and read-filter compilation.
- `types.rs` — `TenantId`, `TableName`, `DocumentId`, `SequenceNumber`, `Timestamp`. All validated on construction (alphanumeric + `_` + `-`, max 128 chars).
- `document.rs` — `Document` struct. Serializes to JSON for wire, while
  backend-specific persistence codecs live below the engine (`MessagePack` in
  legacy redb paths, JSON-at-rest in SQLite). System fields `_id` and
  `_creationTime` are added during JSON serialization.
- `mutation.rs` — `Mutation` enum (`Insert`/`Update`/`Delete`),
  `DurableMutationRecord`, `CommitEntry`, `WriteOp`. The durable journal
  records every mutation; `CommitEntry` is the applied compatibility view used
  by existing engine and transport surfaces.
- `query.rs` — `Query`, `Filter`, `FilterOp`, `OrderBy`. Also `PaginatedQuery`, `Cursor`, `Page` for cursor-based pagination.
- `schema.rs` — `Schema`, `TableSchema`, `FieldSchema`, `FieldType`, `IndexDefinition`. Schema is optional per-table. Validation checks required fields and type matching.
- `scheduled.rs` — `ScheduledJob`, `CronJob`, `CronSchedule`, `ScheduledJobResult`. Interval-based cron.
- `error.rs` — `Error` enum with variants mapped to HTTP status codes in the server layer.

**`neovex-storage`** — Persistence layer. Tenant persistence now runs behind
the durable provider seam, with SQLite as the default embedded provider, redb
retained as another embedded provider, and the global usage store lowered
through a separate embedded redb control-plane provider for cross-tenant
metering.

- `async_storage/` — internal async storage boundary composition root.
  `traits.rs` owns the async storage contracts and `TenantWriteOutcome`,
  `read.rs` owns blocking read execution plus backend-specific tenant and usage
  read adapters, `write.rs` owns blocking write execution and the tenant write
  adapter, `engine.rs` owns `EmbeddedProviderKind` plus the retained embedded
  tenant providers, `control.rs` owns the explicit embedded redb control-plane
  provider, and `helpers.rs` owns shared blocking-task error mapping. The
  boundary still preserves the same cancellable pre-commit versus
  committed-write semantics.
- `sqlite.rs` — SQLite provider composition root. The root now only holds the
  shared store and transaction types plus module wiring while `sqlite/config.rs`
  owns connection opening and pooling, `read.rs` owns snapshot-backed document
  reads and the public read facade, `write.rs` owns transaction-backed document
  writes and execution-unit batch application, `scheduler.rs` owns scheduled
  job and cron store surfaces, `journal.rs` owns durable-journal and
  materialized-snapshot flows, `schema.rs` owns schema-cache and SQLite index
  helpers, and `backend.rs` owns low-level SQLite row or codec utilities.
- `postgres.rs` — Postgres provider composition root. The CM7 split has
  moved provider config, identifier or naming helpers, bootstrap SQL, and pool
  construction into `postgres/config.rs`, LISTEN or NOTIFY payload or listener
  coordination into `postgres/notifications.rs`, provider connect or
  tenant-open flows into `postgres/provider.rs`, the async storage bridge plus
  blocking-write executor into `postgres/storage.rs`, the public read facade
  plus snapshot ownership into `postgres/read.rs`, and the public write facade
  plus transaction ownership into `postgres/write.rs`. `postgres/backend.rs`
  now owns the shared SQL, codec, session, and durable-journal helpers that
  the concept-owned modules reuse, while the root only keeps the shared store
  types, runtime bridge helper, and module wiring.
- `mysql.rs` — MySQL provider composition root. The CM7 split has moved
  provider connect or tenant-open flows into `mysql/provider.rs`, the async
  storage bridge plus blocking-write executor into `mysql/storage.rs`, the
  public read facade plus snapshot ownership into `mysql/read.rs`, and the
  public write facade plus transaction ownership into `mysql/write.rs`.
  `mysql/backend.rs` now owns the shared SQL, identifier, codec, and
  durable-journal helpers that the concept-owned modules reuse, while the root
  keeps the provider config, shared store types, runtime bridge helper, and
  module wiring.
- `libsql.rs` — libsql replica-provider composition root. The completed CM7
  split moved provider connect/open, namespace lifecycle, tenant snapshot
  materialization, metadata-namespace ownership, and opened-tenant accessors
  into `libsql/provider.rs`; the async storage bridge plus blocking-write
  executor into `libsql/storage.rs`; the public read facade into
  `libsql/read.rs`; the public write facade plus transaction ownership into
  `libsql/write.rs`; the remote namespace, snapshot, and admin helpers into
  `libsql/remote.rs`; and the custom transport connector into
  `libsql/transport.rs`. `libsql/freshness.rs` now owns the replica-cache
  freshness state machine and metrics, and `libsql/backend.rs` owns the shared
  libsql codec, bootstrap, and error-mapping helpers. The root keeps the
  shared store types, provider config, transport surface, and module wiring.
- `store.rs` — `TenantStore` wrapping a redb `Database`: the storage
  composition root. It now keeps the shared table definitions and public store
  types while routing the remaining storage concerns through the
  `store/` module tree.
- `store/write.rs` — durable write-path composition root. It now composes
  `store/write/transaction.rs` (`TenantWriteTransaction` lifecycle and commit
  ownership), `scheduled.rs` (scheduled-write deduplication and scheduled-op
  batch integration), `direct.rs` (direct document CRUD helpers plus the
  public `TenantStore` CRUD write surface), `batch.rs` (execution-unit batch
  apply and durable batch commit ownership), and `store_entry.rs`
  (`TenantStore` construction, transaction entry, and write-execution
  helpers).
- `store/journal.rs` — durable journal append, read, replay, apply, recovery,
  and metadata-sequence ownership.
- `store/read.rs` — `TenantReadSnapshot`, document reads, table scans,
  sequence and journal progress reads, and read-snapshot ownership.
- `store/scan.rs` — conservative scan pushdown, scan metrics, and low-level
  MessagePack field probing for scan-time filtering.
- `store/schema_rewrite.rs` — durable schema-aware index rewrite helpers used
  during journal replay and recovery.
- `store/journal_snapshot.rs` / `store/journal_stream.rs` — materialized
  snapshot export/restore/rebuild and durable-journal bootstrap/streaming
  helpers for the authoritative journal model.
- `keys.rs` — Key construction for the DOCUMENTS table. Prefix-based range scans for table isolation.
- `index/` — Composition root for storage indexing ownership. `encoding.rs`
  owns order-preserving scalar and tuple encoding, `keyspace.rs` owns index
  key construction and prefix layout, `bounds.rs` owns composite range-bound
  synthesis, `scan.rs` is now the read-side composition root over
  `scan/read.rs`, `exact.rs`, `prefix.rs`, `range.rs`, and `adapters.rs`, and
  `maintenance.rs` is now the write-side composition root over
  `maintenance/transaction.rs`, `writes.rs`, and `rebuild.rs`.
- `schema_store.rs` — Schema persistence. `replace_table_schema` atomically updates schema and rebuilds indexes in one transaction.
- `scheduler/` — scheduled-work persistence composition root. `jobs.rs` owns
  pending or running job transitions plus the public scheduled-job CRUD
  surface, `results.rs` owns executed-job result persistence and lookup,
  `cron.rs` owns cron CRUD plus enabled-cron next-run scans, `inspection.rs`
  owns next-work and has-work inspection helpers, `recovery.rs` owns
  orphaned-running-job recovery, and `codec.rs` owns the shared scheduler
  key and MessagePack helpers. `claim_due_jobs` still atomically moves due
  jobs from pending to running, and `recover_running_jobs` still handles
  crash recovery.
- `commit_log.rs` — Durable mutation journal serialization plus the internal
  `CommitEntry` projection used by legacy storage-facing helpers and tests.
- `usage_store.rs` — `UsageStore` backed by a separate redb database (`neovex-control.db`). Tracks monthly active users (MAU) by token identifier with per-month counters.

**`neovex-engine`** — Central coordinator. Every read, write, subscription, and scheduled job flows through the `Service` struct — whether the request originates from native HTTP, WebSocket, the background scheduler, or a runtime host operation.

- `service/mod.rs` — `Service` struct: tenant registry plus the async storage
  boundary, the embedded provider selector, the still-local redb usage/control
  storage handle, simulation seams, scheduler wakeups, and a process-wide
  background Tokio runtime handle used for long-lived engine workers.
  `service/bootstrap.rs` now owns typed-persistence bootstrap, control-plane
  provider construction, and provider-specific service startup so `Service`
  stays focused on coordination and runtime access once boot completes.
- `persistence_config.rs` — Typed service-persistence configuration plus local
  encryption policy. It now also owns the private bootstrap-plan normalization
  that turns provider selection, routing, pool settings, and control-plane
  paths into one canonical service-start input.
- `persistence.rs` — Provider-facade composition root. `provider.rs` owns the
  tenant-provider registry, background-task selection, and opened-tenant
  mapping, `control.rs` owns the redb-backed control-plane usage facade,
  `tenant.rs` is now a thin composition root over `tenant/reads.rs`,
  `tenant/writes.rs`, `tenant/journal.rs`, `tenant/scheduler.rs`,
  `tenant/schema.rs`, and `tenant/provider_state.rs`, where async schema load,
  journal progress/recovery, scheduled-work checks, refresh planning, and
  provider-specific apply semantics now live behind the tenant persistence
  facade instead of in service-layer matches. `executor.rs` owns async read or
  write execution, `snapshot.rs` owns the snapshot read facade, `query.rs`
  owns `QueryReadStore` delegation for stores and snapshots, and `write_ops.rs`
  owns the write-transaction capability trait shared by the scheduler and async
  journal paths.
- `service/mutations.rs` — Composition root for the write path. Public
  `apply_mutation` behavior still flows through the same single durable contract
  while the implementation is split across
  `service/mutations/direct/`, where `api.rs` owns the direct CRUD service
  surface plus async/principal/cancellable wrapper normalization, `execution.rs`
  owns execution-mode dispatch and mutation auth staging, `store.rs` owns the
  direct store-apply helpers, and `types.rs` owns the shared execution
  mode/result contract. The remaining write implementation is split across
  `service/mutations/authorization.rs` (shared mutation
  access-policy enforcement), `service/mutations/commit_processing.rs`
  (post-commit cache invalidation plus subscription work enqueue after the
  applied watermark advances), and `service/mutations/journal.rs` (queued async
  journal flow). A tenant-local background worker still owns reactive
  re-evaluation so committed writes no longer wait on full subscription fan-out.
  The durable-journal batch path still coalesces cache invalidation and
  subscription wakeups across multi-record apply batches before handing work to
  that background queue.
- `service/mutations/journal.rs` — Queued async mutation path. Async mutations
  first enter a tenant-local outer admission gate with CoDel shedding. A
  journal worker task spawned on the Service-owned background runtime drains
  that gate into the commit path, preserves admitted work once it crosses into
  the journal path, and owns durable append, ordered apply, and resolution of
  queued async mutation futures.
- `service/execution_units/` — Runtime multi-step mutation execution-unit
  ownership. `mod.rs` owns `MutationExecutionUnit` construction plus the stable
  public surface, `reads.rs` owns snapshot-backed read helpers and dependency
  capture, `staging.rs` owns staged document and scheduler mutation state
  transitions, `state.rs` owns staged-state lifecycle plus resolved write or
  schedule-op construction, and `commit.rs` owns finalization, schema-stability
  checks, and OCC conflict validation before the batch commit path.
- `service/queries.rs` — Composition root for the read path. The public
  `Service` read surface is now split across `service/queries/documents.rs`
  (list/get document reads), `query_api.rs` (query and pagination entrypoints),
  `journal.rs` (durable-journal and latest-sequence reads), `verification.rs`
  (shadow-materializer and consistency verification helpers), and
  `test_hooks.rs` (test-only read instrumentation and pause handles), while the
  private capability tree stays in `authorization.rs`, `planner/`,
  `prepared.rs`, `materialized.rs`, and `snapshot.rs`. The planner root now
  composes `exact.rs` (exact-prefix planning), `range.rs` (range-bound
  derivation and range-plan selection), and `scoring.rs` (candidate scoring and
  order support). Physical document loading now flows through the narrow
  storage-owned `QueryReadStore` seam rather than a redb-specific loading
  adapter. Read planning still merges declarative authorization predicates
  before selecting a semantic index-equality, range, or table-scan path, and
  async read paths still route the same prepared planner/evaluator logic
  through the storage-owned async executor.
- `service/subscriptions.rs` — `subscribe`/`unsubscribe` plus subscription
  lifecycle ownership. Initial evaluation and activation handoff now live under
  `service/subscriptions/bootstrap.rs`, which owns materialized-surface reuse,
  principal snapshot capture, policy revision tracking, and covered-sequence
  bootstrap semantics for conservative auth invalidation and catch-up.
- `service/schema.rs` — Schema CRUD. Setting a schema backfills indexes for existing documents.
- `service/scheduler.rs` — Composition root for the scheduler service
  surface. `service/scheduler/scheduled_jobs.rs` owns scheduled-job CRUD,
  result persistence, and async/cancellable scheduled-write helpers;
  `service/scheduler/cron.rs` owns cron CRUD; `service/scheduler/access.rs`
  owns the shared tenant-runtime/store access wrappers used by scheduler
  operations; and `service/scheduler/coordination.rs` owns loaded-tenant
  scans, next-due work discovery, and startup recovery via
  `load_tenants_with_scheduled_work`.
- `service/tenants.rs` — Tenant CRUD and lifecycle management. Create/delete now use async storage-engine control APIs; deletion evicts the tenant from the registry, rejects new work through a tenant-local lifecycle primitive, waits for in-flight operations to drain, then removes the on-disk store.
- `service/usage.rs` — `record_monthly_active_user` and `current_monthly_active_users` — delegates to the global `UsageStore` through the same async storage boundary used elsewhere.
- `tenant.rs` — `TenantRuntime` is the tenant-local facade and composition root.
  The root now keeps only tenant structure, constructors, lifecycle, and
  cross-domain diagnostics while grouped facade files
  (`tenant/document_cache_facade.rs`, `tenant/materialized_reads_facade.rs`,
  `tenant/mutation_facade.rs`, `tenant/query_planning_facade.rs`, and
  `tenant/subscription_delivery_facade.rs`) own the thin delegation surface
  over `tenant/document_cache.rs`, `tenant/lifecycle.rs`,
  `tenant/materialized_reads/`, `tenant/mutation.rs`,
  `tenant/query_planning.rs`, and `tenant/subscription_delivery.rs`. That
  materialized-read root now composes `snapshot.rs`
  (`ServingSnapshot`, tenant-scoped snapshot retention, waiter wakeups, and
  reader pins), `backend/` (the in-memory `MaterializedServingBackend`
  composition root, with `state.rs` owning table residency plus access
  tracking, `loading.rs` owning warm-load catch-up plus waiter behavior,
  `publication.rs` owning publication ordering and retained-version
  management, and `diagnostics.rs` owning backend stats plus test hooks),
  `warm_load.rs` (shared warm-load coordination and waiter ownership),
  `stats.rs` (public serving metrics snapshot types), and `pause.rs`
  (test-only publish pause control). That
  mutation root now composes `tenant/mutation/requests.rs`
  (queued mutation request or response models plus queue defaults),
  `admission.rs` (the outer mutation admission gate and queue ownership),
  `codel.rs` (CoDel shedding state and drop scheduling),
  `journal.rs` (journal queue state, applied-sequence waiting, and worker
  progress ownership), `stats.rs` (public mutation diagnostics snapshot
  types), and `pause.rs` (test-only pause control reused by the journal worker
  and subscription-bootstrap hooks). That
  subscription-delivery root now composes
  `tenant/subscription_delivery/queue.rs` (queue state, bounded enqueue, and
  drain batching), `worker.rs` (dedicated worker lifecycle and shutdown),
  `stats.rs` (delivery metrics and stats snapshots), and `pause.rs`
  (test-only pause control). `TenantRuntime` still holds tenant-scoped
  persistence handles, async storage handles, `SubscriptionRegistry`,
  `RwLock<Schema>`, a
  tenant-local close-then-drain lifecycle primitive, a bounded
  subscription-delivery worker queue, and per-tenant durable versus applied
  mutation-journal progress. Operation entry still uses RAII guards to keep
  the in-flight count correct; sync waiters use a `Condvar`, async waiters use
  `Notify`, and both share the same deleted-plus-active-operations state. The
  subscription-delivery queue still drains small ready batches, merges
  overlapping queued delivery work before reevaluation, and tracks both
  journal-batch and queue-level coalescing metrics, while the journal worker
  still exposes an async-friendly test pause seam for deterministic
  multi-record apply coverage.
- `evaluator.rs` — Pure evaluation composition root. It now composes
  `evaluator/query.rs` (store-backed and preloaded query evaluation surfaces),
  `pagination.rs` (paginated windowing and page assembly), `filtering.rs`
  (filter evaluation and shared scalar comparison rules), `ordering.rs`
  (document ordering and order-domain validation), and `cursor.rs` (cursor
  encode/decode, query-shape validation, and cursor boundary comparison). No
  I/O.
- `subscriptions.rs` — Composition root for tenant-local subscription
  ownership. The implementation now lives in `subscriptions/registry.rs`
  (registration, activation, cleanup handles, and live delivery projections),
  `dependencies.rs` (dependency derivation plus affected-subscription scans),
  `queue.rs` (queued wakeup work and coalescing), `delivery.rs`
  (reevaluation, monotonic delivery, and terminal error dispatch), and
  `invalidation.rs` (policy-revision teardown and shutdown). `SubscriptionRegistry`
  still tracks the latest delivered sequence per subscription so async
  delivery never publishes older visible state after newer visible state. When
  multiple applied commits are merged into one delivery unit, subscribers
  receive the latest applied sequence and current query result, but not
  per-commit metadata for the intermediate records.
- `scheduler.rs` — Background loop: `run_scheduler` sleeps until the next due tenant-local scheduled or cron work (or a wakeup notification), then fans loaded tenants out through a bounded concurrent scheduler tick so one slow tenant does not stall other due work.

**`neovex-runtime`** — Standalone execution runtime, currently V8-backed, with
zero workspace dependencies. Defines the `HostBridge` trait for
dependency-inverted host integration; the runtime never imports engine or
storage types directly.

- `runtime.rs` — `NeovexRuntime`: the runtime composition root. It now keeps
  the public type surface while delegating public runtime
  construction and convenience invocation entrypoints to `runtime/facade.rs`,
  invocation-driver ownership to the `runtime/driver/` module tree
  (`invocation.rs` for driver lifecycle and finalize paths, `loading.rs` for
  bundle load or invoke flow, `construction.rs` for snapshot bootstrap,
  runtime creation, and retained-runtime reset helpers, and `tracing.rs` for
  snapshot-seeded tracing), cooperative slot startup plus wake/poll handling
  to `runtime/cooperative.rs`, error/serialization helpers to
  `runtime/helpers.rs`, invocation/auth types to `runtime/invocation.rs`,
  bundle identity and integrity handling to `runtime/bundle.rs`, and the
  current V8-backed bootstrap layer to the `runtime/bootstrap/` module tree.
  Service bindings now follow a split contract there: `ctx.services.<name>`
  reads only the invocation snapshot, while `await ctx.services.get("name")`
  resolves missing bindings through the async cancellable host path.
- `runtime/invocation.rs` — `InvocationKind`, `InvocationRequest`, `InvocationAuth`, `RuntimeUserIdentity`, and `VerifiedUserIdentity`: the public invocation and auth payload surface for runtime calls.
- `runtime/bundle.rs` — `RuntimeBundle`: bundle path identity, canonicalization, and per-invocation SHA-256 integrity verification.
- `backends/mod.rs` — worker-local `RuntimeBackendFactory` /
  `RuntimeBackend` traits plus the shared invocation envelope that keeps
  backend-specific state out of `RuntimeExecutor`.
- `backends/v8/` — current V8 backend implementation. `mod.rs` owns the
  worker-local backend adapter plus deferred V8-runtime drop handling,
  `embedder.rs` centralizes the current `deno_core` integration behind a
  V8-owned namespace, `startup.rs` owns startup-snapshot construction and
  build accounting, and `warm_pool.rs` owns reusable-runtime state,
  affinity-aware warm-pool reuse, and pool-bounds enforcement.
- `runtime/bootstrap/mod.rs` — thin composition root for the current V8-backed
  bootstrap module tree. This remains bootstrap glue for the existing V8
  backend, not a proven backend-neutral abstraction.
- `runtime/bootstrap/payloads.rs` — host-call payload schemas plus the runtime host-call envelope used by the bootstrap op surface.
- `runtime/bootstrap/ops.rs` — thin bootstrap op composition root for the
  current V8/`deno_core` host-op surface. It keeps the extension registration
  surface while delegating sync query-builder ops to `ops/sync_query_builder.rs`,
  async query and terminal ops to `ops/async_query.rs`,
  mutation/action/scheduler plus write-effect ops to `ops/async_effects.rs`,
  nested runtime ops to `ops/nested_runtime.rs`, and shared sync/async
  host-call permit glue to `ops/shared.rs`.
- `runtime/bootstrap/source.rs` — bootstrap JavaScript source plus the
  installation/finalization helpers that load it into a `JsRuntime`.
- `runtime/bootstrap/state.rs` — installation of host bridge, cancellation
  state, and shared permit state into V8 `OpState`, plus runtime
  timeout-controller ownership.
- `executor.rs` — `RuntimeExecutor`: the executor composition root and inline
  executor regression surface. `executor/facade.rs` owns the public executor
  type, worker-thread startup and shutdown, and executor test-state scaffolds;
  `executor/invoke.rs` owns direct and worker-backed async or blocking invoke
  entrypoints; and the remaining executor concerns still route through
  `executor/queue/`, `executor/admission.rs`, and `executor/lifecycle.rs`.
- `executor/queue/` — runtime worker-queue composition root. `job.rs` owns
  worker job envelopes plus result channels, `signal.rs` owns worker activity
  signaling, `shutdown.rs` owns executor shutdown state, `router.rs` owns
  affinity-aware worker routing plus dispatch/load tracking, and
  `controller.rs` owns the worker-local queue controller surface that workers
  drain and complete against.
- `executor/admission.rs` — thin executor-admission composition root.
  `admission/dispatch.rs` owns dispatch-handle lifecycle, `admission/permit.rs`
  owns shared permit state plus async host-call suspend/resume behavior, and
  `admission/tenant_fairness.rs` owns queued-tenant bookkeeping and the active
  versus parked versus queued fairness model.
- `executor/lifecycle.rs` — the canonical invocation lifecycle shared by the direct executor path and the worker-loop path: queue admission, execution metrics, cancellation and timeout accounting, debug logging, and permit completion.
- `worker_loop/mod.rs` — `WorkerLoopFactory`, `WorkerLoop`, and execution-model routing for runtime workers.
- `worker_loop/cooperative.rs` — composition root for the cooperative worker loop (the default execution model). Delegates admission and completion flow to `cooperative/execution.rs`, slot-state and parked/runnable scheduling to `cooperative/scheduler.rs`, warm-pool return plus deferred-drop ownership to `cooperative/retention.rs`, and the main worker run/shutdown loop to `cooperative/run.rs`.
- `worker_loop/run_to_completion.rs` — run-to-completion worker-loop implementation, available as an explicit per-bundle execution model option for bundles that need guaranteed fresh-per-invocation isolation.
- `host.rs` — `HostBridge` trait plus the versioned `HostCallRequest` /
  `HostCallEnvelope` / `HostCallPayload` family: the generic runtime-side
  contract between V8 guest code and Rust host operations (db queries,
  mutations, scheduler commands, `ctx.run*` delegation). The runtime now owns
  the backend-neutral payload family and rejects operation/payload mismatches
  before adapter dispatch. Adapter-specific wire names such as Convex
  `convex.*` labels still live at the server adapter boundary rather than in
  the runtime crate.
- `context.rs` — `RuntimeInvocationContext`: per-request metadata (invocation ID, function name, kind, auth identity) threaded through the runtime and host bridge.
- `limits.rs` — `RuntimeLimits` (heap, timeout, max runtime instances, worker
  threads, per-tenant active/in-flight/queued caps, max nested calls) and
  `RuntimePolicy` (enforces limits + owns the runtime concurrency semaphore).
- `metrics.rs` — runtime metrics composition root. It now composes
  `metrics/global.rs` (global runtime-instance, queue, pool, bundle, timeout,
  and
  cancellation counters), `host_operations.rs` (per-operation host-call
  metrics), `tenants.rs` (per-tenant counters plus queue or execution
  distributions), and `correlations.rs` (recent request-correlation retention
  and snapshot assembly) behind the stable `RuntimeMetrics` /
  `RuntimeMetricsSnapshot` surface.
- `module_loader.rs` — `RestrictedModuleLoader`: custom `deno_core` module
  loader that restricts ESM imports to the bundle root.
- `watchdog.rs` — `WatchdogTimer`: shared timeout and external-cancellation watchdog owned by `RuntimeExecutor`, replacing the old per-invocation watchdog OS threads.
- `error.rs` — `NeovexRuntimeError` with variants for timeout, cancellation,
  heap exceeded, contract violations, and user-thrown errors.

**`neovex-sandbox`** — Generic isolation and sandbox-orchestration seam. This
crate owns the stable sandbox nouns plus the first backend-owned krun
implementation slice. The public seam stays generic, while backend-specific
bundle generation, buildah command assembly, conmon/crun launch planning, and
manifest-backed lifecycle scaffolding now live under `backends/krun/`. The
remaining VMM work is host-level smoke execution and restart/log-persistence
proof on Linux.

- `backend.rs` — `SandboxBackend` plus `SandboxBackendKind` and the async
  future alias used for sandbox lifecycle operations.
- `spec.rs` — `SandboxSpec` plus `SandboxFilesystemSpec`,
  `SandboxProcessSpec`, and `SandboxPortBinding`: tenant-scoped sandbox launch
  intent, generic process/filesystem inputs, and published-port intent.
- `instance.rs` — `SandboxId`, `SandboxHandle`, and `SandboxStatus`: stable
  sandbox-instance identity and status projection.
- `endpoint.rs` — `PublishedEndpoint` plus `PublishedEndpointProtocol`: the
  canonical endpoint-publication surface that server-side registries can map
  into runtime-facing service discovery.
- `error.rs` — `SandboxError`: generic backend-unavailable, not-found, and
  operation-failed variants for the sandbox seam.
- `backends/krun/` — backend-owned krun internals. `bundle.rs` writes OCI
  bundle config with the krun handler and TSI port-map annotation,
  `buildah.rs` owns buildah command assembly, `conmon.rs` builds the
  `conmon -> /usr/libexec/neovex/crun` launch plan, `command.rs` holds the
  reusable backend-local command spec, and `vm.rs` is now the backend
  composition root over `vm/launch.rs` (launch planning, image/build
  materialization, guest-user switching, and krun VM-config helpers),
  `vm/lifecycle.rs` (manifest-backed inspect/start/stop/restart/cleanup
  behavior), and `vm/readiness.rs` (published-endpoint visibility and
  readiness probing) while preserving the current plan-only mode used for
  cross-platform verification.
- `backends/oci/` — backend-owned OCI image preparation and container launch
  helpers. `buildah.rs` is now the public buildah composition root over
  `buildah/cli.rs` (CLI wrapper plus mount-session lifecycle),
  `buildah/defaults.rs` (image launch defaults, env merging, and exposed-port
  parsing), `buildah/inspect.rs` (inspect/config payload decoding),
  `buildah/user.rs` (rootfs user resolution for session-backed and extracted
  rootfs flows), `buildah/render.rs` (shell and command-failure rendering),
  and `buildah/tests.rs` (buildah regression coverage) while `builder.rs`,
  `materializer.rs`, `network.rs`, `port_manager.rs`, and `conmon.rs`
  preserve their existing backend-local ownership.

### Async Ownership Boundaries

```mermaid
flowchart LR
    subgraph Request["Request-scoped execution"]
        Native["Native HTTP/WS request future"]
        V8["RuntimeExecutor worker runtime"]
    end

    subgraph ServiceOwned["Service-owned background execution"]
        EngineExec["Engine BackgroundExecutor"]
        StorageExec["Storage BackgroundExecutor"]
        JW["Journal worker"]
        SB["Storage blocking tasks"]
        SD["Subscription delivery worker"]
        Sch["Scheduler loop"]
    end

    Native -->|async mutation| JW
    V8 -->|ctx.runMutation / host op| JW
    EngineExec --> JW
    JW --> SD
    EngineExec --> Sch
    Native -->|storage read/write| SB
    V8 -->|host bridge storage| SB
    StorageExec --> SB
```

Request executors may enqueue work onto Service-owned background workers, but
they must not own the lifetime of those workers. Runtime executor threads are
for request execution. Journal apply, queued async mutation completion, and
similar durable background work are Service responsibilities.

### Execution Domains

| Domain | Owner | Primitive | Live responsibility |
| --- | --- | --- | --- |
| Main server runtime | `neovex-bin` | process Tokio runtime from `#[tokio::main]` | axum HTTP/WS request handling and the root scheduler task |
| Scheduler loop | `neovex-bin` + `Service` | long-lived Tokio task with `watch` shutdown + `Notify` wakeup | sleeps until the next due scheduled or cron work, then fans out a bounded set of per-tenant ticks via `Service` so one slow tenant cannot stall others |
| Service background runtime | `Service` | owned `BackgroundExecutor` field with `TaskTracker`, `CancellationToken`, and explicit `quiesce()` | stable home for long-lived engine async workers with service-owned shutdown semantics |
| Mutation journal worker | `Service` + `TenantRuntime` | service-owned async task | drains the outer mutation admission gate, applies CoDel shedding before journal ownership transfer, durably appends and applies queued mutations in order, and resolves mutation futures |
| Subscription delivery worker | `TenantRuntime` | tenant-owned dedicated OS thread with `Condvar` queue | bounded subscription reevaluation and delivery preparation, including small-batch queue draining and overlap-aware work merging before reevaluation |
| Runtime executor | Convex adapter | oversubscribed OS worker pool; each worker owns one current-thread Tokio runtime and one worker loop; JS permits remain separately bounded | V8 invocation execution, permit suspend/resume during async host I/O, and per-tenant active/in-flight/queued runtime admission |
| Storage async boundary | `Service` + backend-specific tenant or usage adapters | storage-owned `BackgroundExecutor` handle plus read/write semaphores | bounded tenant reads, writes, journal work, and usage operations on the service-owned storage executor; the tenant path is migration-selectable today while the usage path remains redb-backed |
| Session child tasks | WebSocket session / runtime subscription | `OwnedTaskSet` over Tokio `JoinSet` | sender, forwarder, bridge, and bootstrap tasks owned by the parent session |
| Invocation watchdogs | `RuntimeExecutor` | shared `WatchdogTimer` thread plus per-invocation registrations | timeout and external-cancellation termination of a V8 isolate without per-invocation watchdog thread churn |

The most important ownership split is between request execution and durable
background work. Request-scoped paths may wait on Service-owned work, but they
must not be the lifetime owner of that work.

Service owns its engine and storage executors as struct fields, not process-wide
statics. Both executors support a two-phase shutdown model: `quiesce()`
refuses new work and drains in-flight tasks, then runtime drop provides the
stop phase. Storage blocking work runs on the storage-owned executor instead of
borrowing whichever request or V8 runtime happened to call into storage.

`RuntimeExecutor` now also owns a shared `WatchdogTimer`. Runtime invocations
register timeout and external-cancellation termination callbacks against that
timer and explicitly disarm them before `JsRuntime` teardown. This keeps the
watchdog lifecycle executor-owned and replaces the old "up to two watchdog
threads per invocation" model with one shared watchdog thread per executor.

`RuntimeExecutor` also decouples worker threads from JS permits. Parked V8-backed
invocations hold their worker thread because `JsRuntime` is `!Send` and only
one runtime may safely exist per thread, but async host ops now release the JS
permit and the per-tenant active slot through `SharedInvocationPermit`. Another
worker can use that freed capacity while the parked invocation waits to
re-acquire its permit. The executor's primary extensibility seam is
`WorkerLoopFactory` / `WorkerLoop`; the current V8 backend stays
worker-local beneath that seam.

**`neovex-server`** — Network I/O and integration. Neovex-native routes are the default surface. The Convex adapter is an opt-in layer that owns the runtime executor, the `HostBridge` implementation, auth verification, and the function registry — it is the code that bridges the runtime into the engine.

- `lib.rs` — Public server facade. Re-exports the router builders and serve
  helpers; `serve` starts the axum listener.
- `router.rs` — Neovex-native and Convex route composition. The public
  `build_router*` overloads are now thin wrappers over one internal
  `RouterBuildConfig` path that normalizes `LicenseState`, optional Convex
  support, and runtime-service-registry wiring before building the axum
  router. The landed localhost hardening path also lives here: loopback-first
  bind defaults, route-family origin allowlists, local server-access auth, the
  `/ui/*` bootstrap fixture, and the separation between server-access auth and
  tenant/application auth.
- `service_registry.rs` / `service_manager.rs` — runtime service-binding seam.
  Snapshot reads and activation are now split intentionally: the sync runtime
  path only sees already-ready in-memory bindings, while async `ctx.services.get`
  calls can start and wait for declared services through cancellable sandbox
  activation plus readiness polling.
- `http/` — Neovex-native HTTP handlers. Read, control, and durable write routes all await async engine methods directly. Write handlers thread request disconnect cancellation to the engine, but post-commit disconnects remain transport-only failures and do not roll back durable writes.
- `local_server/` — Server-owned localhost security boundary. Owns platform
  path resolution, `server.json` discovery, local admin token lifecycle and
  rotation, signed UI sessions, origin and route-family gate helpers, and the
  append-only security audit log. These credentials authorize server access
  only and never populate Convex `InvocationAuth`, `ctx.auth`, or tenant
  `PrincipalContext`.
- `ws.rs` / `ws/socket.rs` — Neovex-native WebSocket upgrade and session
  composition. `ws/socket/transport.rs` owns socket reader, writer, and
  subscription-forwarder tasks, `ws/socket/pending.rs` owns pending bootstrap
  cancellation tracking, and `ws/socket/session.rs` owns generic subscription
  registration, unsubscribe handling, and disconnect cleanup. The native
  session still explicitly unsubscribes active subscriptions on disconnect and
  owns its child tasks through `OwnedTaskSet`.
- `license/` — `LicenseState`, `LicenseDocument`, `LicenseSnapshot`, `LicenseEntitlements`. Loads from `--license-file`, `NEOVEX_LICENSE_FILE` env, or `.neovex/license.json`. Supports community, trial, and enterprise tiers. Exposes status at `GET /debug/license/status` including MAU usage.
- `convex/mod.rs` — Convex shim request/response types plus the public Convex support handlers. Owns the `RuntimeExecutor`, runtime policy, auth verifier, registry state, and the server-side `HostBridge` implementation.
- `convex/auth/` — Convex auth adapter: OIDC and custom JWT provider config, JWKS key fetching, JWT validation with clock-skew tolerance, and identity extraction for `InvocationAuth`.
- `convex/registry/` and `convex/manifest.rs` — Manifest loading, runtime bundle discovery, function lookup, and Convex support route resolution.
- `convex/host_bridge/` — The `HostBridge` implementation that adapts Neovex
  engine operations into the contract the runtime expects. Async host-call
  routes now await real engine or storage futures directly; only inherently
  synchronous host-side setup stays on the sync bridge path. The server-side
  bridge now consumes a runtime-owned typed host envelope before any
  Convex-specific lowering, so ABI version checks plus top-level
  operation/payload validation happen before the adapter routes into query,
  document, scheduler, or nested-runtime handlers. `bridge.rs` now separates
  durable bridge scope from per-call invocation metadata through
  `ConvexHostBridgeScope` and `ConvexHostBridgeInvocation`, which keeps
  host-bridge construction narrow across runtime-backed call sites. The direct
  ctx-op surface now keeps
  `function_ops/ctx_ops/direct/execution.rs` as the canonical home for direct
  execution-context dispatch and execution-unit short-circuiting, while
  `function_ops/ctx_ops/direct/invocation.rs` owns runtime payload
  decode/validate/encode plus the default-cancellation wrapper flow for
  `ctx.db`, `ctx.mutation`, pagination, and action entrypoints.
- `convex/subscriptions/socket/` — Convex WebSocket session orchestration,
  message handling, runtime transform application, and active-subscription
  cleanup. `named_subscriptions.rs` is now the composition root over
  `named_subscriptions/direct.rs` (native direct-registration flow) and
  `named_subscriptions/runtime_backed.rs` (runtime-backed bootstrap, initial
  publish, and forwarding ownership). The session still owns its sender and
  forwarder tasks through `OwnedTaskSet`, and runtime-backed active
  subscriptions own their bridge tasks so auth changes, unsubscribe, and
  disconnect drain those children explicitly.
- `execution/subscriptions.rs` — Runtime subscription bootstrap for derived base
  queries. It registers the underlying engine subscriptions, rewrites their
  events onto the public subscription id, and now keeps those bridge tasks
  attached to the active subscription lifecycle instead of detaching them.
- `convex/execution/`, `convex/http_actions/`, `convex/subscriptions/`, and
  `convex/handlers/` — Shared Convex support execution, HTTP route dispatch,
  and live subscription plumbing. The runtime-backed execution path now
  centralizes service, registry, tenant, and runtime-service coordination in
  `execution/runtime_backed/invoke/context.rs`, and subscription transform
  reevaluation reuses a sibling `RuntimeTransformContext` so runtime-backed
  route handlers and subscription plumbing share one canonical invocation
  bundle per concern.
- `convex/host_bridge/read_tracking/` — Runtime read-set tracking used by runtime-backed Convex support subscriptions for narrower-than-table-level invalidation.
- `protocol.rs` — Request/response DTOs. `ClientMessage` (Subscribe/Unsubscribe) and `ServerMessage` (SubscriptionResult/Error).
- `sandbox.rs` — `SandboxCatalog` and `EmptySandboxCatalog`: server-owned
  service-discovery seam for mapping tenant-and-name lookups onto sandbox
  handles without coupling `neovex-sandbox` directly into request handlers.
- `state.rs` — `AppState` holds the shared `Service`, optional Convex support registry, `LicenseState`, and the injected `SandboxCatalog`. `AppError` maps `Error` variants to HTTP status codes.

**`neovex`** — Public facade crate for embedders. Re-exports stable types from `neovex-core`, `neovex-engine`, `neovex-runtime`, `neovex-sandbox`, `neovex-server`, and `neovex-storage` so downstream consumers can usually depend on one crate. Low-level localhost-security records, discovery helpers, and router-construction overloads stay on `neovex-server` so the facade does not become an implementation-detail bucket.

**`neovex-bin`** — CLI entry point. `main.rs` is the thin command root, while
`serve/` owns the serve-command composition seam: JSON config and env loading,
provider selection, precedence merging into typed
`ServicePersistenceConfig`, runtime-limit defaults, and the server boot path
that loads tenants with scheduled work, spawns the scheduler, optionally loads
the Convex registry and sandbox-backed services, starts the server, and
handles graceful shutdown. `serve/config.rs` now resolves file, env, and CLI
inputs into separate provider-selection and encryption-selection helpers
before emitting the typed service-persistence config that the engine bootstrap
pipeline consumes. `machine/` now follows the same pattern: `mod.rs` is the
machine CLI composition root, `command.rs` owns clap models, `record.rs` owns
machine roots and persisted record types, `files.rs` owns JSON and lock helper
semantics, `render.rs` owns status/list/info/inspect output shaping, and
`handlers.rs` owns subcommand orchestration while `manager.rs` stays the
machine lifecycle composition surface. The lifecycle ownership now lives under
`machine/manager/`: `launch.rs` owns launch planning and helper command lines,
`image.rs` owns OCI or HTTP image materialization and attestation metadata,
`helpers.rs` owns helper discovery, `ports.rs` owns managed SSH-port leasing,
`ssh.rs` owns localhost SSH and SCP helpers, `readiness.rs` owns bootstrap and
readiness waits, `guest.rs` owns guest bootstrap and binary-sync behavior, and
`stop.rs` owns stop, cleanup, and stale-state recovery. The guest machine API
under `machine/api/` now follows the same split: `api.rs` stays the public
composition root, `routes.rs` owns router construction plus lifecycle and query
handlers, `capabilities.rs` owns capability and readiness probing,
`binaries.rs` owns helper-binary discovery plus backend-path resolution,
`listener.rs` owns direct-socket and systemd socket-activation setup,
`state.rs` owns persisted-container-state refresh and response shaping,
`logs.rs` owns persisted log-tail helpers, `process.rs` owns PID and process
snapshot helpers, and `tests.rs` keeps the machine API regression coverage
packaged beside that surface. `service/` now follows
the same ownership model: `mod.rs` stays the service CLI composition root,
`execution.rs` owns host-platform backend defaults, execution-surface
resolution, and forwarded machine API validation, `lifecycle.rs` owns service
up or down flows plus launch and stop helpers, `render.rs` owns lifecycle,
list, inspect, and process-snapshot output shaping, `logs.rs` owns persisted
and forwarded log reads, and `process.rs` owns PID-file plus process-table
inspection helpers. The Compose plan loader under `service/compose/` now
follows the same concept-owned split: `compose.rs` stays the public Compose
composition surface, `raw.rs` owns decoded Compose document types,
`lower.rs` owns project and service lowering into Neovex plans, `parse.rs`
owns scalar and lifecycle parser helpers, `warnings.rs` owns warning helpers,
`render.rs` owns rendered plan output, and `tests.rs` keeps the Compose
regression coverage packaged beside that surface. The CLI surface still
parses `--port`, local data-directory and control-plane overrides,
provider-specific settings such as
`--postgres-url`, `--app-dir`, `--skip-codegen`, `--license-file`, and runtime limit
flags (`--runtime-heap-mb`, `--runtime-initial-heap-mb`,
`--runtime-timeout-secs`, `--runtime-max-instances`,
`--runtime-worker-threads`, `--runtime-max-nested-calls`).

**`packages/codegen`** — Node.js code generation tool. Reads Convex source
files (`convex/*.ts`) and a `convex/schema.ts`, emits
`.neovex/convex/functions.json` (named-function manifest),
`.neovex/convex/bundle.mjs` (runtime ESM entrypoint),
`.neovex/convex/bundle.sha256` (integrity hash), and generated
`convex/_generated/` TypeScript (api, server, dataModel, scheduled_functions).
The generated files still target `convex/*` imports for compatibility, but
those surfaces now delegate more of their implementation through canonical
`packages/neovex` modules where behavior is shared.

**`packages/convex`** — In-repo Convex compatibility package. Provides
`convex/browser` (`ConvexHttpClient`, named-string and `anyApi` helpers),
`convex/react` (`ConvexReactClient`, provider and hook aliases), `convex/server`
(thin typed wrappers over `neovex/server`), and `convex/values` (validators).
These still speak the Neovex Convex-shaped WebSocket/HTTP protocol, but the
package now prefers wrapper, alias, and re-export seams over copy-forward
duplication whenever behavior matches the canonical `packages/neovex`
implementations.

**`neovex-testing`** — Shared test fixtures (`HttpApiFixture`) for
integration tests. `http_api_fixture/` is now a route-family composition root:
`debug.rs` owns diagnostics helpers, `convex.rs` owns Convex runtime and HTTP
helpers, `tenants.rs` owns tenant lifecycle helpers, `schedule.rs` owns native
scheduling and cron helpers, `schema.rs` owns schema helpers, `documents.rs`
owns document and journal helpers, and `queries.rs` owns native query helpers.

---

## Persistence Seams

Neovex should keep two different seams explicit instead of collapsing them into
one generic "backend" abstraction.

### Durable engine-facing behavior seam

The durable seam is the tenant-scoped persistence contract that the engine
depends on. This is the seam that should survive the SQLite migration and later
support retained embedded providers plus future non-local provider families.

Use `TenantPersistence` as the umbrella name for that seam. It may be one trait
or a composed family of semantically named capabilities, but the capability
names should stay behavior-oriented:

- `TenantQueryRead`
- `TenantMutationPersistence`
- `TenantJournalPersistence`
- `TenantSchedulerPersistence`
- `TenantSnapshotPersistence`
- `TenantSchemaPersistence`

The current `QueryReadStore` in
`crates/neovex-storage/src/query_read.rs` is already a good example of the
intended direction: narrow, planner-driven, and derived from live call sites
rather than from a greenfield CRUD sketch.

This seam belongs between `TenantRuntime` and backend-native persistence. The
engine should keep ownership of:

- auth, validation, and policy merge behavior
- mutation admission and batching
- execution-unit dependency and OCC semantics
- `CommitEntry` and durable or applied head semantics
- subscription fan-out and materialized-read publication semantics
- journal bootstrap, replay, and snapshot product behavior
- tenant routing and request-level coalescing

Backends should keep ownership of:

- transactions, WAL or MVCC, and physical recovery primitives
- SQL or key-value storage layout
- indexes, ordering, range scans, and physical query execution
- prepared statements, pooling, and backend-local caching
- backend-native concurrency behavior

### Separate construction/config seam

Construction and configuration are a different concern and should not be fused
with the durable behavior seam.

Use `PersistenceProvider` as the long-term name for the typed
construction/config seam. It should own backend selection, typed config, tenant
routing, pools, and lifecycle entrypoints for embedded or external backends.

At the service boundary, model persistence inputs as:

- `ServicePersistenceConfig` for the whole service
- `TenantProviderConfig` for tenant-scoped persistence
- `ControlPlaneConfig` for cross-tenant control and usage state

`ServicePersistenceConfig` should keep tenant persistence and cross-tenant
control-path configuration separate so a provider change for tenant data does
not silently move global metering or future control-plane state with it.

The live implementation now reflects that split directly: `Service` lowers
tenant persistence through `PersistenceProvider` and lowers the cross-tenant
usage/control path through a separate control-plane provider role instead of
piggybacking usage-store construction on a tenant provider.

Use names like:

- `EmbeddedRedbProvider`
- `EmbeddedSqliteProvider`
- `LibsqlReplicaProvider`
- `PostgresProvider`
- `MySqlProvider`

Do not treat path-shaped tenant construction as the durable cross-backend API.
That shape is acceptable only for the migration window.

Provider config should distinguish storage dialect from deployment topology.
`sqlite` alone is not enough, because local-file SQLite and replica-connected
SQLite have different latency, consistency, and operational models. The same
principle applies to future retained embedded providers: their coordination or
replication mode should be provider/topology config, not a change to
`TenantPersistence`.

Use separate validated axes such as:

- `PersistenceDialect` with values like `Redb`, `Sqlite`, `Postgres`, and
  `MySql`
- `PersistenceTopology` with values like `EmbeddedStandalone`,
  `ExternalPrimary`, `ExternalPrimaryWithReplicas`, and
  `CoordinatedEmbedded`

Then layer provider-owned `TenantRoutingConfig`, `PoolConfig`, and credential
configuration on top of those axes.

Current embedded constructors such as `Service::new(data_dir)` and
`Service::new_with_embedded_provider(data_dir, EmbeddedProviderKind)` are
still useful convenience wrappers, but they should eventually lower into the
typed config model above instead of becoming the universal construction API.

Operator-facing startup config should follow the same rule. CLI flags,
environment variables, and config files should lower into one typed
`ServicePersistenceConfig` model. Resource identity and execution intent
should stay separate, so a canonical Postgres connection value such as
`NEOVEX_POSTGRES_URL` can be reused across runtime, test, and benchmark
surfaces while the invoking command or profile chooses the intent.

### Naming rules

- Use `persistence` for stable engine-facing contracts that must work for both
  embedded and networked databases.
- Use `provider` for typed backend construction/config and tenant routing.
- Use `backend` only for temporary migration switches or for naming concrete
  implementation families.
- Use `store` only for backend-local physical adapters such as
  `SqliteTenantStore`.

That means migration-only names such as `StorageBackendKind`,
`BackendStorageEngine`, `BackendTenantStore`, and `BackendTenantReadStorage`
should not reappear as the durable public architecture vocabulary; they belong
in the historical migration record, while the live code and docs use
`TenantPersistence` / `PersistenceProvider` terminology instead.

### Embedded vs external cost model

Embedded backends such as SQLite or redb mainly pay local CPU, memory, disk,
and lock costs. External backends such as Postgres or MySQL also pay network
round trips, pool checkout, TLS, remote planning, and server-side concurrency
costs.

That makes the Neovex-owned pre-storage layer even more important for external
SQL, but only for the right kind of work:

- do more semantic shaping above the seam
- do less chatty storage interaction across the seam
- keep physical filtering, ordering, and set execution inside the backend

The architectural consequence is that the stable seam should stay coarse and
semantic. It should expose operations like execution-unit apply, journal
append/stream/bootstrap, scheduler claim/complete, and query-read behavior
derived from the planner. It should not degenerate into tiny CRUD or
scan-shaped primitives that force many remote round trips.

For future external providers, refine the current umbrella
`TenantPersistence` composition root toward explicit semantic capability
families such as `TenantQueryRead`, `TenantMutationPersistence`,
`TenantJournalPersistence`, `TenantSchedulerPersistence`,
`TenantSnapshotPersistence`, and `TenantSchemaPersistence`.

Those capability seams should remain semantic. They must not be replaced by
filesystem-path construction as the universal provider API, hook- or
trigger-driven reactivity as the canonical engine contract, row-at-a-time
remote iterator contracts that make the engine emulate a query planner, or
chatty document CRUD verbs that turn one logical operation into many network
round trips.

For the deeper provider-topology baselines and readiness gates for
`PostgresProvider`, `LibsqlReplicaProvider`, and `MySqlProvider`, see
[docs/reference/provider-topologies.md](docs/reference/provider-topologies.md).

---

## Architecture Invariants

These rules must not be violated. If a change would break one, it requires an
architecture discussion.

1. **`neovex-core` has zero I/O.** It defines types and validation only. If
   you need to read a file or make a network call, it belongs in another crate.

2. **`neovex-runtime` has zero workspace dependencies.** It defines the V8
   execution surface and the `HostBridge` trait. All Neovex-specific
   integration lives in the server's bridge implementation, not in the runtime
   crate.

3. **Durability and visibility are separate, explicit phases.** A mutation is
   acknowledged only after its durable journal record is appended in commit
   order. Read visibility, cache publication, and subscription fan-out happen
   only after ordered materialization updates document and index state and
   advances the applied watermark. Both async and sync serving reads wait for
   that applied watermark before consulting the materialized document or index
   state, and async waits remain cancellable while they are blocked on that
   visibility boundary.

4. **Every mutation — whether from HTTP, WebSocket, the scheduler, or the
   runtime — flows through `Service::apply_mutation`.** There is no separate
   code path for scheduled or runtime-originated mutations. Schema validation
   and subscription fan-out are guaranteed.

5. **Runtime multi-step mutations use an explicit execution unit with
   serializable OCC validation.** Runtime `ctx.db.*`, `ctx.scheduler.*`, and
   direct `ctx.runQuery` or `ctx.runMutation` calls inside a mutation execute
   against a stable read snapshot, stage their writes locally, and commit once
   after validating shared dependency metadata against commits that landed
   after the same applied snapshot sequence they actually read. Repeated
   writes to the same document must collapse to the final logical write before
   commit instead of manufacturing intermediate conflicts. Execution units are
   single-use: once `commit()` is attempted, the unit is finalized whether the
   attempt succeeds, conflicts, or becomes a no-op, and later reads, writes,
   or repeat commits are rejected.

6. **The evaluator is pure.** `evaluate_query` and `evaluate_paginated` take
   data in, return data out. No I/O, no state, no side effects. The service
   layer handles schema lookup and index selection.

7. **Schema is optional.** A table without a schema accepts any document.
   Setting a schema only adds constraints — it never removes the ability to
   write to a previously schemaless table.

8. **Tenant deletion blocks until in-flight operations complete.**
   `begin_delete()` acquires an exclusive lifecycle lock. `enter_operation()`
   acquires a shared lock. New operations after the `deleted` flag is set
   return `TenantNotFound`.

9. **Runtime bundles are integrity-checked.** The SHA-256 hash of the bundle
   is verified before every invocation. A tampered or stale bundle is rejected.

10. **Runtime host operations go through the same Service path as direct
   calls.** `ctx.db.insert(...)` inside a V8 handler ultimately calls the
   same `Service::apply_mutation` as an HTTP `POST`. No bypass.

11. **Long-lived async engine workers are service-owned.** Journal workers and
   similar background write-path tasks must be spawned from a stable runtime
   owned by `Service`, not from request-scoped executors or per-invocation
   current-thread runtimes. Otherwise queued durable writes can outlive the
   runtime that was supposed to resolve their futures.

---

## Key Data Flows

### Write Path (mutation to subscription push)

```mermaid
sequenceDiagram
    participant C as Client
    participant Svc as Service
    participant JW as Journal Worker (service-owned)
    participant St as TenantStore
    participant Sub as SubscriptionRegistry
    participant WS as Subscribed Client

    C->>Svc: insert_document_async / ctx.runMutation
    Svc->>Svc: Schema validation + access policy
    Svc->>JW: enqueue queued mutation request
    Note over Svc,JW: Worker lifetime is owned by Service,<br/>not by the caller runtime
    JW->>St: Append durable mutation record
    Note over St: Durable ordered journal append
    St-->>JW: Acknowledged sequence

    JW->>St: Materialize document + index changes
    Note over St: Ordered apply advances applied watermark
    JW->>Sub: affected subscriptions for applied batch
    Sub-->>Svc: matching subscription ids
    Svc->>Svc: enqueue one coalesced bounded subscription work item
    JW-->>Svc: resolve queued mutation future
    Svc-->>C: mutation returns

    loop Background delivery worker
        Svc->>St: evaluate_with_index
        Note over Svc,St: Reads observe applied state only,<br/>then use index scan or table scan
        Svc-->>WS: SubscriptionUpdate Result
    end
```

### Runtime Bundle Execution Path

```mermaid
sequenceDiagram
    participant C as Client
    participant Srv as Server (Convex adapter)
    participant Exec as RuntimeExecutor
    participant V8 as V8 Isolate
    participant HB as HostBridge async path
    participant Svc as Service

    C->>Srv: Convex mutation/query/action
    Srv->>Exec: invoke(bundle, request)
    Exec->>V8: Run ESM handler
    V8->>HB: async host op (ctx.db.insert, ctx.db.query, ...)
    HB->>Svc: await Service / storage future
    Svc-->>HB: Result
    HB-->>V8: host op result
    V8-->>Exec: handler return value
    Exec-->>Srv: JSON result
    Srv-->>C: Response
```

### Scheduled Mutation Path

```mermaid
sequenceDiagram
    participant C as Client
    participant Svc as Service
    participant St as TenantStore
    participant Sch as Scheduler

    C->>Svc: schedule_mutation
    Svc->>St: SCHEDULED_JOBS.insert
    Svc-->>C: job_id

    Note over Sch: Sleep until next due work<br/>or a `Notify` wakeup

    Sch->>St: claim_due_jobs
    Note over St: Atomic move from<br/>SCHEDULED to RUNNING
    Sch->>Svc: insert/update/delete_document
    Note over Svc: Same write path as above
    Sch->>St: complete + record result

    C->>Svc: GET /schedule/history/job_id
    Svc-->>C: outcome + error if failed
```

---

## Persistence Engine

Neovex now keeps one persistence contract across multiple provider families:

- SQLite is the default embedded tenant provider
- redb remains a supported embedded tenant provider
- Postgres, MySQL, and replica-connected SQLite preserve the same
  engine-visible behavior behind provider-owned seams
- the cross-tenant usage and control database remains local and redb-backed
  today

At the architecture level, the key rules are:

- `DurableMutationRecord` remains the authoritative per-tenant ordered history
- document, index, scheduler, and commit-log effects still materialize
  atomically at the provider boundary
- serving reads still come from applied materialized state rather than from a
  journal-overlay path
- bootstrap and replica catch-up remain snapshot-plus-stream over the same
  durable-history contract
- planner semantics stay backend-independent: exact-index path, range-index
  path, then fallback scan

The backend layouts, durable-journal baseline, serving-snapshot direction,
shadow-materializer posture, and persistence-specific design decisions now live
in [docs/reference/persistence-engine-baseline.md](docs/reference/persistence-engine-baseline.md).

---

## Design Decisions

Persistence-engine decisions that would otherwise dominate this document now
live in
[docs/reference/persistence-engine-baseline.md](docs/reference/persistence-engine-baseline.md).
That reference owns the settled backend layouts, durable-journal baseline,
bootstrap and replay contract, serving-snapshot direction, shadow-materializer
posture, format guidance, and the persistence-specific non-decisions that keep
the storage seam canonical.

**Why keep V8 and still leave room for WASM?** The research guide is right
that schema-generated APIs and WASM plugins are attractive for a database: WASM
is language-agnostic, sandbox-friendly, and a better fit for tightly bounded
engine-local extensions such as policies, triggers, or deterministic compute.
But Neovex also has an explicit Convex-compatibility goal today, and that goal
is best served by keeping the V8 and `deno_core` runtime first-class. The
project decision is therefore:

- keep V8 for the Convex compatibility surface and JavaScript function model
- keep the engine and planner language-agnostic
- treat future WASM support as a complementary Neovex-native extension path,
  not a forced replacement for the compatibility runtime

TigerBeetle is not the reference system for this layer. For runtime surface,
schema API design, and extension boundaries, the better references remain
Convex, Hasura, PostgREST, and Wasmtime.

**Why database-per-tenant?** Tenant boundaries are scaling boundaries. Each
tenant is a self-contained redb file. No distributed transactions, no
cross-tenant interference, trivial data isolation. Horizontal scaling (future)
means distributing tenant files across nodes, not sharding within a database.

**Why `std::sync::RwLock` instead of `tokio::sync::RwLock`?** redb is still a
synchronous engine, but read paths now enter it through explicit async storage
executors that acquire lifecycle and schema guards inside the blocking storage
task instead of holding tokio locks across awaits. Using `std::sync::RwLock`
for tenant registry, schema, and lifecycle state keeps those critical sections
small and lets the storage boundary control where blocking actually happens.

**Why conservative subscription invalidation?** Native subscriptions still
prefer conservative correctness over fragile exactness, but they are no longer
purely table-level. Unfiltered native subscriptions still re-evaluate on any
write to the table, filtered native subscriptions register predicate
dependencies so clearly non-matching writes can be skipped, and limited native
subscriptions now retain a visible-window dependency so writes beyond the
current ordered result boundary can also be skipped. Index-accelerated
re-evaluation keeps the remaining wakeups fast, and the tenant-local delivery
queue moves that work off the synchronous mutation return path while preserving
per-subscription sequence monotonicity. Multi-record journal apply batches now
reuse that same queue by coalescing repeated wakeups for the same subscription
into one delivery unit whenever the visible result can be represented by the
latest applied sequence plus current query output. Runtime-backed subscriptions
still go further by using read-set tracking for returned document IDs and
visible-window boundaries. Narrower native tracking for non-limited ordered
queries remains future work.

Runtime-backed subscriptions now also retain their last emitted runtime value
and suppress duplicate pushes when an approximate wakeup or delayed catch-up
re-evaluation computes the same externally visible result. That keeps the
transport contract aligned with the database contract: Neovex may bootstrap at
an older applied frontier and then catch up, but it should not spam clients
with duplicate payloads just because the runtime subscription wakeup path is a
conservative approximation.

**Why no `Running` state for scheduled jobs?** The scheduler uses a two-phase
pattern: pending (SCHEDULED_JOBS) and running (RUNNING_SCHEDULED_JOBS). If
the server crashes mid-execution, `recover_running_jobs` on startup moves
orphaned jobs back to pending. This avoids the problem of jobs stuck in a
`Running` state that nobody is executing.

**Why execute scheduled mutations through the public Service API?** The
scheduler calls `insert_document`/`update_document`/`delete_document` — the
same methods HTTP handlers use. This guarantees schema validation, index
maintenance, and subscription fan-out happen for scheduled mutations without
any special code path.

**Why a dedicated thread pool for V8?** V8 isolates are not `Send` — they
must run on the thread that created them. The `RuntimeExecutor` spawns a
fixed-size pool of OS threads, each reusing one current-thread Tokio runtime
across jobs. This keeps V8 off the main async executor. Async host operations
from inside V8 currently await engine and storage futures directly through the
server-side `HostBridge`; they do not bounce through an extra runtime-owned
host executor layer.

**Why dependency inversion for the runtime?** The runtime crate has zero
workspace dependencies so it can be tested, fuzzed, and evolved independently.
The `HostBridge` trait is the only contract between V8 guest code and the host.
The server provides the implementation. This means changes to the engine's
internals never require changes to the runtime, and vice versa. The host-call
ABI is versioned and payload-typed at that boundary so adapter changes cannot
silently reinterpret an incompatible top-level payload shape.

**Why a separate usage database?** MAU tracking is global (not per-tenant),
so it lives in a dedicated `neovex-control.db` redb file managed by
`UsageStore`. This avoids coupling usage metering to any single tenant's
lifecycle. It is also why the first Postgres-first non-local activation stays
tenant-scoped: the cross-tenant usage/control path remains local and
redb-backed until a later provider-topology slice gives it an explicit design.

---

## Cross-Cutting Concerns

**Concurrency.** `Service` uses `std::sync::RwLock` for the tenant registry.
`TenantRuntime` uses `std::sync::RwLock` for schema plus a tenant-local
close-then-drain lifecycle primitive for operation admission and deletion
coordination. redb enforces single-writer per database. Read and control paths
enter storage through per-tenant async executors that preserve multiple
concurrent readers with semaphore-based admission while still providing
cooperative cancellation. Durable writes use a separate per-tenant async write
executor with an explicit commit point: cancellation is re-checked
immediately before redb commit, while post-commit cancellation is reported as a
transport concern instead of rewriting the engine result. Runtime-backed async
host calls await those engine or storage futures directly instead of wrapping
them in an extra blocking task. The scheduler loop sleeps until the next due
work or a wakeup notification. The `RuntimeExecutor` runs V8 on dedicated OS
threads with worker-local current-thread Tokio runtimes. Long-lived engine
workers such as the mutation journal run on the Service-owned background
runtime, while subscription delivery remains a tenant-owned dedicated thread.
Timeout and external-cancellation termination for V8 now run through one shared
executor-owned `WatchdogTimer` thread instead of spawning per-invocation
watchdog threads.
That delivery worker drains a small ready batch and merges overlapping queued
subscription work before reevaluation so write storms collapse redundant
delivery passes earlier than the stale-sequence check alone.
Storage blocking work is executed through the Service-owned storage
`BackgroundExecutor`, still bounded by semaphores at the storage layer. Async
mutations enter a per-tenant outer admission gate before the journal queue, so
overload shedding happens at admission while admitted journal work retains its
commit-path guarantee. An isolate concurrency semaphore caps the number of
simultaneous V8 invocations.

**Error handling.** All fallible operations return `neovex_core::Result<T>`.
Runtime errors use `NeovexRuntimeError` with variants for timeout,
cancellation, heap limits, and user-thrown exceptions. The server maps error
variants to HTTP status codes in one place (`state.rs`). Subscription
re-evaluation errors are logged and sent to the client as
`SubscriptionUpdate::Error` — they do not crash the server or abort the
mutation that triggered them.

**Licensing.** `LicenseState` is loaded once at startup and threaded through
`AppState`. The community tier is the default (no file needed). Trial and
enterprise tiers are loaded from a JSON license file. The
`GET /debug/license/status` endpoint exposes the current license kind, status,
entitlements, warnings, and MAU usage.

**Auth.** Authentication and authorization are separate architecture concerns.
The Convex adapter layer supports OIDC and custom JWT providers via
`convex/auth/`. JWT validation uses `ring` for signature verification with JWKS
key fetching and clock-skew tolerance, and validated identities are passed to
runtime handlers as `InvocationAuth`. Neovex-native routes still do not
prescribe one built-in transport authentication mechanism. Authorization now
lives in the engine and planner as declarative schema-level policy so reads,
writes, subscriptions, and runtime host calls share one enforcement model. In
other words: adapters authenticate and normalize principals; the engine
authorizes data access. Live subscriptions capture both a principal snapshot
and a policy revision, and a policy or principal-context change must trigger
revalidation or teardown before more data is delivered.

**Testing.** Unit tests live beside the owning crates, integration tests for
HTTP and WebSocket behavior live in `neovex-server/tests/reactive_loop.rs`, and
shared fixtures live in `neovex-testing`. The live verification architecture
now emphasizes concept-owned regression surfaces, deterministic simulation
seams, shared harness metadata, differential Convex checks, and dedicated
verification-harness corpora instead of growing one flat collection of root
test files.

For the deeper verification topology — simulation seams, harness ownership,
generated-history oracles, differential runners, consistency reports, and
named `pr` / `nightly` / `repro` corpora — see
[docs/reference/verification-architecture.md](docs/reference/verification-architecture.md).
For the proof-discipline guidance that sits above those mechanics — semantic
waits, bounded budgets, deterministic hardship, and CI investigation — see
[docs/reference/reliability-posture.md](docs/reference/reliability-posture.md)
and
[docs/reference/ci-failure-investigation.md](docs/reference/ci-failure-investigation.md).

The first composite-index slice is also in place behind that verification
discipline. Table schemas now treat indexes as `IndexDefinition { name, fields
}` so single-field and multi-field indexes share one canonical shape, storage
builds tuple-ordered composite keys during normal writes and schema backfill,
and missing indexed fields suppress the whole index entry instead of encoding
partial tuples. The planner still intentionally treats composite indexes as
opaque metadata for now, so Stage A widens schema and storage semantics without
quietly changing query, cursor, or dependency behavior before the next planner
slice is fully tested.

The next verified slice now makes a narrower planner promise on top of that
storage base: ordinary query reads can use composite indexes for the longest
exact equality prefix, and for one additional range on the next indexed field.
That is enough to accelerate shapes like `status == "open"` or
`status == "open" AND rank >= 10` against an index on `["status", "rank"]`,
and the planner prefers the composite candidate when it also satisfies the
requested `ORDER BY` field better than a weaker single-field alternative.
Paginated reads can now use that same composite narrowing when the engine still
sorts and pages on the user-visible order field after loading candidate
documents. In other words, the current slice treats the composite index as a
better prefilter, not as a new externally visible pagination order contract.
The follow-through slice therefore generalizes cursor payloads and dependency
boundary payloads to tuple-capable vectors while still populating one visible
order slot today, which removes the old single-slot assumption from execution-
unit dependencies and runtime read tracking without changing the current client
contract. The tenant runtime also records query-plan stats that distinguish
composite-index selections from single-field index and full-scan fallback
paths, so deterministic service tests can verify the composite planner is
actually being exercised instead of silently falling back.

Fallback table scans now also have their first conservative raw-value pushdown
slice. `neovex-storage` can probe just the targeted MessagePack field values
for simple `Eq/Gt/Gte/Lt/Lte` filters, reject rows that are definite
non-matches before building a full `Document`, and expose scan stats that
differentiate scanned rows, decoded rows, and pushdown-rejected rows in tests.
`neovex-engine` only uses that seam as a reject-fast prefilter: unsupported
operators, unsupported value domains, and anything the probe cannot decide all
fall back to the full decode-and-evaluate path so query, pagination, and error
semantics stay unchanged.

The follow-on hot-path clone cleanup now tightens two more steady-state seams
without changing the external contract. Owned Convex execution-unit reads use
`Document::into_json()` all the way through the remaining direct host-bridge
path, so staged runtime reads stop cloning full field maps just to add `_id`
and `_creationTime`. The durable journal path also now keeps only minimal
queued-request state after planning, borrows one durable-record slice across
append and apply, and derives subscription deleted-document payloads from
committed writes instead of carrying duplicate owned vectors through the batch.
One subtle semantic rule is still intentional: insert invalidation uses the new
document as its candidate, delete invalidation uses the removed document as its
candidate, and update invalidation stays conservative by providing no candidate
document so maybe-affected filtered subscriptions still get re-evaluated.

**Serialization.** MessagePack (via `rmp-serde`) for storage. JSON (via
`serde_json`) for HTTP and WebSocket wire format. Documents carry both
representations: `to_msgpack()` for disk, `to_json()` for clients.

**Runtime observability.** When Convex support is enabled, the
`GET /debug/runtime/metrics` endpoint exposes live executor/runtime counters:
active runtime instances, queued invocations, worker dispatches,
cancellations, timeouts, nested local dispatches, and fallback cross-runtime
dispatches. This endpoint is not available in the native-only router.

**Engine observability.** On both the native-only and Convex-enabled routers,
`GET /debug/tenants/{tenant_id}/engine/metrics` exposes per-tenant engine
health: mutation admission-gate state, mutation journal progress and queue
state, subscription-delivery queue state and queue-level merge counters,
materialized-serving residency and coverage, serving-snapshot retention, and
query-planning counters. This is the canonical operator-facing snapshot for
tenant-local durability and derived-state health.
