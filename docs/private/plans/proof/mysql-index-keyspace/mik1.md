# MIK1 Index keyspace table and write-path maintenance

Date: 2026-09-09. Commit: the `storage: maintain a MySQL index_entries keyspace
on every write` commit after 814ae99ab.

## What changed

- `crates/nimbus-storage/src/mysql/backend.rs` `tenant_init_statements`: new
  `index_entries` table per D1 (`PRIMARY KEY (table_id, index_id, document_id)`,
  `KEY idx_index_entries_tuple (table_id, index_id, encoded_tuple(768))`).
- New `crates/nimbus-storage/src/mysql/index_entries.rs`: owns
  `IndexTupleMutation`, computes the mutations once per write batch (one
  schema load per table per batch), upserts the open tuple by primary key,
  deletes the row when a document loses its tuple, and then calls the
  `index_versions` history with the same mutation set (D2).
- `crates/nimbus-storage/src/mysql/index_versions.rs`: `IndexVersionMutation`
  and its computation are gone; `record_index_versions_for_mutations_in_session`
  takes the shared set.
- Commit path `write.rs` `append_commit_entry` and the durable-apply path in
  `backend.rs` both call `record_index_effects_for_*_in_session`, so the
  document write, `index_entries`, `index_versions`, and `commit_log` land in
  one transaction (invariant 1).
- Deviation from the plan step text: a mutation with both a close and an open
  tuple is one upsert by primary key, not a delete followed by an insert. The
  end state is the same and it is one statement fewer.
- Test support `index_entry_rows(connection_string, database_name)` returns
  `(table_id, index_id, document_id, encoded_tuple)`; the tuple bytes replace
  the planned tuple length because the test compares them.

## Evidence

`mysql_writes_maintain_index_entries`
(`crates/nimbus-storage/src/tests/mysql_provider/index_entries.rs`): two
documents on two indexes give 4 rows; an update of `rank` keeps 4 rows and
changes only the `by_rank` tuple of that document; a delete leaves 2 rows;
a document with `team` only adds one `by_team` row and no `by_rank` row.

```
cargo test -p nimbus-storage --features mysql mysql_writes_maintain_index_entries
test result: ok. 1 passed; 0 failed

cargo test -p nimbus-storage --features mysql mysql_provider   (mik1-mysql-provider.txt)
test result: FAILED. 43 passed; 1 failed
```

The one failure is the MIK0 test
`mysql_schema_with_more_than_sixty_four_indexes_applies_and_reads` (error
1069), which MIK2 turns green when schema apply stops creating InnoDB keys.
No other test changed outcome.
