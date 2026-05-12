# Plan: Storage Layer Hardening

This archived plan records the verified storage follow-up work that remained after the
embedded SQLite migration and the first Postgres, MySQL, and replica-connected
SQLite provider workstreams completed.

It does not reopen any archived migration or provider implementation plan as
live progress state.

Reviewed against:

- `ARCHITECTURE.md`
- `docs/plans/README.md`
- `docs/plans/archive/pluggable-storage-backend-plan.md`
- `docs/plans/archive/postgres-storage-provider-plan.md`
- `docs/plans/archive/mysql-storage-provider-plan.md`
- `docs/plans/archive/sqlite-replica-provider-plan.md`
- `crates/nimbus-storage/src/query_read.rs`
- `crates/nimbus-storage/src/sqlite.rs`
- `crates/nimbus-storage/src/libsql.rs`
- `crates/nimbus-storage/src/postgres.rs`
- `crates/nimbus-storage/src/mysql.rs`
- `crates/nimbus-storage/src/async_storage/traits.rs`
- `crates/nimbus-engine/src/service/provider_hints.rs`

---

## Status

- **Status:** `completed`
- **Archived on:** `2026-04-11`
- **Activation source:** promoted on 2026-04-10 after a fresh storage-layer
  review verified real hot-path, observability, and maintainability gaps that
  should not stay as ad hoc cleanup work
- **Scope:** historical storage-layer hardening record only; do not revive this
  completed plan as a live progress tracker

## Purpose

Resolve the verified storage follow-up issues that are now clear in the live
worktree:

- the `QueryReadStore` delegation wall is unnecessary maintenance overhead
- embedded SQLite read-pool ownership is implicit instead of explicit
- Postgres and MySQL planner reads still materialize full tenant snapshots per
  query
- storage error classification is still string-shaped instead of policy-shaped
- replica-connected SQLite can still stall the first stale reader on a full
  cache refresh

## Relationship To Other Plans

- The archived SQLite migration and provider plans remain historical context.
- This plan must not silently reopen `SB*`, `PG*`, `MY*`, or `RS*` items as if
  they were still in progress.
- If this work reveals a new provider-topology or cross-service redesign that
  exceeds hardening scope, record that here and promote a new dedicated plan
  instead of stretching this one indefinitely.

## Current Assessed State

- `TenantPersistence` and `PersistenceProvider` are now the stable engine and
  construction seams for landed storage modes.
- The core async/sync storage boundary is sound: narrow traits, explicit
  blocking executors, serialized writes, and a correct pre-commit versus
  committed-write cancellation split are already in place.
- `crates/nimbus-storage/src/query_read.rs` is still dominated by repetitive
  forwarding impls for the same planner read surface.
- `SqliteTenantStore` keeps a raw `Vec<Connection>` read pool and opens new
  connections on miss without an explicit store-level max-open invariant.
- `PostgresTenantStore` and `MySqlTenantStore` still route planner read methods
  through `read_snapshot()`, which loads schema, journal progress, documents,
  and scheduled execution ids before evaluating point or index-backed reads.
- `LibsqlReplicaTenantStore` already has a provider-owned derivative-cache
  model with retired-cache reaping, but stale reads still refresh that cache
  synchronously on the caller path.
- Postgres preserves SQLSTATE and detail text in its mapped errors, but the
  storage layer as a whole still lacks typed error classification for retry,
  conflict, and resource-exhaustion policy decisions.

## Current Review Findings

- The `QueryReadStore` boilerplate is real, mechanically repetitive, and easy
  to eliminate without changing semantics.
- The SQLite read-pool issue is confirmed but low severity: engine-owned async
  semaphores usually bound it indirectly, yet the store itself should still own
  an explicit max-open rule or explicit invariant.
- The biggest external-provider hot-path issue is not merely "schema caching":
  Postgres and MySQL currently rebuild full read snapshots for planner queries.
- Postgres and MySQL need provider-owned targeted query execution for planner
  reads rather than more layers of snapshot-shaped indirection.
- Replica-connected SQLite already has a background poll worker, so "no
  background mitigation exists" would be overstated, but the first stale reader
  still pays synchronous refresh cost today.
- Replica-connected SQLite should stop using full namespace refresh on the hot
  read path as its primary freshness mechanism once the stronger provider-owned
  catch-up path lands.
- Structured storage error kinds are still worth doing, but this is a
  cross-cutting API change and should be treated as such, not as a trivial
  string cleanup.

## Hardening Invariants

- `TenantPersistence` remains the stable engine-facing semantic seam.
- `PersistenceProvider` remains separate from `TenantPersistence`.
- `Service::apply_mutation` remains the only mutation path.
- Storage atomicity remains intact for every provider.
- Query-path cleanup must not reintroduce CRUD-shaped abstractions or bypass the
  provider-owned planner seam.
- Postgres and MySQL hot-path fixes should push targeted reads into the
  provider-owned SQL layer, not revive full-snapshot loading as the default read
  contract.
- Replica-connected SQLite local cache files remain provider-owned derivative
  state, not a shared mutable database seam.
- Provider hint workers remain hints only; authoritative recovery continues to
  come from durable progress and journal state.
- No compatibility layers for pre-launch legacy storage behavior.

## Control Plane Rules

1. Treat this plan plus the git worktree as the progress record.
2. Resume any `in_progress` item before starting a later item.
3. Implement exactly one roadmap item at a time unless this plan explicitly
   says otherwise.
4. After every meaningful work burst, update:
   - `Roadmap Status Ledger`
   - `Implementation Checkpoints`
   - `Execution Log`
5. If verification is blocked by the environment, record the blocker honestly in
   `Execution Log` instead of silently skipping it.

## Canonical Design Decisions

### External-provider planner reads

- Planner-serving point reads and index-backed reads for external providers
  should use provider-owned targeted queries.
- Full materialized snapshots remain valid for bootstrap, export, verification,
  or other flows that truly need whole-tenant state, but they are not the
  canonical hot path for ordinary planner reads.
- Schema metadata needed for planner reads should be owned explicitly by the
  provider hot path rather than rederived by rebuilding the entire tenant read
  snapshot.

### Snapshot boundaries

- `read_snapshot()` remains a semantic boundary for operations that genuinely
  need a materialized read image.
- `read_snapshot()` should not remain an implicit dependency of ordinary
  Postgres or MySQL planner queries once `SH4` and `SH5` land.

### Replica refresh ownership

- Replica-connected SQLite freshness remains provider-owned.
- Background polling and explicit barrier checks remain valid coordination
  tools, but the first stale reader should not keep paying a synchronous full
  namespace refresh as the primary steady-state mechanism once `SH7` is done.

## Success Criteria

- every verified issue in `Current Review Findings` is either fixed in code or
  closed with an explicit documented rationale in this plan
- Postgres and MySQL planner reads no longer pay a full-tenant snapshot rebuild
  for ordinary point and index-backed query paths
- replica-connected SQLite no longer relies on synchronous full refresh on the
  first stale reader as its primary freshness path
- the storage layer exposes explicit error categories that higher layers can use
  without string parsing
- query-read delegation and embedded SQLite pool ownership are materially
  cleaner than the current baseline
- verification and any targeted benchmark deltas are recorded before closure

## Verification Contract

- always run `cargo fmt --all --check`
- always run `cargo check --workspace`
- run focused crate tests for touched storage or engine paths
- for Postgres and MySQL hot-path work, run focused provider and engine tests
  that exercise point reads, index reads, and query-serving behavior
- for replica-connected SQLite refresh work, run focused provider and engine
  tests plus a targeted freshness or refresh-cost benchmark lane
- before closing the plan, run:
  - `make check`
  - `make test`
  - `make clippy`
  - `make ci` if practical

## Known Risks

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Query hot-path cleanup could accidentally broaden the storage seam into CRUD helpers | high | keep planner-driven read surfaces explicit and provider-owned |
| Postgres/MySQL fixes could optimize the wrong thing and leave full-snapshot reads alive | high | verify final point and index read methods no longer route through `read_snapshot()` |
| Replica refresh cleanup could weaken durable or applied barrier semantics | high | preserve provider-owned durable-progress checks while moving refresh ownership off the hot read path |
| Structured error kinds could sprawl into a wide error-taxonomy rewrite | medium | limit the first slice to storage-relevant categories with provider-specific detail preserved |
| SQLite pool guardrails could introduce deadlock or starvation if modeled poorly | medium | keep the first change explicit, local, and covered by focused tests |

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies | Gate Note |
| --- | --- | --- | --- | --- |
| SH1 | `done` | replaced the repetitive `QueryReadStore` impl wall with a macro-backed single source of truth | none | preserve the exact trait surface and call semantics |
| SH2 | `done` | made the embedded SQLite read-pool ownership explicit with a store-level max-open invariant, provider alignment, and focused coverage | SH1 | normal engine read concurrency now clamps to the store-owned budget instead of silently exceeding it |
| SH3 | `done` | codified the canonical external-provider planner-read contract before changing hot paths | SH2 | planner reads now have an explicit target shape: targeted provider queries, not implicit full snapshots |
| SH4 | `done` | implemented the Postgres targeted planner-read hot path and removed ordinary `read_snapshot()` use from point, scan, and index-backed planner queries | SH3 | bootstrap/export still use materialized snapshots where they remain semantically correct |
| SH5 | `done` | implemented the MySQL targeted planner-read hot path and removed ordinary `read_snapshot()` use from point, scan, and index-backed planner queries | SH4 | bootstrap/export still use materialized snapshots where they remain semantically correct |
| SH6 | `done` | introduced structured storage error kinds and normalized provider mappings without discarding provider detail | SH5 | higher layers can now branch on storage error kinds without parsing backend strings |
| SH7 | `done` | moved replica-connected SQLite refresh off the synchronous first-reader path with background refresh scheduling, durable-journal delta catch-up, and targeted freshness benchmark deltas recorded against a live `sqld` | SH6 | preserve provider-owned barrier semantics while changing refresh ownership |
| SH8 | `done` | reran repo-wide verification, fixed the closeout regressions and clippy nits it exposed, recorded the remaining `cargo deny` baseline, and archived the completed hardening plan cleanly | SH7 | close only after verification and benchmark notes are recorded |

## Dependency Graph

- `SH1` starts the plan.
- `SH2` depends on `SH1`.
- `SH3` depends on `SH2`.
- `SH4` depends on `SH3`.
- `SH5` depends on `SH4`.
- `SH6` depends on `SH5`.
- `SH7` depends on `SH6`.
- `SH8` depends on `SH7`.

## Recommended Delivery Order

1. `SH1` to remove unnecessary maintenance noise before deeper storage edits.
2. `SH2` to make embedded SQLite resource ownership explicit.
3. `SH3` to lock the canonical external-provider planner-read contract.
4. `SH4` and `SH5` to replace Postgres and MySQL full-snapshot planner reads
   with provider-owned targeted query execution.
5. `SH6` to land explicit storage error policy categories.
6. `SH7` to remove replica-connected SQLite's synchronous first-reader refresh
   bottleneck and measure the resulting behavior.
7. `SH8` to rerun repo-wide verification and archive the plan.

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| SH1 | Complete. `crates/nimbus-storage/src/query_read.rs` now uses a single macro-backed implementation instead of eight hand-written forwarding impls, with the trait surface and call graph unchanged. | move to `SH2` and make the embedded SQLite read-pool budget explicit |
| SH2 | Complete. `SqliteTenantStore` now owns an explicit max-open read-connection budget, provider-created SQLite stores align to that budget, and a focused regression test proves direct callers now get `ResourceExhausted` instead of silently growing the pool. | move to `SH3` and write down the canonical planner-read contract before hot-path provider changes |
| SH3 | Complete. The live control plane now explicitly defines targeted provider queries as the canonical planner-read hot path for external providers, keeps full snapshots for true materialization flows only, and records the replica-refresh ownership rule that will govern `SH7`. | start `SH4` and replace ordinary Postgres planner reads that still route through `read_snapshot()` |
| SH4 | Complete. `PostgresTenantStore` now serves ordinary point reads, plain scans, filtered scans, and index-backed planner reads from provider-owned targeted SQL helpers instead of rebuilding a full tenant snapshot, while bootstrap/export flows keep `read_snapshot()` as their explicit materialization boundary. Focused storage and engine verification passed. | move to `SH5` and mirror the same hot-path contract in `MySqlTenantStore` without reviving snapshot-shaped reads |
| SH5 | Complete. `MySqlTenantStore` now serves ordinary point reads, plain scans, filtered scans, and index-backed planner reads from provider-owned targeted SQL helpers instead of rebuilding a full tenant snapshot, while bootstrap/export flows keep `read_snapshot()` as their explicit materialization boundary. Focused storage and engine verification passed. | move to `SH6` and introduce structured storage error kinds without flattening provider detail |
| SH6 | Complete. `Error::Storage` now carries an explicit `StorageErrorKind` plus message, core helpers expose the kind without string parsing, provider mappers classify real unavailable/busy/io/corruption/transient cases, and the server/runtime adapters preserve the kind across HTTP status mapping and Convex runtime encode/decode. Focused core, server, and storage verification passed. | move to `SH7` and remove replica refresh from the synchronous stale-reader path |
| SH7 | Complete. `LibsqlReplicaTenantStore` now schedules provider-owned background refresh work instead of making the first stale reader perform the refresh inline, prefers durable-journal delta catch-up before falling back to full snapshot materialization, preserves the required-sequence barrier across refreshes, and reschedules schema-driven full refreshes explicitly. The live `sqld` harness was also corrected to follow the current `libsql-server` container entrypoint contract, which turned the earlier skip-shaped false negative into real provider and engine passes. Targeted freshness benchmarks against a live local `sqld` improved same-service barrier refresh to `43.35 us` median / `58.17 us` p95 and peer catch-up to `514.46 ms` median / `515.50 ms` p95. | move to `SH8`, rerun the repo-wide verification contract, and archive the plan cleanly if no further doc changes are needed |
| SH8 | Complete. Repo-wide Rust verification passed after fixing the closeout regressions it surfaced: `make test` exposed one `LibsqlReplicaTenantStore` recovery-boundary bug where raw durable appends incorrectly advanced read freshness, and `make clippy` exposed three small storage/server cleanup nits. Those fixes landed, `make check`, `make test`, `make clippy`, `make build-js`, and `make test-js` all passed, and `make ci` was attempted twice: first it failed in the sandbox because `cargo deny` could not take the advisory DB lock, and then it failed outside the sandbox on the existing `libsql` dependency baseline (`RUSTSEC-2026-0049`, `CDLA-Permissive-2.0` rejects via `webpki-roots`, plus duplicate-crate warnings). The plan is now archived and future work should return to `docs/plans/README.md` for the next active owner. | archived |

## Work Items

### SH1. QueryReadStore De-duplication

- Replace the eight hand-written forwarding impls in
  `crates/nimbus-storage/src/query_read.rs` with a macro-backed implementation.
- Keep the trait itself unchanged.
- Do not change planner semantics or call-site ownership.

### SH2. Embedded SQLite Read-Pool Guardrail

- Make `SqliteTenantStore` own an explicit max-open read connection rule.
- Add focused coverage that proves normal pooled reuse still works.
- Keep the async executor semaphore and the store-level rule aligned instead of
  introducing competing limits.

### SH3. External Provider Planner-Read Contract

- Write down the canonical provider-side rule for planner reads:
  point and index-backed reads should use targeted provider queries, while
  snapshot export or bootstrap paths may still load full materialized state when
  they genuinely need it.
- Update `ARCHITECTURE.md` if the ownership map becomes clearer or changes.

### SH4. Postgres Targeted Planner Reads

- Replace ordinary Postgres planner reads that currently route through
  `read_snapshot()` with provider-owned targeted SQL paths.
- Keep journal/bootstrap behavior separate from planner hot paths.
- Add focused verification for point reads, index reads, and mixed query lanes.

### SH5. MySQL Targeted Planner Reads

- Apply the same hot-path contract to MySQL.
- Avoid reviving the current snapshot-heavy loading path for ordinary planner
  reads.
- Add focused verification for point reads, index reads, and mixed query lanes.

### SH6. Structured Storage Error Kinds

- Introduce explicit storage error categories that higher layers can branch on
  without string parsing.
- Preserve provider-specific detail text where it already exists.
- Limit the first slice to the categories needed by the live storage layer.

### SH7. Replica Refresh Hardening

- Move replica-connected SQLite refresh ownership off the synchronous first-read
  path.
- Reduce stale-refresh cost, preferring delta-oriented catch-up when practical.
- Record targeted freshness and refresh-cost benchmark data before closure.

### SH8. Closeout

- Run repo-wide verification.
- Update `ARCHITECTURE.md`, `docs/README.md`, and `docs/plans/README.md` if the
  ownership or verification story changed.
- Archive this plan once the roadmap is complete.

## Execution Log

| Date | Item | Outcome | Summary | Verification | Next Step |
| --- | --- | --- | --- | --- | --- |
| 2026-04-10 | meta | created | Promoted the verified storage review into a dedicated active hardening control plane instead of leaving the follow-up as ad hoc cleanup. Locked the scope around the confirmed `QueryReadStore`, SQLite pool, Postgres/MySQL planner-read, structured-error, and replica-refresh issues. | docs review; code review | execute `SH1` and replace the `QueryReadStore` delegation wall |
| 2026-04-10 | SH1 | done | Replaced the eight hand-written `QueryReadStore` forwarding impls with a single macro-backed implementation in `crates/nimbus-storage/src/query_read.rs`, preserving the exact trait surface and call graph while materially shrinking the maintenance surface. | `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace` | execute `SH2` and make the embedded SQLite read-pool budget explicit |
| 2026-04-10 | SH2 | done | Added a store-owned max-open read-connection budget to `SqliteTenantStore`, aligned provider-created embedded and replica-backed SQLite stores to that budget, clamped async SQLite read parallelism to the store ceiling, and added a focused regression test proving direct callers now receive a clear `ResourceExhausted` error when they exceed the explicit pool limit. | `cargo fmt --all`; `cargo test -p nimbus-storage sqlite_store_enforces_direct_read_connection_limit -- --nocapture`; `cargo fmt --all --check`; `cargo check --workspace` | execute `SH3` and codify the provider planner-read contract before changing Postgres and MySQL hot paths |
| 2026-04-10 | SH3 | done | Wrote the canonical external-provider planner-read contract directly into this control plane: point and index-backed planner reads move toward targeted provider queries, full snapshots stay reserved for true materialization flows, and replica freshness remains provider-owned rather than caller-owned. | plan write-back | start `SH4` on the Postgres planner-read hot path |
| 2026-04-10 | SH4 | done | Replaced ordinary `PostgresTenantStore` planner reads with targeted provider SQL helpers: point reads now fetch a single document directly, plain and filtered scans load only the table being queried, and index-backed reads build provider-owned candidate queries before applying the existing Nimbus-side predicate and range semantics. The remaining `read_snapshot()` call sites in `postgres.rs` are now bootstrap/export-only. | `cargo fmt --all`; `cargo test -p nimbus-storage postgres_index_reads_round_trip_after_schema_write -- --nocapture`; `cargo test -p nimbus-engine typed_postgres_config_supports_async_schema_mutation_journal_and_scheduler_paths -- --nocapture`; `cargo fmt --all --check`; `cargo check --workspace` | execute `SH5` and mirror the targeted planner-read contract in MySQL |
| 2026-04-10 | SH5 | done | Replaced ordinary `MySqlTenantStore` planner reads with targeted provider SQL helpers: point reads now fetch a single document directly, plain and filtered scans load only the table being queried, and index-backed reads build provider-owned candidate queries over the generated index columns before applying the existing Nimbus-side predicate and range semantics. The remaining `read_snapshot()` call site in `mysql.rs` is now bootstrap/export-only. | `cargo fmt --all`; `cargo test -p nimbus-storage mysql_index_reads_round_trip_after_schema_write -- --nocapture`; `cargo test -p nimbus-engine typed_mysql_config_supports_async_schema_mutation_journal_and_scheduler_paths -- --nocapture`; `cargo fmt --all --check`; `cargo check --workspace` | execute `SH6` and introduce structured storage error kinds with provider detail preserved |
| 2026-04-10 | SH6 | done | Replaced the string-shaped storage error payload with `Error::Storage { kind, message }`, added `StorageErrorKind` helpers in `nimbus-core`, updated redb/SQLite/libsql/Postgres/MySQL mappers to classify retryable, unavailable, IO, corruption, and generic backend failures explicitly, and preserved the kind through HTTP status mapping plus Convex runtime error encode/decode. | `cargo fmt --all`; `cargo test -p nimbus-core storage_error -- --nocapture`; `cargo test -p nimbus-server storage_error -- --nocapture`; `cargo test -p nimbus-storage sqlite_store_enforces_direct_read_connection_limit -- --nocapture`; `cargo fmt --all --check`; `cargo check --workspace` | execute `SH7` and move replica freshness off the synchronous first-reader path |
| 2026-04-10 | SH7 | done | Moved replica-connected SQLite freshness off the synchronous stale-reader path by adding provider-owned background refresh scheduling, durable-journal delta catch-up before full snapshot fallback, and explicit schema-triggered full-refresh rescheduling inside `LibsqlReplicaTenantStore`. The live libsql harness was also fixed to match the current `ghcr.io/tursodatabase/libsql-server:latest` entrypoint contract after verifying that the container wrapper already injects `--http-listen-addr`; removing the duplicate flag turned the earlier 60-second readiness skips into real live `sqld` passes. Targeted freshness benchmarks then recorded the new steady-state delta: same-service barrier refresh `43.35 us` median / `58.17 us` p95, and peer catch-up `514.46 ms` median / `515.50 ms` p95, improving on the archived provider baseline without weakening barrier semantics. | `cargo fmt --all`; `cargo test -p nimbus-storage libsql_direct_writes_refresh_derivative_cache_and_round_trip_journal_progress -- --nocapture`; `cargo test -p nimbus-engine sqlite_replica_background_poll_refreshes_loaded_runtime_schema_and_journal_state -- --nocapture`; `NIMBUS_SQLITE_URL=http://127.0.0.1:18080 NIMBUS_SQLITE_ADMIN_URL=http://127.0.0.1:18081 make bench-sqlite-replica-provider REPORT=/tmp/sh7-barrier-refresh.md WORKLOAD=barrier-refresh`; `NIMBUS_SQLITE_URL=http://127.0.0.1:18080 NIMBUS_SQLITE_ADMIN_URL=http://127.0.0.1:18081 make bench-sqlite-replica-provider REPORT=/tmp/sh7-peer-catch-up.md WORKLOAD=peer-catch-up`; `cargo fmt --all --check`; `cargo check --workspace` | execute `SH8` and close the plan with repo-wide verification plus archive/index updates |
| 2026-04-11 | SH8 | done | Closed the plan with full repo-wide verification and archival cleanup. `make test` exposed and verified a real libsql regression fix: raw durable-journal append now stays durable-only until explicit recovery instead of advancing replica read freshness. `make clippy` exposed four small cleanup nits that were fixed in `mysql.rs`, `postgres.rs`, and the Convex runtime error adapter. Final verification passed for `make check`, `make test`, `make clippy`, `make build-js`, and `make test-js`. `make ci` was attempted in-sandbox and outside the sandbox; the only remaining red is the existing `cargo deny` baseline in the `libsql` dependency tree (`RUSTSEC-2026-0049`, `CDLA-Permissive-2.0` license rejects from `webpki-roots`, and duplicate-crate warnings), which is recorded here instead of hidden. | `make check`; `make test`; `cargo test -p nimbus-storage libsql_durable_journal_recovery_refreshes_local_cache_from_remote_records -- --nocapture`; `cargo test -p nimbus-storage libsql_direct_writes_refresh_derivative_cache_and_round_trip_journal_progress -- --nocapture`; `make clippy`; `make build-js`; `make test-js`; `make ci` | archived; future work should start from `docs/plans/README.md` |
