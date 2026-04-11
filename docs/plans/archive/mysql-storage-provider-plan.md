# Plan: MySQL Tenant Persistence Provider

This plan owns the first concrete implementation of a MySQL-backed Neovex
tenant persistence provider.

It is promoted from
`docs/plans/external-sql-storage-backends-plan.md` after the umbrella
provider-topology work established the durable `TenantPersistence` /
`PersistenceProvider` seam, the explicit control-plane split, the canonical
runtime config lowering, and the MySQL-specific design decision.

Reviewed against:

- `ARCHITECTURE.md`
- `docs/plans/README.md`
- `docs/plans/external-sql-storage-backends-plan.md`
- `docs/plans/archive/postgres-storage-provider-plan.md`
- `crates/neovex-engine/src/persistence.rs`
- `crates/neovex-engine/src/persistence_config.rs`
- `crates/neovex-engine/src/service/mod.rs`
- `crates/neovex-bin/src/main.rs`
- `crates/neovex-storage/src/async_storage/traits.rs`
- `crates/neovex-storage/src/postgres.rs`

Primary external references:

- MySQL 8.4: [InnoDB Locking Reads](https://dev.mysql.com/doc/refman/8.4/en/innodb-locking-reads.html)
- MySQL 8.4: [Secondary Indexes and Generated Columns](https://dev.mysql.com/doc/refman/8.4/en/create-table-secondary-indexes.html)
- MySQL 8.4: [INFORMATION_SCHEMA `SCHEMATA`](https://dev.mysql.com/doc/refman/8.4/en/information-schema-schemata-table.html)
- Rust `mysql_async`: [crate docs](https://docs.rs/mysql_async/latest/mysql_async/)
- Rust `sqlx`: [`query!` macro requirements](https://docs.rs/sqlx/latest/sqlx/macro.query.html)
- Rust `testcontainers-modules`: [MySQL module](https://docs.rs/testcontainers-modules/latest/testcontainers_modules/mysql/struct.Mysql.html)

---

## Status

- **Status:** `active`
- **Primary owner:** first concrete MySQL implementation workstream
- **Activation source:** promoted from the completed umbrella provider plan on
  2026-04-10
- **Sequencing decision:** MySQL goes first; replica-connected SQLite remains
  a separate deferred follow-on plan and must not reshape this workstream
- **Scope:** tenant-scoped MySQL persistence while the cross-tenant
  usage/control path remains on the explicit local redb control-plane seam

## Purpose

Land a production-quality MySQL provider on top of the settled Neovex seam
without weakening transaction semantics, turning the engine contract into
chatty CRUD, or importing Postgres-specific assumptions where MySQL needs a
different physical design.

## Current Assessed State

- The durable engine-facing seam is `TenantPersistence`.
- The separate construction/config seam is `PersistenceProvider` plus typed
  `ServicePersistenceConfig`, `TenantProviderConfig`, and `ControlPlaneConfig`.
- Embedded SQLite is the default local provider. Embedded redb is retained as
  another local provider.
- The first external provider implementation, `PostgresProvider`, is complete
  and archived at `docs/plans/archive/postgres-storage-provider-plan.md`.
- `PersistenceDialect::MySql` already exists in
  `crates/neovex-engine/src/persistence_config.rs`, so the typed config model
  already anticipates MySQL, but there is no `TenantProviderConfig::mysql`,
  no runtime CLI/env/config lowering for MySQL, no `PersistenceProvider`
  branch for MySQL, and no storage-side MySQL provider implementation.
- The control-plane split is explicit and should remain so: tenant-scoped
  provider work must not silently pull the global usage/control path off the
  local redb-backed control plane.
- The umbrella plan already fixed the intended MySQL physical shape:
  provider metadata database, tenant database per tenant, InnoDB MVCC,
  `AUTO_INCREMENT` commit-log sequencing, generated-column JSON indexing, and
  durable-progress recovery rather than Postgres-style notifications.

## Current Review Findings

- The best first-fit Rust connector stack for Neovex MySQL work is
  `mysql_async` directly. It is Tokio-native, already ships with an async
  pool, and keeps transaction control, locking reads, and dynamic fully
  qualified SQL explicit.
- `sqlx` remains a valid Rust option in the abstract, but it is not the best
  first fit here. Its strongest advantage is the `query!` macro family, and
  those checks require a build-time database or metadata plus string-literal
  SQL. A Neovex MySQL provider will rely on dynamic `tenant_db.table_name`
  SQL and provider-owned maintenance statements, so it would give up most of
  the macro advantage.
- `diesel-async` also remains valid in the abstract, but it is still centered
  on an ORM/query-builder model. That is the wrong center of gravity for a
  provider that needs backend-native SQL and explicit physical execution.
- MySQL must not be treated as “Postgres with different syntax.” The provider
  must start from MySQL-native database-per-tenant routing, generated-column
  JSON indexing, wake/catch-up strategy, and replication assumptions.
- Canonical automated verification should be containerized and self-contained
  via `testcontainers-modules::mysql`, with an explicit connection override
  for externally managed MySQL when containers are unavailable.

## Implementation Invariants

- `TenantPersistence` remains the stable engine-facing semantic contract.
- `PersistenceProvider` remains separate from `TenantPersistence`.
- The first MySQL slice is tenant-scoped only; the control plane remains on
  `ControlPlaneProvider::EmbeddedRedb`.
- Durable journal rows remain serialized `DurableMutationRecord` blobs.
- All mutations still flow through `Service::apply_mutation`.
- Runtime host operations still flow through the same service mutation/query
  paths.
- The implementation must not collapse into chatty remote CRUD or
  row-at-a-time iterator contracts.
- MySQL tenant routing must use fully qualified `tenant_db.table_name` SQL,
  not mutable default-database session state.
- Generated-column index maintenance and queue-claim semantics belong below
  the provider seam, not in the engine.

## Success Criteria

- typed construction/config for MySQL lands behind the existing service seam
- the MySQL provider owns tenant registry, routing, lifecycle, pooling, and
  database-per-tenant layout
- query reads, mutation writes, scheduler state, schema persistence, durable
  journal behavior, and snapshot/bootstrap behavior preserve Neovex semantics
- runtime startup can construct the MySQL mode from the canonical typed
  CLI/env/config surface without abusing embedded-only constructors
- the benchmark and operational gate is run and recorded
- the cross-tenant usage/control path remains explicit and unchanged

## Verification Contract

- always run `cargo fmt --all --check`
- always run `cargo check --workspace`
- run focused tests for touched crates as MySQL coverage is added
- before closing the plan, run:
  - `make check`
  - `make test`
  - `make clippy`
  - `make ci` if practical
- run a MySQL benchmark and operational gate analogous to the Postgres-first
  pass, including:
  - steady-state and cold-start lanes
  - latency-sensitive / injected-RTT contrast
  - CRUD, point-read, indexed query, journal stream/bootstrap,
    subscription fan-out, mixed multi-tenant load, and tenant lifecycle
  - reconnect/failure drills, queue-claim drills, pool-pressure drills, and
    tenant cleanup verification
- if environment restrictions block a required command, record that
  limitation in the Execution Log instead of silently skipping it

## Known Risks

| Risk | Impact | Mitigation |
| --- | --- | --- |
| MySQL implementation could inherit Postgres assumptions and become physically wrong even if tests pass | high | keep MySQL-native database routing, generated-column indexing, and wake/catch-up strategy explicit from the first slice |
| Dynamic tenant-database SQL could tempt the implementation toward stringly construction without clear ownership | high | keep routing and identifier construction provider-owned and deterministic |
| Queue-claim semantics could become replication-sensitive in surprising ways | high | treat `SKIP LOCKED` and claim logic as a first-class design item and record replication assumptions explicitly |
| The implementation could overfit to local loopback performance instead of networked cost shaping | medium | require steady-state, cold-start, and RTT-sensitive lanes before closure |
| Runtime config could regress into purpose-specific DSN env vars | medium | keep one canonical resource input and let command/profile semantics choose runtime vs test vs benchmark behavior |
| Cross-tenant control-plane concerns could silently expand scope | medium | keep `ControlPlaneProvider` explicit and out of scope for this first MySQL pass |

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies | Gate Note |
| --- | --- | --- | --- | --- |
| MY0 | `done` | landed the explicit MySQL activation boundary, typed config constructor, runtime lowering, and a service-level MySQL config branch while preserving the control-plane split | none | do not let MySQL start from embedded-only `PathBuf` assumptions |
| MY1 | `done` | implemented the storage-side `MySqlProvider` foundation on `mysql_async` with provider metadata bootstrap, deterministic tenant database naming, registry/lifecycle operations, and container-backed lifecycle coverage | MY0 | finish this slice below the engine seam before broader engine wiring |
| MY2 | `done` | integrated MySQL tenant lifecycle with the engine seam and implemented the tenant read foundation, snapshot boundary, and planner-driven query-read support | MY1 | async service tenant opening must no longer assume only Postgres or embedded providers |
| MY3 | `done` | implement MySQL mutation, schema, scheduler, durable journal, generated-column indexing, and recovery behavior | MY2 | `TenantWriteCommit<T>` / `TenantWriteOutcome<T>`, `CommitEntry`, durable-head, and applied-head semantics must stay intact |
| MY4 | `done` | integrate MySQL wake/catch-up behavior, service wiring, and runtime construction while keeping the redb control plane explicit | MY3 | wake signals are hints only; global usage/control remains local redb |
| MY5 | `done` | completed the MySQL benchmark and operational gate, recorded the full report, and fixed the async reconnect/cold-start wedge by routing async engine call sites back through `TenantPersistenceExecutor` instead of blocking store access on runtime threads | MY4 | MySQL need not beat SQLite, but it must show predictable costs and no semantic regressions |
| MY6 | `done` | reran the full repo verification contract, aligned docs, fixed the already-loaded scheduler reconnect race discovered under repo-wide verification, and archived this plan cleanly | MY5 | broader provider-topology work returns to the umbrella baseline after closure |

## Dependency Graph

- `MY0` gates everything else.
- `MY1` depends on `MY0`.
- `MY2` depends on `MY1`.
- `MY3` depends on `MY2`.
- `MY4` depends on `MY3`.
- `MY5` depends on `MY4`.
- `MY6` depends on `MY5`.

## Recommended Delivery Order

1. `MY0` to land the explicit MySQL activation boundary and typed runtime
   construction.
2. `MY1` and `MY2` to establish provider-owned tenant lifecycle plus read
   foundations.
3. `MY3` to land writes, schema, generated-column index maintenance,
   scheduler, journal, and recovery.
4. `MY4` to wire MySQL into service/runtime construction and provider wake /
   catch-up behavior without moving the control plane.
5. `MY5` to run the MySQL benchmark and operational gate.
6. `MY6` to align docs, verification, and ownership cleanup.

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| MY0 | Complete. Typed MySQL runtime construction now exists through `ServicePersistenceConfig::mysql`, `TenantProviderConfig::mysql`, `TenantRoutingConfig::DatabasePerTenant`, runtime CLI/env/config lowering, and a service-level MySQL config branch that preserves the explicit local redb control plane while returning a clear “not implemented yet” provider error until `MY1` lands. | start `MY1` by adding the storage-side `mysql_async` provider foundation and deterministic tenant-lifecycle coverage |
| MY1 | Complete. `mysql_async` is now wired into a concrete storage-side `MySqlProvider` with provider metadata bootstrap, deterministic database-per-tenant routing, lifecycle operations, `OpenedMySqlTenant`, and container-backed lifecycle coverage using the canonical `NEOVEX_MYSQL_URL` override path. | start `MY2` by extending the engine seam and giving `MySqlTenantStore` a real read foundation instead of identity-only accessors |
| MY2 | Complete. The engine seam now accepts MySQL, `MySqlTenantStore` exposes snapshot-backed query reads, empty bootstrap/journal behavior, and deterministic tenant lifecycle through `Service`, and focused engine coverage proves async create/list/reopen/query behavior against a live MySQL target. | start `MY3` by replacing the temporary MySQL write-path stubs with real transactional schema, mutation, scheduler, journal, and generated-column index behavior |
| MY3 | Complete. `MySqlTenantStore` now preserves direct-write, schema, scheduler, durable-journal, generated-column index, and recovery semantics through a real transactional path; the async engine seam consumes the same MySQL write contract without stubbed “not implemented yet” branches. | start `MY4` by adding the MySQL-native steady-state catch-up worker and cross-service scheduler loading behavior |
| MY4 | Complete. The engine now starts a MySQL-native background poller instead of pretending MySQL has Postgres-style notifications; the poller refreshes loaded schema/journal state, loads unloaded tenants with scheduled work, and wakes the scheduler when the observed next-due frontier changes. | start `MY5` by building and running the MySQL benchmark and operational gate |
| MY5 | Complete. The dedicated MySQL benchmark harness now runs through the canonical `make bench-mysql-provider` path, writes `docs/research/mysql-provider-benchmark-report.md`, covers CRUD/point-read/indexed-query/journal/subscription/mixed-load/tenant-lifecycle lanes plus RTT and pool-pressure observations, and no longer wedges during cold-start reconnect drills because async engine startup/poll/recovery paths now use `TenantPersistenceExecutor` instead of synchronous store access on runtime threads. | start `MY6` by running `make check`, `make test`, `make clippy`, and `make ci` if practical, then archive this plan cleanly |
| MY6 | Complete. Repo-wide closure verification passed through `make check`, `make test`, `make clippy`, and `make ci`; the wider run also caught and fixed an already-loaded scheduler recovery race so provider wake/reconnect no longer requeues in-flight claimed work. This plan is ready to live as archived history while replica-connected SQLite becomes the next active provider-topology pass. | activate `docs/plans/sqlite-replica-provider-plan.md` at `RS0` and hand future provider-topology implementation work to that control plane |

## Execution Log

| Date | Phase | Outcome | Summary | Verification | Next Step |
|------|-------|---------|---------|--------------|-----------|
| 2026-04-10 | meta | created | Promoted the first MySQL provider implementation into its own active control plane after the umbrella provider-topology plan was completed. This plan inherits the settled `TenantPersistence` / `PersistenceProvider` seam, the explicit local redb control-plane split, and the MySQL-specific design decision from the umbrella baseline. | docs review against `ARCHITECTURE.md`, `docs/plans/README.md`, `docs/plans/external-sql-storage-backends-plan.md`, and `docs/plans/archive/postgres-storage-provider-plan.md`; `git diff --check` | start `MY0` by wiring MySQL into the typed service/runtime config surface without changing the control-plane boundary |
| 2026-04-10 | MY0 | done | Added `ServicePersistenceConfig::mysql`, `TenantProviderConfig::mysql`, and `TenantRoutingConfig::DatabasePerTenant`; extended the runtime CLI/env/config surface with canonical MySQL resource and routing inputs; and wired a service-level MySQL persistence branch that preserves the explicit local redb control plane while failing clearly until the provider foundation lands. | `cargo fmt --all --check`; `cargo test -p neovex-bin -- --nocapture`; `cargo check --workspace`; `git diff --check` | start `MY1` with a storage-side `mysql_async` provider foundation, deterministic tenant database naming, and container-backed lifecycle coverage |
| 2026-04-10 | MY1 | done | Added a concrete `MySqlProvider` on `mysql_async` with a shared no-default-database pool, provider metadata bootstrap, deterministic database-per-tenant naming, registry/lifecycle operations, `OpenedMySqlTenant`, and identity/read-executor foundations; added container-backed lifecycle tests that use `NEOVEX_MYSQL_URL` as the canonical external override and otherwise start a testcontainers MySQL instance. | `cargo fmt --all`; `cargo fmt --all --check`; `cargo check -p neovex-storage`; `cargo test -p neovex-storage mysql_provider -- --nocapture`; `cargo check --workspace`; `git diff --check` | start `MY2` by wiring MySQL into the engine persistence seam and replacing the identity-only tenant store with planner-ready read and snapshot foundations |
| 2026-04-10 | MY2 | done | Wired MySQL through `PersistenceProvider`, `TenantPersistence`, and the service construction path; expanded `MySqlTenantStore` into a snapshot-backed read surface with schema/doc/journal bootstrap loading and planner-facing query operations; added focused engine coverage proving async create/list/reopen/query/bootstrap behavior through `Service` against a live MySQL target while keeping writes explicitly deferred to the next slice. | `cargo fmt --all`; `cargo fmt --all --check`; `cargo test -p neovex-storage mysql_provider -- --nocapture`; `cargo test -p neovex-engine mysql_provider -- --nocapture`; `cargo check -p neovex-storage`; `cargo check -p neovex-engine`; `cargo check --workspace`; `git diff --check` | start `MY3` by replacing the temporary MySQL write-path stubs with real transactional schema, mutation, scheduler, journal, and generated-column index behavior |
| 2026-04-10 | MY3 | done | Replaced the temporary MySQL write-path stubs with a real transactional `MySqlWriteTransaction`, async write executor, generated-column index DDL, scheduler/cron persistence, durable journal append/replay, recovery behavior, and engine-side MySQL write delegation; added focused storage coverage for direct writes, execution-unit batches, durable replay, and MySQL-specific generated-column/index lifecycle. | `cargo test -p neovex-storage mysql_provider -- --nocapture`; `cargo test -p neovex-engine mysql_provider -- --nocapture`; `cargo fmt --all --check`; `cargo check --workspace` | start `MY4` by wiring a MySQL-native steady-state catch-up strategy through the engine background-worker seam |
| 2026-04-10 | MY4 | done | Added a MySQL-native provider background poller instead of borrowing Postgres notification assumptions; the engine now refreshes loaded MySQL tenants’ schema/journal state, loads unloaded tenants with scheduled work, and wakes the scheduler when the observed MySQL next-due frontier changes. Added cross-service MySQL engine tests for loaded-runtime catch-up and unloaded scheduled-work pickup. | `cargo test -p neovex-engine mysql_provider -- --nocapture`; `cargo fmt --all --check`; `cargo check --workspace` | start `MY5` by building the dedicated MySQL benchmark and operational report |
| 2026-04-10 | MY5 | done | Completed the MySQL benchmark and operational gate, wrote `docs/research/mysql-provider-benchmark-report.md`, and fixed the cold-start/reconnect wedge that had stalled the mixed-load lane by routing async tenant startup, poll-worker refresh, scheduler recovery, and mutation-journal recovery paths back through `TenantPersistenceExecutor` instead of blocking synchronous store calls on Tokio runtime threads. The recorded report shows SQLite winning all local loopback contrast lanes, near-parity only on steady-state point reads, and pronounced RTT sensitivity for MySQL (for example 46.46x on CRUD and 42.34x on mixed-load) while pool-pressure observation kept provider-attributed MySQL threads capped at the configured pool bound of 2. | `cargo test -p neovex-engine mysql_provider -- --nocapture`; `cargo run -p neovex-engine --release --example mysql_provider_benchmarks -- --workload mixed-load`; `make bench-mysql-provider REPORT=docs/research/mysql-provider-benchmark-report.md`; `cargo fmt --all --check`; `cargo check --workspace` | start `MY6` by running the repo-wide verification contract and then archive the completed MySQL plan |
| 2026-04-10 | MY6 | done | Closed the MySQL workstream with the full repo-wide verification contract and archival cleanup. Repo-wide verification surfaced one broader correctness issue: already-loaded scheduler wake/reconnect paths were incorrectly rerunning `recover_running_jobs`, which could requeue claimed work after provider wake or reconnect. The fix keeps recovery on unloaded/startup activation only and leaves live claim ownership with the running scheduler. | `cargo test -p neovex-engine postgres_restart_recovers_due_scheduler_work_after_reopen -- --nocapture`; `cargo fmt --all --check`; `cargo check --workspace`; `make check`; `make test`; `make clippy`; `make ci` | archive this plan and activate `docs/plans/sqlite-replica-provider-plan.md` at `RS0` |
