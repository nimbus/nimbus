# SA8 Negative-Zero Digest Contract

Date: 2026-08-25  
Baseline: `5465c9b80624a6fc78fc9e1c870b0e1e2addfd76`  
Work commit: `dbc8c8ceb3dfeddb767159906f8e50a6d643ac63`  
Merge commit: `2e8d744ea0f85680516bf00419df320af81f87bb`  
Pull request: #310

## Result

The materialized-position codec now treats scalar and GeoPoint negative zero
as separate contracts.

- `SpecialDouble::NegativeZero` is a client-visible stored scalar. Its finite
  body keeps the IEEE-754 sign bit, so its digest differs from plain `0.0`.
- GeoPoint latitude and longitude remain coordinates. They use the existing
  finite-float path, which normalizes `-0.0` to `0.0` before hashing.
- NaN, positive infinity, negative infinity, and ordinary finite doubles keep
  their previous canonical bytes.

This is a pre-launch correction to the version 2 codec. It adds no migration
or compatibility path.

## Fail-Before Evidence

The new scalar regression test failed before the codec repair:

```text
cargo test -p nimbus-storage \
  canonical_leaf_distinguishes_scalar_negative_zero -- --nocapture

assertion `left != right` failed
left:  83b51ae8b98374f62ef5105beb9ba8934e1dff2d9ef3b4bb83e5874ae4cc934b
right: 83b51ae8b98374f62ef5105beb9ba8934e1dff2d9ef3b4bb83e5874ae4cc934b
```

## Verification

```text
cargo fmt --all --check
PASS

cargo test -p nimbus-storage canonical_leaf -- --nocapture
5 passed; 0 failed

cargo test -p nimbus-storage materialized_position -- --nocapture
14 passed; 0 failed

cargo test -p nimbus-engine \
  materialized_position_golden_matches_shipped_graph -- --nocapture
1 passed; 0 failed

cargo test -p nimbus-storage -- --nocapture
unit: 365 passed; 0 failed; 3 ignored
generated-history integration: 0 passed; 0 failed; 1 ignored

cargo clippy -p nimbus-storage --all-targets -- -D warnings
PASS

Nimbus autoreview --gate pre-pr --mode auto
PASS; no accepted or actionable findings
```

PR #310 merged after local acceptance and autoreview. Its hosted jobs were
still running when GitHub accepted the merge. The Band SA final gate must run
the required repository checks on merged main.
