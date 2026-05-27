# CST3 Table-Aware Document Identity

Date: 2026-05-27

## Status

status: done

CST3 is implemented. Nimbus now has a typed document-identity boundary for
Convex-compatible IDs: Convex-facing document IDs carry the developer table
name, are resolved before storage access, and wrong-table usage is rejected
with `InvalidInput` instead of silently reading, patching, or deleting through
the wrong table context.

## Landed

- `nimbus_core::ResolvedDocumentId` records the developer table context and raw
  storage `DocumentId` after protocol resolution.
- `ResolvedDocumentId::encode_table_scoped` and
  `ResolvedDocumentId::resolve_table_scoped` implement the Convex-facing ID
  codec. The first `:` separates the table context from the raw document key,
  so custom raw IDs may still contain `:`.
- Convex adapter document reads and writes now serialize `_id` as a
  table-scoped Convex ID.
- Convex `ctx.db.get`, `ctx.db.patch`, `ctx.db.delete`, manifest-backed
  `Get`/`Update`/`Delete`, direct function reads, query-builder reads, and
  paginated reads resolve Convex IDs before calling the engine.
- Convex read tracking unwraps serialized Convex IDs back to raw storage
  `DocumentId` values before recording dependencies.
- Native, Firebase, MongoDB, and other non-Convex adapter contracts are not
  forced into the Convex ID shape. Storage still keys documents by
  `(TableId, DocumentId)` behind the table catalog.

## Boundary Decision

Nimbus adopted Convex's table-bearing ID guardrail at the adapter boundary but
kept the storage `TableId` catalog internal. The public Convex-compatible ID
shape is `table_name:document_id`; storage resolution still goes through the
active table catalog and stores rows under `(TableId, DocumentId)`.

This is intentionally narrower than exposing Nimbus `TableId` in public
adapter strings. Today there is no public table rename/drop API whose old
public IDs must survive across same-name lifetimes, and CST2 already prevents
physical row inheritance through lifecycle state plus `TableId` storage keys.
CST3's required guarantee is that Convex code cannot pass an ID for one table
to another table and receive silent wrong-table behavior.

## Tests Run

```text
cargo check -p nimbus-core -p nimbus-server

Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.91s
Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.11s
```

```text
cargo test -p nimbus-core resolved_document_id --lib

running 2 tests
test tests::resolved_document_id_round_trips_table_scoped_ids ... ok
test tests::resolved_document_id_rejects_wrong_table ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 86 filtered out
```

```text
cargo test -p nimbus-server adapters::convex::tests::authorization --lib

running 11 tests
test adapters::convex::tests::authorization::runtime_host_bridge_rejects_wrong_table_convex_document_ids ... ok
test adapters::convex::tests::authorization::convex_read_get_round_trips_custom_table_scoped_ids ... ok
test adapters::convex::tests::authorization::runtime_mutation_bridge_stages_writes_until_commit_and_reads_its_own_writes ... ok
...

test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 846 filtered out
```

```text
cargo test -p nimbus-server adapters::convex::tests --lib

running 23 tests
test adapters::convex::tests::authorization::runtime_host_bridge_rejects_wrong_table_convex_document_ids ... ok
test adapters::convex::tests::authorization::convex_read_get_round_trips_custom_table_scoped_ids ... ok
...

test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 834 filtered out
```

## Notes

- Generated Convex IDs are scoped on insert and returned as scoped `_id`
  fields on reads.
- Custom IDs remain supported, including raw IDs containing `:`.
- Wrong-table `get`, `patch`, and `delete` all fail before storage dispatch.
- Read dependencies keep raw storage document IDs, not serialized
  `table:document` protocol IDs.
