# NDS3 Cycle 91 - WebCrypto Sign/Verify Slow-Budget Promotion

Date: 2026-06-14

## Scope

Promoted `test/parallel/test-webcrypto-sign-verify.js` in both required lanes:
Node22 and Node24.

Fork tag remained `nimbus/deno` `v2.8.3-nimbus.38`
(`ced4fb1626 node: snapshot CommonJS exports for ESM wrappers`). This cycle did
not publish a new Deno fork tag.

Nimbus pin remained on Deno-family crates at `v2.8.3-nimbus.38` and
`nimbus/rusty_v8` remained on `v149.4.0-nimbus.1`; no V8 or rusty_v8 code was
changed. No upstream Node fixture or checker was edited. No generated posture
JSON was hand-edited.

## Harness Fix

The official fixture is a broad WebCrypto sign/verify matrix covering:

- RSASSA-PKCS1-v1_5
- RSA-PSS with a 4096-bit key
- ECDSA P-384
- HMAC
- Ed25519
- Ed448

After the prior Deno fork WebCrypto primitive work, the fixture no longer failed
a semantic assertion; prior diagnostics showed it exceeding the default 35s
harness wall-clock budget. Cycle 91 gives this fixture the same finite 120s
execution budget already used for the broad WebCrypto wrap/unwrap matrix, while
keeping the harness wall-clock cap finite at 125s.

## Dynamic Proof

Promotion guard on the current published Deno tag:

```text
$ gtimeout -s KILL 150 env CARGO_NET_OFFLINE=true \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle91-webcrypto-sign-verify-promotion-tag38-1 \
  cargo test -p nimbus-runtime --lib cycle91_webcrypto_sign_verify -- --nocapture

node_compat node22-supported-lane-executes-cycle91-webcrypto-sign-verify-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node22-supported-lane-executes-cycle91-webcrypto-sign-verify-batch node22 summary artifact: /Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle91-webcrypto-sign-verify-promotion-tag38-1/batch/node22__node22_supported_lane_executes_cycle91_webcrypto_sign_verify_batch__summary.json
node_compat node24-default-lane-executes-cycle91-webcrypto-sign-verify-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle91-webcrypto-sign-verify-batch node24 summary artifact: /Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle91-webcrypto-sign-verify-promotion-tag38-1/batch/node24__node24_default_lane_executes_cycle91_webcrypto_sign_verify_batch__summary.json
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 903 filtered out; finished in 70.52s
```

The retained batch summaries confirm:

```text
node22: selected=1, passed=1, skipped=0, failed=0
node24: selected=1, passed=1, skipped=0, failed=0
```

## Regeneration

The classification/posture pipeline was regenerated with:

```text
$ /opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
$ for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do /opt/homebrew/bin/python3.12 scripts/runtime/node/$s.py >/dev/null; done
```

Regenerated counts:

```text
node22 gaps = 6, pass_rate_percent = 99.75
node24 gaps = 8, pass_rate_percent = 99.67
```

Generator and formatting checks:

```text
$ /opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --preserve-existing --check
node20.json is up to date
node22.json is up to date
node24.json is up to date
node26.json is up to date

$ /opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py --check
node default support posture: pass

$ /opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py --check
node required-surface blocker inventory: pass

$ cargo fmt --all --check
pass

$ git diff --check
pass
```

## Diagnostics

Diagnostic roots retained:

- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle85-webcrypto-focused-local-final/general/node22__test_parallel_test_webcrypto_sign_verify_js.json`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle87-webcrypto-signverify-node24-tag-probe/general/node24__test_parallel_test_webcrypto_sign_verify_js.json`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle91-webcrypto-sign-verify-promotion-tag38-1`

The prior roots captured the old timeout-only state:

```text
node22: runtime execution timed out after 30s, elapsed_ms=34101
node24: wall_clock_timeout, elapsed_ms=35002
```

## Remaining Gate

Remaining required gaps after this cycle:

```text
node22 (6):
test/es-module/test-esm-dynamic-import-commonjs.js
test/es-module/test-esm-dynamic-import-commonjs.mjs
test/es-module/test-esm-dynamic-import.js
test/es-module/test-esm-loader-mock.mjs
test/es-module/test-esm-virtual-json.mjs
test/parallel/test-vm-module-import-meta.js

node24 (8):
test/es-module/test-esm-dynamic-import-commonjs.js
test/es-module/test-esm-dynamic-import-commonjs.mjs
test/es-module/test-esm-dynamic-import.js
test/es-module/test-esm-loader-mock.mjs
test/es-module/test-esm-require-race-condition.js
test/es-module/test-esm-virtual-json.mjs
test/parallel/test-vm-module-hastoplevelawait.js
test/parallel/test-vm-module-import-meta.js
```

The gate remains red and honest; this cycle removes one Node22 and one Node24
V8-isolate-required gap.
