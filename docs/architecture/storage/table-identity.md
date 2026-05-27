# Table Identity

Nimbus public APIs are table-name based. Storage internals use a separate
stable table identity contract so a logical table instance can be audited and
tracked independently from its current public name.

## Contract

- Public adapter protocols accept and emit `TableName`.
- Tenant storage resolves active `(namespace, table_name)` entries to a stable
  `TableId` at the transaction boundary.
- Table identities carry explicit lifecycle state: `active`, `hidden`, or
  `deleting`. Normal reads and writes use only active identities in the
  `default` namespace. Hidden identities live under `hidden:<table_id>` for
  import/staging flows. Retired identities live under `deleting:<table_id>`
  until hard delete removes their table-owned physical storage.
- Hidden activation is an atomic table replacement: the hidden identity moves
  into the `default` namespace as `active`, and any previous active identity for
  the same public name moves to `deleting:<old_table_id>`. A same-name recreate
  therefore receives a new `TableId` and cannot inherit old rows by accident.
- Durable `WriteOp` records carry both the public `TableName` and the internal
  `TableId`, so crash recovery, journal replay, and materialized snapshots do
  not regenerate table identities from names.
- Convex-compatible document IDs are table-aware at the adapter boundary.
  Convex responses encode `_id` as `table_name:document_id`, and Convex
  document operations resolve that value back to
  `ResolvedDocumentId { table, document_id }` before storage dispatch. Other
  adapters keep their protocol-native document key shapes.
- Runtime read dependencies and subscription invalidation carry `TableId` for
  materialized table reads. `TableName` remains only as query/diagnostic
  context. Reads against a table that does not yet have a catalog identity use
  a missing-table sentinel, narrowed by filters when possible, so first writes
  still wake the right subscriptions without making same-name table reuse match
  old dependencies.
- Secondary indexes carry stable `IndexId` plus `IndexState`. Public index
  names remain the adapter/developer contract, but redb key prefixes and SQL
  physical index names use `IndexId`. Backfilling and enabled indexes are
  maintained on writes; only enabled indexes are queryable.
- Materialized journal snapshots include read-only table identity entries and
  restore those identities, including lifecycle state, before replaying
  documents or index rows.
- Schema deletion is not table deletion: schemas are optional metadata, so
  deleting a schema must not remove the table catalog entry or stored
  documents. Explicit table lifecycle hard delete removes a retiring catalog
  entry and its table-owned physical data; it removes schema metadata only when
  no active same-name table identity remains.
- Backends own their physical layout. They must not expose backend-specific
  physical names through adapter APIs.
- `TableCatalog*` helper types are internal storage details. Public visibility
  uses read-only DTOs such as `TableIdentitySnapshotEntry` and
  `TableIdentityDiagnostic`, not mutable catalog constructors.
- `TableIdentityDiagnostic` exposes `table_name`, `table_id`, lifecycle
  `state`, `backend_layout`, and summary posture. Exact document counts are
  reported where the backend can compute them without exposing internal catalog
  mutation APIs; otherwise the summary status must explicitly say unsupported.

## Physical Layout

| Backend | Layout |
| --- | --- |
| redb | Native keyspaces persist `table_catalog`; document keys use `table_id` prefixes, and secondary-index keys use `table_id + index_id` prefixes while schemas remain logical-name metadata. |
| SQLite | Tenant database keeps shared tables plus `table_catalog`; document storage posture is `shared_documents_by_table_id`; SQL index names are derived from `IndexId`. |
| Postgres | Tenant schema keeps shared tables plus `table_catalog`; document storage posture is `shared_documents_by_table_id`; SQL index names are derived from `IndexId`. |
| MySQL | Tenant database keeps shared tables plus `table_catalog`; document storage posture is `shared_documents_by_table_id`; SQL index names are derived from `IndexId`; generated columns are table-id-neutral and `table_id` remains the leading indexed/query column so replacement `TableId`s stay indexable. |
| libSQL | Remote primary and local SQLite cache keep the SQLite-compatible shared layout and replicate `table_catalog` plus schema-carried index identity with tenant data. |

Per-table UUID physical SQL tables are not the default. They remain a measured
backend-specific optimization only if shared `documents(table_id, id)` becomes
a proven bottleneck.
