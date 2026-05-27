# CST4 TableId Dependencies

Date: 2026-05-27

## Status

status: done

CST4 is implemented. Runtime read tracking, subscription dependencies, and
mutation/read intersection now match materialized table reads by stable
`TableId` instead of only by public `TableName`.

## Landed

- `DependencySet` now stores stable table dependencies as:
  - `TableDependency { table, table_id }`
  - `DocumentDependency { table, table_id, document_id }`
  - `PredicateDependency { table, table_id, filters }`
  - `PaginatedWindowDependency { table, table_id, ... }`
  - `IndexRangeDependency { table, table_id, ... }`
- `commit_intersects_dependency_set` and durable-record intersection compare
  dependency table identity against `WriteOp.table_id`.
- Same public table name plus a different `TableId` no longer intersects old
  table/document dependencies.
- Missing-table reads are still represented explicitly so subscriptions on a
  table that does not yet have a catalog identity wake on the first relevant
  write.
- Missing filtered reads use `MissingPredicateDependency`, so a filtered query
  against a not-yet-materialized table does not wake for non-matching first
  inserts.
- Engine mutation execution units resolve table identity from their read
  snapshot when recording read dependencies, and from the runtime store when
  recording write dependencies.
- Runtime read tracking resolves `TableId` before converting host read sets to
  core dependency sets.
- Subscription bootstrap and re-evaluation dependencies are rebuilt from the
  active table identity after evaluating the query.

## Tests Run

```text
cargo check -p nimbus-core -p nimbus-engine -p nimbus-server

Finished `dev` profile [unoptimized + debuginfo] target(s) in 14.41s
```

```text
cargo test -p nimbus-core dependency --lib

running 9 tests
test dependency::tests::table_dependency_uses_table_id_not_reused_table_name ... ok
test dependency::tests::document_dependency_uses_table_id_not_reused_table_name_and_document_key ... ok
test dependency::tests::missing_predicate_dependency_matches_only_possible_first_writes ... ok
...

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 83 filtered out
```

```text
cargo test -p nimbus-engine subscriptions --lib

running 25 tests
test tests::subscriptions::filters::service_insert_only_notifies_filtered_subscriptions_for_matching_documents ... ok
test tests::subscriptions::filters::service_only_notifies_subscriptions_for_affected_tables ... ok
test tests::subscriptions::filters::service_limited_subscriptions_skip_out_of_window_ordered_writes ... ok
...

test result: ok. 25 passed; 0 failed; 0 ignored; 0 measured; 243 filtered out
```

```text
cargo test -p nimbus-server read_tracking --lib

running 5 tests
test execution::read_tracking::tests::runtime_read_set_converts_to_shared_dependency_set_without_losing_skip_behavior ... ok
test execution::read_tracking::tests::shared_dependency_matching_uses_previous_document_snapshots_for_updates ... ok
...

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 852 filtered out
```

```text
cargo test -p nimbus-server adapters::convex::tests::authorization --lib

running 11 tests
test adapters::convex::tests::authorization::runtime_mutation_bridge_commit_detects_occ_conflicts ... ok
test adapters::convex::tests::authorization::runtime_host_bridge_rejects_wrong_table_convex_document_ids ... ok
...

test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 846 filtered out
```

## Notes

- `TableName` remains in dependency structs only as diagnostic and query
  synthesis context. Intersection decisions use `TableId` whenever a material
  table identity exists.
- The missing-table sentinel is intentionally name-based because no stable
  table identity exists before the first table catalog row. It is narrowed by
  filters when the original read was filtered.
