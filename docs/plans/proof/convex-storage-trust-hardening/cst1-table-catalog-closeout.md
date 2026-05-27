# CST1 Table Catalog Closeout

Date: 2026-05-27

## Status

status: done

CST1 closes the already-landed stable table identity baseline. The remaining
CST phases build on this baseline instead of reopening the physical layout
decision.

## Contract

- Public adapter APIs remain `TableName` based.
- Storage resolves `(namespace, table_name)` to stable `TableId` at the
  transaction boundary.
- Durable `WriteOp` records carry both `table` and `table_id`.
- Journal replay, snapshot restore, and shadow materialization preserve the
  table id from durable records and snapshots instead of regenerating it from
  names.
- SQL backends use shared physical document tables keyed by `(table_id, id)`.
- redb uses `TableId` key prefixes for document and secondary-index keyspaces.
- Mutable catalog helper types remain crate-private/test-only; public
  visibility uses read-only snapshot DTOs.

## Backend Evidence

| Backend | Evidence |
| --- | --- |
| redb | `crates/nimbus-storage/src/store/table_catalog.rs` persists `TABLE_CATALOG`; `store/write/{direct,batch}.rs`, `store/journal.rs`, and `store/journal_snapshot.rs` resolve or ensure table ids before touching document storage. `crates/nimbus-storage/src/keys.rs` and index keyspace helpers key by `TableId`. |
| SQLite | `crates/nimbus-storage/src/sqlite.rs` creates `table_catalog` and `documents(table_id, id, ...)`; `sqlite/backend.rs`, `sqlite/write.rs`, `sqlite/read.rs`, and `sqlite/journal.rs` resolve or ensure table ids for direct writes, reads, replay, and restore. |
| Postgres | `crates/nimbus-storage/src/postgres/config.rs` creates per-tenant `table_catalog` and shared documents keyed by `table_id`; `postgres/backend.rs` and `postgres/write.rs` resolve, create, ensure, and replay table ids in backend-owned transactions. |
| MySQL | `crates/nimbus-storage/src/mysql.rs` and `mysql/backend.rs` create per-tenant `table_catalog` plus shared `documents(table_id, id, ...)`; MySQL write/replay paths ensure durable `WriteOp.table_id` before materializing document rows. |
| libSQL | `crates/nimbus-storage/src/libsql.rs`, `libsql/backend.rs`, and `libsql/remote.rs` keep the SQLite-compatible `table_catalog` shape on remote primary and local snapshots, including snapshot refresh into the local cache. |

## Tests Run

```text
cargo test -p nimbus-storage table_id --lib

running 6 tests
test table_identity::tests::catalog_resolves_stable_id_for_active_name ... ok
test table_identity::tests::catalog_recreate_gets_new_id_after_remove ... ok
test table_identity::tests::catalog_rejects_duplicate_id_for_distinct_names ... ok
test materializer::tests::shadow_materializer_keys_documents_by_table_id_and_document_id ... ok
test tests::sqlite_foundation::schema::sqlite_documents_are_physically_keyed_by_table_id ... ok
test tests::crud_and_journal::native_documents_and_indexes_are_physically_keyed_by_table_id ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 206 filtered out
```

```text
cargo test -p nimbus-storage materialized_snapshot_plus_journal_tail_rebuild_matches_live_state --lib

running 2 tests
test tests::sqlite_foundation::snapshot::sqlite_materialized_snapshot_plus_journal_tail_rebuild_matches_live_state ... ok
test store::journal_snapshot::tests::materialized_snapshot_plus_journal_tail_rebuild_matches_live_state ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 210 filtered out
```

```text
env NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 \
  cargo test -p nimbus-storage durable_journal_recovery --lib

running 3 tests
test tests::mysql_provider::mysql_durable_journal_recovery_applies_pending_records ... ok
test tests::postgres_provider::postgres_durable_journal_recovery_applies_pending_records ... ok
test tests::libsql_provider::libsql_durable_journal_recovery_refreshes_local_cache_from_remote_records ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 209 filtered out
```

The external-provider command was run with implicit fixtures disabled, so it
confirms the provider-specific durable recovery tests compile and take the
fixture-aware path in this local environment. Live Postgres/MySQL/libSQL
fixture execution remains part of CST8 cross-backend conformance.

## Debt Closure

- `S-004` is marked `done`; CST2 owns explicit lifecycle state before table
  drop/import/rename.
- `A-003` is marked `done`; this proof records the current catalog baseline.

## Follow-On Boundary

CST1 does not add table-drop, import-staging, rename, or table-bearing public
ID semantics. Those are intentionally owned by CST2 and CST3 so the stable
catalog baseline remains small and auditable.
