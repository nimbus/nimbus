# Plan: Postgres Tenant Persistence Provider

This plan owns the first concrete implementation of a non-local Neovex
provider: `PostgresProvider`.

It is activated from
`docs/plans/archive/external-sql-storage-backends-plan.md` after the Postgres-first
scope, config model, execution contract, provider shape, and readiness gate
were made explicit there. This plan turns those design decisions into code.

Reviewed against:

- `ARCHITECTURE.md`
- `docs/plans/README.md`
- `docs/plans/archive/external-sql-storage-backends-plan.md`
- `docs/plans/archive/pluggable-storage-backend-plan.md`
- `crates/neovex-engine/src/persistence.rs`
- `crates/neovex-engine/src/service/mod.rs`
- `crates/neovex-engine/src/service/tenants.rs`
- `crates/neovex-engine/src/service/usage.rs`
- `crates/neovex-engine/src/lib.rs`
- `crates/neovex-storage/src/async_storage/traits.rs`
- `crates/neovex-storage/src/async_storage/engine.rs`
- `crates/neovex-bin/src/main.rs`

---

## Status

- **Status:** `done`
- **Primary owner:** archived record of this completed workstream
- **Activation source:** promoted from `PX7` in
  `docs/plans/archive/external-sql-storage-backends-plan.md` on 2026-04-09
- **Completion:** `PG6` closed and this plan was archived on 2026-04-10
- **Scope:** first concrete implementation of tenant-scoped Postgres
  persistence while the cross-tenant usage/control path remains local redb

## Purpose

Land a production-quality first implementation of `PostgresProvider` on top of
the settled `TenantPersistence` / `PersistenceProvider` seam without
reintroducing path-shaped, hook-shaped, or chatty CRUD abstractions.

## Current Assessed State

- `TenantPersistence` is the stable engine-facing persistence seam, but the
  live implementation still only supports embedded redb and embedded SQLite.
- `PersistenceProvider` is still a concrete enum over embedded providers.
- `Service::new(data_dir)` and `Service::new_with_embedded_provider(...)`
  remain the dominant public construction surfaces.
- Sync tenant lifecycle helpers in `service/tenants.rs` still depend on file
  paths and embedded file extensions.
- `crates/neovex-engine/src/lib.rs` and `crates/neovex/src/lib.rs` still
  publicly re-export `EmbeddedProviderKind`, which reflects the current
  embedded-only construction story.
- The global usage/control path is still local and redb-backed through
  `UsageStore` and `RedbUsageStorage`, and it remains out of scope for this
  first Postgres implementation.
- A first storage-side `PostgresProvider` foundation now exists in
  `neovex-storage`: it owns provider metadata bootstrap plus tenant registry
  and schema-per-tenant lifecycle below the engine seam, and it is covered by
  focused lifecycle tests. The engine-facing persistence enum and service
  wiring still stop short of the Postgres path.

## Current Review Findings

- The most important first implementation slice is typed construction and
  service wiring. If the code starts from `PathBuf` plus `EmbeddedProviderKind`
  and tries to "grow" Postgres from there, the implementation will inherit the
  wrong seam.
- The Postgres path should be async-first. Existing blocking embedded
  convenience constructors and methods may remain for embedded providers, but
  the networked provider should not force the architecture to preserve a
  path-based blocking control surface as the canonical API.
- Query planning is already in a good place for Postgres-first work because the
  engine planner depends on the narrow planner-driven `QueryReadStore` seam
  instead of on redb-specific loading helpers.
- The tenant lifecycle surface still needs provider-owned routing and registry
  metadata before Postgres can support `list_tenants`, `create_tenant`,
  `open_existing_tenant`, and `delete_tenant`.
- The cross-tenant usage/control path must remain visibly separate throughout
  this plan so Postgres tenant persistence does not quietly expand into a
  global control-plane redesign.
- The idiomatic Postgres client stack for this repo is `tokio-postgres` plus
  a small async pool layer such as `deadpool-postgres`. That matches the
  existing Tokio runtime, keeps transaction and notification control explicit,
  and fits the provider-owned dynamic SQL and schema-qualified routing this
  provider needs.
- `sqlx` and `diesel-async` remain valid Rust options in the abstract, but
  they are not the best first fit here. `sqlx` is strongest when literal query
  macros and offline metadata are central, while this provider will rely on
  dynamic schema-qualified SQL and provider-owned statements. `diesel-async`
  is strongest when the application wants a schema-first ORM or query-builder
  layer, which is not the architectural direction of this provider seam.
- Canonical automated Postgres integration tests should be containerized and
  self-contained rather than assuming a developer-installed database. Prefer
  `testcontainers-modules::postgres` for the normal automated path, with an
  explicit environment override for an already-running Postgres instance when
  Docker is unavailable. A Homebrew-installed local Postgres remains a
  developer fallback and manual verification path, not the primary automated
  contract.

## Implementation Invariants

- `TenantPersistence` remains the stable engine-facing semantic contract.
- `PersistenceProvider` remains separate from `TenantPersistence`.
- The first Postgres slice is tenant-scoped only; `UsageStore` /
  `RedbUsageStorage` stay local redb.
- The Postgres layout is one provider-owned database plus one provider metadata
  schema and one Postgres schema per tenant.
- Durable journal rows remain serialized `DurableMutationRecord` blobs.
- Postgres sequence or identity allocation replaces embedded sequence
  bookkeeping for tenant journal ordering.
- Postgres notifications are wake hints only, never the canonical reactive
  contract.
- All mutations still flow through `Service::apply_mutation`.
- Runtime host operations still flow through the same service mutation/query
  paths.
- The implementation must not collapse into chatty remote CRUD or row-at-a-time
  iterator contracts.

## Success Criteria

- typed construction/config for Postgres lands behind the service/provider seam
- the Postgres provider owns tenant registry, routing, lifecycle, pooling, and
  schema-per-tenant storage layout
- query reads, mutation writes, scheduler state, schema persistence, durable
  journal behavior, and snapshot/bootstrap behavior preserve Neovex semantics
- the service and server can construct the Postgres mode without abusing the
  embedded-only path-based API
- the Postgres benchmark and operational gate is run and recorded
- the cross-tenant usage/control path is still explicit and unchanged

## Verification Contract

- always run `cargo fmt --all --check`
- always run `cargo check --workspace`
- run focused tests for touched crates as Postgres coverage is added
- before closing the plan, run:
  - `make check`
  - `make test`
  - `make clippy`
  - `make ci` if practical
- run the Postgres benchmark and operational gate defined in
  `docs/plans/archive/external-sql-storage-backends-plan.md`
- if environment restrictions block a required command, record the limitation
  in the Execution Log instead of silently skipping it

## Known Risks

| Risk | Impact | Mitigation |
| --- | --- | --- |
| The implementation could keep bending around `PathBuf` and `EmbeddedProviderKind` | high | make typed config and async-first provider wiring the first code slice |
| Sync service surfaces could force awkward blocking network behavior into the design | high | treat embedded sync helpers as convenience wrappers, not the canonical Postgres API |
| Postgres tenant routing could leak session state such as mutable `search_path` across pooled connections | high | use provider-owned registry metadata plus fully qualified schema SQL |
| Notification delivery could get mistaken for the authoritative journal contract | high | keep notifications as wake hints and recover from durable head state plus journal replay |
| Cross-tenant usage/control concerns could silently expand the scope | high | keep `UsageStore` / `RedbUsageStorage` explicitly local and out of scope |
| Per-tenant ordering could weaken under concurrent Postgres writers | medium | keep provider-owned per-tenant serialization around journal append and apply |
| Automated Postgres tests could become workstation-specific or flaky if they depend on a hand-managed local server | medium | make containerized integration tests the canonical path, allow an explicit Postgres URL override, and treat Homebrew/local services as manual fallback only |

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies | Gate Note |
| --- | --- | --- | --- | --- |
| PG0 | `done` | implemented `ServicePersistenceConfig`, `TenantProviderConfig`, and `ControlPlaneConfig` together with an async service constructor that embedded wrappers now lower into | none | do not let `PathBuf` plus `EmbeddedProviderKind` remain the canonical service config seam |
| PG1 | `done` | implemented the storage-side `PostgresProvider` foundation, including metadata bootstrap, provider-owned tenant registry, deterministic schema-per-tenant naming, and focused lifecycle tests on `tokio-postgres` plus `deadpool-postgres` | PG0 | finish this slice below the engine seam first so the later engine wiring can depend on a real provider instead of path-shaped placeholders |
| PG2 | `done` | integrated Postgres tenant lifecycle with the engine seam and implemented the tenant read foundation, snapshot boundary, and planner-driven query-read support | PG1 | async service tenant opening no longer assumes path-backed stores and preserves planner semantics behind the existing read seam |
| PG3 | `done` | implemented Postgres mutation, schema, scheduler, durable journal, and recovery behavior | PG2 | `TenantWriteCommit<T>` / `TenantWriteOutcome<T>`, `CommitEntry`, durable-head, and applied-head semantics are now preserved through a concrete provider-owned transactional path |
| PG4 | `done` | integrated Postgres notifications, service wiring, and CLI construction paths while keeping the redb control plane explicit | PG3 | notifications are wake hints only; global usage/control remains local redb |
| PG5 | `done` | ran the Postgres benchmark and operational gate, fixed the cold-start tenant-open and tenant-lifecycle harness blockers, and recorded the final report | PG4 | the provider is operationally ready, but the benchmark report shows it remains an opt-in external mode rather than a latency replacement for embedded SQLite |
| PG6 | `done` | reran the repo-wide verification contract after the final Postgres test-helper fixes, aligned the repo entry docs with the completed Postgres-first workstream, and archived this control plane cleanly | PG5 | the Postgres-first implementation is complete, benchmarked, and now historical context; broader future provider-topology work returns to the deferred umbrella plan |

## Dependency Graph

- `PG0` gates everything else.
- `PG1` depends on `PG0`.
- `PG2` depends on `PG1`.
- `PG3` depends on `PG2`.
- `PG4` depends on `PG3`.
- `PG5` depends on `PG4`.
- `PG6` depends on `PG5`.

## Recommended Delivery Order

1. `PG0` to land typed config and async-first construction.
2. `PG1` and `PG2` to establish provider-owned tenant lifecycle plus read
   foundations.
3. `PG3` to land the write, scheduler, journal, schema, and recovery contract.
4. `PG4` to wire Postgres mode into the service and public construction
   surfaces without moving the cross-tenant redb control path.
5. `PG5` to run the benchmark and operational gate.
6. `PG6` to align docs, verification, and ownership cleanup.

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| PG0 | Complete. The live code now exposes `ServicePersistenceConfig`, `TenantProviderConfig`, `ControlPlaneConfig`, and `Service::new_with_persistence_config(...)`, while the embedded constructors lower into the same construction logic and keep existing behavior green. | start `PG1` and replace filesystem-shaped tenant lifecycle assumptions with provider-owned registry and routing metadata for the Postgres path |
| PG1 | Complete. `neovex-storage` now owns a concrete `PostgresProvider` foundation on top of `tokio-postgres` plus `deadpool-postgres`, including metadata bootstrap, deterministic tenant-schema routing, provider-owned tenant registry and lifecycle methods, and focused lifecycle tests that run against either `testcontainers-modules::postgres` or an explicit Postgres URL override. Local manual verification is now available via Homebrew PostgreSQL 17.9 on this machine. | start `PG2` by extending the engine persistence seam to the Postgres provider and replacing the remaining path-backed async tenant lifecycle assumptions |
| PG2 | Complete. The live code now has a concrete `PostgresTenantStore`, `PostgresReadSnapshot`, and `PostgresTenantStorage` wired through `PersistenceProvider`, `TenantPersistence`, and the typed Postgres service constructor. Async tenant lifecycle, empty-read foundations, snapshot loading, journal progress reads, and planner-facing `QueryReadStore` calls all run through the existing semantic seam instead of path-shaped constructors. | start `PG3` by replacing the Postgres write/scheduler/schema/journal stubs with a real transactional implementation that preserves the existing commit and head-tracking semantics |
| PG3 | Complete. The Postgres provider now owns transactional schema writes, direct validated writes, execution-unit batch application, scheduled-execution dedupe, scheduler state, cron state, durable journal append/apply/recovery, and applied-head / durable-head updates. Focused storage and engine tests verify direct commits, journal replay, scheduler round-trips, async journaled mutations, and async schema writes against a live Postgres target. | start `PG4` by wiring the provider into the remaining service/server construction surfaces and by adding the provider-owned notification wake seam without touching the local redb control plane |
| PG4 | Complete. The Postgres provider now emits provider-owned `LISTEN`/`NOTIFY` wake hints for schema, journal, and scheduler visibility; the engine runs a reconnecting hint worker that reloads schema and catches up durable journal state from authoritative storage; async tenant preloading can now load tenants with scheduled work through the generic provider seam; and the CLI constructs typed Postgres persistence explicitly without weakening the local redb control-plane split. Focused storage, engine, and binary tests verify the notification path, cross-process catch-up, scheduler wake behavior, and typed CLI configuration against a live Postgres target. | start `PG5` and run the Postgres benchmark plus operational gate, including RTT-sensitive lanes and failure drills, from the now-complete provider wiring baseline |
| PG5 | Complete. The Postgres benchmark and operational gate now records a full report in `docs/research/postgres-provider-benchmark-report.md`, including steady-state, cold-start, RTT-sensitive, pool-pressure, and tenant-lifecycle lanes. The final harness fixes were: removing sync Postgres runtime construction from async tenant-open paths, adding a cold-start multi-tenant reopen regression test, and correcting the tenant-lifecycle SQLite contrast so it no longer tries an impossible second in-process open against the embedded redb control plane. The resulting benchmark outcome is explicit: Postgres is operationally ready and preserves the Neovex contract, but it is materially slower than embedded SQLite across this local benchmark mix and highly RTT-sensitive, so it should remain an external opt-in provider mode rather than a performance replacement for the embedded default. | start `PG6` by running repo-wide verification and aligning the active/deferred docs with the landed Postgres-first implementation state and benchmark conclusion |
| PG6 | Complete. The final closure pass fixed the default container-backed Postgres test path so workspace verification no longer panics when Docker is unavailable, and the later Docker-backed rerun closed a real restart-recovery starvation bug by widening the engine background executor from one worker thread to two. Repo-wide verification now passes with the canonical container-backed provider suites, and this plan is archived as historical implementation context. | resume future provider-topology work from `docs/plans/archive/external-sql-storage-backends-plan.md`, not from this archived record |

## Execution Log

| Date | Phase | Outcome | Summary | Verification | Next Step |
|------|-------|---------|---------|--------------|-----------|
| 2026-04-09 | meta | created | Promoted the Postgres-first provider design from `docs/plans/archive/external-sql-storage-backends-plan.md` into a dedicated active implementation control plane. Locked the implementation around the previously decided tenant-scoped boundary, typed config seam, schema-per-tenant layout, durable journal semantics, and Postgres readiness gate. | docs review | start `PG0` with typed config and async-first service/provider construction |
| 2026-04-09 | PG0 | done | Added `ServicePersistenceConfig`, `TenantProviderConfig`, `ControlPlaneConfig`, `PersistenceDialect`, `PersistenceTopology`, and related typed config helpers in `neovex-engine`, re-exported them through the public facade, and added `Service::new_with_persistence_config(...)` as the async-first construction boundary. Existing embedded constructors now lower into the same embedded construction logic, preserving current sqlite and redb behavior while returning an explicit "not implemented yet" error for the Postgres branch until later items land. | `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo test -p neovex-engine embedded_providers` | start `PG1` and implement provider-owned tenant registry, routing, and lifecycle for the Postgres path |
| 2026-04-09 | PG1 | blocked | Started the provider-foundation slice by inspecting the dependency surface and local verification options. There is currently no local `psql`, and `docker ps` fails because the Docker daemon is not running, so a high-confidence Postgres registry/lifecycle implementation cannot be verified yet. This is an environment blocker, not an architecture blocker; the typed config seam from `PG0` is ready for the next slice once a live Postgres target exists. | `docker ps` (failed: local Docker daemon not running); `which docker`; `which psql`; `ps -ax | rg postgres` | resume `PG1` with a live Postgres target, then land provider-owned tenant registry, routing, and schema-per-tenant lifecycle |
| 2026-04-09 | PG1 | refined | Reviewed current Rust/Postgres client and test options against the settled Neovex seam. Chose `tokio-postgres` plus `deadpool-postgres` as the provider foundation because the codebase is already Tokio-native and the provider needs explicit transactions, dynamic schema-qualified SQL, and `LISTEN`/`NOTIFY` wake hints. Chose containerized integration tests via `testcontainers-modules::postgres` as the canonical automated path, with an explicit environment URL override for externally managed Postgres and Homebrew/local services treated as manual fallback only. | primary-source doc review: `tokio-postgres`, `deadpool-postgres`, `sqlx`, `diesel-async`, `testcontainers`, `testcontainers-modules` | add Postgres deps and provider foundation, then resume `PG1` implementation against containerized or externally provided Postgres |
| 2026-04-09 | PG1 | resumed | Installed Homebrew `postgresql@17`, started the local service, and verified reachability with `psql`. Began the first concrete storage-side provider slice by adding `tokio-postgres`, `deadpool-postgres`, and `testcontainers-modules`, then landing a `PostgresProvider` registry/lifecycle foundation plus env-or-container-backed lifecycle tests in `neovex-storage`. The code still needs a compile-and-test tightening pass before the slice can be marked done. | `brew install postgresql@17`; `brew services start postgresql@17`; `which psql`; `psql -d postgres -c "select version();"` | finish the focused `neovex-storage` compile pass, fix any API mismatches, and run the Postgres lifecycle tests against the live local service |
| 2026-04-09 | PG1 | done | Finished the storage-side Postgres provider foundation in `neovex-storage`: added `PostgresProvider` and `PostgresProviderConfig`, provider-owned metadata bootstrap, deterministic tenant schema naming, tenant lifecycle methods, richer Postgres error reporting, and focused lifecycle tests that support either `testcontainers-modules::postgres` or an explicit `NEOVEX_TEST_POSTGRES_URL`. Verified the focused lane against the live local Homebrew PostgreSQL 17.9 service. | `cargo check -p neovex-storage`; `NEOVEX_TEST_POSTGRES_URL='host=/tmp user=jack dbname=postgres' cargo test -p neovex-storage postgres_provider -- --nocapture`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace`; `git diff --check` | start `PG2` and extend the engine persistence seam plus async tenant lifecycle to the Postgres provider |
| 2026-04-09 | PG2 | in_progress | Started the engine-side Postgres bridge. The active implementation approach is to keep the settled semantic seam intact and add a concrete Postgres tenant store/read-snapshot layer that satisfies `load_schema`, `journal_progress`, snapshot, and planner-driven `QueryReadStore` needs, rather than broadening the seam or reviving path-shaped constructors. | plan and code review | finish the concrete Postgres tenant store and wire async tenant lifecycle plus typed service construction through it |
| 2026-04-09 | PG2 | done | Finished the engine-side Postgres bridge. Added a concrete `PostgresTenantStore`, `PostgresReadSnapshot`, and `PostgresTenantStorage`, wired them through `PersistenceProvider`, `TenantPersistence`, and the typed Postgres service constructor, and closed the remaining path-backed async tenant lifecycle assumptions. Focused storage and engine tests now verify empty-read foundations, snapshot loading, planner-facing query reads, async tenant lifecycle, and reopen behavior against a live local Postgres target. | `env NEOVEX_TEST_POSTGRES_URL='host=/tmp user=jack dbname=postgres' cargo test -p neovex-storage postgres_provider -- --nocapture`; `env NEOVEX_TEST_POSTGRES_URL='host=/tmp user=jack dbname=postgres' cargo test -p neovex-engine postgres_provider -- --nocapture`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace` | start `PG3` and replace the Postgres write, schema, scheduler, durable-journal, and recovery stubs with a real transactional implementation |
| 2026-04-09 | PG3 | in_progress | Began the Postgres write-bearing slice from the now-verified read foundation. The implementation focus is to keep the existing engine seam and commit semantics intact while replacing the Postgres stubs with transactional writes, schema persistence, scheduler persistence, durable journal append/recovery, and head tracking inside the provider. | plan and code review | inspect the current SQLite and redb write paths, then land the first concrete Postgres transactional write/journal implementation |
| 2026-04-09 | PG3 | done | Finished the Postgres write-bearing slice. Replaced the remaining Postgres stubs with a concrete transactional implementation for schema persistence, direct validated writes, execution-unit batch application, scheduler and cron state, scheduled-execution dedupe, durable journal append/apply/recovery, and durable/applied head tracking, while keeping the existing engine-facing persistence seam intact. Added focused provider and engine tests that exercise direct commits, execution-unit batching, scheduler state, durable journal replay, async schema writes, and async journaled mutations against a live Postgres target. | `cargo check -p neovex-storage`; `cargo check -p neovex-engine`; `cargo test -p neovex-storage postgres_provider --no-run`; `cargo test -p neovex-engine postgres_provider --no-run`; `env NEOVEX_TEST_POSTGRES_URL='host=/tmp user=jack dbname=postgres' cargo test -p neovex-storage postgres_provider -- --nocapture`; `env NEOVEX_TEST_POSTGRES_URL='host=/tmp user=jack dbname=postgres' cargo test -p neovex-engine postgres_provider -- --nocapture`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace`; `git diff --check` | start `PG4` and wire Postgres mode through the remaining public construction surfaces plus a provider-owned notification wake path |
| 2026-04-09 | PG4 | in_progress | Began the Postgres wiring slice from the now-complete provider contract. The next work is to inspect the remaining service, server, and CLI entrypoints, keep the local redb control plane explicit, and add a provider-owned Postgres notification wake seam that acts as a hint rather than as the canonical journal contract. | plan and code review | inspect service/server construction and background wake paths, then land the first Postgres notification and public wiring changes |
| 2026-04-09 | PG4 | done | Finished the Postgres wiring slice. Added provider-owned Postgres wake-hint notifications for schema, journal, and scheduler visibility; a reconnecting service-side hint worker that reloads schema and catches up durable journal state from authoritative storage; async scheduled-work preloading through the generic provider seam; and typed CLI construction for the Postgres provider mode while keeping the cross-tenant usage/control path explicitly local and redb-backed. Added focused storage, engine, and binary tests that verify notification delivery, cross-process schema/journal catch-up, scheduler-driven tenant loading, and typed CLI config against a live local Postgres target. | `cargo fmt --all`; `cargo fmt --all --check`; `cargo check -p neovex-storage`; `cargo check -p neovex-engine`; `cargo test -p neovex-bin --no-run`; `cargo check --workspace`; `psql -d postgres -c 'select version();'` (required escalated local socket access); `NEOVEX_TEST_POSTGRES_URL='host=/tmp user=jack dbname=postgres' cargo test -p neovex-storage postgres_provider -- --nocapture` (required escalated local socket access); `NEOVEX_TEST_POSTGRES_URL='host=/tmp user=jack dbname=postgres' cargo test -p neovex-engine postgres_provider -- --nocapture` (required escalated local socket access); `cargo test -p neovex-bin -- --nocapture`; `git diff --check` | start `PG5` and run the Postgres benchmark plus operational gate from the now-complete provider wiring baseline |
| 2026-04-09 | PG5 | in_progress | Started the Postgres benchmark and operational gate. The next work is to adapt the retained embedded-provider benchmark harness to the Postgres provider mode, preserve the umbrella plan's steady-state, cold-start, and RTT-sensitive lanes, and add the listener-loss/reconnect, restart, pool-pressure, and tenant-lifecycle drills needed before closing the plan. Local Postgres socket verification in this sandbox still requires escalation because direct access to `/tmp/.s.PGSQL.5432` is denied without it. | plan review against `docs/plans/archive/external-sql-storage-backends-plan.md`; code review of `crates/neovex-engine/benches/embedded-provider-benchmarks.rs` | build the Postgres benchmark entrypoint and focused operational drills, then run and record the resulting report |
| 2026-04-09 | PG5 | refined | Tightened the reconnect recovery semantics by making listener reattachment trigger a provider-wide catch-up sweep and by adding a focused engine regression for the missed-hint path. The live debugging result is mixed: listener reconnect itself is reliable, but a deterministic reconnect drill that proves missed-notification recovery via authoritative catch-up is still not closed yet. This is now the active correctness blocker inside `PG5`; do not close the operational gate until the reconnect drill is stable and the resulting operator expectations are written down next to the benchmark report. | `cargo fmt --all`; `cargo check -p neovex-engine`; focused live-Postgres regression reruns via `NEOVEX_TEST_POSTGRES_URL='host=127.0.0.1 port=5432 user=jack dbname=postgres' cargo test -p neovex-engine postgres_listener_reconnect_recovers_missed_journal_hints -- --nocapture` (currently failing while listener restore succeeds but deterministic missed-hint catch-up remains unresolved) | continue `PG5` from the benchmark-harness side, but return to the reconnect drill before promotion and record the final outcome explicitly |
| 2026-04-09 | PG5 | refined | Closed the reconnect or missed-notification correctness gap. `TenantRuntime` schema snapshots now use atomic `ArcSwap` replacement, the Postgres provider exposes async schema and journal recovery reads, loaded-tenant catch-up during provider hints runs through the async Postgres path instead of blocking sync store calls, and tenants loaded before listener attachment reconcile inline from authoritative storage. Focused live-Postgres engine and storage regressions now verify steady-state notification refresh plus deterministic reconnect recovery of missed journal hints. | `cargo fmt --all --check`; `cargo check -p neovex-storage`; `cargo check -p neovex-engine`; `cargo check --workspace`; `NEOVEX_TEST_POSTGRES_URL='host=127.0.0.1 port=5432 user=jack dbname=postgres' cargo test -p neovex-storage postgres_notification_listener_reports_schema_journal_and_scheduler_hints -- --nocapture` (required escalated local TCP Postgres access); `NEOVEX_TEST_POSTGRES_URL='host=127.0.0.1 port=5432 user=jack dbname=postgres' cargo test -p neovex-engine postgres_notifications_refresh_loaded_runtime_schema_and_journal_state -- --nocapture` (required escalated local TCP Postgres access); `NEOVEX_TEST_POSTGRES_URL='host=127.0.0.1 port=5432 user=jack dbname=postgres' cargo test -p neovex-engine postgres_listener_reconnect_recovers_missed_journal_hints -- --nocapture` (required escalated local TCP Postgres access) | finish the Postgres benchmark harness and readiness report, then run the remaining restart, pool-pressure, and tenant-lifecycle operational drills |
| 2026-04-09 | PG5 | refined | Finished the remaining operational-drill fixes before the benchmark/report pass. Added provider-owned pooled-connection `application_name` tagging for Postgres observability, added focused engine drills for transient pooled-backend termination and tenant delete/recreate schema cleanup, and fixed restart recovery by making startup scheduled-work recovery run before the first Postgres hint-worker attach can race it. The binary startup path now uses the same explicit startup recovery helper. Focused storage and engine verification against the live local Postgres target now covers reconnect, notification catch-up, unloaded-tenant scheduler wake, restart recovery, pooled-backend failure recovery, and tenant lifecycle cleanup. | `cargo fmt --all`; `cargo fmt --all --check`; `cargo check -p neovex-storage`; `cargo test -p neovex-engine postgres_provider --no-run`; `cargo check --workspace`; `git diff --check`; `NEOVEX_TEST_POSTGRES_URL='host=127.0.0.1 port=5432 user=jack dbname=postgres' cargo test -p neovex-storage postgres_provider -- --nocapture` (required escalated local TCP Postgres access); `NEOVEX_TEST_POSTGRES_URL='host=127.0.0.1 port=5432 user=jack dbname=postgres' cargo test -p neovex-engine postgres_listener_reconnect_recovers_missed_journal_hints -- --nocapture --test-threads=1` (required escalated local TCP Postgres access); `NEOVEX_TEST_POSTGRES_URL='host=127.0.0.1 port=5432 user=jack dbname=postgres' cargo test -p neovex-engine postgres_notifications_load_unloaded_tenants_with_scheduled_work -- --nocapture --test-threads=1` (required escalated local TCP Postgres access); `NEOVEX_TEST_POSTGRES_URL='host=127.0.0.1 port=5432 user=jack dbname=postgres' cargo test -p neovex-engine postgres_restart_recovers_due_scheduler_work_after_reopen -- --nocapture --test-threads=1` (required escalated local TCP Postgres access); `NEOVEX_TEST_POSTGRES_URL='host=127.0.0.1 port=5432 user=jack dbname=postgres' cargo test -p neovex-engine typed_postgres_config_supports_async_tenant_lifecycle_and_empty_read_paths -- --nocapture --test-threads=1` (required escalated local TCP Postgres access); `NEOVEX_TEST_POSTGRES_URL='host=127.0.0.1 port=5432 user=jack dbname=postgres' cargo test -p neovex-engine postgres_transient_pool_backend_termination_recovers_subsequent_mixed_ops -- --nocapture --test-threads=1` (required escalated local TCP Postgres access) | move to the dedicated Postgres benchmark harness and report, then close the remaining pool-pressure observation and latency lanes |
| 2026-04-09 | PG5 | done | Closed the benchmark and readiness gate. Fixed the remaining harness blockers by removing synchronous Postgres runtime initialization from async tenant-open paths, adding a focused regression for concurrent multi-tenant reopen on a Postgres-backed service, and correcting the tenant-lifecycle SQLite contrast so it honors the embedded redb control-plane single-open constraint. The full `docs/research/postgres-provider-benchmark-report.md` report now lands cleanly with steady-state, cold-start, RTT-sensitive, pool-pressure, and tenant-lifecycle coverage. The benchmark outcome is explicit: Postgres preserves the Neovex semantics and passes the operational drills, but it is materially slower than embedded SQLite across the local benchmark mix and highly RTT-sensitive, so it should remain an opt-in external provider mode rather than a performance replacement for the embedded default. | `cargo fmt --all`; `cargo check -p neovex-engine --bench postgres-provider-benchmarks`; `NEOVEX_TEST_POSTGRES_URL='host=127.0.0.1 port=5432 user=jack dbname=postgres' cargo test -p neovex-engine typed_postgres_config_reopens_multiple_tenants_for_concurrent_mixed_ops -- --nocapture` (required escalated local TCP Postgres access); `make bench-postgres-provider REPORT=docs/research/postgres-provider-benchmark-report.md` (required escalated local TCP Postgres access); `cargo fmt --all --check`; `cargo check --workspace` | start `PG6` and finish repo-wide verification plus doc and ownership alignment for plan closure |
| 2026-04-10 | PG6 | done | Closed the Postgres-first implementation workstream. Fixed the default Postgres testcontainers path to use `AsyncRunner` instead of the sync runner so workspace tests skip cleanly without nested-runtime panics when Docker is unavailable, cleaned up follow-on clippy findings in the typed config and benchmark harness, reran the full repo verification contract, and archived this control plane while returning future provider-topology ownership to the deferred umbrella plan. | `cargo test -p neovex-engine postgres_provider -- --nocapture`; `cargo test -p neovex-storage postgres_provider -- --nocapture`; `cargo fmt --all`; `cargo fmt --all --check`; `make check`; `make test`; `make clippy`; `make ci` (required escalated filesystem access after the sandboxed `cargo deny` step could not write the shared Cargo registry under `/Users/jack/.cargo`) | workstream complete; use `docs/plans/archive/external-sql-storage-backends-plan.md` for future provider-topology work and this archived plan only for historical review |
| 2026-04-10 | PG6 | verified | With Docker Desktop available again, reran the canonical container-backed Postgres provider suites instead of relying on the earlier clean-skip path. That exposed a real restart-recovery starvation issue: a single-thread `neovex-engine` background executor could let the long-lived Postgres hint worker block mutation-journal response progress during scheduler recovery. Fixed the live code by widening the engine background executor to two worker threads, kept the restart drill deterministic, and reran the Docker-backed provider suites plus the repo-wide verification contract successfully. | `cargo fmt --all --check`; `cargo test -p neovex-storage postgres_provider -- --nocapture`; `cargo test -p neovex-engine postgres_restart_recovers_due_scheduler_work_after_reopen -- --nocapture`; `cargo test -p neovex-engine postgres_provider -- --nocapture`; `cargo check --workspace`; `make check`; `make test`; `make clippy`; `make ci` (initial sandboxed run failed at `cargo deny` because the advisory DB lock under `/Users/jack/.cargo` was read-only; elevated rerun passed); `git diff --check` | historical record complete; future provider-topology work should start from a new active plan rather than reopening this archived implementation pass |
