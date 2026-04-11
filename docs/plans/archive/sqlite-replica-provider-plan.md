# Plan: Replica-Connected SQLite Provider

This plan was the serial follow-on for replica-connected SQLite after the
MySQL workstream completed.

It is promoted from
`docs/plans/external-sql-storage-backends-plan.md`, which already closed the
high-level design decision that replica-connected SQLite must not mean
"SQLite over a network-mounted file."

Reviewed against:

- `ARCHITECTURE.md`
- `docs/plans/README.md`
- `docs/plans/external-sql-storage-backends-plan.md`
- `docs/plans/archive/pluggable-storage-backend-plan.md`
- `docs/plans/archive/postgres-storage-provider-plan.md`
- `crates/neovex-engine/src/persistence.rs`
- `crates/neovex-engine/src/persistence_config.rs`
- `crates/neovex-storage/src/sqlite.rs`

Primary external references:

- SQLite: [Use Of SQLite Over A Network](https://www.sqlite.org/useovernet.html)
- SQLite: [Write-Ahead Logging](https://www.sqlite.org/wal.html)
- Rust `libsql`: [Builder](https://docs.rs/libsql/latest/libsql/struct.Builder.html)
- Rust `libsql`: [Database replication methods](https://docs.rs/libsql/latest/libsql/struct.Database.html)
- Rust `rusqlite`: [Connection](https://docs.rs/rusqlite/latest/rusqlite/struct.Connection.html)

---

## Status

- **Status:** `complete`
- **Readiness:** archived after verification and benchmark gate completion
- **Primary owner:** historical record for the first concrete
  replica-connected SQLite implementation
- **Sequencing decision:** MySQL completed first; this follow-on plan is now
  finished and should not be resumed as live progress state
- **Target family:** the completed first implementation is the `libsql`-first
  remote-primary plus provider-owned replica-cache family, not a generic "any
  replicated SQLite" abstraction

## Purpose

Land a concrete replica-connected SQLite provider without weakening Neovex's
semantic contract or pretending that local SQLite drivers alone solve
replication, read barriers, failover, or catch-up behavior.

## Current Assessed State

- Embedded SQLite is already the default local provider and remains the
  strongest embedded fit for Neovex.
- The stable engine-facing seam is `TenantPersistence`, and the separate
  construction/config seam is `PersistenceProvider`.
- The umbrella provider plan already closed the high-level design decision:
  raw network-mounted SQLite files are not an acceptable provider mode.
- The repo already has a strong local SQLite foundation in
  `crates/neovex-storage/src/sqlite.rs`. Replica-topology storage foundations
  now exist for the `libsql` remote-primary plus provider-owned cache shape,
  and the remote-primary write, recovery, and provider-poll catch-up path now
  exist end to end. The replica-specific benchmark gate is complete, the
  report is recorded, and the remaining work for this file is archival
  context only.
- The completed control plane left the storage layer with a concrete
  `LibsqlReplicaProvider` foundation with provider-owned metadata and tenant
  namespace lifecycle, deterministic remote-snapshot refresh into local SQLite
  cache files, and opened-tenant activation on those cache files below the
  engine seam.
- Engine integration now includes primary-routed writes, provider-poll-driven
  catch-up for loaded and unloaded tenants, explicit durable or applied target
  tracking across the mutation journal, and recovery that replays pending
  durable records onto the remote primary before refreshing the derivative
  cache.

## Current Review Findings

- The best first-fit Rust connector family for this work is `libsql`, not a
  plain local SQLite driver stretched into a topology problem.
- `libsql` already models the provider shapes Neovex cares about here:
  remote or local replicas, synced databases, delegated writes,
  `read_your_writes`, and explicit sync/catch-up behavior.
- `rusqlite` and `sqlx::Sqlite` remain strong local SQLite tools, but they are
  not enough on their own for replica topology, remote-write delegation, or
  sequence-barrier reads.
- This plan should activate one concrete `libsql` family shape up front,
  rather than promising a generic abstraction over many possible replica
  transports.
- The data-plane `libsql` URL is not sufficient for provider-owned
  namespace-per-tenant lifecycle. Local `sqld` requires an explicit admin API
  to create namespaces, so the concrete provider config must model a separate
  provisioning/control-plane endpoint instead of assuming first-connect
  namespace creation.
- Plain `libsql` remote connectivity is viable in the current harness, but the
  embedded-replica activation path is not yet. A control probe against the
  same local `sqld` server can open `libsql::Builder::new_remote(...)`, while
  `libsql::Builder::new_remote_replica(...).build()` exits abnormally before
  the tenant can serve reads. The `RS2` redesign must therefore treat replica
  sync ownership as an explicit provider boundary instead of assuming the main
  Neovex process can safely host the embedded-replica client directly.
- The first concrete sync model should now be Neovex-owned and deterministic:
  read a consistent snapshot from the remote `libsql` namespace over SQL and
  refresh provider-owned local SQLite cache files directly, rather than making
  `new_remote_replica(...)` the first activation path.
- The redesigned storage seam is now proven in the live harness: an opened
  tenant can refresh a remote `libsql` namespace into a provider-owned local
  SQLite cache file, reopen that file through the canonical
  `SqliteTenantStore` / `SqliteTenantStorage` path, and serve indexed reads
  correctly without reviving the in-process embedded-replica runtime path.
- The engine seam now lazy-loads replica-backed tenants through the real
  `LibsqlReplicaProvider` path, routes writes and scheduler mutations to the
  remote primary, and relies on provider-owned cache refresh or poll-driven
  catch-up before planner reads consume the derivative local SQLite cache.
- The first meaningful replica durable or applied barrier contract is now
  implemented through remote-primary journal apply, explicit required-cache
  sequence tracking, and provider polling that recovers both loaded and
  unloaded tenants from authoritative remote state instead of trusting cache
  freshness alone.

## RS0 First Provider Family Decision

The first concrete replica-connected SQLite implementation will target a
`libsql` remote-primary plus provider-owned replica-cache family, not a generic
"replicated SQLite" abstraction.

The exact `RS0` decision is:

- keep `dialect = Sqlite` and represent the new mode through
  `topology = ExternalPrimaryWithReplicas`
- implement a concrete `LibsqlReplicaProvider` family below that typed config
  rather than stretching `EmbeddedSqliteProvider` across networked semantics
- pair a remote-primary `libsql::Builder::new_remote(...)` connection for
  writes and strong reads with provider-owned replica cache files, and make
  the first sync/catch-up path a provider-owned snapshot refresher that reads
  the remote namespace over SQL and rebuilds those cache files deterministically
- use one provider metadata namespace on the remote primary plus one namespace
  per tenant; keep the cross-tenant control plane itself on explicit local
  redb
- treat namespace provisioning as an explicit provider control-plane concern:
  the concrete `libsql` family must carry separate management endpoint/credential
  inputs whenever provider-owned namespace lifecycle is enabled, rather than
  assuming the read/write URL can create namespaces on first connect
- keep local replica files provider-owned under a configured replica cache
  root; do not accept arbitrary user-supplied SQLite files as the
  replica-topology seam
- keep in-process SQLite query execution on Neovex-owned local SQLite
  foundations only after the provider-owned sync boundary has produced or
  refreshed those replica files; do not make `new_remote_replica(...)` the
  default hot path
- keep mutations, scheduler claim/complete paths, journal append/stream,
  bootstrap/export, and mutation-adjacent reads on the primary or behind an
  explicit provider-owned barrier refresh / catch-up path
- allow planner-driven query reads to use the embedded replica only after a
  provider-owned durable/applied sequence barrier is proven locally or after
  an explicit `sync_until` catch-up
- defer `libsql::Builder::new_synced_database(...)` and offline-capable local
  writes from the first slice because they would expand this roadmap into
  conflict resolution, disconnected-write policy, and multi-writer semantics
  that Neovex does not need for the first activation
- prefer provider-owned automated verification through a self-hosted
  `libsql-server` harness, with a canonical external resource override for
  managed environments, instead of assuming a raw filesystem test fixture

## Implementation Invariants

- `TenantPersistence` remains the stable engine-facing semantic contract.
- `PersistenceProvider` remains separate from `TenantPersistence`.
- The first replica-connected SQLite slice remains tenant-scoped only; the
  control plane stays on the explicit local redb control-plane seam.
- Replica reads must sit behind a provider-owned durable/applied sequence
  barrier. The first slice may re-establish that barrier through
  provider-owned cache refresh or poll-driven catch-up instead of direct
  planner reads against the primary.
- Durable journal rows remain serialized `DurableMutationRecord` blobs.
- All mutations still flow through `Service::apply_mutation`.
- Wake signals remain hints only; authoritative recovery comes from durable
  progress and journal replay.
- The implementation must not rely on raw network-mounted SQLite files.
- The main Neovex process must not depend on in-process
  `libsql::Builder::new_remote_replica(...).build()` until that runtime path
  is proven stable in the live harness.
- The first replica refresh path may be a full provider-owned snapshot rebuild
  as long as it preserves durable/applied sequence semantics and keeps the
  local cache explicitly derivative of the remote primary.

## Success Criteria

- one concrete `libsql`-family provider mode is chosen and implemented
- typed config distinguishes replica topology from embedded local SQLite
- replica reads, barrier refresh/catch-up, bootstrap, journal, and recovery
  preserve Neovex semantics
- the provider has a documented sync-owner boundary, sync/catch-up, failover,
  and promotion story
- benchmark and operational gates are recorded before closure

## Verification Contract

- always run `cargo fmt --all --check`
- always run `cargo check --workspace`
- run focused tests for touched crates as replica-SQLite coverage is added
- before closing the plan, run:
  - `make check`
  - `make test`
  - `make clippy`
  - `make ci` if practical
- run a replica-specific benchmark and operational gate including:
  - replica lag and catch-up latency
  - same-service barrier refresh / strong-read recovery latency
  - steady-state and cold-start lanes
  - mixed read/write multi-tenant load
  - failover/promotion or delegated-write drills for the chosen family

## Known Risks

| Risk | Impact | Mitigation |
| --- | --- | --- |
| The implementation could accidentally mean “SQLite over a network file” | high | keep the chosen provider family explicit and reject raw network-mounted files from the start |
| Replica progress could be treated too loosely and weaken semantic reads | high | require explicit sequence-barrier logic plus provider-owned refresh/catch-up proof before replica reads claim fresh state |
| A generic abstraction over multiple replica families could dilute the first implementation | medium | activate one concrete `libsql` family shape first |
| The roadmap could drift into offline-write or multi-writer `SyncedDatabase` semantics too early | medium | keep the first activation on remote-primary plus embedded-replica topology only; defer disconnected-write semantics explicitly |
| Local SQLite assumptions could leak into topology, failover, or wake behavior | medium | keep sync/catch-up, delegated writes, and promotion policy explicit in provider config and operational drills |
| Namespace lifecycle could be modeled as if the data-plane SQL URL were enough | high | require explicit control-plane provisioning inputs for provider-owned namespace creation/deletion and verify them against a live `sqld` admin API harness |
| The `libsql` embedded-replica runtime path could remain unstable even when plain remote `libsql` connectivity works | high | keep the sync-owner boundary explicit, prove stability with isolated control probes plus provider harness coverage, and do not make the main Neovex process the default host for `new_remote_replica(...)` until that proof exists |

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies | Gate Note |
| --- | --- | --- | --- | --- |
| RS0 | `done` | fixed the first concrete provider family as a `libsql` remote-primary plus provider-owned replica-cache mode, with remote strong-read fallback, provider-owned replica cache paths, and explicit rejection of offline-write `SyncedDatabase` semantics for the first slice | none | do not reopen the rejected network-mounted-file shape |
| RS1 | `done` | added typed config, CLI/env/config lowering, documented flags, and a clear service-level activation boundary for the chosen remote-primary plus embedded-replica `libsql` family | RS0 | dialect and topology must stay separate |
| RS2 | `done` | implement the storage-side provider foundation, snapshot-refresh sync boundary, control-plane namespace lifecycle, and tenant lifecycle for the chosen `libsql` remote-primary plus provider-owned replica-cache family | RS1 | completed below the engine seam without promising in-process embedded-replica activation |
| RS3 | `done` | integrate the replica read foundation into the engine seam, wire real lazy-open/query reads through `LibsqlReplicaProvider`, and make replica-backed local writes fail clearly until the primary-write path lands | RS2 | query reads and bootstrap must preserve semantic boundaries |
| RS4 | `done` | implemented remote-primary writes, provider-poll catch-up, durable journal replay or recovery, scheduler semantics, and the first explicit replica durable or applied barrier contract for provider-owned local caches | RS3 | wake signals remain hints only |
| RS5 | `done` | ran the benchmark and operational gate for the chosen replica family, recorded the full report, and confirmed the freshness drills against the live `sqld` environment | RS4 | barrier refresh and peer catch-up drills must stay explicitly recorded before closure |
| RS6 | `done` | reran repo-wide verification, aligned the docs index, and archived the completed plan with the remaining repo-baseline `cargo deny` failure recorded honestly | RS5 | broader provider-topology work returns to the umbrella baseline after closure |

## Dependency Graph

- `RS0` gates everything else.
- `RS1` depends on `RS0`.
- `RS2` depends on `RS1`.
- `RS3` depends on `RS2`.
- `RS4` depends on `RS3`.
- `RS5` depends on `RS4`.
- `RS6` depends on `RS5`.

## Recommended Delivery Order

1. `RS1` and `RS2` to land config plus provider foundations before broad engine
   wiring.
2. `RS3` to land engine read integration and the explicit read-only contract.
3. `RS4` to add primary-route writes, the first meaningful sequence-barrier /
   strong-read-fallback contract, journal semantics, scheduler semantics, and
   recovery.
4. `RS5` to measure replica-specific correctness and performance before closure.
5. `RS6` to rerun repo-wide verification and archive the plan cleanly.

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| RS0 | Complete. The first concrete target is now fixed as a paired `libsql` family: remote primary for writes and strong reads, provider-owned replica cache files for read-serving, remote metadata namespace plus per-tenant namespaces, and no first-slice offline-write `SyncedDatabase` semantics. The sync owner for those replica files remains an explicit provider concern rather than an implied in-process `new_remote_replica(...)` call. | start `RS1` by extending typed config with the remote-primary resource, auth, namespace/routing, replica cache root, and catch-up policy inputs this family needs |
| RS1 | Complete. `ServicePersistenceConfig::sqlite_replica(...)` and `TenantProviderConfig::sqlite_replica(...)` now model the chosen `libsql` family with explicit remote primary credentials, namespace routing, and provider-owned replica cache inputs; the CLI/env/config surface lowers those inputs canonically, and service construction fails clearly until the provider foundation lands. | start `RS2` by implementing the storage-side `libsql` provider foundation, tenant lifecycle, and sync/catch-up seams below the engine boundary |
| RS2 | Complete. The storage layer now has a concrete `LibsqlReplicaProvider` with explicit admin-driven namespace lifecycle, deterministic remote snapshot refresh into provider-owned local SQLite cache files, and opened-tenant activation that reuses the canonical `SqliteTenantStore` / `SqliteTenantStorage` seam. Live verification proves the provider can materialize a remote namespace into a local cache file and serve indexed reads from that derivative SQLite state without reviving the unstable in-process embedded-replica runtime path. | start `RS3` by moving the engine seam onto the proven opened-tenant read surfaces and adding sequence-barrier plus refresh/catch-up semantics |
| RS3 | Complete. The engine now has a real `LibsqlReplicaProvider` composition-root path, maps opened replica tenants onto a dedicated replica-backed `TenantPersistence` read seam, lazy-loads and queries the provider-owned local SQLite cache successfully, and rejects local replica writes clearly instead of mutating the cache. That lands the engine read foundation without pretending the embedded cache is already a writable primary. | start `RS4` by routing mutations, scheduler writes, and durable journal updates to the remote primary while defining the first durable/applied barrier target and refresh/catch-up contract |
| RS4 | Complete. Replica-backed tenants now route writes, scheduler mutations, durable journal append or apply, and crash recovery to the remote primary while keeping planner reads on provider-owned local SQLite cache generations. The engine also starts a dedicated replica poll worker so loaded runtimes refresh schema and journal state, unloaded tenants with scheduled work are discovered, and applied-head waits no longer depend on synchronous cache refresh during the async mutation journal path. | start `RS5` by measuring replica lag, steady-state read or write behavior, and the operational barrier-refresh / delegated-write drills that decide whether this provider family is ready to close |
| RS5 | Complete. The dedicated benchmark report now lives at `docs/research/sqlite-replica-provider-benchmark-report.md`, with the full contrast scorecard plus the replica-only freshness drills recorded against the live local `sqld` environment. The measured readiness gate shows same-service barrier refresh at 16.21 ms median / 17.83 ms p95 and peer catch-up at 537.88 ms median / 563.94 ms p95, which matches the expected provider poll-driven freshness model. | start `RS6` by rerunning repo-wide verification and then archive the plan cleanly |
| RS6 | Complete. Repo-wide Rust checks, tests, clippy, and JS build/test lanes all passed after the benchmark gate closed, and this plan is ready to archive. `make ci` was also run and failed only on the existing `cargo deny` baseline (`RUSTSEC-2026-0049` in the `libsql` tree plus the current license/duplicate policy surface), which is recorded here rather than hidden. | archive this completed plan and return future provider-topology work to the umbrella baseline or to a newly promoted active plan |

## Execution Log

| Date | Phase | Outcome | Summary | Verification | Next Step |
|------|-------|---------|---------|--------------|-----------|
| 2026-04-10 | meta | created | Authored the dedicated replica-connected SQLite follow-on plan after the umbrella provider-topology plan closed its design baseline. This plan inherits the explicit rejection of raw network-mounted SQLite files and the `libsql`-first connector recommendation, but remains deferred until the MySQL workstream completes. | docs review against `ARCHITECTURE.md`, `docs/plans/README.md`, `docs/plans/external-sql-storage-backends-plan.md`, and the archived SQLite/Postgres implementation plans; `git diff --check` | keep deferred until the active MySQL implementation pass is complete, then activate `RS0` |
| 2026-04-10 | RS0 | in_progress | Closed the MySQL workstream, archived its control plane, and activated the replica-connected SQLite follow-on. The immediate `RS0` task is to validate the exact first `libsql` family target and activation boundary against the settled `TenantPersistence` / `PersistenceProvider` seam, the archived embedded-SQLite migration decisions, and current official connector capabilities before landing any new provider code. | docs review against `ARCHITECTURE.md`, `docs/plans/README.md`, `docs/plans/external-sql-storage-backends-plan.md`, `docs/plans/archive/pluggable-storage-backend-plan.md`, and `docs/plans/archive/mysql-storage-provider-plan.md`; `make ci` | complete the official provider-family review and write the exact `RS0` decision back into this plan before implementation continues |
| 2026-04-10 | RS0 | done | Completed the first-provider-family review against the current official SQLite and `libsql` sources and fixed the concrete activation boundary. The first slice will be a `libsql` remote-primary plus embedded-replica provider family, not a generic remote SQLite abstraction and not an offline-write `SyncedDatabase` surface. Strong reads and mutation-adjacent work stay on the primary or a primary fallback path, while replica-served query reads must prove sequence progress locally or force an explicit catch-up. | docs review against `ARCHITECTURE.md`, `docs/plans/external-sql-storage-backends-plan.md`, `crates/neovex-engine/src/persistence.rs`, and `crates/neovex-engine/src/persistence_config.rs`; primary-source review of SQLite network/WAL guidance plus `libsql` builder/database docs; `make ci` | start `RS1` by adding typed config for the chosen remote-primary plus embedded-replica family |
| 2026-04-10 | RS1 | in_progress | Started the typed-config slice for replica-connected SQLite. The implementation will keep `dialect = Sqlite` plus `topology = ExternalPrimaryWithReplicas`, add the remote primary resource/auth and namespace routing inputs required by the chosen `libsql` family, and make the provider-owned replica cache root explicit instead of smuggling it through local embedded-SQLite path semantics. | docs review against `ARCHITECTURE.md`, `docs/plans/external-sql-storage-backends-plan.md`, `crates/neovex-bin/src/main.rs`, `crates/neovex-engine/src/persistence_config.rs`, and `crates/neovex-engine/src/service/mod.rs` | land the typed config constructors, CLI/env/config lowering, and focused config tests |
| 2026-04-10 | RS1 | done | Landed the typed replica-SQLite config surface end to end. `TenantRoutingConfig` now has an explicit namespace-per-tenant mode with a provider-owned replica cache root, `ProviderCredentials` now models the remote primary URL plus optional auth token for this family, the CLI/env/config lowering now accepts `sqlite-replica` with canonical `NEOVEX_SQLITE_*` inputs, `docs/reference/cli.md` documents the new flags, and `Service::new_with_persistence_config(...)` now fails clearly with a dedicated “not implemented yet” message until the storage-side provider foundation lands. | `cargo fmt --all`; `cargo test -p neovex-bin -- --nocapture`; `cargo test -p neovex-engine embedded_providers -- --nocapture`; `cargo fmt --all --check`; `cargo check --workspace` | start `RS2` by implementing the storage-side `libsql` provider foundation and tenant lifecycle below the engine seam |
| 2026-04-10 | RS2 | in_progress | Started the storage-side provider slice for replica-connected SQLite. The implementation is now narrowing to the concrete `libsql` provider config, provider metadata/tenant namespace lifecycle, replica cache ownership, and the first sync/catch-up seams by studying the existing Postgres/MySQL provider patterns and the local SQLite foundation before engine integration expands. | docs review against `crates/neovex-storage/src/postgres.rs`, `crates/neovex-storage/src/mysql.rs`, `crates/neovex-storage/src/sqlite.rs`, `crates/neovex-storage/Cargo.toml`, and `crates/neovex-engine/src/persistence.rs` | land the `libsql` provider foundation, focused storage tests, and the first engine construction path |
| 2026-04-10 | RS2 | in_progress | Landed the first `LibsqlReplicaProvider` foundation, focused test harness, and local HTTP connector path, then tightened the container-backed verification until it failed honestly against a live `sqld` server. The decisive finding is that provider-owned metadata and tenant namespaces cannot be bootstrapped from the data-plane SQL URL alone: local `sqld` requires `POST /v1/namespaces/{name}/create` on a separate admin API, so the concrete replica-SQLite config and provider seam must grow explicit management-endpoint inputs before `RS2` can complete. | `cargo check -p neovex-storage`; repeated `cargo test -p neovex-storage libsql_provider -- --nocapture`; manual live probe with `NEOVEX_SQLITE_URL=http://127.0.0.1:18080 cargo test -p neovex-storage libsql_provider_manages_tenant_registry_and_namespaces -- --nocapture`; direct admin API verification with `curl -i -sS -X POST http://127.0.0.1:18081/v1/namespaces/neovex_probe/create -H 'content-type: application/json' --data '{}'` | extend replica-SQLite typed config and provider construction with explicit admin/control-plane inputs, then wire real namespace create/delete into the provider and rerun the focused storage lane without skips |
| 2026-04-10 | RS2 | in_progress | Extended the replica-SQLite config and provider seam with explicit admin/control-plane inputs, wired real namespace create/delete against the live `sqld` admin API, and reran the focused storage lane without skip-based success. A stronger blocker surfaced immediately afterward: plain `libsql` remote connectivity succeeds against the same local `sqld` server, but the embedded-replica builder still exits abnormally during `new_remote_replica(...).build()` in both the Neovex harness and an isolated control probe, so the provider can manage namespaces and tenant registrations but cannot yet open a tenant through an in-process embedded-replica path. | `cargo test -p neovex-bin -- --nocapture`; `cargo test -p neovex-engine embedded_providers -- --nocapture`; `cargo check -p neovex-storage`; `cargo fmt --all --check`; `cargo check --workspace`; live control probe with `curl -X POST http://127.0.0.1:18081/v1/namespaces/<ns>/create ...`; live control probe with `PROBE_MODE=remote cargo run --quiet` (success) and `PROBE_MODE=replica cargo run --quiet` (abnormal exit) under `/tmp/libsql-replica-probe` | redesign `RS2` around an explicit provider-owned replica sync boundary before reopening opened-tenant activation or engine read integration |
| 2026-04-10 | RS2 | done | Completed the storage-side redesign around a provider-owned snapshot boundary. `LibsqlReplicaProvider` now refreshes a consistent remote namespace snapshot into provider-owned local SQLite cache files, reopens those files through `SqliteTenantStore` / `SqliteTenantStorage`, and serves indexed reads from the derivative cache while keeping admin-driven namespace lifecycle on the remote `sqld` control plane. This replaces the earlier “opened tenant must fail clearly” placeholder without reviving the unstable `new_remote_replica(...)` runtime path. | `cargo check -p neovex-storage`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace`; `NEOVEX_SQLITE_URL=http://127.0.0.1:18080 NEOVEX_SQLITE_ADMIN_URL=http://127.0.0.1:18081 cargo test -p neovex-storage libsql_provider -- --nocapture` | start `RS3` by wiring the engine seam onto the proven opened-tenant read path and designing the first replica-sequence barrier / strong-read-fallback contract |
| 2026-04-10 | RS3 | in_progress | Started the engine-integration slice. The immediate goal is to replace the typed-config “not implemented yet” branch with a real `LibsqlReplicaProvider` composition-root path, map opened replica tenants onto `TenantPersistence` / `TenantPersistenceExecutor`, and prove the service can create or reopen replica-backed tenants through the same engine-owned runtime path before sequence-barrier logic broadens. | docs and code review against `crates/neovex-engine/src/service/mod.rs`, `crates/neovex-engine/src/persistence.rs`, `crates/neovex-engine/src/tests/embedded_providers.rs`, and `crates/neovex-storage/src/libsql.rs` | land the provider variant and service wiring, then replace the old failing test with a service-level replica-backed tenant proof |
| 2026-04-10 | RS3 | done | Landed the engine read foundation for replica-connected SQLite. `Service::new_with_persistence_config(...)` now constructs a real `LibsqlReplicaProvider`, replica-backed tenants lazy-load through dedicated `TenantPersistence` / `TenantPersistenceExecutor` variants that reuse the local SQLite read seam, and the new engine harness proves provider-backed query reads work while local write attempts fail clearly instead of mutating the cache. The write-refusal path was also tightened so intentional replica-write rejection remains an `InvalidInput` through the mutation-journal boundary rather than being mislabeled as an internal storage fault. | `cargo check -p neovex-engine`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace`; `NEOVEX_SQLITE_URL=http://127.0.0.1:18080 NEOVEX_SQLITE_ADMIN_URL=http://127.0.0.1:18081 cargo test -p neovex-engine sqlite_replica_provider -- --nocapture` | start `RS4` by routing mutations, scheduler writes, and durable journal updates to the remote primary and defining the first barrier/fallback semantics around that write route |
| 2026-04-10 | RS4 | in_progress | Started the primary-write slice. The live codebase now has a read-only replica-backed engine seam, and the next implementation cut is to add the real remote-primary mutation path, durable/applied barrier targets, strong-read fallback, and safe opened-cache refresh/reload behavior without backsliding into local-cache writes or in-process embedded-replica assumptions. | design review against `crates/neovex-engine/src/service/mutations/*`, `crates/neovex-engine/src/service/scheduler/*`, `crates/neovex-engine/src/service/provider_hints.rs`, `crates/neovex-engine/src/persistence.rs`, and `crates/neovex-storage/src/libsql.rs` | implement the first remote-primary write route and make the read-side barrier contract depend on explicit primary progress instead of implicit local cache freshness |
| 2026-04-10 | RS4 | done | Completed the first writable replica-connected SQLite slice. `LibsqlReplicaTenantStore` now routes document, scheduler, and durable-journal mutations to the remote primary, `recover_durable_journal()` replays pending durable records onto the primary before refreshing the derivative local cache, the engine starts a dedicated replica poll worker for loaded and unloaded tenant catch-up, and the async mutation-journal path no longer blocks applied-head progress on synchronous cache refresh. The focused storage and engine harnesses now prove real write, recovery, reopen, schema refresh, and scheduled-work wake behavior against a live local `sqld` server rather than skip-based container fallbacks. | `cargo check -p neovex-storage`; `cargo check -p neovex-engine`; `NEOVEX_SQLITE_URL=http://127.0.0.1:18080 NEOVEX_SQLITE_ADMIN_URL=http://127.0.0.1:18081 cargo test -p neovex-storage libsql_provider -- --nocapture`; `NEOVEX_SQLITE_URL=http://127.0.0.1:18080 NEOVEX_SQLITE_ADMIN_URL=http://127.0.0.1:18081 cargo test -p neovex-engine sqlite_replica_provider -- --nocapture`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace` | start `RS5` by running the replica-specific benchmark and operational gate against the same live `sqld` environment |
| 2026-04-10 | RS5 | in_progress | Started the benchmark and operational-gate slice for replica-connected SQLite. The next step is to reuse or extend the existing provider benchmark harnesses so replica lag, poll-driven catch-up, steady-state reads or writes, and scheduled-work wake behavior are measured against the same live `sqld` environment that verified `RS4`. | plan review against the `RS5` gate plus code review of the existing provider benchmark examples and reports | run the replica benchmark lanes, record the results, and decide whether any fallback or catch-up semantics still need tuning before closure |
| 2026-04-10 | RS5 | done | Completed the replica benchmark and operational gate against the live local `sqld` environment and wrote the full report to `docs/research/sqlite-replica-provider-benchmark-report.md`. The contrast scorecard still favors embedded SQLite overall for local-only performance, but the shipped replica contract now has explicit operational proof: same-service barrier refresh measured 16.21 ms median / 17.83 ms p95, and peer catch-up measured 537.88 ms median / 563.94 ms p95. The benchmark harness remains env/CLI-driven on explicit `NEOVEX_SQLITE_URL` plus `NEOVEX_SQLITE_ADMIN_URL` endpoints so provider provisioning stays outside the measured process. | `cargo fmt --all`; `cargo check -p neovex-engine --example sqlite_replica_provider_benchmarks`; `NEOVEX_SQLITE_URL=http://127.0.0.1:18080 NEOVEX_SQLITE_ADMIN_URL=http://127.0.0.1:18081 make bench-sqlite-replica-provider REPORT=docs/research/sqlite-replica-provider-benchmark-report.md` | start `RS6` by rerunning repo-wide verification and archiving the completed plan cleanly |
| 2026-04-10 | RS6 | done | Completed the closure pass for the replica-connected SQLite provider workstream. `cargo fmt --all --check`, `cargo check --workspace`, `make check`, `make test`, `make clippy`, `make build-js`, and `make test-js` all passed. `make ci` was also run and failed in the preexisting `cargo deny` lane on `RUSTSEC-2026-0049` (`rustls-webpki` through `libsql`) plus the current duplicate/license policy surface, so the plan is archived with that repo-baseline limitation recorded explicitly instead of being misattributed to the replica-SQLite work. | `cargo fmt --all --check`; `cargo check --workspace`; `make check`; `make test`; `make clippy`; `make build-js`; `make test-js`; `make ci` (fails at `cargo deny` on `RUSTSEC-2026-0049` and current license/duplicate policy checks) | archive the completed plan and return future provider-topology work to the umbrella baseline or a newly promoted active plan |
