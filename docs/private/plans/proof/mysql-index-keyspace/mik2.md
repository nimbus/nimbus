# MIK2 Schema apply and table lifecycle through the keyspace

Date: 2026-09-09. Commit: the `storage: apply MySQL schema changes through
the index_entries keyspace` commit after 359300aaa.

## What changed

- `crates/nimbus-storage/src/mysql/index_entries.rs` now owns every keyspace
  statement: the write effects from MIK1, the schema reconcile
  (`reconcile_index_entries_for_table_schema_in_session`), the table purge
  (`purge_index_entries_for_table_in_session`), the per-index purge, the
  backfill, and the candidate select
  (`load_index_candidate_documents_from_session`).
- Schema apply diffs the maintained indexes of the previous and the current
  schema by `IndexId` and fields. An index that left the schema or changed
  its fields is purged by `(table_id, index_id)`. An index that entered the
  schema is backfilled from `documents` in keyset pages of 500
  (`INDEX_BACKFILL_PAGE_SIZE`) with `INSERT ... ON DUPLICATE KEY UPDATE`.
  The reconcile runs inside the schema write transaction in
  `mysql/write.rs` (`replace_table_schema`) and in the durable-apply path in
  `mysql/backend.rs` (`apply_schema_change_in_session`).
- Schema delete, `DeleteTable`, and `hard_delete_table_identity` purge the
  keyspace by `table_id` inside the same transaction as the schema or
  catalog delete.
- The candidate read moved to the keyspace: a range select on
  `index_entries` joined to `documents` and `table_catalog`, ordered by
  `encoded_tuple, document_id`. New dialect-neutral planner
  `crates/nimbus-storage/src/sql/index_keyspace.rs`
  (`IndexTupleScanBounds::for_scan`) builds the tuple bounds on
  `range_scan_bounds_for_match_prefix`, which is now `pub(crate)`.
  `filter_index_documents_with_cancel` stays the authority (invariant 3).
- Deleted: `create_mysql_indexes_for_table_schema`,
  `drop_mysql_indexes_for_table_schema`, `mysql_document_column_exists`,
  `mysql_document_index_exists`, `mysql_index_key_part`, and every
  generated-column helper in `mysql/query_helpers.rs`
  (`mysql_generated_column_expr`, `mysql_generated_column_name`,
  `mysql_index_name`, `mysql_index_text_value`, `mysql_numeric_value`,
  `mysql_numeric_column_expr`, `append_mysql_range_clause`,
  `field_type_for_table_schema`, `unique_index_fields`) with their unit
  tests, and the `MYSQL_MAX_INDEX_KEY_*` constants in `mysql.rs`.
  `map_owned_index_range_bound` is now `postgres`-only.
- Deviation from the plan: the candidate read (planned as MIK3 steps 1, 2,
  and 4) landed here, because the MIK2 acceptance tests read through an
  index and the generated columns they read from are gone. MIK3 keeps the
  ordering and EXPLAIN tests and the architecture notes.

## Evidence

Tests in `crates/nimbus-storage/src/tests/mysql_provider/schema.rs`:

- `mysql_schema_write_populates_and_clears_index_entries_without_ddl`:
  after the schema apply the keyspace holds exactly one row for the
  complete document and none for the partial one; `documents` has zero
  `gcol_*` columns and zero `idx_*` keys; a prefix scan returns the
  complete document; a schema delete leaves no rows, the default schema,
  and a readable document.
- `mysql_schema_apply_backfills_indexes_added_after_documents_exist`: the
  second schema keeps the `by_rank` id and rows, purges `by_team`, and
  backfills `by_status` for the two documents that have a status; prefix
  and range scans read through the new and the kept index.
- `mysql_schema_apply_fault_before_commit_leaves_no_partial_state` (F2):
  `FaultPoint::StorageCommitBeforeVisibility` fires inside
  `fenced_replace_table_schema`; the call errors with
  `CommitterLeaseError::Storage(Error::Internal(..))`, the schema stays
  default, the keyspace is empty, the journal heads and the lease durable
  sequence do not move; the retry advances the durable head by one, seeds
  one keyspace row, and the prefix scan returns the document.
- `mysql_schema_with_more_than_sixty_four_indexes_applies_and_reads`
  (MIK0, was red with error 1069) is green.

```
cargo test -p nimbus-storage --features mysql mysql_provider   (mik2-mysql-provider.txt)
test result: ok. 46 passed; 0 failed

cargo test -p nimbus-storage --features mysql --lib index      (with the fixture env)
test result: ok. 69 passed; 0 failed

cargo test -p nimbus-storage --features mysql index_keyspace   (planner unit tests)
test result: ok. 4 passed; 0 failed

grep -rn "CREATE INDEX\|ALTER TABLE\|GENERATED ALWAYS" crates/nimbus-storage/src/mysql/ crates/nimbus-storage/src/mysql.rs
(no output; the bootstrap CREATE TABLE IF NOT EXISTS statements are the only DDL)

cargo fmt --all --check                                          clean
cargo clippy -p nimbus-storage --features "mysql postgres libsql" --all-targets -- -D warnings   clean
cargo check -p nimbus-storage --tests                            clean (default features)
cargo check -p nimbus-storage --features postgres --tests        clean
cargo check -p nimbus-storage --features libsql --tests          clean
```

Note for MIK3: `docs/private/architecture/storage/typed-key-columns.md`
does not exist; only `persistence-engine-baseline.md` is present.
