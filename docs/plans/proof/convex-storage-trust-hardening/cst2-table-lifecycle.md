# CST2 Table Lifecycle

Date: 2026-05-27

## Status

status: done

CST2 is implemented. Nimbus now has an explicit table lifecycle model for
storage-owned table replacement and deletion flows without exposing mutable
catalog helpers as application API.

## Landed

- `nimbus_core::TableState` exists with canonical states:
  - `active`
  - `hidden`
  - `deleting`
- `TableIdentitySnapshotEntry` now carries `state`, defaulting to `active` for
  constructed snapshot entries.
- redb table-catalog values now persist structured `{ table_id, state }`
  values instead of a bare table-id string.
- SQLite, Postgres, MySQL, and libSQL table-catalog schemas now include
  `state TEXT/VARCHAR NOT NULL DEFAULT 'active'`.
- Active table resolution rejects non-active states with explicit conflict
  errors, so deleting tables cannot receive normal writes.
- Storage backends expose the same lifecycle transitions:
  - `stage_hidden_table_identity(table, table_id)`
  - `activate_hidden_table_identity(table, table_id)`
  - `mark_table_deleting(table)`
  - `hard_delete_table_identity(table_id)`
- Hidden import/replacement identities live under `hidden:<table_id>`.
- Retired identities live under `deleting:<table_id>`, so a same-name recreate
  receives a new active `TableId` instead of inheriting old physical rows.
- Hard delete removes retired table-owned document/index storage before
  removing the catalog identity on SQL backends with foreign-key constraints.
- Materialized journal snapshot fingerprints include lifecycle state.
- `docs/architecture/storage/table-identity.md` documents active, hidden, and
  deleting semantics.

## Backend Matrix

| Backend | Lifecycle state | Hidden activation | Hard delete |
| --- | --- | --- | --- |
| redb | JSON table-catalog values persist `table_id` and `state`. | Moves hidden identity to `default`, moves previous active identity to `deleting:<old_table_id>`. | Removes retired document and secondary-index key ranges; removes schema only when no active same-name identity remains. |
| SQLite | `table_catalog.state` persists lifecycle state. | Same transition as redb in one write transaction. | Deletes retired `documents` rows before catalog removal; drops schema/index metadata only when no active identity remains. |
| Postgres | Tenant `table_catalog.state` persists lifecycle state. | Same transition through the tenant write transaction and advisory lock. | Deletes retired `documents` rows before catalog removal to satisfy the `documents.table_id -> table_catalog.table_id` FK. |
| MySQL | Tenant `table_catalog.state` persists lifecycle state. | Same transition through the tenant write transaction. | Deletes retired `documents` rows before catalog removal to satisfy the SQL identity FK. |
| libSQL | Remote primary and local replica cache carry SQLite-compatible state. | Same transition on the remote primary, then schedules replica refresh. | Deletes retired remote rows/catalog identity and schedules replica refresh. |

## Tests Run

```text
cargo check -p nimbus-core -p nimbus-storage

Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.26s
```

```text
cargo test -p nimbus-core table_state --lib

running 1 test
test tests::table_state_parses_canonical_lifecycle_values ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 85 filtered out
```

```text
cargo test -p nimbus-storage table_id --lib

running 9 tests
test table_identity::tests::catalog_recreate_gets_new_id_after_remove ... ok
test table_identity::tests::catalog_resolves_stable_id_for_active_name ... ok
test table_identity::tests::catalog_records_lifecycle_state ... ok
test table_identity::tests::catalog_rejects_duplicate_id_for_distinct_names ... ok
test materializer::tests::shadow_materializer_keys_documents_by_table_id_and_document_id ... ok
test tests::sqlite_foundation::schema::sqlite_writes_reject_deleting_table_identity ... ok
test tests::sqlite_foundation::schema::sqlite_documents_are_physically_keyed_by_table_id ... ok
test tests::crud_and_journal::native_writes_reject_deleting_table_identity ... ok
test tests::crud_and_journal::native_documents_and_indexes_are_physically_keyed_by_table_id ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 206 filtered out
```

```text
cargo test -p nimbus-storage table_lifecycle --lib

running 2 tests
test tests::sqlite_foundation::schema::sqlite_table_lifecycle_activates_hidden_identity_and_hard_deletes_old_data ... ok
test tests::crud_and_journal::native_table_lifecycle_activates_hidden_identity_and_hard_deletes_old_data ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 215 filtered out
```

```text
cargo check -p nimbus-storage

Finished `dev` profile [unoptimized + debuginfo] target(s) in 9.11s
```

```text
cargo check -p nimbus-engine -p nimbus-server

Finished `dev` profile [unoptimized + debuginfo] target(s) in 18.29s
```

## Notes

- redb and SQLite have focused lifecycle behavior tests because they run
  without external services in the normal storage unit suite.
- Postgres, MySQL, and libSQL carry the same transition code through their
  tenant write transactions. Their heavier fixture-gated conformance pass is
  owned by CST8, where every backend matrix lane is run together.
- Resource path bindings still use table-name document locators today. CST3's
  table-aware document identity work owns that boundary rather than hiding it
  inside table lifecycle cleanup.
