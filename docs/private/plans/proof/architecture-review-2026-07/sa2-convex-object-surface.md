# SA2 Convex Object Manifest Identity

Date: 2026-08-26  
Baseline: `f75ff877ed2fe376704fd2c21e26cf316bd63fcf`  
Work commit: `5f1ad7f6937d64a597423153106f3f6614940aa2`  
Merge commit: `dfa00606e59277dfe0d6c072cf2a11407f167171`  
Pull request: #322

## Result

Each `ConvexStorageId` now owns one deterministic internal manifest key.

- Concurrent imports for the same logical file replace one manifest slot. They
  cannot publish multiple live manifests for the same storage ID.
- Import receives the exact superseded manifest from the object commit
  authority and releases only blobs that the replacement does not retain.
- Get, metadata, URL verification, and delete use an exact manifest read or
  delete. They do not list the full bucket.
- Point reads validate that the manifest system metadata belongs to the
  requested `ConvexStorageId` before they return it.
- Export keeps its intentional full-bucket scan because it must enumerate all
  Convex objects.

## Fail-Before Evidence

The two new regressions failed against the original implementation:

```text
convex_storage_id_operations_use_point_manifest_access
expected 0 bucket scans; observed 5

concurrent_convex_imports_share_one_manifest_identity
expected 1 live manifest for the storage ID; observed 2
```

The original random per-import manifest key allowed both concurrent stores to
survive. Point operations found those manifests only through a bucket-wide
list.

## Verification

```text
cargo test -p nimbus-s3 convex_storage_id_operations_use_point_manifest_access
PASS

cargo test -p nimbus-s3 concurrent_convex_imports_share_one_manifest_identity
PASS

cargo test -p nimbus-s3 convex_
6 passed; 0 failed

cargo test -p nimbus-s3
31 passed; 0 failed

cargo clippy -p nimbus-s3 --all-targets -- -D warnings
PASS

cargo fmt --all --check
PASS

Nimbus autoreview --gate pre-pr --mode auto
PASS; no accepted or actionable findings

PATH=/opt/homebrew/opt/node@24/bin:$PATH \
  CARGO_TARGET_DIR=/Users/jack/src/github.com/nimbus/nimbus/target \
  make ci
PASS; Node 24.19.0, 517 runtime tests, 7,646 non-runtime tests across 84
binaries, required verification harness, JavaScript build/typecheck/tests, and
proof helpers completed with exit 0
```

The first isolated-cache CI attempt stopped when its duplicate 13 GiB target
filled the disk. The disposable worktree target was removed, and the complete
gate passed from a clean source tree against the repository shared target.

