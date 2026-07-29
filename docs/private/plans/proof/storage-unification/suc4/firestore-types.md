# SUC4.2 — Firestore typed values on the write path

Branch `codex/suc4b-firestore-types`, based on `a082b9776`.

## The bug, as it actually stands

The scout brief described `serializer::into_nimbus_value` rejecting
`timestampValue` / `bytesValue` / `referenceValue` / `geoPointValue` for
*document fields*. That part is already fixed: commit `9b4f709ee` (#231)
introduced `StoredValue`, and `commit_request::lower_document_fields` has
routed document fields through `FirestoreValue::into_stored_value` since.
`TypedScalarValue` already carries `FirestoreTimestamp`, `Bytes`,
`Reference`, and `GeoPoint`, so no new variant was needed, and the test the
brief flagged as a trap
(`firestore_value_rejects_firestore_only_types_for_nimbus_conversion`) no
longer exists — it was replaced by
`firestore_value_preserves_firestore_only_types_in_stored_values`.

What survived that fix is the residue this change closes. Two Firestore
surfaces still lowered wire values through the legacy plain-JSON converter,
and both rejected all four typed kinds outright:

1. **Array transforms** — `appendMissingElements` (arrayUnion) and
   `removeAllFromArray` (arrayRemove), on both REST `:commit` and the gRPC
   `Write` stream. A client could write a timestamp into a document field
   but could not append that same timestamp to an array field.
2. **Query filter and cursor operands** — `structuredQuery.where` field
   filters and `startAt`/`endAt` cursors. A client could store a typed value
   and then had no way to filter on it.

The root cause sat below the adapter: `FieldTransformOperation`'s four array
variants in `nimbus-core` carried `Vec<serde_json::Value>`, so a typed
operand had nowhere to live even if the adapter had decoded it.

### Blast radius

- `nimbus-core`: `FieldTransformOperation::{AppendElements, AppendMissingElements,
  RemoveAllFromArray}` now carry `Vec<StoredValue>`.
- `nimbus-engine`: `execution_units::batch::apply_field_transform` — the array
  arms read and write through the typed tree.
- `nimbus-firebase`: REST commit lowering, gRPC write-stream lowering, query
  value decoding.
- `nimbus-mongodb`: `$addToSet` / `$push` / `$pull` / `$pullAll` adapted to the
  retyped operand. MongoDB operands are plain JSON, which is already the
  canonical spelling for a metadata-free value, so behavior is unchanged.

## Fail-before

Three wire-level tests were run against **unmodified HEAD source**: the eight
implementation files and the engine test file were reverted with `git checkout
HEAD --`, leaving only the new `nimbus-server` tests in the tree (they use wire
types only, so they compile against HEAD). All three fail:

```
FAIL nimbus-server tests::firebase_write_stream::firebase_write_stream_array_transforms_roundtrip_typed_values
  Status { code: InvalidArgument, message: "unsupported Firestore Value type `timestampValue`" }

FAIL nimbus-server tests::firebase_rest_crud::firebase_commit_array_transforms_roundtrip_typed_values
  {"error":{"code":"op.invalid_input","message":"invalid input: unsupported Firestore Value type `timestampValue`", ...}}
  left: 400  right: 200

FAIL nimbus-server tests::firebase_rest_query::firebase_run_query_filters_on_typed_scalar_field_values
  {"error":{"code":400,"message":"invalid input: invalid Firestore RunQuery request: invalid query value: unsupported Firestore Value type `timestampValue`","status":"INVALID_ARGUMENT"}}
  left: 400  right: 200

Summary 3 tests run: 0 passed, 3 failed, 601 skipped
```

The engine test cannot be run against literal HEAD — it asserts on a type
(`Vec<StoredValue>` operands) that does not exist there, so a literal revert
is a compile error rather than a failure. Its RED was produced by a semantic
revert of the two new helpers in `batch.rs` to their pre-change behavior
(read arrays only from `get_field`, always write back with `set_field`),
which is exactly what the old plain-JSON code did:

```
FAIL nimbus-engine ... atomic_write_batch_array_transforms_preserve_typed_elements_at_every_depth
  assertion `left == right` failed: arrayUnion must keep typed elements and dedupe repeats by typed identity
    left: None
   right: Some(List { items: [Json { value: String("seed") },
     TypedScalar { value: FirestoreTimestamp { rfc3339: "2024-01-02T03:04:05.123456789Z" } },
     TypedScalar { value: Bytes { data: [1, 2, 3, 4] } },
     TypedScalar { value: GeoPoint { latitude: 37.7749, longitude: -122.4194 } },
     Map { entries: {"attachment": TypedScalar { value: Bytes { data: [1, 2, 3, 4] } },
                     "label": Json { value: String("kept") }} }] })
```

Every reverted file was restored from a saved copy, not from `git checkout`,
and the restored diff was verified byte-identical to the pre-revert patch.

## Design

**Typed operands, end to end.** The three array variants of
`FieldTransformOperation` carry `StoredValue`. Both Firestore lanes decode
operands with the same lowering the document-field path uses
(`decode_proto_json_stored_value` for REST, `FirestoreValue::into_stored_value`
for gRPC), so an operand and a stored element are produced by the same code
and are directly comparable.

**Canonical spelling.** `StoredValue::canonical()` is new in `nimbus-core`.
Two producers spell a metadata-free value differently: `from_json_tree`
builds `Map`/`List` nodes all the way down, while the adapter lowering
collapses a metadata-free result to `Json`. Both mean the same value, so
arrayUnion dedupe and arrayRemove matching would silently misbehave when the
two spellings met. `canonical()` collapses every metadata-free subtree back
to plain JSON; both sides of every transform comparison run through it.

**Storage stays plain when it can.** `write_array_elements` only writes the
typed sidecar when some element still carries metadata; otherwise it writes
the plain projection with `set_field`. Removing the last typed element from
an array drops the sidecar, which the engine test pins.

**Nesting is supported at every depth.** `Document.typed_fields` is keyed by
top-level field name and holds an arbitrarily deep `StoredValue` tree;
`Map` and `List` nodes nest freely. No level cap exists and none was added.
The deepest case — a typed scalar inside a `mapValue` inside an array element,
appended by transform and later matched for removal — is pinned by
`atomic_write_batch_array_transforms_preserve_typed_elements_at_every_depth`
and, at the parse level, by
`lowers_typed_array_transform_operands_into_stored_values_at_every_depth`.

**Query operands lower to the stored projection, not to typed metadata.**
Filters and cursors compare against `Document.fields`, the plain JSON
projection stored beside the typed sidecar, so a typed operand decodes to
exactly `StoredValue::projected_json()`. This makes the equality filter in
the REST query test match. The bespoke `referenceValue` special case in
`run_query_request` was deleted because its output is provably identical to
`TypedScalarValue::Reference`'s projection. The structural recursion for
`arrayValue` / `mapValue` in that file was deliberately **kept**: `in` and
`not-in` legitimately carry an array of array candidates, which the document
write path rejects, so routing containers through the stored lowering would
have regressed those queries.

**Known limitation, unchanged by this work.** A Firestore timestamp projects
to its RFC3339 string, and RFC3339 strings do not sort lexicographically in
chronological order (`"...05.5Z"` sorts before `"...05Z"`, because `.` is
0x2E and `Z` is 0x5A). Equality filters on timestamp fields are exact;
range and ordering queries on them are not. This is a property of the
projection introduced by #231 and is independent of write acceptance —
recorded here as a separate finding, not fixed under this change.

## Verification

All under `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1`, `set -o
pipefail`, exit status checked directly (never through a pipe).

| crate | result |
| --- | --- |
| `nimbus-core` | 193 run, 193 passed, 0 skipped |
| `nimbus-firebase` | 73 run, 73 passed, 0 skipped |
| `nimbus-engine` | 656 run, 656 passed, 5 skipped |
| `nimbus-storage` | 435 run, 435 passed, 2 skipped |
| `nimbus-mongodb` | 288 run, 288 passed, 0 skipped |
| `nimbus-server` | 579 run, 579 passed, 25 skipped |

Crate set chosen by grepping `TypedScalarValue|StoredValue` across `crates/`,
which returns exactly these six. The skips are `#[ignore]` lanes, none of them
Firestore: `nimbus-server`'s 25 are the 15 Node-version canaries, 6
verification-harness corpora, 2 runtime-owner subprocess conformance tests,
and 2 transport-liveness campaigns.

- `cargo clippy -p nimbus-core -p nimbus-firebase -p nimbus-engine -p
  nimbus-mongodb -p nimbus-server --all-targets -- -D warnings` — exit 0.
- `cargo fmt --all --check` — exit 0.

### Tests added

- `nimbus-server` `firebase_commit_array_transforms_roundtrip_typed_values` —
  REST arrayUnion of a string plus all four typed kinds, `batchGet` returns the
  exact wire array, typed sidecar present; then arrayRemove of the timestamp
  and geo point leaves exactly the other three.
- `nimbus-server` `firebase_write_stream_array_transforms_roundtrip_typed_values`
  — the same round trip over the gRPC `Write` stream; bytes must come back as
  `bytesValue`, not as their base64 projection.
- `nimbus-server` `firebase_run_query_filters_on_typed_scalar_field_values` —
  equality filters on a `timestampValue` field (selective) and a `bytesValue`
  field (matches both documents).
- `nimbus-engine`
  `atomic_write_batch_array_transforms_preserve_typed_elements_at_every_depth`
  — typed elements including one nested inside a map inside an array element;
  dedupe by typed identity; plain projection kept in step with the typed tree;
  arrayRemove matching typed elements by value; sidecar dropped when the last
  typed element leaves.
- `nimbus-firebase`
  `lowers_typed_array_transform_operands_into_stored_values_at_every_depth` —
  parse-level pin on operand lowering, including the nested map.
- `nimbus-firebase`
  `decodes_typed_scalar_filter_and_cursor_values_to_their_stored_projection` —
  parse-level pin on the query projection semantics; guards the deleted
  `referenceValue` special case.
- `nimbus-core`
  `canonical_collapses_metadata_free_subtrees_so_equal_values_compare_equal`.
