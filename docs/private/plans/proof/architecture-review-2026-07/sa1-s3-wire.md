# SA1 S3 Wire Integrity Contracts

Date: 2026-08-25  
Baseline: `cf8d2e2303301b6b673597fe938ae5d4235eff44`  
Work commit: `58bbcad976442dacf0b89b47e32f4fda161d6ba0`  
Merge commit: `d5edda7260a3c1ddf60956abd3f620411bb31ee9`  
Pull request: #311

## Result

The S3 adapter now enforces the three confirmed SA1 wire contracts.

- `ListObjectsV2` uses the last consumed object key as its exclusive
  continuation cursor. Pagination returns every key and rolled-up common
  prefix exactly once, and `IsTruncated` is true exactly when a next token is
  present.
- `DeleteObject` translates ETag, size, and last-modified conditions into a
  protocol-neutral delete condition. The tenant committer evaluates every
  clause against its serialized manifest read before sequence assignment.
  Rejection returns `412 Precondition Failed`, leaves the object intact, and
  consumes no sequence. An absent object remains an idempotent success.
- `CompleteMultipartUpload` recomputes a supplied full-object CRC64NVME across
  the selected part blobs with a bounded read buffer. A mismatch returns
  `BadDigest` before the multipart upload is consumed. Only the verified value
  is stored and returned.
- CRC32, CRC32C, SHA1, and SHA256 remain deliberately unsupported. Nimbus
  fails closed instead of accepting a checksum that it cannot verify. The
  public S3 compatibility reference records this stock-SDK compatibility
  decision.

## Fail-Before Evidence

The three new S3 contract tests failed against the original implementation:

```text
cargo test -p nimbus-s3 -- --nocapture

25 passed; 3 failed

list_objects_v2_continuation_returns_every_key_exactly_once
listed keys: ["page/a", "page/b", "page/d"]

conditional_delete_checks_etag_size_and_last_modified_before_deleting
a stale ETag delete succeeded and removed the object

multipart_completion_recomputes_or_rejects_crc64nvme
completion accepted and persisted "AAAAAAAAAAA=" without recomputation
```

## Verification

```text
cargo test -p nimbus-s3 -- --nocapture
29 passed; 0 failed

cargo test -p nimbus-engine \
  object_meta_conditional_delete_is_decided_before_sequence_assignment \
  -- --nocapture
1 passed; 0 failed

cargo test -p nimbus-engine object_meta -- --nocapture
4 passed; 0 failed

cargo test -p nimbus-engine objects:: -- --nocapture
8 passed; 0 failed

cargo check -p nimbus-engine -p nimbus-server
PASS

cargo clippy -p nimbus-s3 -p nimbus-engine -p nimbus-server \
  --all-targets -- -D warnings
PASS; vendored dependency warnings only

cargo fmt --all --check
PASS

bash scripts/check-docs.sh
PASS; 109 pages

bash scripts/verify-nimbus-docs-site.sh
PASS; 17 tests

make test-rust-docs verify-harness build-js typecheck-js test-js proof-helpers
PASS

Nimbus autoreview --gate pre-pr --mode auto
PASS; no accepted or actionable findings
```

`make ci` passed format, workspace Clippy, dependency audit, the Node anchor
gate, and 517 runtime tests. The broad nextest lane then reported one failure
after 6,450 passing tests: the MongoDB spec child server exited before opening
its listener. The exact test passed on an immediate focused rerun:

```text
NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 \
  cargo test -p nimbus-server --test mongodb_spec \
  spec_executor_crud_execution_report -- --nocapture

1 passed; 0 failed
```

This is an unrelated listener-startup flake under the 7,592-test broad load,
not an SA1 contract failure. PR #311 merged with no required hosted check
failure. The Band SA final gate must run the required repository checks on
merged main.
