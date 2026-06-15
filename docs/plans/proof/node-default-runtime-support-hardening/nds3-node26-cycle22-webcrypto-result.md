# NDS3 node26 cycle 22 - WebCrypto broad promotion

Date: 2026-06-15
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

This wave promoted 27 Node26 WebCrypto required-surface fixtures after proving
the broad WebCrypto selector against immutable Deno fork tags. Node26
`v8_isolate_required` posture moved from `167` gaps / `92.06%` to `140` gaps /
`93.34%`.

Movement came from:

- 27 dynamically promoted Node26 WebCrypto fixtures.
- A Deno fork WebCrypto compatibility batch in `v2.8.3-nimbus.55`.
- A corrective Deno fork slot update in `v2.8.3-nimbus.56` after the first
  immutable promoted proof caught a `wrapKey` / `unwrapKey` regression.

Deno fork tag: `v2.8.3-nimbus.56`
Commit: `e352a65f709c66be9fe0f745fc3bb09306b97857`

Nimbus was temporarily pinned to the canonical local Deno worktree while
proving the corrective `unwrapKey` slot fix, then repinned to the immutable
published `v2.8.3-nimbus.56` tag before the final broad and promoted proofs.

## Root Cause

The Node26 WebCrypto cluster exposed missing Deno fork behavior across several
Node26 WebCrypto surfaces:

- `CryptoKey` needed Node-compatible internal hidden-slot behavior for clone,
  brand, own-symbol, and internal-slot checks.
- ML-DSA sign/verify and import/export needed AWS-LC-backed key and signature
  handling rather than falling through to unsupported behavior.
- TurboSHAKE digest fixtures needed Node-compatible error and digest handling.
- CFRG, ECDH, HKDF, ML-KEM, RSA, ECDSA, EdDSA, HMAC, and ChaCha20-Poly1305
  WebCrypto fixtures were green once the fork carried the upstream-compatible
  WebCrypto behavior and Nimbus promoted the proven paths.
- The first immutable promoted proof on `v2.8.3-nimbus.55` caught a real slot
  refactor miss: `SubtleCrypto.unwrapKey()` still assigned to getter-backed
  `CryptoKey` symbol properties. `v2.8.3-nimbus.56` now mutates the
  `assertCryptoKey(result)` slot record for `extractable`, `usages`, and
  `publicUsages`.

The sandbox boundary stayed intact. This wave did not add host process exit,
OS signal handlers, subprocess execution, global host-cwd mutation, native
addon loading, or wider host filesystem grants.

## Promoted Fixtures

The following Node26 WebCrypto paths were promoted into
`WEBCRYPTO_PROMOTED_NODE26_PATHS`:

- `test/parallel/test-webcrypto-constructors.js`
- `test/parallel/test-webcrypto-cryptokey-brand-check.js`
- `test/parallel/test-webcrypto-cryptokey-clone-transfer.js`
- `test/parallel/test-webcrypto-cryptokey-hidden-slots.js`
- `test/parallel/test-webcrypto-cryptokey-no-own-symbols.js`
- `test/parallel/test-webcrypto-derivebits-cfrg.js`
- `test/parallel/test-webcrypto-derivebits-ecdh.js`
- `test/parallel/test-webcrypto-derivebits-hkdf.js`
- `test/parallel/test-webcrypto-derivekey-cfrg.js`
- `test/parallel/test-webcrypto-derivekey-ecdh.js`
- `test/parallel/test-webcrypto-digest-turboshake-rfc.js`
- `test/parallel/test-webcrypto-digest-turboshake.js`
- `test/parallel/test-webcrypto-digest.js`
- `test/parallel/test-webcrypto-encap-decap-ml-kem.js`
- `test/parallel/test-webcrypto-encrypt-decrypt-chacha20-poly1305.js`
- `test/parallel/test-webcrypto-export-import-cfrg.js`
- `test/parallel/test-webcrypto-export-import-ec.js`
- `test/parallel/test-webcrypto-export-import-ml-dsa.js`
- `test/parallel/test-webcrypto-export-import-ml-kem.js`
- `test/parallel/test-webcrypto-export-import-rsa.js`
- `test/parallel/test-webcrypto-get-public-key.mjs`
- `test/parallel/test-webcrypto-internal-slots.mjs`
- `test/parallel/test-webcrypto-sign-verify-ecdsa.js`
- `test/parallel/test-webcrypto-sign-verify-eddsa.js`
- `test/parallel/test-webcrypto-sign-verify-hmac.js`
- `test/parallel/test-webcrypto-sign-verify-ml-dsa.js`
- `test/parallel/test-webcrypto-sign-verify-rsa.js`

`test/parallel/test-webcrypto-derivebits-argon2.js` remains unpromoted because
the official Node fixture self-skips on this host with `requires OpenSSL >= 3.2`.

## Verification

Immutable-tag broad proof on `v2.8.3-nimbus.55`:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-webcrypto-tag55-errors1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_webcrypto_required_gap_watchpoint -- --ignored --nocapture
# selected=28, passed=27, skipped=1, failed=0
# skipped: test/parallel/test-webcrypto-derivebits-argon2.js
```

First immutable-tag promoted proof on `v2.8.3-nimbus.55`, intentionally
retained as a false-tag guard:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-webcrypto-tag55-promote1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_webcrypto_promoted_batch_fixture -- --nocapture
# selected=42, passed=41, skipped=0, failed=1
# failed: test/parallel/test-webcrypto-wrap-unwrap.js
```

The failing diagnostic was:

```text
TypeError: Cannot set property Symbol([[extractable]]) of #<CryptoKey> which has only a getter
```

Corrective local proof against the canonical local Deno worktree:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-webcrypto-local-slotfix-promote1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_webcrypto_promoted_batch_fixture -- --nocapture
# selected=42, passed=42, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 953 filtered out
```

Corrected immutable-tag broad proof on `v2.8.3-nimbus.56`:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-webcrypto-tag56-errors1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_webcrypto_required_gap_watchpoint -- --ignored --nocapture
# selected=28, passed=27, skipped=1, failed=0
# skipped: test/parallel/test-webcrypto-derivebits-argon2.js
# test result: ok. 1 passed; 0 failed; 953 filtered out
```

Corrected immutable-tag promoted proof on `v2.8.3-nimbus.56`:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-webcrypto-tag56-promote1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_webcrypto_promoted_batch_fixture -- --nocapture
# selected=42, passed=42, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 953 filtered out
```

Generator and integrity checks:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
# wrote node20, node22, node24, node26 classification catalogs

/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py sync
# wrote tests/runtime/node/expectations/rust-watchpoints.json

/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py validate
# validated node-compat watchpoint catalog: 150 entries

/opt/homebrew/bin/python3.12 scripts/runtime/node/status.py
# wrote target/node-compat/status/status-summary.{json,md}

/opt/homebrew/bin/python3.12 scripts/runtime/node/dashboard.py
# wrote target/node-compat/dashboard/dashboard-summary.{json,md}

/opt/homebrew/bin/python3.12 scripts/runtime/node/trends.py
# wrote target/node-compat/trends/trend-summary.{json,md}

/opt/homebrew/bin/python3.12 scripts/runtime/node/publish_evidence.py
# published tests/runtime/node/compat/node-compat-evidence/latest/*

/opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py
# wrote private and public node-default-support-posture artifacts

/opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py
# node22 required gaps: 0
# node24 required gaps: 0
```

Current posture after regeneration:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`, `2363 / 2363`.
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`, `2400 / 2400`.
- Node26 `v8_isolate_required`: `140` gaps, `93.34%`, `1963 / 2103`.

Verifier:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 14 passed, 20 failed
```

Step 9 remains green for Node22/Node24. The overall verifier remains red
honestly because Node26 still has `140` required gaps and the final closeout
proof rows are not complete.

## Diagnostics

Useful diagnostic artifacts from this wave:

- `/private/tmp/nds-node26-webcrypto-tag55-errors1`
- `/private/tmp/nds-node26-webcrypto-tag55-promote1`
- `/private/tmp/nds-node26-webcrypto-local-slotfix-promote1`
- `/private/tmp/nds-node26-webcrypto-tag56-errors1`
- `/private/tmp/nds-node26-webcrypto-tag56-promote1`

Summary artifacts:

- `/private/tmp/nds-node26-webcrypto-tag55-errors1/batch/node26__node26_current_lane_webcrypto_required_gap_watchpoint__summary.json`
- `/private/tmp/nds-node26-webcrypto-tag55-promote1/batch/node26__node26_current_lane_executes_webcrypto_promoted_batch__summary.json`
- `/private/tmp/nds-node26-webcrypto-local-slotfix-promote1/batch/node26__node26_current_lane_executes_webcrypto_promoted_batch__summary.json`
- `/private/tmp/nds-node26-webcrypto-tag56-errors1/batch/node26__node26_current_lane_webcrypto_required_gap_watchpoint__summary.json`
- `/private/tmp/nds-node26-webcrypto-tag56-promote1/batch/node26__node26_current_lane_executes_webcrypto_promoted_batch__summary.json`

## Disk Cleanup During Checkpoint

The user flagged disk pressure during this wave. I drained the running
immutable-tag broad proof before cleaning anything, checked for live
Cargo/rustc/nextest/Nimbus proof processes, then reclaimed only PR-owned or
Deno-fork build cache:

```bash
cargo clean --manifest-path /Users/jack/src/github.com/nimbus/deno/Cargo.toml
# Removed 2961 files, 642.7MiB total

cargo clean
# from the NDS worktree
# Removed 25806 files, 11.3GiB total
```

I did not clean `/Users/jack/src/github.com/nimbus/nimbus/target` because it is
the main checkout target, not this PR worktree's target. The NDS worktree target
was intentionally allowed to rebuild for the final promoted proof.

## Remaining Node26 Required Gaps

After regeneration, Node26 has `140` required gaps. The Node22/Node24 required
blocker inventory is empty, and the generated Node26 posture records:

- Full corpus: `5578`
- Current passed: `1963`
- Required gaps: `140`
- Optional gaps: `512`
- Diagnostic non-isolate: `2065`
- Harness only: `814`
- Upstream/platform: `84`
- Remaining unpromoted: `0`

Recommended next action is a fresh ROI scan over the remaining Node26 required
classification set, with preference for coherent cluster waves rather than
single-fixture moves.

## Integrity

- No V8 or rusty_v8 changes were made.
- No official upstream Node fixture or checker was edited.
- No generated JSON was hand-edited to fake a green result.
- No local Deno path pin remains in `Cargo.toml` or `Cargo.lock`.
- Deno fork is clean on branch `nimbus/v2.8.3` at `v2.8.3-nimbus.56`.
- `measure_ah.sh` and other scratch files remain untracked.
