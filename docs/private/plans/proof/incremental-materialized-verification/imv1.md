# IMV1 Canonical Materialized Values

Date: 2026-08-20  
Baseline: `9bf332dbd4e81012616bd317cf2574dc9ed7e4e7`  
Work commit: `0da288204`  
Host: macOS arm64

## Result

IMV1 replaces the feature-sensitive JSON digest. It uses materialized-position
format version 2. The storage-owned codec writes domain-tagged canonical bytes
directly into SHA-256. It does not build a serialized state payload.

The fixed cross-graph fixture is:

```text
version = 2
digest = cc10a2a6579d2df620010321813fa1ca2bc715288280c0d62a502b5281a7ca68
```

Storage-only and shipped-engine tests assert both literals.

## Canonical value contract

`StoredValue` is the normalized Nimbus logical value tree for values that need
typed database semantics. Plain JSON is its lossless subset and projection.
Adapter lowering remains the only protocol-to-logical conversion. The
implementation makes these consumers use that shared model:

- document persistence retains the logical tree in the existing typed sidecar.
- semantic equality compares canonical `StoredValue` trees before it applies a
  protocol-specific numeric rule.
- index maintenance consumes the stored logical value when one exists.
- the materialized codec walks the same logical value directly.

`Document::set_typed_field` canonicalizes at the document boundary. Equivalent
metadata-free `Json`, `Map`, and `List` spellings therefore collapse to one
logical value. The codec also treats equivalent plain JSON and `StoredValue`
trees identically. It sorts every object key and set value itself, independent
of `serde_json/preserve_order`.

IMV1 does not change index key bytes. Integer `1` and floating-point `1.0`
still collide in the universal numeric index encoding. The RR31 input records
the collision, large-integer risk, adapter consequences, and required successor
inventory. This preserves the approved boundary: RR31 owns numeric equality and
ordering policy.

## Float, position, and restore contracts

The codec assigns distinct encodings to finite values, NaN, positive infinity,
and negative infinity. It normalizes negative zero to zero where logical
equality requires it. Firestore gRPC GeoPoints now use the same finite and range
validation as REST before a typed value can reach storage.

`CanonicalMaterializedState` and `MaterializedPosition` now have private
fields. The position constructor and deserializer reject unsupported versions
and non-lowercase SHA-256 values. Point-in-time restore validates the position
and its applied sequence before it validates or writes destination state. The
state-derived target digest check still runs after replay.

Format version 2 deliberately rejects version 1 positions. Nimbus is
pre-launch, so the task adds no migration shim.

## Governing documentation

The work force-tracks
`docs/private/architecture/storage/persistence-engine-baseline.md`. It records
the version 2 codec, opaque construction, shared logical model, total float
encoding, and PITR preflight. The archived SIC plan and SIC4 proof now define
the original proof boundary. SIC4 proved collection ordering and sequence
binding. IMV1 repairs the shipped Cargo graph, stored-value spelling, and
non-finite float gaps.

The workspace manifest and the CLI application-manifest comment no longer state
the false `preserve_order` rule. The CLI still retains raw source text because
`serde_json::Map` does not retain raw formatting.

## Verification

Focused acceptance commands passed:

```text
cargo test -p nimbus-storage canonical_leaf -- --nocapture
  4 passed
cargo test -p nimbus-storage materialized_position -- --nocapture
  13 passed
cargo test -p nimbus-storage journal_snapshot -- --nocapture
  11 passed
cargo test -p nimbus-engine materialized_position_golden_matches_shipped_graph -- --nocapture
  1 passed
cargo test -p nimbus-firebase geo_point -- --nocapture
  2 passed
cargo test -p nimbus-engine atomic_write_batch_array_transforms -- --nocapture
  2 passed
```

The PITR preflight test covers both an unsupported position version and an
applied-sequence mismatch. Each case leaves the destination at sequence zero
with no document or schema write.

Broader local lanes passed:

```text
NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 \
  cargo test -p nimbus-core -p nimbus-storage -p nimbus-firebase
  PASS; storage summary 483 passed, 3 ignored

NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 \
  cargo test -p nimbus-engine
  676 passed, 5 ignored

cargo fmt --all --check
  PASS
make clippy
  PASS
bash scripts/check-docs.sh
  PASS; 109 pages
git diff --check
  PASS
```

`make ci` passed the format check, Clippy, dependency policy, and runtime lane.
It also passed 7,538 of 7,539 workspace tests. One test failed under parallel
workspace load:
`nimbus-server::mongodb_spec spec_executor_crud_execution_report`.

That unchanged MongoDB report passed three isolated reruns. Two of those
reruns used Nextest. No changed IMV path reaches that report. Hosted CI remains
the merge source of truth for the full lane.

External PostgreSQL, MySQL, and libSQL fixture lanes are `UNVERIFIED` locally.
The ordinary workspace run used the repository-required provider opt-out.

## Verifier transition

The plan verifier now reports:

```text
Summary: 6 passed, 10 failed
```

Conditions 3, 4, and 5 are green for canonical values and floats, PITR
preflight, and the streaming reference digest. Conditions 7 through 16 remain
red for IMV2 through IMV7, as required. IMV2 is the next task.
