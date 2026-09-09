# MIK3 Index candidate reads through the keyspace

Date: 2026-09-09. Commit: the `storage: prove MySQL index reads plan over the
index_entries keyspace` commit after ab7f781ad.

## What changed

- The candidate read itself moved in MIK2 (see `mik2.md`). MIK3 adds the
  read-path proofs and the architecture note.
- `crates/nimbus-storage/src/mysql/index_entries.rs`: the candidate select
  is built by `index_candidate_select(database_name, table_id, index_id,
  start_key, end_key) -> (String, Params)`, which
  `load_index_candidate_documents_from_session` executes. The module is
  `pub(crate)` so the EXPLAIN test can plan the exact statement the read
  path runs.
- New `crates/nimbus-storage/src/tests/mysql_provider/index_reads.rs` owns
  the read-path proofs; `support.rs` gains `explain_rows` (table alias,
  access type, chosen key per `EXPLAIN` row) and re-exports `TenantStore`
  for the redb comparison.
- `docs/private/architecture/storage/persistence-engine-baseline.md`: the
  MySQL row of the history layout describes the `index_entries` keyspace,
  the bootstrap-only key, the transactional maintenance, and the removed
  64-key bound; the query-planning section names
  `sql/index_keyspace.rs` and `mysql/index_entries.rs`. The plan's
  `typed-key-columns.md` does not exist in this tree, so there is no
  second note to update.
- No typed read helper was left over after MIK2; step 4 had nothing to
  delete.

## Evidence

- `mysql_index_range_scans_order_numbers_numerically`: documents with
  `rank` 2, 10, 9 and `label` "2", "10", "9" are inserted into the MySQL
  store and into an in-memory redb `TenantStore`; a number range
  `[1, ..)` on `by_rank` returns 2, 9, 10 on both stores, and a string
  range `["1", ..)` on `by_label` returns "10", "2", "9" on both stores,
  compared as ordered id lists.
- `mysql_index_reads_use_the_keyspace_not_a_table_scan`: 128 documents,
  range `[40, 48)` on `by_rank`; `EXPLAIN` on the statement
  `index_candidate_select` builds reads `index_entries` with access type
  `range` on key `idx_index_entries_tuple`, joins `documents` with
  `eq_ref` on `PRIMARY`, and no step has access type `ALL` or `index`;
  the store read returns ranks 40 through 47 in order.

```
cargo test -p nimbus-storage --features mysql mysql_index_r
test result: ok. 3 passed; 0 failed   (the two new tests and the round-trip test)

cargo test -p nimbus-storage --features mysql mysql_provider   (mik3-mysql-provider.txt)
test result: ok. 48 passed; 0 failed

cargo test -p nimbus-storage --features mysql
test result: ok. 466 passed; 0 failed; 3 ignored

cargo fmt --all --check                                          clean
cargo clippy -p nimbus-storage --features "mysql postgres libsql" --all-targets -- -D warnings   exit 0
cargo check -p nimbus-storage --tests                            clean (default features)
cargo check -p nimbus-storage --features postgres --tests        clean
cargo check -p nimbus-storage --features libsql --tests          clean
```

The first run of the ordering test failed on the redb side with an empty
result because redb's plain `insert` does not maintain indexes; the test
uses `insert_with_indexes` like the other redb tests. The EXPLAIN test
passed on its first run.
