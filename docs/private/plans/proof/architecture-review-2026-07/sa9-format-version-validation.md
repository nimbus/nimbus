# SA9 Durable Format Version Validation

Date: 2026-08-26  
Baseline: `a1b044a4b`  
Work commit: `286a9529e`  
Merge commit: `c967908bc03e36ef4b303cb52ae61a8b3b4d3143`  
Pull request: #324

## Result

Durable readers now reject unsupported versions before they interpret current
format content.

- `TenantEventRecord::validate_integrity` rejects any version other than the
  current version before it computes or compares the record integrity hash.
- The object backup reader reports a recognized numeric bundle-version mismatch
  as `InvalidInput`, not storage corruption.
- The framed AEAD reader reports a recognized `NBF` version mismatch as
  `InvalidInput`, not storage corruption.
- Both byte readers keep unrecognized magic classified as corruption.

This closes the silent journal replay path while keeping deployment-version
skew distinct from damaged durable bytes.

## Fail-Before Evidence

The three new regression tests first ran against the baseline:

```text
future_record_version_with_matching_integrity_is_rejected
FAIL: a version 4 record with a recomputed valid integrity hash returned Ok

backup_bundle_decode_reports_future_format_as_version_skew
FAIL: NIMBUSOBJBACKUP2 was classified as StorageErrorKind::Corruption

framed_blob_reports_future_format_as_version_skew
FAIL: NBF3 was classified as StorageErrorKind::Corruption
```

Separate final tests prove that unrelated magic still reports corruption.

## Verification

```text
cargo test -p nimbus-core -- --nocapture
195 passed; 0 failed

cargo test -p nimbus-crypto -- --nocapture
85 passed; 0 failed

cargo test -p nimbus-blob -- --nocapture
251 passed; 0 failed

cargo test -p nimbus-storage -- --nocapture
388 passed; 0 failed; 3 ignored

cargo clippy -p nimbus-core -p nimbus-crypto -p nimbus-blob \
  -p nimbus-storage --all-targets -- -D warnings
PASS

cargo fmt --all --check
PASS

Nimbus autoreview --gate pre-pr --mode auto
PASS; no accepted or actionable findings; Trufflehog clean

PATH=/opt/homebrew/opt/node@24/bin:$PATH \
  CARGO_TARGET_DIR=/Users/jack/src/github.com/nimbus/nimbus/target \
  make ci
Format, Clippy, dependency policy, runtime tests, and all SA9-focused tests
passed. The workspace lane ran 7,652 tests: 7,651 passed and one unrelated CLI
machine teardown deadline test failed under load.

cargo nextest run -p nimbus-cli \
  -E 'test(=machine::backend::provision::tests::teardown_substitution::exact_guest_teardown_accept_fails_within_its_deadline_when_a_call_is_missing)'
1 passed in 0.14 seconds; 1,061 skipped

cargo test --workspace --exclude nimbus-runtime --doc
PASS; both compile-fail doctests passed

bash scripts/verification-harness.sh required all
PASS; storage, engine, server, and runtime required campaigns passed

make build-js typecheck-js test-js proof-helpers
PASS; JavaScript builds and type checks, 832 UI tests, package tests, and proof
helpers completed with exit 0
```
