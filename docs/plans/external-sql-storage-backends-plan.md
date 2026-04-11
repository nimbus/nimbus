# Plan: External SQL and Future Provider Topologies

This file keeps its historical filename, but it now owns the broader
follow-on design work for replica-connected SQLite, Postgres/MySQL, and other
non-local or coordinated provider topologies after the local embedded-provider
seam is complete and stable.

It is intentionally separate from the completed SQLite migration. The archived
SQLite migration plan established the durable engine-facing seam and the
retained embedded-provider model; this plan is where future provider-topology
work should start when we are ready to move beyond local embedded deployments.

Reviewed against:

- `ARCHITECTURE.md`
- `docs/plans/README.md`
- `docs/plans/archive/pluggable-storage-backend-plan.md`
- `docs/plans/encryption-at-rest-plan.md`
- `crates/neovex-engine/src/persistence.rs`
- `crates/neovex-engine/src/service/mod.rs`
- `crates/neovex-storage/src/async_storage/traits.rs`
- `crates/neovex-storage/src/async_storage/engine.rs`
- `crates/neovex-storage/src/sqlite.rs`

Additional external references:

- SQLite: [Use Of SQLite Over A Network](https://www.sqlite.org/useovernet.html)
- SQLite: [Write-Ahead Logging](https://www.sqlite.org/wal.html)
- Rust `libsql`: [Builder](https://docs.rs/libsql/latest/libsql/struct.Builder.html)
- Rust `libsql`: [Database replication methods](https://docs.rs/libsql/latest/libsql/struct.Database.html)
- Rust `rusqlite`: [Connection](https://docs.rs/rusqlite/latest/rusqlite/struct.Connection.html)
- MySQL 8.4: [InnoDB Locking Reads](https://dev.mysql.com/doc/refman/8.4/en/innodb-locking-reads.html)
- MySQL 8.4: [Secondary Indexes and Generated Columns](https://dev.mysql.com/doc/refman/8.4/en/create-table-secondary-indexes.html)
- MySQL 8.4: [INFORMATION_SCHEMA `SCHEMATA`](https://dev.mysql.com/doc/refman/8.4/en/information-schema-schemata-table.html)
- Rust `mysql_async`: [crate docs](https://docs.rs/mysql_async/latest/mysql_async/)
- Rust `sqlx`: [`query!` macro requirements](https://docs.rs/sqlx/latest/sqlx/macro.query.html)
- Rust `testcontainers-modules`: [MySQL module](https://docs.rs/testcontainers-modules/latest/testcontainers_modules/mysql/struct.Mysql.html)

---

## Status

- **Status:** `complete`
- **Readiness:** the Postgres-first design and implementation work is complete
  and archived at `docs/plans/archive/postgres-storage-provider-plan.md`; the
  replica-connected SQLite implementation follow-on is also complete and
  archived at `docs/plans/archive/sqlite-replica-provider-plan.md`; the
  umbrella `PX8`/`PX9` cleanup slices and the missing `PX3`/`PX5` provider
  design decisions are now also complete, so this file should be treated as
  historical design baseline rather than live progress state
- **Primary owner:** historical baseline for future provider-topology work
- **Sequencing decision:** `Postgres` was the first non-local provider target;
  replica-connected SQLite and MySQL now have recorded design decisions, but
  any concrete implementation should start from a new dedicated active plan
- **Activation gate:** the Postgres-first design gate was met and promoted on
  2026-04-09; the dedicated implementation workstream completed and was
  archived on 2026-04-10 at
  `docs/plans/archive/postgres-storage-provider-plan.md`; this umbrella plan's
  remaining design and cleanup slices closed on 2026-04-10

## Purpose

This plan owns future work for replica-connected SQLite, Postgres/MySQL, and
other non-local or coordinated storage-provider topologies. It is separate from
the SQLite migration because those providers change the async model,
configuration model, tenant isolation model, pooling model, operational story,
and change-notification strategy.

Those concerns should not reshape the settled embedded-provider seam
prematurely.

## Current Assessed State

- The SQLite migration is complete and archived at
  `docs/plans/archive/pluggable-storage-backend-plan.md`.
- The stable engine-facing seam is now `TenantPersistence`, and the separate
  typed construction/config seam is `PersistenceProvider`.
- SQLite is the default embedded provider and redb is retained as another
  embedded provider.
- The current embedded provider selector is:
  `EmbeddedProviderKind`, `EmbeddedSqliteProvider`, and
  `EmbeddedRedbProvider`.
- The cross-tenant usage/control path now lowers through an explicit
  control-plane provider seam and remains backed by local embedded redb
  today.
- The engine still owns `CommitEntry`, logical journaling, scheduler dedupe,
  policy/auth behavior, subscription fan-out, and snapshot/bootstrap semantics.
- Current benchmark evidence justifies SQLite as the default embedded provider
  and Postgres as an opt-in external provider. It does not yet justify a
  generic replica-connected SQLite mode or a MySQL implementation path.
- The first concrete Postgres provider implementation is complete and archived
  at `docs/plans/archive/postgres-storage-provider-plan.md`, including its
  benchmark report and operational gate.
- The first concrete replica-connected SQLite provider implementation is
  complete and archived at
  `docs/plans/archive/sqlite-replica-provider-plan.md`, including the
  freshness-drill benchmark report and the explicit `sqld`-endpoint benchmark
  operating model.
- The runtime-facing config story now lowers idiomatic CLI flags, environment
  variables, and JSON config into `ServicePersistenceConfig`,
  `TenantProviderConfig`, and `ControlPlaneConfig`.
- `NEOVEX_POSTGRES_URL` is now the canonical Postgres resource input, while
  test, benchmark, and runtime intent remain separate from the resource
  identity and are chosen by the invoking surface.
- Future replica-connected SQLite work must reject raw network-mounted SQLite
  files and instead target a concrete provider family with explicit primary
  and replica semantics. The first such family is now implemented and
  archived, so any new work should start from that archived plan plus a newly
  promoted active control plane rather than reopening this umbrella file.
- Future MySQL work must start from MySQL-native transaction, queue, indexing,
  and replication behavior rather than assuming it can reuse the Postgres
  physical design unchanged.

## Current Review Findings

- The plan's original direction was correct, and the missing design work is
  now closed: Postgres-first execution is complete, the control-plane and
  runtime-config cleanup slices are landed, and the remaining replica-connected
  SQLite and MySQL decisions are now written down explicitly.
- The `ControlPlaneConfig` split is no longer theoretical. `PX8` landed the
  explicit control-plane provider seam, so future networked provider work no
  longer needs to smuggle cross-tenant redb construction through tenant
  providers.
- The typed runtime config story is also landed. `PX9` now lowers CLI, env,
  and JSON config through one typed persistence model and keeps resource
  identity separate from execution intent.
- Replica-connected SQLite must not mean "open a SQLite file over the network."
  SQLite's own network and WAL guidance make that shape unacceptable. Future
  replica-connected SQLite work must target a concrete client/server or
  embedded-replica provider family with explicit sequence-barrier reads.
- The best first-fit Rust connector family for that future replica-connected
  SQLite work is `libsql`, because it already models remote replicas, synced
  databases, delegated writes, and explicit catch-up/read-your-writes
  semantics. Plain local SQLite drivers remain useful for embedded SQLite, but
  they are not enough on their own for the replica-topology problem.
- MySQL must not be treated as "Postgres with different syntax." The first
  design has to account for MySQL's database-per-tenant analog to schemas,
  generated-column JSON indexing, queue-claim semantics, and replication-mode
  constraints.
- The best first-fit Rust connector stack for future MySQL work is
  `mysql_async` directly, because it is Tokio-native, already includes a pool,
  and keeps transaction, locking-read, and dynamic fully qualified SQL control
  explicit. `sqlx` and `diesel-async` remain valid in the abstract, but they
  are not the best first fit for Neovex's provider seam.
- Future provider work should continue to start from settled Neovex semantics,
  not from a least-common-denominator CRUD trait.
- With `PX3` and `PX5` complete, this umbrella plan has finished its design
  and cleanup role. Future concrete provider implementation work should
  promote or author a new active plan rather than reopening this file as live
  progress state.

## PX0 Activation Boundary Decision

The first Postgres-first activation is explicitly **tenant-scoped**.

Included in the first Postgres path:

- tenant-scoped persistence for documents, schemas, indexes, scheduler state,
  durable journal rows, query reads, and snapshot/bootstrap behavior
- typed provider config for Postgres credentials, pools, tenant routing, and
  topology
- tenant lifecycle ownership that is required to create, discover, open, or
  delete Postgres-backed tenants
- Postgres-native transaction, notification, recovery, and batching strategy
  for tenant-scoped state

Explicitly out of scope for the first Postgres path:

- the global MAU usage ledger in `UsageStore`
- the async global usage executor in `RedbUsageStorage`
- any broader cross-tenant control-plane redesign beyond what is strictly
  needed to keep tenant-scoped Postgres persistence coherent
- replica-connected SQLite, MySQL, or future embedded-provider coordination
  mechanisms

This means the first non-local activation may intentionally run with
Postgres-backed tenant persistence while the cross-tenant usage/control path
remains local and redb-backed. That split is architectural, not accidental,
and later slices may revisit it only through an explicit plan item.

## PX1 Typed Provider Config Decision

The durable typed construction/config model for future provider work is:

- `ServicePersistenceConfig` as the service-level persistence input
- `TenantProviderConfig` as the tenant-scoped provider input that feeds
  `PersistenceProvider`
- `ControlPlaneConfig` as the separate cross-tenant control-path input

`TenantProviderConfig` must keep these concerns explicit instead of flattening
them into a filesystem path or a single backend enum:

- `dialect` via `PersistenceDialect`
  - `Redb`
  - `Sqlite`
  - `Postgres`
  - `MySql`
- `topology` via `PersistenceTopology`
  - `EmbeddedStandalone`
  - `ExternalPrimary`
  - `ExternalPrimaryWithReplicas`
  - `CoordinatedEmbedded`
- `routing` via `TenantRoutingConfig`
- `pool` via `PoolConfig`
- `credentials` via a provider-owned credential source/config

The durable rule is that `dialect` and `topology` stay separate axes and are
validated together. Legal combinations are owned by provider construction, not
by ad hoc string parsing.

For the current Postgres-first path, that means:

- tenant persistence is modeled as `dialect = Postgres` plus a networked
  topology
- retained embedded providers are modeled as `dialect = Sqlite` or `Redb`
  plus `topology = EmbeddedStandalone`
- future replica-connected SQLite keeps `dialect = Sqlite` while changing only
  topology
- future coordinated embedded providers keep `dialect = Sqlite` or `Redb`
  while changing only topology

`ControlPlaneConfig` stays separate from `TenantProviderConfig` so the service
can evolve tenant persistence without implicitly moving the global usage/control
path. For the first Postgres slice, `ControlPlaneConfig` remains explicitly
equivalent to local embedded redb.

Current embedded constructors such as `Service::new(data_dir)` and
`Service::new_with_embedded_provider(data_dir, EmbeddedProviderKind)` may
remain as convenience wrappers, but they should eventually lower into
`ServicePersistenceConfig`; they are not the canonical cross-provider API.

The future runtime-facing rule is:

- service startup should load one typed persistence model from idiomatic CLI
  flags, environment variables, and config files, then lower that into
  `ServicePersistenceConfig`
- the canonical Postgres connection resource may be represented once in typed
  config and optionally surfaced through a generic env input such as
  `NEOVEX_POSTGRES_URL`
- test, benchmark, and runtime intent should be chosen by command/profile/CLI
  semantics rather than by encoding purpose into different resource variable
  names
- harness-specific env vars may remain temporary compatibility aliases or
  explicit overrides, but they are not the canonical long-term contract

## PX2 External-Provider Execution Contract Decision

The common external-provider contract must stay coarse, semantic, and grouped
by ownership rather than by physical CRUD verbs.

Use these capability families as the durable contract vocabulary:

- `TenantQueryRead`
  - planner-driven query reads and pagination
  - point reads needed by evaluator or execution-unit dependency checks
  - provider-owned consistent read boundary for query, bootstrap, and
    materialized-read serving semantics
- `TenantMutationPersistence`
  - validated direct write application
  - execution-unit batch apply over `ResolvedWrite` and `ResolvedScheduleOp`
  - durable commit-point behavior that preserves
    `TenantWriteCommit<T>` / `TenantWriteOutcome<T>`
- `TenantJournalPersistence`
  - durable append
  - ordered apply and recovery progress
  - stream, bootstrap, replay, durable-head, and applied-head semantics
- `TenantSchedulerPersistence`
  - claim, complete, cancel, result persistence, cron persistence, and dedupe
- `TenantSnapshotPersistence`
  - snapshot export, restore, rebuild, and the consistent read boundary needed
    by materialized serving and downstream bootstrap
- `TenantSchemaPersistence`
  - schema load plus atomic schema replace/delete with whatever backend-native
    index maintenance that provider requires

The contract explicitly must not become:

- a chatty document CRUD API that forces many remote round trips
- a filesystem-path API (`tenant_path`, extension scanning, `PathBuf`
  construction) masquerading as a provider seam
- a hook-driven reactive seam (`update_hook`, `preupdate_hook`, database
  triggers, or notifications as the canonical engine contract)
- a row-at-a-time remote iterator contract that makes the engine emulate the
  database planner

## PX3 Replica-Connected SQLite Mode Decision

The future `SqliteReplicaProvider` name remains valid, but it must not mean
"SQLite over a network-mounted file."

### Admissible shape

- Raw SQLite database files on network-mounted filesystems are not an
  acceptable provider mode. SQLite's own networking guidance calls out network
  latency and unreliable locking, and SQLite WAL requires participating
  processes to stay on the same host.
- Replica-connected SQLite therefore has to be a concrete client/server or
  proxy-mediated provider family with local SQLite state on each participating
  machine, not `PathBuf`-based remote file access.
- The first implementation must target one explicit provider family with
  documented primary/replica semantics. Do not promise a generic "any remote
  SQLite" mode.

### Authority and sequencing model

- One authoritative primary owns writes, schema changes, scheduler mutations,
  durable journal append, sequence allocation, durable-head, and applied-head
  metadata.
- Replica or embedded-replica nodes may serve read-only query traffic only
  behind a provider-owned sequence barrier. If the provider cannot prove
  replica progress is at or beyond the required durable or applied sequence,
  it must catch up first or route the read to the primary.
- Execution-unit reads that participate in later writes, bootstrap/export
  flows, journal stream/bootstrap, and mutation-adjacent reads must use the
  primary or an equivalent strong-consistency path.

### Recovery and notification model

- Replica notifications are hints only. Authoritative recovery comes from
  durable journal progress and head metadata, not replica-session continuity.
- Provider failover, promotion, and catch-up behavior belong in provider and
  topology config plus operator policy, not in `TenantPersistence`.

### Rust client and test stack fit

- The best first fit for a future replica-connected SQLite implementation is a
  `libsql`-style provider family, not a local-only SQLite driver stretched
  across a topology problem. `libsql`'s `Builder` already models the provider
  shapes Neovex actually cares about here: `new_remote_replica`,
  `new_local_replica`, `new_synced_database`, optional delegated writes, and
  explicit `read_your_writes` / `sync_until` behavior.
- `rusqlite` and `sqlx::Sqlite` remain good tools for plain local SQLite, but
  they are not the best first fit for this roadmap slice because they solve
  local SQL execution rather than replica topology, remote-write delegation,
  or provider-owned sequence-barrier catch-up.
- The first active implementation plan should therefore pick one concrete
  `libsql` family shape up front, such as remote embedded replicas or synced
  offline-capable databases, instead of trying to define an abstract
  "replicated SQLite" layer above multiple transports.
- Canonical automated verification should stand up the chosen replica family
  self-contained, ideally through a provider-owned container or local harness,
  with an explicit connection override available for externally managed
  environments. The test contract should follow the same resource-identity
  rule as Postgres: one canonical typed resource value, with runtime versus
  test versus benchmark intent chosen by the invoking surface instead of by
  multiplying DSN names.

### Future activation rule

- Any `SqliteReplicaProvider` implementation must start from a new dedicated
  active plan that names the concrete provider family, measures replica lag
  and catch-up latency, documents failover and promotion assumptions, and
  verifies strong-read fallback behavior before code lands.

The current umbrella `TenantPersistence` enum in `crates/neovex-engine/src/persistence.rs`
is acceptable as a live composition root, but future Postgres/MySQL work should
refine it toward the capability families above rather than expanding the enum
with more backend-specific branches.

Provider-specific execution rules for external modes:

- provider implementations may coalesce planner or scheduler requests into
  fewer backend round trips as long as Neovex semantics stay unchanged
- provider implementations own transaction scope, pooling, retry policy,
  server-side planning, and notification/catch-up mechanics below the seam
- the engine owns policy merge, `CommitEntry`, journal meaning, execution-unit
  OCC semantics, and subscription/materialized-read behavior above the seam

## PX4 Postgres Provider Mode Decision

The first concrete non-local provider mode is `PostgresProvider`.

### Tenant layout

- Use one provider-owned Postgres database with a small provider metadata
  schema for tenant registry and routing metadata.
- Store each tenant in its own Postgres schema rather than in a separate
  Postgres database or in a shared prefixed table namespace.
- Mirror the current SQLite logical tables inside each tenant schema:
  `documents`, `schemas`, `scheduled_jobs`, `running_scheduled_jobs`,
  `scheduled_job_results`, `scheduled_job_executions`, `cron_jobs`,
  `commit_log`, and `metadata`.
- Use fully qualified schema names in provider SQL rather than relying on
  mutable session `search_path`.

### Transaction and sequencing model

- Preserve the current Neovex logical journal model instead of collapsing it
  into "the database transaction log".
- Keep durable journal rows as serialized `DurableMutationRecord` blobs in
  `commit_log`.
- Allocate journal sequence numbers from Postgres-native sequence/identity
  machinery owned per tenant schema.
- Preserve durable-head and applied-head semantics in tenant metadata.
- Enforce per-tenant ordered journal append and apply through provider-owned
  serialization inside Postgres so concurrent service requests do not weaken
  Neovex's logical ordering guarantees.
- Use Postgres MVCC transactions to provide the consistent read boundary needed
  by planner-driven reads, execution-unit OCC, bootstrap, and materialized
  serving behavior.

### Notification and catch-up model

- Postgres notifications are hints, not the canonical reactive seam.
- If `LISTEN` / `NOTIFY` is used, it should only wake catch-up work after a
  durable commit; authoritative state still comes from journal stream,
  bootstrap, and head metadata.
- Lost notifications, reconnects, or listener failover must recover by reading
  durable progress and replaying the journal from the last known sequence.
- Subscription fan-out, materialized-read publication, and scheduler semantics
  remain engine-owned even when Postgres emits wake hints.

### Recovery and lifecycle model

- `create_tenant`, `open_existing_tenant`, `list_tenants`, and `delete_tenant`
  move behind provider-owned registry and routing metadata rather than file
  enumeration.
- Tenant deletion should drop provider-owned tenant metadata plus tenant schema
  only after the service-side lifecycle gate has drained in-flight work.
- Recovery starts from tenant metadata, durable/applied head state, and
  journal replay; it must not depend on long-lived listener state surviving a
  process restart.
- The global usage/control path remains local redb and outside this provider
  mode.

## PX5 MySQL Provider Mode Decision

The first MySQL mode should preserve the same Neovex semantics as the
Postgres-first path while using MySQL-native physical mechanics.

### Tenant layout

- Use one provider-owned MySQL deployment with one small provider metadata
  database and one tenant database per Neovex tenant.
- In MySQL, schemas and databases share the same namespace, so
  database-per-tenant is the closest analog to SQLite file-per-tenant and
  Postgres schema-per-tenant without collapsing tenant isolation into shared
  table prefixes.
- Use fully qualified `tenant_db.table_name` SQL rather than relying on a
  mutable default database for correctness.
- Mirror the current logical per-tenant tables inside each tenant database:
  `documents`, `schemas`, `scheduled_jobs`, `running_scheduled_jobs`,
  `scheduled_job_results`, `scheduled_job_executions`, `cron_jobs`,
  `commit_log`, and `metadata`.

### Transaction and sequencing model

- Keep durable journal rows as serialized `DurableMutationRecord` blobs in
  `commit_log`.
- Use InnoDB MVCC transactions to provide the consistent read boundary needed
  by planner-driven reads, execution-unit OCC, bootstrap, and materialized
  serving semantics.
- Use tenant-local `BIGINT AUTO_INCREMENT` sequencing on `commit_log` as the
  physical journal order. Durable-head and applied-head remain explicit
  metadata rather than inferred from wake delivery.
- Queue-like scheduler claim paths may use `SELECT ... FOR UPDATE SKIP LOCKED`
  where appropriate, but if replicated MySQL is used, that path must be
  treated as row-based-replication territory instead of relying on
  statement-based replication behavior.

### Index and document model

- Documents remain JSON at rest.
- MySQL JSON columns are not directly indexable, so the provider must
  materialize indexed fields and composite key parts via generated columns and
  then build secondary indexes over those generated columns plus any remaining
  ordering suffixes.
- Query shapes must match the generated-column expressions the optimizer can
  use, rather than assuming Postgres-style expression indexes or SQLite-style
  JSON-expression indexes translate directly.

### Notification and recovery model

- The first MySQL design should assume durable-progress polling or
  provider-owned wake hints rather than a Postgres-style in-database pub/sub
  primitive.
- Lost wakes or reconnects recover from durable head metadata plus journal
  replay, not from session continuity.

### Rust client and test stack fit

- The best first fit for a future MySQL provider is `mysql_async` directly.
  Its crate docs describe it as a Tokio-based asynchronous MySQL client, and
  its built-in `Pool` is already cloneable, `Send + Sync`, and paired with
  explicit transaction control via `Conn::start_transaction`. That matches
  Neovex's need for provider-owned fully qualified dynamic SQL, explicit
  locking reads, prepared statements, and coarse transaction boundaries below
  the seam.
- `sqlx` remains a valid Rust option in the abstract, but it is not the best
  first fit here for the same reason it was not the best first fit for the
  Postgres provider: its main advantage is the `query!` macro family, and the
  official docs require a build-time database or `.sqlx` metadata plus a
  string-literal query. A MySQL provider that issues dynamic
  `tenant_db.table_name` SQL and provider-owned claim/index maintenance
  statements would give up most of that advantage.
- `diesel-async` also remains valid in the abstract, but it is still an async
  Diesel ORM/query-builder layer. That is the wrong center of gravity for the
  Neovex provider seam, which wants backend-native SQL and explicit physical
  control rather than a schema-first DSL.
- Canonical automated MySQL verification should use
  `testcontainers-modules::mysql` for self-contained integration tests, with
  an explicit connection override for externally managed environments. As with
  Postgres, future runtime/test/benchmark intent should stay separate from the
  resource identity; a future active implementation plan may surface one
  canonical typed MySQL resource value, potentially through a generic env name
  such as `NEOVEX_MYSQL_URL`, while keeping purpose-specific behavior in the
  invoking command or profile.

### Future activation rule

- Any MySQL implementation must start from a new dedicated active plan that
  records replication assumptions, queue-claim strategy, generated-column
  maintenance costs, failover behavior, and benchmark plus operational gates
  analogous to the Postgres-first pass.

## PX6 Postgres Benchmark And Operational Gate

The first Postgres mode is not ready for implementation promotion until it has
its own measured benchmark and operational gate. SQLite/redb results do not
substitute for this gate.

### Benchmark lanes

Record all of these lanes for the Postgres-first mode:

- steady-state loopback lane against a local Postgres instance
- cold-start lane that includes pool creation, connection warmup, and first
  prepared-statement or plan-cache population costs
- round-trip-sensitive lane with injected network latency so request shaping is
  measured under non-zero RTT

### Required workloads

- document CRUD throughput
- point-read latency
- indexed query latency, including composite index paths
- durable journal stream latency
- durable journal bootstrap latency
- subscription bootstrap plus catch-up latency
- subscription fan-out latency after durable commit
- concurrent mixed multi-tenant read/write load
- tenant create/open/delete latency

### Operational drills

- listener disconnect and reconnect with no durable event loss
- missed notification recovery via journal catch-up
- service restart during outstanding journal or scheduler activity
- Postgres restart or transient connection failure during mixed load
- bounded-pool pressure and head-of-line blocking observation
- tenant create/delete idempotency and schema cleanup verification

### Promotion criteria

- all measured lanes above are recorded with median and tail latency, not just
  throughput
- benchmark results are compared against embedded SQLite as a contrast point,
  but Postgres does not need to beat SQLite to pass; it must instead show
  predictable costs and no semantic regressions
- operational drill outcomes are written down together with operator
  assumptions for pool sizing, connection count, and notification strategy
- any failure that would require changing `TenantPersistence`, the journal
  contract, or the first-slice scope decision blocks promotion into `PX7`

## Scope

This plan will cover:

- typed provider configuration that distinguishes backend dialect from topology
- runtime-facing provider configuration that lowers idiomatic CLI/env/config
  inputs into typed service persistence config
- replica-connected SQLite design and implementation planning
- Postgres provider design and implementation planning
- MySQL provider design and implementation planning
- explicit scoping of the cross-tenant usage/control path for non-local modes
- cleanup of the remaining redb-backed cross-tenant control-plane construction
  seam as its own follow-on provider-topology slice
- notification, catch-up, and recovery strategies for networked providers
- benchmark and operational gates for each provider mode
- future coordination or replication requirements for retained embedded
  providers at the provider/topology layer, without committing to a mechanism
  until a dedicated activation

This plan does not cover:

- the completed SQLite default cutover itself
- retained redb encryption-at-rest work
- user-facing `env.DB` / `env.HYPERDRIVE` bindings
- committing to a concrete coordination mechanism such as `raft`, `etcd`, or
  similar before a dedicated activation slice requires it

## Provider-Topology Invariants

- `TenantPersistence` remains the stable engine-facing semantic contract.
- `PersistenceProvider` remains separate from `TenantPersistence`; it owns
  typed construction, config, routing, pools, credentials, and topology.
- All mutations still flow through `Service::apply_mutation`.
- The engine continues to own `CommitEntry`, policy/auth semantics,
  subscription fan-out, scheduler dedupe, and logical journal/bootstrap
  meaning.
- Backend-native physical execution stays below the seam: filtering, ordering,
  transactions, pooling, SQL planning, and physical durability should be owned
  by the provider implementation, not reimplemented in the engine.
- External/networked modes must bias toward coarse semantic operations and
  fewer round trips instead of chatty CRUD abstractions.
- Provider config must distinguish backend dialect from deployment topology.
- Future coordination or replication policy must not be baked into
  `TenantPersistence`; it belongs in provider/topology config.
- Each activated provider mode needs its own benchmark and operational gate.
- The cross-tenant usage/control path must be explicitly scoped in the first
  activation slice; do not let a hidden single-node assumption leak into a
  networked design.

## Success Criteria

- the plan is specific enough to activate without inventing the architecture
  from scratch
- the first activated provider mode is explicitly fixed to Postgres rather than
  trying to design replica-connected SQLite, Postgres, MySQL, and coordinated
  embedded providers all at once
- typed config cleanly separates dialect, topology, credentials, pools, and
  tenant routing
- runtime provider selection is eventually driven by a typed CLI/env/config
  contract rather than by ad hoc harness env vars or filesystem-shaped
  constructors
- resource identity is modeled separately from execution intent, so one
  canonical Postgres connection value can be reused across runtime, test, and
  benchmark surfaces while the surrounding behavior stays purpose-specific
- the cross-tenant usage/control path has an explicit scope decision for the
  activated mode
- the global usage/control path can later evolve through an explicit
  control-plane seam rather than through hidden `EmbeddedRedbProvider`
  construction inside service startup
- the activated provider mode has a clear transaction, journal, notification,
  recovery, and benchmark story
- no part of the plan assumes filesystem-path construction or local-file
  semantics as the universal provider model

## Verification Contract

- docs-only refinements must at least pass `git diff --check`
- activation and implementation slices must continue to run
  `cargo fmt --all --check`, `cargo check --workspace`, and the focused
  verification needed for the touched crates
- each provider mode must define and record:
  - steady-state and cold-start benchmarks
  - round-trip-sensitive latency benchmarks
  - notification/catch-up latency
  - mixed read/write multi-tenant load
  - operational assumptions and failure-mode drills
- no provider mode is "ready" until its benchmark and operational criteria are
  written down and actually run

## Known Risks

| Risk | Impact | Mitigation |
| --- | --- | --- |
| The local redb-backed usage/control path could silently become a hidden single-node dependency for networked modes | high | make control-plane scope an explicit early activation decision |
| Harness-only env vars could accidentally become the product-facing persistence contract | medium | require a typed CLI/env/config lowering step for runtime startup, keep resource identity separate from execution intent, and treat any test/benchmark-specific env names as aliases or overrides rather than the durable contract |
| Replica-connected SQLite could get treated like ordinary local SQLite | high | keep dialect and topology separate in config and roadmap slices |
| Postgres/MySQL work could collapse into a chatty CRUD abstraction | high | keep `TenantPersistence` semantic and force backend-native batching/query execution |
| Postgres-first work could accidentally overfit the seam and make later MySQL or replica-connected SQLite support awkward | medium | keep typed config and provider semantics dialect-aware instead of smuggling Postgres assumptions into the common seam |
| Provider work could overgeneralize before a concrete first mode is chosen | medium | activate one target mode first, then expand from the landed contract |
| Benchmarks from embedded providers could get misapplied to networked providers | medium | require per-mode benchmarks and operational gates |

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies | Gate Note |
| --- | --- | --- | --- | --- |
| PX0 | `done` | scoped the Postgres-first activation boundary to tenant persistence only while explicitly leaving the global usage/control path local and redb-backed for the first slice | none | the first target is already fixed to Postgres; do not reopen that decision unless the plan is amended first |
| PX1 | `done` | codified `ServicePersistenceConfig`, `TenantProviderConfig`, and `ControlPlaneConfig`, keeping dialect and topology as separate validated axes and treating current embedded constructors as wrappers rather than the durable cross-provider API | PX0 | `PersistenceProvider` must stay separate from `TenantPersistence` |
| PX2 | `done` | codified the common external-provider execution contract around semantic capability families, explicit remote-cost rules, and a refusal to let the seam collapse into filesystem paths, hooks, or chatty CRUD | PX1 | no chatty CRUD seam may be introduced |
| PX3 | `done` | designed the replica-connected SQLite mode as a concrete primary/replica provider family with primary-owned writes and sequence-barrier replica reads, while explicitly rejecting raw network-mounted SQLite files as an acceptable provider model | PX7 | any implementation must start from a new dedicated active plan for one concrete provider family |
| PX4 | `done` | designed the first concrete Postgres provider mode around schema-per-tenant layout, provider-owned tenant registry metadata, Postgres-native sequencing, notification-as-hint, and recovery from durable journal state rather than listener state | PX2 | define transaction, journal, notification, tenant-layout, and recovery behavior |
| PX5 | `done` | designed the MySQL provider mode around a provider metadata database plus tenant databases, InnoDB MVCC, auto-increment journal sequencing, generated-column JSON indexing, and durable-progress-based recovery | PX7 | any implementation must start from a new dedicated active plan with explicit replication and benchmark assumptions |
| PX6 | `done` | defined the Postgres-specific benchmark lanes, workloads, operational drills, and promotion criteria needed before any implementation control plane can be activated | PX4 | the first activated mode needs its own measured readiness gate |
| PX7 | `done` | promoted the Postgres-first provider mode into a dedicated implementation control plane, which has since completed and been archived at `docs/plans/archive/postgres-storage-provider-plan.md` | PX6 | use the archived Postgres plan for historical context only; future provider-topology work resumes from this deferred umbrella plan |
| PX8 | `done` | made the cross-tenant usage/control path lower through an explicit control-plane provider seam instead of hard-wiring local redb construction in service setup | PX7 | kept this separate from `TenantPersistence`; retained local redb remains the current control-plane provider |
| PX9 | `done` | defined the idiomatic runtime CLI/env/config surface for provider selection and topology so typed config, not harness env vars, becomes the operator-facing contract, while resource identity stays separate from test/bench/runtime intent | PX7 | `NEOVEX_POSTGRES_URL` is now the canonical Postgres resource input, with command/profile semantics deciding runtime vs test vs benchmark intent |

## Dependency Graph

- `PX0` gates everything else.
- `PX1` depends on `PX0`.
- `PX2` depends on `PX1`.
- `PX4` depends on `PX2`.
- `PX6` depends on `PX4`.
- `PX7` depends on `PX6`.
- `PX8` and `PX9` depend on `PX7`.
- `PX3` and `PX5` were intentionally sequenced after `PX7`; both design
  slices are now complete.
- Any future implementation of replica-connected SQLite or MySQL must start
  from a new dedicated active plan rather than reopening this completed
  umbrella record as live progress state.

## Recommended Delivery Order

1. `PX0` through `PX2` to lock the Postgres-first scope, config shape, and
   common seam rules.
2. Complete `PX4` as the first provider-mode slice.
3. Complete `PX6` and only then promote the Postgres-first mode into active
   implementation ownership via `PX7`.
4. After the first Postgres path settles, decide whether `PX8` or `PX9`
   should activate next to clean up the control-plane seam and the
   operator-facing runtime config contract before broadening provider modes.
5. After the Postgres-first path and the cleanup slices settle, complete `PX3`
   and `PX5` as explicit follow-on design decisions for replica-connected
   SQLite and MySQL.
6. With all roadmap items complete, treat this file as historical design
   baseline only. Future provider-topology implementation work should start
   from a new active plan.

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| PX0 | Complete. The first Postgres activation is now explicitly tenant-scoped, and the plan records that `UsageStore` / `RedbUsageStorage` stay local and redb-backed for the first slice while tenant persistence moves independently. | start `PX1` and codify typed provider config that separates dialect from topology, credentials, pools, and tenant routing |
| PX1 | Complete. The plan now names the durable config inputs as `ServicePersistenceConfig`, `TenantProviderConfig`, and `ControlPlaneConfig`, keeps dialect and topology separate, and explicitly demotes `Service::new(data_dir)` plus `EmbeddedProviderKind` to embedded convenience wrappers. | start `PX2` and codify the common external-provider execution contract around coarse semantic operations and round-trip-aware cost rules |
| PX2 | Complete. The plan now maps the future common contract onto `TenantQueryRead`, `TenantMutationPersistence`, `TenantJournalPersistence`, `TenantSchedulerPersistence`, `TenantSnapshotPersistence`, and `TenantSchemaPersistence`, and it explicitly forbids path-shaped, hook-shaped, iterator-shaped, or chatty CRUD seams for external providers. | start `PX4` and design the first concrete Postgres provider mode on top of this contract |
| PX3 | Complete. The plan now rejects raw network-mounted SQLite files as an acceptable provider model, defines `SqliteReplicaProvider` as a concrete primary/replica family with primary-owned writes and sequence-barrier replica reads, and requires any future implementation to start from a dedicated active plan for a named provider family. | this roadmap is complete; future provider-topology implementation should start from a new active plan instead of reopening this file as live progress state |
| PX4 | Complete. `PostgresProvider` is now scoped as one provider-owned Postgres database with a provider metadata schema plus per-tenant schemas, durable journal blobs, per-tenant sequencing and head tracking, fully qualified SQL, notification-as-hint, and recovery from metadata plus journal state. | start `PX6` and define benchmark plus operational gates for this exact Postgres mode |
| PX5 | Complete. The plan now defines MySQL around a provider metadata database plus tenant databases, InnoDB MVCC, auto-increment journal sequencing, generated-column JSON indexing, and durable-progress recovery instead of Postgres-style notifications. It also records the replication and queue-claim assumptions that any future implementation must satisfy. | this roadmap is complete; future provider-topology implementation should start from a new active plan instead of reopening this file as live progress state |
| PX6 | Complete. The plan now requires steady-state, cold-start, and RTT-sensitive benchmark lanes plus listener-loss, reconnect, restart, pool-pressure, and tenant-lifecycle drills before the Postgres mode can promote into implementation ownership. | start `PX7` and promote the Postgres-first mode into a dedicated active implementation control plane |
| PX7 | Complete. The dedicated Postgres-first implementation control plane has landed, completed, and been archived at `docs/plans/archive/postgres-storage-provider-plan.md`, while this file returns to follow-on ownership for broader provider-topology design. | keep this plan deferred until replica-connected SQLite, MySQL, or later control-plane expansion work is activated |
| PX8 | Complete. The global usage/control path now lowers through an explicit `ControlPlaneProvider` role backed by `EmbeddedRedbControlPlaneProvider`, and embedded tenant persistence no longer has to smuggle control-plane construction through `EmbeddedRedbProvider`. Split control-plane directories are supported in typed service config. | `PX9` complete; keep the umbrella plan deferred until `PX3`, `PX5`, or a later control-plane/provider-topology slice activates |
| PX9 | Complete. `neovex-bin` now lowers CLI, env, and JSON config into `ServicePersistenceConfig`, supports explicit `control_data_dir`, and treats `NEOVEX_POSTGRES_URL` as the canonical Postgres resource input while keeping execution intent in the invoking command/profile surface. | roadmap complete; use this file as historical design baseline only and start any later provider-topology implementation from a new active plan |

## Execution Log

| Date | Phase | Outcome | Summary | Verification | Next Step |
|------|-------|---------|---------|--------------|-----------|
| 2026-04-08 | meta | created | Split Postgres/MySQL out of the SQLite migration plan so external backends no longer constrain the current storage replacement seam. | doc review | activate only after SQLite migration stabilizes |
| 2026-04-08 | meta | refined | Clarified that this follow-on plan owns the long-term post-SQLite seam shape: a stable engine-facing behavior contract derived from the settled SQLite contract, plus a typed backend construction/config seam for URL/config/pool-based backends rather than filesystem-path-shaped construction. | doc review | keep the SQLite migration seam temporary and defer permanent backend-construction design to this plan |
| 2026-04-09 | meta | refined | Added the durable naming and cost-model guardrails for the post-SQLite phase: call the stable tenant-scoped behavior seam `TenantPersistence`, call the separate typed config/routing seam `PersistenceProvider`, and treat external round-trip costs as a first-class design input so future Postgres/MySQL work stays coarse-grained and backend-native instead of becoming a chatty CRUD layer. | doc review | activate only after the SQLite migration is complete and the temporary backend-selection layer is gone |
| 2026-04-09 | meta | refined | Expanded this deferred plan from "external SQL only" into the broader owner of future non-local or coordinated provider work. It now explicitly covers replica-connected SQLite, distinguishes backend dialect from deployment topology, and reserves future embedded-provider coordination mechanisms for a later activation without forcing those implementation choices into the current SQLite cutover. | doc review | activate only after the local embedded-provider seam is stable and explicit product demand exists for networked or coordinated deployments |
| 2026-04-09 | meta | reviewed | Tightened this deferred plan into an activation-ready control plane by adding current-state findings, provider-topology invariants, success criteria, explicit risks, a sequenced roadmap, and the cross-tenant usage/control-path scope decision as an early gating item. | docs review against `ARCHITECTURE.md`, `docs/plans/README.md`, the archived SQLite migration plan, the redb encryption plan, and the current provider-seam code; `git diff --check` | keep deferred until we choose the first target mode, then start at `PX0` |
| 2026-04-09 | meta | sequenced | Fixed the first non-local provider target to Postgres because it currently has more traction than MySQL, and rewrote the roadmap so `PX0` now scopes the Postgres-first activation boundary instead of reopening the mode-selection question. Replica-connected SQLite and MySQL remain explicit follow-on slices. | docs review; `git diff --check` | when this plan is activated, start at `PX0` and keep the run Postgres-first unless the plan is amended explicitly |
| 2026-04-09 | meta | activated | Promoted this plan from deferred to active for the Postgres-first execution pass. Fresh contexts should now treat it as the live control plane for future provider-topology work rather than as an activation-ready note. | doc review | finish `PX0` by writing the explicit first-slice scope decision and corresponding architecture note |
| 2026-04-09 | PX0 | done | Scoped the first Postgres activation to tenant persistence only after confirming from `Service::new_with_simulation_and_embedded_provider`, `service/usage.rs`, `UsageStore`, and `RedbUsageStorage` that the current cross-tenant usage/control path is a separate local redb concern. Recorded that `UsageStore` / `RedbUsageStorage` stay local for the first non-local slice instead of silently expanding Postgres scope into a global control-plane redesign. | doc/code review; `git diff --check` | start `PX1` and codify the typed provider config model around this boundary |
| 2026-04-09 | PX1 | done | Codified the durable typed config model from the live embedded-only surfaces. Recorded that the future service-level config should split into `ServicePersistenceConfig`, `TenantProviderConfig`, and `ControlPlaneConfig`, with `PersistenceDialect` and `PersistenceTopology` as separate validated axes. Also recorded that `Service::new(data_dir)`, `Service::new_with_embedded_provider(...)`, `EmbeddedProviderKind`, and file-extension/path-based tenant discovery are retained embedded conveniences, not the durable cross-provider API. | doc/code review; `git diff --check` | start `PX2` and codify the common external-provider execution contract around coarse semantic operations and external cost-model constraints |
| 2026-04-09 | PX2 | done | Hardened the external-provider contract around capability families derived from the live seam: `TenantQueryRead`, `TenantMutationPersistence`, `TenantJournalPersistence`, `TenantSchedulerPersistence`, `TenantSnapshotPersistence`, and `TenantSchemaPersistence`. Recorded that external providers may optimize round trips below the seam, but the common contract must not collapse into path-shaped construction, hook-driven reactivity, row-at-a-time remote iterators, or chatty CRUD. | doc/code review; `git diff --check` | start `PX4` and design the concrete Postgres provider mode on top of this contract |
| 2026-04-09 | PX4 | done | Designed `PostgresProvider` as the first concrete non-local mode. The plan now fixes the layout to one provider-owned Postgres database with provider registry metadata plus per-tenant schemas, preserves Neovex logical journal rows as serialized blobs with Postgres-native sequence allocation, keeps notifications as wake hints rather than the reactive contract, and makes recovery depend on metadata plus journal replay rather than listener continuity. | doc/code review; `git diff --check` | start `PX6` and define the benchmark and operational gates for this Postgres mode |
| 2026-04-09 | PX6 | done | Defined the Postgres-specific readiness gate: steady-state, cold-start, and RTT-sensitive lanes; CRUD/query/journal/subscription/mixed-load and tenant-lifecycle workloads; and listener-loss, reconnect, restart, pool-pressure, and tenant cleanup drills. Recorded that Postgres need not beat embedded SQLite, but it must show predictable costs, explicit operator assumptions, and no semantic regressions before promotion. | doc/code review; `git diff --check` | start `PX7` and promote the Postgres-first mode into a dedicated active implementation control plane |
| 2026-04-09 | PX7 | done | Promoted the Postgres-first provider mode into `docs/plans/postgres-storage-provider-plan.md`, which now owns the active implementation work. Returned this umbrella plan to deferred follow-on ownership for replica-connected SQLite, MySQL, later control-plane expansion, and other non-local or coordinated provider-topology slices. | doc/code review; `git diff --check` | keep deferred until a later provider-topology slice needs activation |
| 2026-04-10 | meta | updated | Recorded that the dedicated Postgres-first implementation workstream is complete and archived at `docs/plans/archive/postgres-storage-provider-plan.md`, including the benchmark gate and operator-facing conclusion that Postgres remains an opt-in external mode rather than a latency replacement for the embedded default. This umbrella plan now resumes deferred ownership for future replica-connected SQLite, MySQL, and broader coordinated-provider follow-on work. | doc review | keep deferred until a later provider-topology slice needs activation |
| 2026-04-10 | meta | refined | Added two explicit follow-on cleanup slices to this umbrella plan: `PX8` for the remaining redb-backed cross-tenant control-plane seam, and `PX9` for the idiomatic runtime CLI/env/config contract that should lower into `ServicePersistenceConfig`. Recorded that harness env vars like `NEOVEX_TEST_POSTGRES_URL` and `NEOVEX_BENCH_POSTGRES_URL` remain harness inputs rather than the operator-facing runtime surface. | doc review | activate `PX8` or `PX9` explicitly when we are ready to clean up the control-plane seam or formalize runtime provider startup config |
| 2026-04-10 | meta | refined | Clarified the config principle for future provider cleanup: resource identity and execution intent should stay separate. A single typed Postgres connection value, potentially exposed as `NEOVEX_POSTGRES_URL`, is acceptable as the canonical resource input, while test, benchmark, and runtime semantics should be chosen by the invoking surface rather than by multiplying DSN env-var names. | doc review | when `PX9` activates, prefer one canonical resource input plus explicit command/profile semantics over purpose-specific DSN naming |
| 2026-04-10 | meta | activated | Reactivated this umbrella plan for the control-plane and runtime-config cleanup pass after the completed Postgres-first implementation. `PX8` became the live roadmap item and `PX9` followed immediately after it. | doc review | implement `PX8` in code, verify it, and then start `PX9` |
| 2026-04-10 | PX8 | done | Split the cross-tenant usage/control path into an explicit control-plane provider role. Landed `EmbeddedRedbControlPlaneProvider`, added `ControlPlaneProvider` to the engine seam, removed the hidden usage-store role from `EmbeddedRedbProvider`, and allowed typed embedded configs to use a separate control-plane data directory. | `cargo test -p neovex-engine embedded_providers -- --nocapture`; `cargo check -p neovex-engine`; `cargo fmt --all --check`; `cargo check --workspace` | start `PX9` and formalize the runtime CLI/env/config surface on top of the settled typed config seam |
| 2026-04-10 | PX9 | done | Landed the canonical runtime config lowering in `neovex-bin`: CLI, env, and JSON config now all lower into `ServicePersistenceConfig`, `control_data_dir` is explicit, and `NEOVEX_POSTGRES_URL` is the canonical Postgres resource input while runtime/test/benchmark intent stays with the invoking command/profile surface. Also refreshed `ARCHITECTURE.md` to match the live seams. | `cargo test -p neovex-bin -- --nocapture`; `cargo test -p neovex-engine postgres_provider -- --nocapture`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace`; `make check`; `make test`; `make clippy` | keep this umbrella plan deferred again until `PX3`, `PX5`, or a later provider-topology slice is activated explicitly |
| 2026-04-10 | PX3 | done | Closed the remaining replica-connected SQLite design gap. Recorded that raw network-mounted SQLite files are not an acceptable provider mode, defined future replica SQLite as a concrete primary/replica provider family with primary-owned writes and sequence-barrier reads, and required any implementation to start from a new dedicated active plan instead of reopening this umbrella file as live progress state. | doc review against `ARCHITECTURE.md`, `docs/plans/README.md`, and official SQLite network/WAL guidance; `git diff --check` | close the remaining MySQL design gap so this umbrella plan can become historical baseline only |
| 2026-04-10 | PX5 | done | Closed the remaining MySQL design gap. Recorded MySQL as a provider metadata database plus tenant-database model with InnoDB MVCC, auto-increment journal sequencing, generated-column JSON indexing, durable-progress recovery, and explicit replication/queue-claim assumptions that future implementation work must validate in a dedicated active plan. | doc review against `ARCHITECTURE.md`, `docs/plans/README.md`, and official MySQL locking/indexing/schema docs; `git diff --check` | rerun canonical provider verification with Docker available and write back any final implementation findings before treating this plan as closed historical baseline |
| 2026-04-10 | meta | completed | Closed the umbrella provider-topology plan as historical baseline. Once Docker Desktop was available, reran the container-backed Postgres provider suites and full repo verification. That surfaced a real provider concurrency bug: a single-thread `neovex-engine` background executor let the long-lived Postgres hint worker starve the mutation journal response path during restart recovery. Fixed the live code by widening the engine background executor to two worker threads, kept the restart-recovery drill deterministic, and reran the Docker-backed provider suites plus repo-wide verification successfully. | `cargo fmt --all --check`; `cargo test -p neovex-storage postgres_provider -- --nocapture`; `cargo test -p neovex-engine postgres_restart_recovers_due_scheduler_work_after_reopen -- --nocapture`; `cargo test -p neovex-engine postgres_provider -- --nocapture`; `cargo check --workspace`; `make check`; `make test`; `make clippy`; `make ci` (initial sandboxed run failed at `cargo deny` because the advisory DB lock under `/Users/jack/.cargo` was read-only; elevated rerun passed); `git diff --check` | roadmap complete; use this file as historical design baseline only and start future provider-topology implementation from a new active plan |
| 2026-04-10 | meta | refined | Added the same style of connector-fit analysis that guided the Postgres implementation choice. The umbrella plan and `ARCHITECTURE.md` now explicitly recommend a `libsql`-style provider family as the best first fit for replica-connected SQLite and `mysql_async` as the best first fit for MySQL, while recording why local-only SQLite drivers, `sqlx`, and `diesel-async` are weaker fits for Neovex's provider seam. | primary-source review of `libsql`, `mysql_async`, `sqlx`, `rusqlite`, and `testcontainers-modules::mysql`; `git diff --check` | future replica-SQLite or MySQL implementation should start from a new active plan that reuses these connector recommendations unless the source landscape changes materially |
