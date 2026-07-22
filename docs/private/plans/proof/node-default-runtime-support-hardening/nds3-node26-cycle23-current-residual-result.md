# NDS3 node26 cycle 23 - Current residual promotion and triage

Date: 2026-06-15
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

This wave drained the Node26 Current broad pre-run residual from `34` required
gaps to `15` required gaps. Node26 `v8_isolate_required` posture moved from
`140` gaps / `93.34%` to `121` gaps / `94.23%`.

Movement came from:

- 13 dynamically promoted Node26 Current residual fixtures.
- 6 source-confirmed reclassifications out of `v8_isolate_required` because the
  official Node26 fixtures self-skip for host memory or BoringSSL/provider
  boundaries.
- A new ignored `node26_current_lane_broad_residual_watchpoint` so the remaining
  Current-lane residual has a repeatable broad selector.

No Deno fork tag changed in this wave. Nimbus remained pinned to immutable
`nimbus/deno` tag `v2.8.3-nimbus.56`.

## Broad Batch

The first broad Current residual run selected all 34 remaining
`node26_current_broad_pre_run_residual` fixtures:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-current-residual-cycle23-broad1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_broad_residual_watchpoint -- --ignored --nocapture
# selected=34, passed=13, skipped=6, failed=15
```

The 13 green fixtures were promoted into
`NODE26_CURRENT_RESIDUAL_PROMOTED_PATHS` and proven in a non-ignored promoted
batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-current-residual-cycle23-promoted1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_current_residual_promoted_batch_fixture -- --nocapture
# selected=13, passed=13, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 955 filtered out
```

Promoted paths:

- `test/parallel/test-fs-opendir.js`
- `test/parallel/test-fs-promises-file-handle-dispose.js`
- `test/parallel/test-fs-promises-file-handle-readLines.mjs`
- `test/parallel/test-fs-symlink.js`
- `test/parallel/test-fs-write-stream-autoclose-option.js`
- `test/parallel/test-module-multi-extensions.js`
- `test/parallel/test-readline-promises-csi.mjs`
- `test/parallel/test-stream-compose.js`
- `test/parallel/test-stream-pipeline.js`
- `test/parallel/test-stream-readable-emittedReadable.js`
- `test/parallel/test-stream-readable-infinite-read.js`
- `test/parallel/test-stream-typedarray.js`
- `test/parallel/test-stream-uint8array.js`

After promotion, the broad selector shrank to the expected 21 fixtures:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-current-residual-cycle23-broad2 \
  cargo test -p nimbus-runtime --lib node26_current_lane_broad_residual_watchpoint -- --ignored --nocapture
# selected=21, passed=0, skipped=6, failed=15
```

## Source-Confirmed Reclassifications

The six skipped fixtures were read from the official Node26 vendored source
before reclassification:

- `test/parallel/test-buffer-tostring-rangeerror.js` calls
  `common.skip('skipped due to memory requirements')` when
  `common.enoughTestMem` is false. It is host-memory stress evidence, not a
  default isolate support claim.
- `test/parallel/test-crypto-default-shake-lengths-oneshot.js` skips when
  `process.features.openssl_is_boringssl` is true because default SHAKE XOF
  lengths are unsupported by the linked provider.
- `test/parallel/test-crypto-dh-group-setters.js`,
  `test/parallel/test-crypto-dh-modp2-views.js`, and
  `test/parallel/test-crypto-dh-modp2.js` skip when
  `process.features.openssl_is_boringssl` is true because their
  Diffie-Hellman group / MODP2 surfaces are unsupported by the linked provider.
- `test/parallel/test-crypto-oneshot-hash-xof.js` skips when
  `process.features.openssl_is_boringssl` is true because BoringSSL does not
  support XOF hash functions.

After reclassification, the broad selector contains only the 15 failing
fixtures:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-current-residual-cycle23-broad3 \
  cargo test -p nimbus-runtime --lib node26_current_lane_broad_residual_watchpoint -- --ignored --nocapture
# selected=15, passed=0, skipped=0, failed=15
```

## Remaining Failure Groups

Current residual failures after this wave:

- Buffer encoding semantics:
  `test/parallel/test-buffer-indexof.js` fails with
  `TypeError [ERR_UNKNOWN_ENCODING]: Unknown encoding: 3`.
- Crypto provider/error-shape semantics:
  `test/parallel/test-crypto-dh.js`,
  `test/parallel/test-crypto-gcm-implicit-short-tag.js`, and
  `test/parallel/test-crypto-scrypt.js`.
- Filesystem sandbox/root traversal:
  `test/parallel/test-fs-glob.mjs` tries to stat `/` and is denied by the
  runtime read-capability boundary.
- HTTP/2 and TLS session semantics:
  `test/parallel/test-http2-misbehaving-flow-control-paused.js`,
  `test/parallel/test-http2-misbehaving-flow-control.js`,
  `test/parallel/test-http2-options-max-headers-exceeds-nghttp2.js`, and
  `test/parallel/test-https-agent-session-reuse.js`.
- Process/env permission shape:
  `test/parallel/test-process-load-env-file.js` expects Node's restricted API
  error text for one denied path but receives an `EACCES`-shaped file error.
- Node test runner API:
  `test/parallel/test-runner-get-test-context.js` fails because
  `getTestContext` is not exported as a function.
- Stream warning semantics:
  `test/parallel/test-stream-duplex.js` misses an expected warning callback.
- Trace-events binding surface:
  `test/parallel/test-trace-events-dynamic-enable.js` fails because `trace` is
  not a function.
- URL and dotenv/env parsing:
  `test/parallel/test-url-parse-invalid-input.js` and
  `test/parallel/test-util-parse-env.js`.

## Generator and Integrity Checks

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
# wrote node20, node22, node24, node26 classification catalogs

/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py sync
# wrote tests/runtime/node/expectations/rust-watchpoints.json

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

git diff --check
# no output

cargo fmt --all --check
# no output

/opt/homebrew/bin/python3.12 -B scripts/runtime/node/default_support_posture.py --check
# node default support posture: pass

/opt/homebrew/bin/python3.12 -B scripts/runtime/node/required_surface_blockers.py --check
# node required-surface blocker inventory: pass

/opt/homebrew/bin/python3.12 -B scripts/runtime/node/watchpoints.py validate
# validated node-compat watchpoint catalog: 151 entries
```

Current posture after regeneration:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`, `2363 / 2363`.
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`, `2400 / 2400`.
- Node26 `v8_isolate_required`: `121` gaps, `94.23%`, `1976 / 2097`.

Verifier:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 14 passed, 20 failed
```

Step 9 remains green for Node22/Node24. The overall verifier remains red
honestly because Node26 still has `121` required gaps and final closeout rows
are not complete.

## Diagnostics

Useful diagnostic roots from this wave:

- `/private/tmp/nds-node26-unpromoted-parallel-cycle23-broad1`
- `/private/tmp/nds-node26-current-residual-cycle23-broad1`
- `/private/tmp/nds-node26-current-residual-cycle23-promoted1`
- `/private/tmp/nds-node26-current-residual-cycle23-broad2`
- `/private/tmp/nds-node26-current-residual-cycle23-broad3`

Summary artifacts:

- `/private/tmp/nds-node26-current-residual-cycle23-broad1/batch/node26__node26_current_lane_broad_residual_watchpoint__summary.json`
- `/private/tmp/nds-node26-current-residual-cycle23-promoted1/batch/node26__node26_current_lane_executes_current_residual_promoted_batch__summary.json`
- `/private/tmp/nds-node26-current-residual-cycle23-broad2/batch/node26__node26_current_lane_broad_residual_watchpoint__summary.json`
- `/private/tmp/nds-node26-current-residual-cycle23-broad3/batch/node26__node26_current_lane_broad_residual_watchpoint__summary.json`

## Remaining Node26 Required Gaps

After regeneration, Node26 has `121` required gaps:

- `40` `node-compat/unpromoted-surface`
- `23` `loader-context/vm`
- `18` `loader-context/domain`
- `15` `node-compat/current-lane`
- `7` `runtime/v8`
- `6` `process-and-timing/process-host`
- `5` `streams-local-io/fs-host-io`
- `4` `core-semantics/console`
- `2` `core-semantics/assert`
- `1` `core-semantics/url`

Recommended next action is a Deno/Nimbus implementation batch over the 15-path
Current residual failure catalog, especially the low-risk polyfill items:
buffer numeric encoding, `util.parseEnv()` null-prototype output,
`node:test` `getTestContext`, stream warning shape, and URL invalid-input
behavior. The fs-glob root traversal path needs sandbox-boundary care before any
classification or implementation change.

## Integrity

- No V8 or rusty_v8 changes were made.
- No official upstream Node fixture or checker was edited.
- No generated JSON was hand-edited to fake a green result.
- No local Deno path pin was introduced in `Cargo.toml` or `Cargo.lock`.
- `measure_ah.sh` and other scratch files remain untracked.
