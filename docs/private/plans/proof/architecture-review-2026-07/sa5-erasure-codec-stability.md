# SA5 NBLE1 Erasure Codec Stability

Date: 2026-08-26  
Baseline: `9fd80eef1`  
Work commit: `a422b4ae7`  
Merge commit: `ad43a052c97372eac12dc6bb08e2347c967b9eda`  
Pull request: #323

## Result

NBLE1 now records its exact erasure-codec commitment.

- `reed-solomon-simd` remains pinned to exact version `3.1.0`, with a warning
  that an output change requires a deliberate NBLE2 format change.
- The manifest and encode seams state why parity bytes are durable: manifests
  persist parity hashes, and heal must re-encode the same bytes.
- A checked-in k=3, m=2 golden vector fixes both parity shards for a 65-byte
  payload. Its padded shard size is 22 bytes, so it covers the accepted Nimbus
  boundary that is not divisible by 64.
- A dependency update that changes either parity shard now fails before it can
  strand repair of NBLE1 manifests.

## Fail-Before Evidence

The golden test first ran against an intentionally empty fixture and failed
with both actual parity shards:

```text
988e2f48b4fe5148e7d429f4f9fdf9fdc5c1cbcf2d06
a2b51b7d9ad17961c5f735e9efeae9eccfcacfca5338
```

The final test compares those exact bytes to the checked-in fixture. A codec
output change cannot pass by re-encoding its own expected value in-process.

## Verification

```text
cargo test -p nimbus-blob nble1_parity_bytes_match_golden_vector
PASS

cargo test -p nimbus-blob -- --nocapture
249 passed; 0 failed

cargo clippy -p nimbus-blob --all-targets -- -D warnings
PASS

cargo fmt --all --check
PASS

Nimbus autoreview --gate pre-pr --mode auto
PASS; no accepted or actionable findings; Trufflehog clean

PATH=/opt/homebrew/opt/node@24/bin:$PATH \
  CARGO_TARGET_DIR=/Users/jack/src/github.com/nimbus/nimbus/target \
  make ci
Lint, dependency policy, and 517 runtime tests passed. The workspace lane ran
7,647 tests: 7,646 passed and one unrelated MongoDB execution-report test
failed intermittently.

cargo nextest run -p nimbus-server --test mongodb_spec \
  spec_executor_crud_execution_report --no-capture
1 passed; 22 filtered

make test-rust-docs verify-harness build-js typecheck-js test-js proof-helpers
PASS; Rust doc tests, required harness, JavaScript build and type checks, 832 UI
tests, package tests, and proof helpers completed with exit 0
```
