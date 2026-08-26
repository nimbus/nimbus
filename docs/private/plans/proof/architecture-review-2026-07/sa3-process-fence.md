# SA3 Engine Process Fence

Date: 2026-08-25  
Baseline: `80f8459522f94c3c7e4c2ee545c825cb487598e9`  
Work commit: `4a38d3bf4ccf4d345325957ae4ac8a68fe6f3788`  
Merge commit: `c4f9a29a3875c29b05e0763df612aad53670a407`  
Pull request: #312

## Result

Every Engine bootstrap now acquires an exclusive advisory lock for each
distinct canonical local persistence, control-plane, or replica-cache root
before it opens a provider.

- Canonicalization, sorting, and deduplication make aliases one ownership
  domain and give multi-root bootstraps a deterministic acquisition order.
- A contending Engine fails immediately with
  `StorageErrorKind::Busy`. It does not wait or select a weaker mode.
- The Engine retains all lock files for its full lifetime and drops provider
  and executor fields before it releases the locks.
- The fence covers embedded redb and SQLite, encrypted redb's custom file
  backend, remote-provider local control roots, and libSQL replica caches.
- The change adds no committer lease or fourth mutation path.

## Fail-Before Evidence

The fresh-process regression test failed against the original implementation.
The parent and child both opened the same encrypted-redb root, and the child
then failed its assertion that another live process owned the root:

```text
cargo test -p nimbus-engine --test process_fence \
  encrypted_embedded_engine_refuses_a_second_process_on_the_same_root \
  -- --nocapture

child panic: a live Engine in another process must own the root
parent panic: fresh process did not observe the live Engine fence
```

## Verification

```text
cargo test -p nimbus-engine --test process_fence -- --nocapture
1 passed; 0 failed; 1 ignored subprocess entry point

NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 \
  cargo test -p nimbus-engine -- --nocapture
unit: 694 passed; 0 failed; 5 ignored
process-fence integration: 1 passed; 0 failed; 1 ignored

NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 \
  cargo test -p nimbus-engine --no-default-features
unit: 627 passed; 0 failed; 5 ignored
process-fence integration: 1 passed; 0 failed; 1 ignored

cargo clippy -p nimbus-engine --all-targets -- -D warnings
PASS; vendored dependency warnings only

cargo fmt --all --check
PASS

bash scripts/check-docs.sh
PASS; 109 pages

npm --prefix website run build
PASS; 110 pages built

bash scripts/verify-nimbus-docs-site.sh
PASS; 17/17 conditions

make ci
PASS; 7,595-test nextest roster, required verification harness, JavaScript
build/typecheck/tests, and proof helpers completed with exit 0

Nimbus autoreview --gate pre-pr --mode auto
PASS; no accepted or actionable findings
```

PR #312 merged after the local full gate and clean autoreview. The process
fence is now the single-node zombie-writer boundary required by Band SA.
