# Provider Topologies

This document extends [ARCHITECTURE.md](../../ARCHITECTURE.md) with the deeper
provider-topology reference material that should not live in the stable
architecture root. The root architecture doc keeps the crate map, invariants,
and major data flows; this doc keeps the more detailed provider-shape guidance
for external and replica-connected persistence families.

## Cost Model And Capability Seams

Embedded backends such as SQLite or redb mainly pay local CPU, memory, disk,
and lock costs. External backends such as Postgres, MySQL, or replica-connected
SQLite also pay network round trips, pool checkout, TLS, remote planning, and
server-side concurrency costs.

That makes the Nimbus-owned pre-storage layer even more important for external
SQL, but only for the right kind of work:

- do more semantic shaping above the seam
- do less chatty storage interaction across the seam
- keep physical filtering, ordering, and set execution inside the backend

The stable seam should stay coarse and semantic. It should expose operations
like execution-unit apply, journal append/stream/bootstrap, scheduler
claim/complete, and query-read behavior derived from the planner. It should not
degenerate into tiny CRUD or scan-shaped primitives that force many remote
round trips.

For future external providers, refine the current umbrella
`TenantPersistence` composition root toward explicit capability families:

- `TenantQueryRead`
- `TenantMutationPersistence`
- `TenantJournalPersistence`
- `TenantSchedulerPersistence`
- `TenantSnapshotPersistence`
- `TenantSchemaPersistence`

Those capability seams should remain semantic. They must not be replaced by:

- filesystem-path construction as the universal provider API
- hook- or trigger-driven reactivity as the canonical engine contract
- row-at-a-time remote iterator contracts that make the engine emulate a query
  planner
- chatty document CRUD verbs that turn one logical operation into many network
  round trips

External providers may still optimize aggressively below that seam by bundling
round trips, using server-side planning, or choosing backend-native
notification and recovery mechanisms, as long as the Nimbus-owned semantics
above the seam stay unchanged.

## Postgres Provider Shape

The first concrete non-local mode should be `PostgresProvider`.

Its intended shape is:

- one provider-owned Postgres database for the Nimbus service
- one small provider metadata schema for tenant registry and routing metadata
- one Postgres schema per Nimbus tenant
- the same logical per-tenant tables Nimbus already uses in SQLite
- fully qualified tenant-schema SQL instead of mutable session `search_path`

The journal model remains Nimbus-owned:

- `commit_log` stores serialized `DurableMutationRecord` blobs
- Postgres sequence or identity allocation owns physical sequence numbering
- provider-owned serialization preserves per-tenant ordered append and apply
- durable-head and applied-head stay explicit metadata, not inferred from
  notification delivery

Notifications such as `LISTEN` / `NOTIFY` may be used as wake hints, but not as
the authoritative reactive contract. Lost notifications must recover from head
metadata plus journal replay. The cross-tenant usage/control path remains local
redb in the first Postgres slice.

The readiness gate for this mode must measure more than local throughput. At a
minimum, it needs steady-state, cold-start, and latency-injected RTT lanes;
CRUD, indexed query, journal, subscription, mixed-load, and tenant-lifecycle
workloads; and operational drills for reconnect, listener loss, restart, pool
pressure, and tenant cleanup.

## Replica-Connected SQLite Provider Shape

The concrete first replica-connected SQLite family is
`LibsqlReplicaProvider`. It must not mean "open a SQLite file across the
network." SQLite's own network and WAL guidance make raw network-mounted
database files an unacceptable provider shape.

The admissible future shape is a concrete client/server or embedded-replica
provider family with:

- one authoritative primary that owns writes, schema changes, scheduler
  mutations, journal append, and head metadata
- read replicas or embedded replicas that may serve read-only queries only
  behind a provider-owned durable or applied sequence barrier
- provider-owned refresh/catch-up whenever replica progress cannot be proven
  sufficient for the requested semantic boundary; any future direct
  primary-read fallback would still belong behind that same provider boundary

If this path is activated, the best first-fit Rust connector family is
`libsql`, not a local-only SQLite driver stretched into a replication story.
`libsql` already exposes remote and local replica builders, synced databases,
delegated remote writes, `read_your_writes`, and `sync_until`, which is much
closer to the provider semantics Nimbus needs. Plain `rusqlite` or
`sqlx::Sqlite` remain good local SQLite tools, but they do not solve the
replica-topology problem on their own.

The current concrete activation is narrower than "any libsql mode." The
`LibsqlReplicaProvider` family pairs a remote-primary `libsql` connection for
writes and authoritative state with provider-owned per-tenant local SQLite
cache files for read-serving only.

That distinction matters:

- embedded SQLite means the local tenant file is authoritative state
- `LibsqlReplicaProvider` means the remote `libsql` primary is authoritative
  and the local SQLite file is derivative cache state
- replica reads remain correct only when the provider-owned durable/applied
  sequence barrier proves that cache freshness is sufficient

Today the safest first sync model is a Nimbus-owned snapshot or catch-up
refresher over the remote `libsql` SQL connection, producing provider-owned
local SQLite cache files directly. The main Nimbus process must not assume it
can host `new_remote_replica(...)` directly until that runtime path is proven
stable in the live harness. That means:

- keep the public typed config on `dialect = Sqlite` plus a replica topology,
  not on a new filesystem-shaped provider seam
- use a provider metadata namespace plus one tenant namespace per tenant on
  the remote primary
- keep local replica files provider-owned under an explicit cache root rather
  than accepting arbitrary SQLite files as the topology contract
- keep the sync owner for those replica files explicit: the first activation
  may refresh them via deterministic remote snapshot or catch-up work, and any
  future `libsql` embedded-replica client still belongs behind the same
  provider-owned boundary until the in-process runtime path is proven safe
- keep journal append, scheduler mutation paths, bootstrap/export, and other
  mutation-adjacent reads on the primary or behind an explicit provider-owned
  barrier refresh / catch-up path
- let only planner-driven read-only query lanes serve from the embedded
  replica, and only after provider-owned sequence-barrier proof or explicit
  `sync_until` catch-up

The current implementation state follows that split explicitly: the engine
lazy-loads replica-backed tenants through the normal `TenantPersistence` seam,
routes writes, scheduler mutations, and durable journal apply or recovery to
the remote primary, and serves planner-driven reads from the provider-owned
local SQLite cache after explicit cache refresh or poll-driven catch-up. The
embedded cache remains derivative rather than authoritative, while the provider
poll worker keeps loaded and unloaded tenants aligned with remote schema,
journal, and scheduled-work state.

The first slice explicitly defers `libsql` synced/offline-write database
shapes. Nimbus does not need disconnected local writes for this activation, and
bringing them in early would expand the roadmap into conflict resolution,
multi-writer policy, and promotion semantics that do not belong in the first
replica-connected SQLite pass.

Failover, promotion, and replica catch-up policy belong in provider and
topology config, not in `TenantPersistence`. Any actual implementation should
start from a new dedicated active plan for one named provider family rather
than from a generic "remote SQLite" promise.

## MySQL Provider Shape

The first MySQL mode should preserve the same Nimbus semantics as the
Postgres-first path while using MySQL-native physical mechanics.

Its intended shape is:

- one provider-owned MySQL deployment for the Nimbus service
- one small provider metadata database for tenant registry and routing
- one tenant database per Nimbus tenant, using fully qualified
  `tenant_db.table_name` SQL
- InnoDB MVCC transactions for consistent reads, execution-unit OCC, and
  durable journal append behavior
- tenant-local `AUTO_INCREMENT` commit-log sequencing plus explicit durable and
  applied head metadata
- generated-column-backed JSON indexing instead of assuming Postgres-style
  expression indexes or SQLite JSON-expression indexes

If this path is activated, the best first-fit Rust connector stack is
`mysql_async` directly. It is Tokio-native, already ships with a pooled async
connection model and explicit transaction APIs, and fits Nimbus's need for
dynamic fully qualified SQL, locking reads, and provider-owned statement
control. `sqlx` and `diesel-async` remain valid options in the abstract, but
they are not the best first fit for a provider that will rely on dynamic
tenant-database SQL rather than on macro-checked literal queries or an ORM
query builder.

Queue-like scheduler claim paths may use `FOR UPDATE SKIP LOCKED`, but if a
replicated MySQL deployment is used, that path should be treated as
row-based-replication territory rather than relying on statement-based
replication behavior. Recovery and catch-up should assume durable-progress
polling or provider-owned wake hints, not a Postgres-style in-database pub/sub
primitive.
