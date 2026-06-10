status: done
date: 2026-05-27
phase: CST8

# CST8 Cross-Backend Conformance Proof

## Decision

The storage trust model is now backend-conformant rather than redb-only or
SQLite-only. The common invariant is:

`logical TableName -> stable TableId + TableState -> backend-owned physical layout`

Every backend keeps the developer and adapter contract table-name based while
document rows, durable writes, diagnostics, and index reads are anchored to
stable storage identity.

## Backend Matrix

| Backend | Physical layout | CST8 evidence |
| --- | --- | --- |
| redb | Native keyspaces use `table_id` prefixes for documents and `table_id + index_id` prefixes for secondary indexes. | `native_documents_and_indexes_are_physically_keyed_by_table_id`, `native_table_lifecycle_activates_hidden_identity_and_hard_deletes_old_data`, `redb_table_identity_diagnostics_are_read_only_and_count_documents`, full storage suite. |
| SQLite | Shared SQL tables use `documents(table_id, id)` plus `table_catalog(namespace, table_name, table_id, state)`. | `sqlite_documents_are_physically_keyed_by_table_id`, `sqlite_table_lifecycle_activates_hidden_identity_and_hard_deletes_old_data`, `sqlite_table_identity_diagnostics_report_layout_state_and_counts`, full storage suite. |
| Postgres | Per-tenant schema uses shared `documents(table_id, id)` and repeatable-read snapshots over `table_catalog`. | `postgres_table_lifecycle_activates_hidden_identity_and_diagnostics_track_layout`, `postgres_index_reads_round_trip_after_schema_write`, provider recovery/resource-path/journal tests, full storage suite. |
| MySQL | Per-tenant database uses shared `documents(table_id, id)`, `IndexId`-named indexes, and table-id-neutral generated columns with `table_id` as the leading indexed/query column. | `mysql_table_lifecycle_activates_hidden_identity_and_diagnostics_track_layout`, `mysql_schema_write_creates_and_drops_generated_index_columns`, `mysql_index_reads_round_trip_after_schema_write`, provider recovery/resource-path/journal tests, full storage suite. |
| libSQL | Remote primary plus local SQLite cache replicate `table_catalog`, schema, documents, and index identity into the SQLite-compatible shared layout. | `libsql_table_lifecycle_activates_hidden_identity_and_diagnostics_track_layout`, `libsql_opened_tenant_materializes_local_sqlite_snapshot`, provider recovery/resource-path/journal tests, full storage suite. |

## Additional Finding Closed

The CST8 matrix caught a real MySQL lifecycle/index bug: generated column
expressions were scoped to the old active `TableId`, so activating a hidden
replacement table could make the replacement table's index unreadable.

The fix keeps generated columns table-id-neutral and relies on the explicit
`table_id` query predicate plus leading index column for isolation. That keeps
same-name table replacement clean while preserving the shared
`documents(table_id, id)` layout.

## Verification

- `cargo check -p nimbus-storage --all-targets` passed.
- `cargo test -p nimbus-storage table_lifecycle --lib`: 5 passed, 0 failed.
- `cargo test -p nimbus-storage mysql_schema_write --lib`: 1 passed, 0 failed.
- `cargo test -p nimbus-storage --lib`: 222 passed, 0 failed, 2 ignored.
- `cargo check -p nimbus-core -p nimbus-storage -p nimbus-engine -p nimbus-server --all-targets` passed.
- `npm run typecheck` passed; TanStack route generation emitted existing non-route warnings and exited 0.
