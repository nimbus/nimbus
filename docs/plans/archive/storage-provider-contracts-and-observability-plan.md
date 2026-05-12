# Storage Provider Contracts And Observability Plan

Archived execution record for the storage follow-up after the hardening pass:
make provider topology semantics explicit, reduce SQLite-versus-replica naming
confusion, add replica freshness observability, and close the remaining
external-provider schema metadata hot-path gap without reopening completed
migration plans as live progress state.

Reviewed against:

- `ARCHITECTURE.md`
- `docs/plans/README.md`
- `docs/plans/archive/storage-layer-hardening-plan.md`
- `docs/plans/archive/sqlite-replica-provider-plan.md`
- `docs/plans/archive/postgres-storage-provider-plan.md`
- `docs/plans/archive/mysql-storage-provider-plan.md`
- `crates/nimbus-engine/src/persistence.rs`
- `crates/nimbus-engine/src/service/mutations/direct/store.rs`
- `crates/nimbus-storage/src/async_storage/traits.rs`
- `crates/nimbus-storage/src/sqlite.rs`
- `crates/nimbus-storage/src/libsql.rs`
- `crates/nimbus-storage/src/postgres.rs`
- `crates/nimbus-storage/src/mysql.rs`
- `crates/nimbus-storage/src/query_read.rs`
- `crates/nimbus-storage/src/tests/libsql_provider.rs`

---

## Status

- Archived
- Owner: storage architecture follow-up
- Last updated: 2026-04-11

## Purpose

Own the next storage follow-up work that is now clear after the completed
storage-layer hardening pass:

- make the difference between embedded SQLite and the `libsql`-backed replica
  provider explicit in code and docs
- document and preserve the real storage consistency contracts instead of
  letting topology names imply the wrong thing
- expose the replica provider's freshness and refresh behavior through explicit
  observability surfaces
- remove the remaining repetitive schema-metadata hot-path cost in the external
  providers

This plan is intentionally not another migration or provider bring-up plan.
The core provider families already exist. The goal here is to make the current
families easier to reason about, easier to operate, and less likely to confuse
future work.

## Relationship To Other Plans

- `docs/plans/archive/storage-layer-hardening-plan.md` is completed historical
  context. This plan owns the remaining semantic-clarity and operability gaps
  that are still visible after that closeout.
- `docs/plans/archive/sqlite-replica-provider-plan.md` is completed historical
  context for the current `libsql` remote-primary plus provider-owned local
  cache design.
- `docs/plans/archive/postgres-storage-provider-plan.md` and
  `docs/plans/archive/mysql-storage-provider-plan.md` remain historical context
  for the external SQL provider families. This plan does not reopen those
  implementation migrations.
- `docs/plans/archive/dependency-baseline-cleanup-plan.md` is completed
  historical context for the `libsql` dependency-shape and deny-baseline
  cleanup. If new dependency work surfaces while executing this plan, record it
  here and promote a new active cleanup plan instead of silently reopening that
  archived record.

## Current Assessed State

- The core storage architecture is sound. The async storage seam is narrow and
  intentionally avoids flattening planner, journal, scheduler, and transaction
  ownership into CRUD-only helpers.
- The engine still routes all providers through the same read and write
  executor seam, and the direct mutation path remains unified through
  `Service::apply_mutation` plus the canonical persistence executor boundary.
- Embedded SQLite is an authoritative local store with local pooled reads and a
  single local durable transaction for document, schema, scheduler, and commit
  log changes.
- The replica-connected SQLite provider is a concrete `libsql` family with a
  remote primary and a provider-owned local SQLite derivative cache. It now
  uses required-sequence tracking, background refresh scheduling,
  durable-journal delta catch-up, full-snapshot fallback when needed, and a
  provider-owned freshness snapshot that records barrier path, refresh cause,
  duration, fallback, and error state.
- Engine-facing enums, service/config seams, the runtime CLI/docs, and the
  active engine/storage developer harnesses now use the concrete
  `LibsqlReplica` family name where they model the `libsql` remote-primary
  plus derivative-cache provider. The remaining `SqliteReplica` terminology is
  limited to archived plan/report filenames and other historical records.
- Postgres and MySQL no longer force ordinary planner reads through full
  `read_snapshot()` rebuilds, and they now keep provider-owned schema metadata
  caches in the tenant stores so targeted query/index paths do not round-trip
  to the `schemas` table on every read.
- Those external-provider schema caches are invalidated at the correct
  ownership boundaries: local schema commits clear the store-local cache, the
  Postgres notification path and listener-reattach authoritative catch-up clear
  it before schema refresh, and the MySQL poll worker clears it before
  provider-owned schema comparison so external schema writes still become
  visible.
- Tenant engine diagnostics now surface `libsql` replica freshness state when a
  tenant is backed by the `LibsqlReplica` provider, so operators can inspect
  required sequence, local durable/applied heads, barrier path, refresh cause,
  duration, fallback path, and the last refresh error through the existing
  debug seam.

## Current Review Findings

- The live code, operator surfaces, and active developer harnesses now use the
  canonical `LibsqlReplica` family name. Remaining `SqliteReplica`
  identifiers are historical record names in archived plans and reports.
- The architecture docs should say explicitly that embedded SQLite is
  authoritative local state, while the replica provider is remote-primary state
  plus a local provider-owned cache that must satisfy a freshness barrier.
- Replica freshness behavior is now explicit in provider-owned stats and
  targeted tracing instead of living only in local reasoning.
- Postgres and MySQL now own explicit schema caches in the tenant stores.
- Post-archive review found one remaining Postgres reconnect correctness gap:
  schema changes could still be missed while the LISTEN worker was down. That
  was closed immediately in
  `docs/plans/archive/postgres-listener-reconnect-schema-recovery-plan.md`,
  which now serves as the final historical record for that follow-up.
- The verification contract closed green on a fresh rebuilt target after the
  environment-only blockers were resolved. This plan is complete and should be
  treated as historical context unless a new storage follow-up plan is
  promoted.
- There is still structural duplication across external providers, but the
  next priority after this plan is a future cleanup/refactor decision, not more
  contract or cache work inside this roadmap.

## Cleanup Invariants

- `Service::apply_mutation` remains the only mutation path.
- Storage atomicity remains intact for every provider.
- `TenantPersistence` remains the stable engine-facing seam.
- Embedded SQLite and replica-connected SQLite must not be described as if they
  offered the same authority model.
- The replica provider's local SQLite files remain provider-owned derivative
  state, not a second authority or a shared writable seam.
- Planner-read cleanup must not broaden the storage abstraction into generic
  CRUD helpers.
- If schema metadata caching lands, invalidation must follow authoritative
  writes and provider-owned change signals rather than best-effort guessing.
- Pre-launch rules still apply: prefer clean replacements over compatibility
  shims if a rename or API cleanup is needed.

## Feature Preservation Matrix

| Area | Must stay true during this plan | Notes |
| --- | --- | --- |
| Unified mutation semantics | all writes still cross the same durable seam | naming or telemetry work must not fork write behavior |
| Replica freshness correctness | required-sequence barriers still gate replica reads | telemetry must describe the barrier, not weaken it |
| Embedded SQLite behavior | local read-after-write and WAL-backed local authority stay unchanged | do not let docs suggest new remote semantics for embedded mode |
| External provider planner reads | targeted provider queries remain the hot path | schema cache follow-up should tighten metadata cost, not revive snapshot-shaped hot reads |
| Config and operator semantics | any rename should make provider identity clearer | pre-launch breaking cleanup is allowed, but only when the replacement is cleaner |

## Control Plane Rules

- Work one roadmap item at a time.
- Land terminology and architecture-contract cleanup before adding deeper
  provider caching changes.
- Update this plan, `docs/plans/README.md`, and any touched storage
  architecture docs in the same change set when canonical naming changes.
- If observability work reveals a deeper control-plane or topology redesign,
  record it here and promote a new dedicated plan instead of stretching this
  one indefinitely.
- Record exact verification and benchmark evidence in the execution log rather
  than relying on chat history.

## Canonical Design Decisions

### Provider family names should be explicit where topology alone is not enough

- `dialect = Sqlite` and `topology = ExternalPrimaryWithReplicas` remain the
  architectural model.
- The concrete first family is still `libsql` remote-primary plus
  provider-owned local SQLite cache.
- Where code, docs, or operator surfaces need to distinguish that concrete
  family, prefer an explicit `LibsqlReplica` family name over the ambiguous
  `SqliteReplica` label.

### Authority and cache roles must be named explicitly

- Embedded SQLite means the local tenant file is authoritative state.
- The replica provider means remote `libsql` primary state is authoritative and
  the local SQLite file is derivative query-serving cache.
- Docs, naming, and metrics should make that distinction obvious to a new
  contributor without requiring a full source dive.

### Replica freshness is barrier-driven, not best-effort cache sync

- The correctness contract remains provider-owned required-sequence and
  applied-sequence coordination.
- Background refresh, durable-journal delta catch-up, and full snapshot rebuild
  are implementation tools behind that barrier, not alternative semantics.
- Observability should expose which tool was used and how long it took.

### External-provider schema metadata should be provider-owned hot-path state

- Postgres and MySQL should own an explicit schema metadata cache instead of
  treating schema lookup as unstructured repeated storage I/O.
- Postgres should use provider-owned invalidation tied to write completion and,
  where practical, its existing notification channel.
- MySQL should use provider-owned invalidation on local writes plus an explicit
  follow-up strategy for reconnect or multi-writer visibility if needed.

## Success Criteria

This plan is complete only when all of the following are true:

1. Storage docs and engine/storage naming make the embedded SQLite versus
   `libsql` replica distinction clear without relying on tribal knowledge.
2. The replica provider emits explicit freshness and refresh observability data
   that can answer "why was this read stale/blocked/refreshing?" without
   source inspection.
3. Postgres and MySQL no longer pay avoidable repeated schema-metadata lookup
   cost on ordinary planner paths.
4. The plan index points future storage follow-up work at this plan while it is
   active, and the execution log records the actual verification evidence.

## Verification Contract

- Always run `cargo fmt --all --check` before closing code changes.
- Always run `cargo check -p nimbus-storage -p nimbus-engine` for touched
  storage work.
- For replica terminology or observability work, run focused provider and
  engine tests that exercise replica reads, barrier refresh, and recovery:
  - `cargo test -p nimbus-storage libsql_provider -- --nocapture`
  - `cargo test -p nimbus-engine libsql_replica_provider -- --nocapture`
- For Postgres/MySQL schema-cache work, run focused provider and engine tests
  that exercise point reads, index reads, and schema updates for those
  providers.
- Before closing the plan, run:
  - `make check`
  - `make test`
  - `make clippy`

## Known Risks

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Renaming `SqliteReplica` surfaces could create broad churn across config, docs, and tests | medium | make the canonical target explicit first, then land one cohesive rename pass |
| Replica observability could accidentally add lock contention or noisy tracing on hot paths | medium | prefer cheap gauges/counters and targeted spans tied to refresh boundaries |
| Schema metadata caching could become stale and mislead planner logic | high | tie invalidation to authoritative writes and provider-owned refresh points, then cover it with focused tests |
| The plan could drift into a generic provider-abstraction rewrite | high | keep the scope on clarity, observability, and explicit provider-owned metadata hot paths |

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies | Gate Note |
| --- | --- | --- | --- | --- |
| SP1 | `done` | aligned active provider naming and storage-consistency docs so embedded SQLite and `libsql` replica semantics are explicit across live code, operator inputs, and developer harnesses | none | complete |
| SP2 | `done` | added provider-owned `libsql` replica freshness stats plus targeted refresh tracing and surfaced the snapshot through tenant diagnostics | SP1 | complete |
| SP3 | `done` | added provider-owned Postgres/MySQL schema metadata caches with invalidation on local schema commits plus the relevant notification, reconnect, and poll boundaries | SP1 | complete |
| SP4 | `done` | reran workspace verification from a fresh rebuilt target, recorded the final green baseline, and archived the plan/index references cleanly | SP2, SP3 | complete |

## Dependency Graph

- `SP1` starts the plan.
- `SP2` depends on `SP1`.
- `SP3` depends on `SP1`.
- `SP4` depends on `SP2` and `SP3`.

## Recommended Delivery Order

1. `SP1` to lock the canonical naming and storage-consistency contract before
   code churn starts.
2. `SP2` to make the replica provider operable and debuggable under that
   clarified contract.
3. `SP3` to tighten the remaining external-provider metadata hot-path cost.
4. `SP4` to rerun verification and either archive the plan or leave an honest
   handoff state.

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| SP1 | Complete. `AGENTS.md`, `ARCHITECTURE.md`, the engine persistence enums, service/config seams, runtime CLI/docs, active engine/storage test lanes, and the active replica benchmark harness now all use the concrete `LibsqlReplica` family name where they model the `libsql` remote-primary plus derivative-cache provider. Remaining `SqliteReplica` names are historical archived artifact identifiers only. | archived |
| SP2 | Complete. `LibsqlReplicaTenantStore` now owns a freshness snapshot with required sequence, local durable/applied heads, barrier-path counters, refresh cause/path/duration, and last-error state; refresh boundaries emit targeted tracing, and tenant diagnostics surface the snapshot for operator inspection. | archived |
| SP3 | Complete. `PostgresTenantStore` and `MySqlTenantStore` now own best-effort schema caches for hot query/index reads, local schema commits invalidate those caches on commit, Postgres invalidates on both the provider notification path and listener-reattach authoritative catch-up, and MySQL invalidates on the provider poll boundary before schema comparison. | archived |
| SP4 | Complete. The workspace verification contract closed green from a fresh target rebuild: `make test`, `make clippy`, and `make check` all passed, and the plan plus repo indexes now record this workstream as archived historical context. | archived |

## Work Items

### SP1. Provider Naming And Consistency Contract Cleanup

- Audit engine, storage, config, and docs surfaces that still use
  `SqliteReplica` for the concrete `libsql` family.
- Decide and record the canonical naming rule for architecture-facing and
  operator-facing surfaces.
- Update the active docs so they explicitly describe:
  - authoritative local embedded SQLite
  - authoritative remote `libsql` primary plus provider-owned local cache
  - the role of required-sequence and applied-sequence barriers

### SP2. Replica Freshness And Refresh Observability

- Add low-cost metrics and tracing around replica refresh and catch-up:
  required sequence, local applied sequence, refresh cause, refresh duration,
  fallback path, and refresh error.
- Make it possible to tell whether a read used an already-current cache,
  incremental durable-journal catch-up, or full snapshot rebuild.
- Preserve the current barrier semantics while surfacing them explicitly.

### SP3. External-Provider Schema Metadata Cache

- Add provider-owned schema metadata caching for Postgres and MySQL.
- Invalidate or refresh cache entries on authoritative schema writes and the
  relevant provider-owned reconnect or notification boundaries.
- Keep ordinary planner reads on targeted provider queries rather than
  snapshot-shaped fallback logic.

### SP4. Closeout

- Run focused storage and engine verification for every touched provider path.
- Run workspace verification required by the verification contract.
- Update `docs/plans/README.md`, this plan's ledger/checkpoints, and the
  execution log to reflect the final state.
- Archive this plan only if the listed success criteria are actually met.

## Execution Log

| Date | Item | Outcome | Summary | Verification | Next Step |
| --- | --- | --- | --- | --- | --- |
| 2026-04-11 | SP1 | `in_progress` | Promoted a new active control plan after a fresh storage review confirmed that the remaining follow-up is no longer "provider implementation" work but provider-contract clarity, replica observability, and the remaining external schema-metadata hot-path gap. | storage code review across `async_storage`, `sqlite`, `libsql`, `postgres`, `mysql`, and engine persistence wiring; `cargo check -p nimbus-storage` | land the canonical naming and consistency-contract cleanup first |
| 2026-04-11 | SP1 | `in_progress` | Updated `AGENTS.md` to point storage follow-up work at this active plan and tightened `ARCHITECTURE.md` so the concrete first replica-connected SQLite family is named `LibsqlReplicaProvider` with explicit remote-authority versus local-cache semantics. This starts the docs-first slice of the naming cleanup without yet renaming the broader engine enum and config surfaces. | doc review across `AGENTS.md` and `ARCHITECTURE.md`; targeted naming audit via `rg "SqliteReplica|LibsqlReplica"` | continue the code-surface rename audit and decide the next cohesive `SqliteReplica` -> `LibsqlReplica` slice |
| 2026-04-11 | SP1 | `in_progress` | Renamed the concrete config and provider-hint seams from `SqliteReplica` to `LibsqlReplica` where they directly model the `libsql` family: `ServicePersistenceConfig`, `TenantProviderConfig`, `ProviderCredentials`, the service construction helper, and the provider-hint worker naming now align with the documented architecture. The broader engine persistence enums and CLI provider naming still use `SqliteReplica`, so the next slice should finish that rename coherently. | `cargo check -p nimbus-engine -p nimbus-bin` | continue the broader engine enum and operator-surface rename |
| 2026-04-11 | SP1 | `in_progress` | Finished the broader runtime naming pass. The engine-facing `PersistenceProvider` / `TenantPersistence` family now uses `LibsqlReplica`, the binary exposes `--tenant-provider=libsql-replica` plus `NIMBUS_LIBSQL_*` and `persistence.libsql_*` operator inputs, and the CLI reference now describes the live provider as a remote `libsql` primary with a provider-owned local SQLite derivative cache. Historical benchmark/test lane names and archived file paths still use `SqliteReplica`, so the remaining SP1 choice is whether to rename those developer-facing handles or treat them as stable historical identifiers. | `bash scripts/cargo-isolated.sh -- check -p nimbus-engine -p nimbus-bin`; `cargo check -p nimbus-bin`; `cargo test -p nimbus-bin -- --nocapture` | decide whether to close SP1 at the new naming boundary or do one final developer-harness rename pass before starting SP2 |
| 2026-04-11 | SP1 | `done` | Finished the active developer-harness rename pass. The focused engine replica test module now compiles and runs under `libsql_replica_provider`, the replica benchmark target and Make entrypoint now use `libsql-replica-provider-benchmarks` / `bench-libsql-replica-provider`, and the live engine/storage harness env inputs now use `NIMBUS_LIBSQL_*`. That closes SP1 for active code and leaves only archived plan/report filenames as historical `SqliteReplica` records. | `cargo check -p nimbus-engine --tests --bench libsql-replica-provider-benchmarks`; `cargo test -p nimbus-engine libsql_replica_provider -- --nocapture`; `cargo test -p nimbus-storage libsql_provider -- --nocapture`; `cargo fmt --all --check` | start SP2 by choosing the replica freshness telemetry surface and wiring it through refresh/catch-up boundaries |
| 2026-04-11 | SP2 | `done` | Added provider-owned `libsql` replica freshness stats and targeted refresh tracing. `LibsqlReplicaTenantStore` now records barrier-path counters plus refresh cause, path, duration, and last-error state, and tenant engine diagnostics surface that snapshot through the existing debug seam for `LibsqlReplica` tenants. | `cargo fmt --all --check`; `cargo check -p nimbus-storage -p nimbus-engine`; `cargo test -p nimbus-storage libsql_provider -- --nocapture`; `cargo test -p nimbus-engine libsql_replica_provider -- --nocapture` | start SP3 by auditing the exact Postgres/MySQL schema callers and invalidation seams |
| 2026-04-11 | SP3 | `done` | Added provider-owned Postgres/MySQL schema metadata caches for hot `load_schema()` / `load_table_schema()` paths and invalidated them at the provider-owned change boundaries: local schema commits, Postgres schema notifications plus listener-reattach authoritative catch-up, and MySQL poll-based schema comparison. That preserves external schema visibility without paying a `schemas` round-trip on every targeted query/index read. | `cargo fmt --all --check`; `cargo check -p nimbus-storage -p nimbus-engine`; `cargo test -p nimbus-storage postgres_provider -- --nocapture`; `cargo test -p nimbus-storage mysql_provider -- --nocapture`; `cargo test -p nimbus-engine postgres_provider -- --nocapture`; `cargo test -p nimbus-engine mysql_provider -- --nocapture` | finish SP4 with workspace verification and final plan/archive updates |
| 2026-04-11 | SP4 | `done` | Archival review later found that Postgres listener reconnect still missed schema changes during LISTEN downtime for already-loaded tenants. That gap was closed immediately under `docs/plans/archive/postgres-listener-reconnect-schema-recovery-plan.md`, which added authoritative schema-plus-journal catch-up on reattach and the missing reconnect regression. This archived record now reflects the corrected final state. | `cargo fmt --all --check`; `cargo check -p nimbus-engine`; `cargo test -p nimbus-engine postgres_provider -- --nocapture` | archived |
| 2026-04-11 | SP4 | `done` | Closed the plan from a clean environment after resolving Docker Desktop disk exhaustion, shared-target churn from a concurrent `cargo clean`, and the stale single-flight lock left by the interrupted workspace test. With a fresh `target/`, the full verification contract passed and the plan was archived so future storage follow-up work starts from a new active control plane instead of resuming this completed record. | `make test`; `make clippy`; `make check` | archived |
