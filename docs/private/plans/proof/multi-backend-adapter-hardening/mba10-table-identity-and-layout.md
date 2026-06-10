# MBA10 Table Identity And Physical Layout Decision

Date: 2026-05-27

logical_identity: table_id_catalog
redb: key_prefix_table_id
SQLite: shared_documents_by_table_id
Postgres: shared_documents_by_table_id
MySQL: shared_documents_by_table_id
libSQL: shared_documents_by_table_id

## Prior Storage Shape

Before MBA10, Nimbus treated the user-facing `TableName` as the storage
identity. The name is validated in `crates/nimbus-core/src/types.rs` and allows
only ASCII letters, numbers, `_`, and `-`, with a 128-character limit.

SQL backends did not create one physical SQL table per user table. They used a
shared `documents` table per tenant namespace and bound the logical table name
as a value:

- SQLite created `documents(table_name, id, ...)`.
- Postgres created `documents(table_name, id, ...)` inside the tenant schema.
- MySQL created `documents(table_name, id, ...)` inside the tenant database.
- libSQL followed the SQLite layout and snapshotted rows by `table_name, id`.

The embedded redb family also keys schema/index/document state by the validated
logical table name rather than by a stable table catalog.

Convex does not rely solely on user-facing table names internally. Its
open-source backend has `TableMapping`, which maps `TableName` to internal
`TabletId` and `TableNumber` values, keeps tablet-to-name mappings for all
tablets, and keeps name/number reverse mappings only for active canonical
tables because inactive/deleted tablets can conflict on names. Convex's current
developer document ID format also encodes the table number, while newer public
`ctx.db` APIs explicitly pass `(tableName, id)` so future custom IDs do not
depend on that special encoding.

## Decision

Adopt a stable logical table identity catalog, but do not force UUID-named
physical tables across storage backends.

The enterprise-trust requirement is the stable logical identity: commit-log
entries, journal replay, schema/index metadata, and backup/restore must not
infer table identity from only the user-facing table name. Physical per-table
SQL DDL is not required for that property. Nimbus does not currently expose an
explicit table-drop/recreate lifecycle; schema deletion is optional metadata
cleanup and intentionally preserves the table catalog entry. If an explicit
table-drop API is added later, it must remove the catalog mapping so a
same-name recreate receives a new `TableId`.

Each backend keeps a simple physical layout behind the same logical identity:

- redb persists a native `table_catalog` and uses `table_id` key prefixes for
  document and secondary-index keyspaces. Schemas and resource-path bindings
  keep logical table names because they are metadata/API surfaces, while
  durable `WriteOp` records carry both the public `TableName` and internal
  `TableId`.
- SQLite keeps shared physical tables and keys document rows as
  `documents(table_id, id, ...)`.
- Postgres keeps the current per-tenant schema and shared physical tables,
  with document rows keyed as `documents(table_id, id, ...)`.
- MySQL keeps the current per-tenant database and shared physical tables,
  with document rows keyed as `documents(table_id, id, ...)`.
- libSQL keeps the SQLite-compatible shared layout so remote snapshots and
  local replicas move the table catalog plus `table_id`-keyed rows together.

Per-table UUID physical SQL tables remain a possible future optimization for a
specific backend, but they should require measured evidence: catalog pressure,
query planning, vacuum/retention, or isolation behavior that shared
`documents(table_id, id)` cannot satisfy.

## Implementation Evidence

- `crates/nimbus-core/src/types.rs` defines `TableId`.
- `crates/nimbus-core/src/mutation.rs` stores `table_id` on each durable
  `WriteOp` and bumps the durable mutation record version for the breaking
  wire-format change.
- `crates/nimbus-storage/src/table_identity.rs` keeps mutable catalog helpers
  crate-private/test-only and exposes only the read-only
  `TableIdentitySnapshotEntry` snapshot DTO.
- redb tenant storage defines a `TABLE_CATALOG` key-value table. Native
  document keys and index keys now take `TableId`, and read/write/journal/
  snapshot paths preserve the journal/snapshot `TableId` before touching
  document or index keyspaces.
- SQLite, Postgres, MySQL, and libSQL tenant initialization includes a
  `table_catalog` table with a unique `table_id` and document primary keys on
  `(table_id, id)`.
- SQL write paths resolve or create the table id in the same transaction before
  inserting a document row. SQL replay paths preserve the journal `table_id`,
  and SQL read paths resolve the logical name through the catalog before
  reading by `table_id`.
- `crates/nimbus-storage/src/tests/crud_and_journal.rs` includes a redb
  physical-layout regression test that rejects table-name document/index
  prefixes, verifies logical reads and index scans still work, and asserts
  commit records carry the physical `TableId`.
- `crates/nimbus-storage/src/store/journal_snapshot/tests.rs` verifies
  snapshot rebuild preserves stable table identities.
- `crates/nimbus-storage/src/materializer/mod.rs` verifies the shadow
  materializer keys documents by `(table_id, document_id)`.
- `crates/nimbus-storage/src/tests/sqlite_foundation/schema.rs` includes a
  physical-layout regression test that rejects a `table_name` column in
  `documents` and verifies the stored row references `table_catalog.table_id`.
- `docs/architecture/storage/table-identity.md` is the canonical architecture
  contract.
