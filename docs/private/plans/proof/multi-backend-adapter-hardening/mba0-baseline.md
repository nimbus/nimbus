# MBA0 Baseline

Date: 2026-05-27

This proof captures the current Nimbus state before executing MBA1-MBA14.
It is intentionally a baseline, not a completion claim.

## Storage Surface

Current provider selection is typed in `crates/nimbus-engine/src/persistence/provider.rs`.
The active tenant persistence variants are:

- `Redb(Arc<EmbeddedRedbProvider>)`
- `Sqlite(Arc<EmbeddedSqliteProvider>)`
- `LibsqlReplica(Arc<LibsqlReplicaProvider>)`
- `Postgres(Arc<PostgresProvider>)`
- `MySql(Arc<MySqlProvider>)`

The current async storage traits live in
`crates/nimbus-storage/src/async_storage/traits.rs`:

- `EmbeddedPersistenceProvider`
- `TenantReadStorage`
- `TenantWriteStorage`
- `UsageStorage`

Those traits are intentionally narrower than the old concrete store surface,
but they are not yet the MBA2 final segregation. The engine still relies on
provider-specific opened tenant shapes and `PersistenceProvider` enum dispatch.

The retained redb control plane is separate from tenant data. Local encryption
providers wrap local persistence families rather than acting as tenant data
providers themselves, but they are in MBA scope because key-provider decisions
affect backend registration and SQL/file safety posture.

## Provider-Coupled Workers

Provider background work is currently owned by
`crates/nimbus-engine/src/service/provider_hints.rs` and selected through
`ProviderBackgroundTask`:

- Postgres uses a LISTEN/NOTIFY catch-up worker.
- MySQL uses polling.
- libSQL replica uses polling.
- redb and sqlite currently opt out.

MBA4 should move this selection into backend-owned runtime hooks instead of
adding more provider-specific worker branches to the engine.

## Adapter Dispatch

The current adapter module root is `crates/nimbus-server/src/adapters/mod.rs`
with direct module declarations for:

- `cloud_functions`
- `convex`
- `firebase`
- `mongodb`

Routes and capabilities are listed manually in
`crates/nimbus-server/src/system_tenant/inventory.rs`. There is no adapter
self-registration surface equivalent to ExtendDB's inventory registrations.

## SQL Storage Shape

Nimbus SQL backends currently use shared physical tables per tenant namespace:

- SQLite initializes one `documents` table keyed by `(table_name, id)`.
- Postgres initializes one tenant schema with `documents`, `schemas`,
  `resource_path_bindings`, scheduler tables, trigger tables, `commit_log`,
  and `metadata`.
- MySQL initializes equivalent shared tables in a tenant database.
- libSQL replica mirrors the SQLite family and remote namespace model.

This differs from ExtendDB's DynamoDB-table-per-physical-table design and from
Convex's internal name-to-tablet mapping. MBA10 should adopt stable logical
table identity without assuming a per-table SQL physical-table rewrite.

## SQL Safety And Typed Keys

Current SQL backends use a mix of parameterized values and helper-built SQL
identifiers:

- Postgres: `quote_identifier`, `quote_literal`, `qualified_table`, plus
  generated index helper names in `crates/nimbus-storage/src/postgres/`.
- MySQL: `quote_identifier`, `qualified_table`, generated-column helper names
  in `crates/nimbus-storage/src/mysql/`.
- SQLite: fixed table names plus generated index names and `json_extract`
  expressions in `crates/nimbus-storage/src/sqlite/schema.rs`.

Postgres and MySQL already route numeric range predicates through numeric
expressions in some index scan paths. SQLite still orders index expressions
through `json_extract`, which needs MBA11 evidence for numeric ordering.

## Latency Instrumentation

Nimbus has targeted timing in places such as tenant-load profiling and SQLite
open profiling. There is no unified per-adapter, per-request latency budget
schema with parse/auth/dispatch/storage/serialize segments and WARN events.
MBA8 owns the schema and implementation.

## Auth Cache Baseline

There is no repo-wide auth-caching ADR. Cache-related code exists in several
forms, including system update-check cache code, tenant/runtime caches, schema
caches, Convex auth provider configuration, and document caches. MBA6 must
classify which are security-sensitive auth/policy caches versus operational or
data-path caches, then make the code match the ADR.

## Technical Debt Baseline

Nimbus does not yet have `docs/technical-debt.md`. Existing debt signals are
spread across plan archives, code comments, and route-specific docs. MBA1
creates the consolidated tracker.

## Baseline Gaps For MBA Execution

- The verifier exists after MBA0 but is expected to fail until later MBA rows
  land.
- Storage trait segregation must include every current provider family, not
  only embedded redb/sqlite.
- Registration work must preserve Nimbus's typed opened-tenant and
  provider-background semantics; direct `inventory` use is optional and should
  be justified by cross-crate plugin-style registration needs.
- Logical table identity should follow Convex's stable internal table mapping
  lesson, while physical storage stays backend-owned and should not be copied
  mechanically from ExtendDB's per-table SQL layout.
- `AGENTS.md`, not `CLAUDE.md`, is the routing document in this repo.
