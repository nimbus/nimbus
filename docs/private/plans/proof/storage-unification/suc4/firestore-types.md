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

What survived that fix is the residue this change closes: **array
transforms**. `appendMissingElements` (arrayUnion) and `removeAllFromArray`
(arrayRemove), on both REST `:commit` and the gRPC `Write` stream, still
lowered operands through the legacy plain-JSON converter and rejected all four
typed kinds outright. A client could write a timestamp into a document field
but could not append that same timestamp to an array field.

The root cause sat below the adapter: `FieldTransformOperation`'s four array
variants in `nimbus-core` carried `Vec<serde_json::Value>`, so a typed
operand had nowhere to live even if the adapter had decoded it.

Query filter and cursor operands are the adjacent surface with the same
symptom, and they are deliberately **not** opened up here. Filters compare
against the stored plain-JSON projection, which cannot distinguish a typed
value from a lookalike plain one and does not order it correctly, so accepting
typed operands there would trade a clear rejection for silent wrong answers.
They stay rejected, with a better diagnostic; see the Design section.

### Blast radius

- `nimbus-core`: `FieldTransformOperation::{AppendElements, AppendMissingElements,
  RemoveAllFromArray}` now carry `Vec<StoredValue>`.
- `nimbus-engine`: `execution_units::batch::apply_field_transform` — the array
  arms read and write through the typed tree.
- `nimbus-firebase`: REST commit lowering and gRPC write-stream lowering carry
  typed operands; query value decoding gains an explicit, explained rejection
  for the three projection-unsafe types.
- `nimbus-mongodb`: `$addToSet` / `$push` / `$pull` / `$pullAll` adapted to the
  retyped operand. MongoDB operands are plain JSON, which is already the
  canonical spelling for a metadata-free value, so behavior is unchanged.

## Fail-before

The two wire-level write-lane tests were run against **unmodified HEAD
source**: the eight implementation files and the engine test file were reverted
with `git checkout HEAD --`, leaving only the new `nimbus-server` tests in the
tree (they use wire types only, so they compile against HEAD). Both fail:

```
FAIL nimbus-server tests::firebase_write_stream::firebase_write_stream_array_transforms_roundtrip_typed_values
  Status { code: InvalidArgument, message: "unsupported Firestore Value type `timestampValue`" }

FAIL nimbus-server tests::firebase_rest_crud::firebase_commit_array_transforms_roundtrip_typed_values
  {"error":{"code":"op.invalid_input","message":"invalid input: unsupported Firestore Value type `timestampValue`", ...}}
  left: 400  right: 200
```

The first revision of this change also carried a third RED, for a test that
accepted typed query operands. The review found that behavior wrong and the
test was deleted, so it is not evidence for anything that ships. The query
surface now keeps HEAD's rejection, which means there is deliberately **no**
fail-before for it: nothing about the accept/reject verdict changes. Its
evidence is positive instead — the collision test shows why the rejection is
the correct contract, and the parse-level tests pin the improved diagnostic.

The engine tests cannot be run against literal HEAD — they assert on a type
(`Vec<StoredValue>` operands) that does not exist there, so a literal revert
is a compile error rather than a failure. Both REDs were produced by a semantic
revert of the specific helper under test, disclosed here as such.

Reverting the two new helpers in `batch.rs` to their pre-change behavior (read
arrays only from `get_field`, always write back with `set_field`), which is
exactly what the old plain-JSON code did:

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

Reverting `firestore_transform_values_equivalent` to its non-recursive form
(numeric equivalence only when both whole operands are scalar numbers) leaves
both duplicate spellings alive, which is the defect the review reported:

```
FAIL nimbus-engine ... atomic_write_batch_array_transforms_apply_numeric_equivalence_at_nested_leaves
  assertion `left == right` failed: int and double spellings must dedupe at nested leaves, keeping the first appended
    left: Some(List { items: [Map { entries: {"at": TypedScalar { ... }, "n": Json { value: Number(3) }} },
                              Map { entries: {"at": TypedScalar { ... }, "n": Json { value: Number(3.0) }} },
                              Json { value: Object {"counts": Array [Number(1), Number(2)]} },
                              Json { value: Object {"counts": Array [Number(1.0), Number(2.0)]} }] })
   right: Some(List { items: [Map { entries: {"at": TypedScalar { ... }, "n": Json { value: Number(3) }} },
                              Json { value: Object {"counts": Array [Number(1), Number(2)]} }] })
```

Timestamp canonicalization has a wire-level RED that needs no revert, because
the fix is additive to a function that already existed:

```
FAIL nimbus-server ... firebase_commit_array_transforms_dedupe_equivalent_timestamp_spellings
  assertion `left == right` failed: equivalent spellings of one instant must dedupe to one canonical element
    left: [{"timestampValue": "2024-01-02T03:04:05Z"},
           {"timestampValue": "2024-01-02T04:04:05+01:00"},
           {"timestampValue": "2024-01-02T03:04:05.123456789Z"},
           {"timestampValue": "2024-01-02T03:04:05.123456Z"}]
   right: [{"timestampValue": "2024-01-02T03:04:05Z"},
           {"timestampValue": "2024-01-02T03:04:05.123456Z"}]
```

Four operands, two instants, four surviving elements — arrayUnion appended a
duplicate for each equivalent spelling. The paired unit test failed on the same
run with `left: "2024-01-02T03:04:05.123456789Z"`, `right:
"2024-01-02T03:04:05.123456Z"`.

Every reverted file was restored from a saved copy, not from `git checkout`,
and each restored file was re-run green afterwards.

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

**Numeric equivalence applies at every leaf.** Firestore treats an int64 and a
double of the same magnitude as one value, so `3` and `3.0` must dedupe
together. The old comparator applied that rule only when both whole operands
were scalar numbers, because `FiniteNumericTransformValue::from_document`
reads only scalars — a nested leaf fell through to structural JSON equality,
where `Number(3)` and `Number(3.0)` differ. `firestore_transform_values_equivalent`
now recurses through `Map`, `List`, and plain JSON objects and arrays,
applying numeric equivalence at numeric leaves. Object keys are matched by
lookup rather than by zipped iteration order, so the result does not depend on
`serde_json`'s map ordering. This corrects plain JSON containers as well as
typed ones; the gap predated typed operands.

**Timestamps are canonicalized at lowering.** A Firestore timestamp is stored
as its RFC 3339 text, and every comparison downstream is on that stored text,
so one instant with several spellings would defeat array-transform dedupe and
removal. `normalize_firestore_timestamp` already reformatted through `time`'s
RFC 3339 parser, which collapsed `+00:00`, `-00:00`, lowercase `t`/`z`, and
trailing fractional zeros to one form. Two gaps remained: a non-UTC offset was
preserved verbatim, so `2024-01-02T04:04:05+01:00` and `2024-01-02T03:04:05Z`
stored differently despite being one instant; and nanosecond precision was
retained, so `.123456789Z` and `.123456Z` stored differently despite being one
value in Firestore, which keeps timestamps to microsecond precision truncated
toward the start of time.

Both are now closed at lowering: convert to UTC, then floor the
nanosecond-of-second to a microsecond. That component is non-negative
regardless of era, so flooring it truncates toward the start of time for
pre-epoch instants too, rather than toward zero.

Canonicalizing at lowering was chosen over normalizing at comparison. Lowering
is a single choke point every write lane already passes through — REST
`:commit`, the gRPC `Write` stream, and array-transform operands all reach
`FirestoreValue::into_stored_value`, and it is the only place outside tests
that constructs a `FirestoreTimestamp`. Fixing it there makes storage, dedupe,
removal, and rendering agree by construction, whereas a comparison-time
normalizer would have to be repeated at each comparison site and would still
leave two documents holding one instant rendering differently on read.

This changes observable output: a client that writes `.123456789Z` reads back
`.123456Z`, and one that writes `+01:00` reads back the `Z` form. That is what
real Firestore does, so the round-trip tests that asserted the caller's exact
spelling were pinning a divergence from Firestore. They now assert the
truncation explicitly instead of carrying a quietly swapped literal.

**Bytes and geo points need no equivalent fix, and a test pins why.**
`bytesValue` is decoded to `Vec<u8>` at parse and compared as bytes, so base64
spelling cannot reach storage at all; only the standard padded alphabet parses,
and URL-safe or mispadded input is refused rather than admitted as a second
spelling. `geoPointValue` coordinates are `f64` compared by derived
`PartialEq`, where IEEE equality already makes `-0.0` and `0.0` one value and
integer and double JSON spellings parse identically through `as_f64`. NaN,
infinities, and out-of-range coordinates are refused at parse, so no value that
is unequal to itself can be stored.

**Typed operands are rejected in query filters and cursors.** Query evaluation
compares against `Document.fields` — the plain JSON projection — at
`queries/structured/finalize.rs`, where equality is `field_value == value` and
ordering runs through `compare_structured_order_values`. A projection cannot
carry type identity, so lowering a typed operand to its projection produces
false matches across types: a `bytesValue` projects to its base64 string and
would equal a plain string of that text, a `timestampValue` to its RFC3339
string, a `geoPointValue` to an ordinary two-key map. Ordering is worse —
RFC3339 strings do not sort chronologically (`…05.5Z` sorts before `…05Z`,
because `.` is 0x2E and `Z` is 0x5A), base64 does not sort in byte order, and a
map has no ordering at all — so range filters and cursors would silently omit
or misplace documents.

Making comparison type-aware means carrying `StoredValue` through
`FieldFilter.value`, `StructuredCursor.values`, the prepared-filter machinery,
and index selection, across every adapter that builds a filter. That is a
separate design with its own index-correctness question, not a detail of a
write-path fix. So the contract here is the smallest correct one:
`timestampValue`, `bytesValue`, and `geoPointValue` are refused in filters and
cursors — at any depth, including inside an `in` candidate array — with an
error naming the type and the reason. This is what the surface did before this
change, so nothing regresses; only the diagnostic improves.

`referenceValue` stays accepted, unchanged from before: `__name__` document-ID
filters are built on it, and it is compared against the document name rather
than a stored field. It carries the same projection-collision property against
a plain string field, which is pre-existing and out of scope here.

The structural recursion for `arrayValue` / `mapValue` is kept, because `in`
and `not-in` legitimately carry an array of array candidates that the document
write path rejects; routing containers through the stored lowering would have
broken those queries.

Writes are unaffected. A client can store all four typed kinds, read them back
with full fidelity, and use them in array transforms; it cannot yet filter or
sort on the three whose projection is ambiguous.

## Review findings and dispositions

A structured second-model review of the first revision raised three findings.
All three were verified against the real code paths and all three were real.

| finding | disposition |
| --- | --- |
| Cross-type false matches from projecting typed query operands | Fixed by rejecting the three projection-unsafe types in filters and cursors. Collision demonstrated end to end by `firebase_run_query_rejects_typed_operands_that_projection_matching_cannot_separate`, which shows a plain `stringValue` operand matching both a stored `bytesValue` and a lookalike string. |
| Timestamp range/cursor ordering is not chronological | Fixed by the same rejection. The reviewer's framing is the right one: this change is what would newly have accepted those operands, so accepting them with known-wrong ordering was not separable from the change. Recorded above as the reason for the contract rather than as a deferred ticket. |
| Numeric equivalence not applied inside typed containers | Fixed by recursing in `firestore_transform_values_equivalent`. Scope was wider than reported: plain JSON containers had the same gap before this change. |

A second review pass raised one more finding, of the same equivalence class.

| finding | disposition |
| --- | --- |
| `FirestoreTimestamp` compared by stored RFC 3339 string, so equivalent spellings compare unequal | Fixed by canonicalizing to UTC microseconds at lowering. Partly refuted as reported: the specific `+00:00` vs `Z` example was already collapsed by the existing `time` reformat, as were `-00:00`, lowercase `t`/`z`, and trailing fractional zeros. The class is real via the two spellings that survived — non-UTC offsets, and sub-microsecond precision — and `firebase_commit_array_transforms_dedupe_equivalent_timestamp_spellings` failed at the wire with all four operand spellings surviving as four distinct elements. |
| Analogous hazard in `bytesValue` and `geoPointValue` (asked, not asserted) | No fix needed; pinned rather than assumed. `bytes_and_geo_points_have_no_spelling_equivalence_hazard` passes unchanged against the pre-fix code, which is the point: it records why these two are structurally immune, so a future change that admits a second spelling fails loudly. |

## Verification

All under `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1`, `set -o
pipefail`, exit status checked directly (never through a pipe).

| crate | result |
| --- | --- |
| `nimbus-core` | 193 run, 193 passed, 0 skipped |
| `nimbus-firebase` | 76 run, 76 passed, 0 skipped |
| `nimbus-engine` | 657 run, 657 passed, 5 skipped |
| `nimbus-storage` | 435 run, 435 passed, 2 skipped |
| `nimbus-mongodb` | 288 run, 288 passed, 0 skipped |
| `nimbus-server` | 580 run, 580 passed, 25 skipped |

One unrelated engine test, `mutation_journal::arm_selection::
opaque_internal_job_cannot_overtake_ordered_publisher`, failed once under full-
suite load and passed on a full-suite rerun and three isolated runs. It is a
load-sensitive concurrency assertion (`journal.len() == 2`) in the committer
arm-selection lane; no engine file changed in this revision, so it is not
attributable to this work and is recorded rather than swept up.

Crate set chosen by grepping `TypedScalarValue|StoredValue` across `crates/`,
which returns exactly these six. The skips are `#[ignore]` lanes, none of them
Firestore: `nimbus-server`'s 25 are the 15 Node-version canaries, 6
verification-harness corpora, 2 runtime-owner subprocess conformance tests,
and 2 transport-liveness campaigns.

- `cargo clippy -p nimbus-core -p nimbus-firebase -p nimbus-engine -p
  nimbus-mongodb -p nimbus-server -p nimbus-storage --all-targets -- -D
  warnings` — exit 0.
- `cargo fmt --all --check` — exit 0.

### Tests added

Write lane:

- `nimbus-server` `firebase_commit_array_transforms_roundtrip_typed_values` —
  REST arrayUnion of a string plus all four typed kinds, `batchGet` returns the
  exact wire array, typed sidecar present; then arrayRemove of the timestamp
  and geo point leaves exactly the other three.
- `nimbus-server` `firebase_write_stream_array_transforms_roundtrip_typed_values`
  — the same round trip over the gRPC `Write` stream; bytes must come back as
  `bytesValue`, not as their base64 projection.
- `nimbus-engine`
  `atomic_write_batch_array_transforms_preserve_typed_elements_at_every_depth`
  — typed elements including one nested inside a map inside an array element;
  dedupe by typed identity; plain projection kept in step with the typed tree;
  arrayRemove matching typed elements by value; sidecar dropped when the last
  typed element leaves.
- `nimbus-engine`
  `atomic_write_batch_array_transforms_apply_numeric_equivalence_at_nested_leaves`
  — `3` and `3.0` dedupe together at a leaf inside a typed map and inside a
  plain JSON object, and the double spelling removes an element stored with the
  integer spelling.
- `nimbus-firebase`
  `lowers_typed_array_transform_operands_into_stored_values_at_every_depth` —
  parse-level pin on operand lowering, including the nested map.
- `nimbus-core`
  `canonical_collapses_metadata_free_subtrees_so_equal_values_compare_equal`.

Timestamp equivalence:

- `nimbus-server`
  `firebase_commit_array_transforms_dedupe_equivalent_timestamp_spellings` —
  arrayUnion of four operands spelling two instants dedupes to two canonical
  elements, and arrayRemove with a `-05:00` spelling the client never wrote
  removes the element stored in `Z` form.
- `nimbus-firebase` `firestore_timestamps_lower_to_canonical_utc_microseconds`
  — sub-microsecond truncation, truncation toward the start of time for a
  pre-epoch instant, and seven spellings of one instant lowering identically.
- `nimbus-firebase` `bytes_and_geo_points_have_no_spelling_equivalence_hazard`
  — the negative pin: standard-alphabet-only base64 decoding, `-0.0` equal to
  `0.0`, integer and double coordinate spellings equal, non-finite and
  out-of-range coordinates refused.

Query contract, both sides:

- `nimbus-server`
  `firebase_run_query_rejects_typed_operands_that_projection_matching_cannot_separate`
  — writes typed values and lookalike plain strings; shows an accepted
  `stringValue` operand matching **both**, which is the collision; then pins
  that each of the three types is refused with `400` naming the type, in
  filters and in cursors; then confirms the documents still read back with full
  typed fidelity.
- `nimbus-firebase` `rejects_projection_unsafe_typed_values_in_filters_and_cursors`
  — parse-level rejection for all three types in filters, in cursors, and
  nested inside an `in` candidate array.
- `nimbus-firebase` `projection_collision_shows_why_typed_query_operands_stay_rejected`
  — pins the projections that collide and the non-chronological string ordering,
  so the rejection's justification fails loudly if a projection ever changes.
- `nimbus-firebase` `parses_reference_values_for_document_id_filters_and_cursors`
  (pre-existing) — `referenceValue` still accepted for `__name__`.
