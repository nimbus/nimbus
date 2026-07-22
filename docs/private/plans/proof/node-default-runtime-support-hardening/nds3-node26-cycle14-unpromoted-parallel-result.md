# NDS3 Node26 Cycle 14: unpromoted parallel discovery promotion

## Scope

This checkpoint burns Node26 Current required-surface gaps from the residual
`node-compat/unpromoted-surface` `test/parallel` discovery cluster. It extends
the existing node22/node24 unpromoted-parallel broad discovery guard to Node26,
runs the ignored broad batch with the same host/native/CLI/stress/fatal-family
exclusions, and promotes only the dynamically green Node26 paths into a
non-ignored batch.

No V8 or rusty_v8 changes, fixture edits, checker edits, Deno fork edits, local
Deno pins, or generated false-green JSON hand edits were made in this cycle.
Nimbus remained pinned to the published immutable Deno tag
`v2.8.3-nimbus.49`.

Before this wave, Node26 `v8_isolate_required` posture was `458` gaps /
`79.17%`.

## Broad Pre-Run

Immutable `v2.8.3-nimbus.49` broad unpromoted-parallel pre-run:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-unpromoted-parallel-wave1-tag49 \
  cargo test -p nimbus-runtime --lib node26_current_lane_unpromoted_parallel_discovery_watchpoint -- --ignored --nocapture
```

Result:

- Rust test result: failed, as expected for the broad diagnostic batch.
- Fixture summary: `selected=35`, `passed=31`, `skipped=0`, `failed=4`.
- Summary artifact:
  `/private/tmp/nds-node26-unpromoted-parallel-wave1-tag49/batch/node26__node26_current_lane_unpromoted_parallel_discovery_watchpoint__summary.json`
- Diagnostic root:
  `/private/tmp/nds-node26-unpromoted-parallel-wave1-tag49`

Failed paths:

- `test/parallel/test-async-local-storage-run-scope.js`
- `test/parallel/test-async-local-storage-weak-asyncwrap-leak.js`
- `test/parallel/test-eventsource.js`
- `test/parallel/test-structuredClone-global.js`

The selector intentionally excludes already-owned host/native/CLI/stress/fatal
families and separate coherent clusters such as fs, http, module, stream,
trace, util, webcrypto, and QUIC. The 31 broad passes became the only promotion
candidates.

## Promotion Proof

Immutable `v2.8.3-nimbus.49` enforced promoted batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-unpromoted-parallel-wave1-tag49-promote1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_unpromoted_parallel_discovery_promoted_batch_fixture -- --nocapture
```

Result:

- Rust test result: `1 passed; 0 failed; 0 ignored`.
- Fixture summary: `selected=31`, `passed=31`, `skipped=0`, `failed=0`.
- Summary artifact:
  `/private/tmp/nds-node26-unpromoted-parallel-wave1-tag49-promote1/batch/node26__node26_current_lane_executes_unpromoted_parallel_discovery_promoted_batch__summary.json`

## Promoted Fixtures

The 31 promoted paths are the exact entries added to
`UNPROMOTED_PARALLEL_DISCOVERY_PROMOTED_NODE26_PATHS` in
`crates/nimbus-runtime/src/runtime/tests/node/cases/watchpoints_extended.rs`.
They cover the dynamically green async-local-storage, async-wrap,
AsyncResource, beforeExit, internal binding/helper, MessageEvent,
NodeEventTarget, require/process, require.resolve, source map, stringbytes,
perf_hooks JSON, and TLS deprecation warning fixtures from this discovery
slice.

The new non-ignored Rust test
`node26_current_lane_executes_unpromoted_parallel_discovery_promoted_batch_fixture`
now enforces those 31 paths against the Node26 lane with the existing
unpromoted-parallel extra directories.

## Remaining Failure Buckets

The broad run preserved diagnostics for all four failures:

- `test-async-local-storage-run-scope.js`: Node26 expects
  `AsyncLocalStorage.prototype.withScope`; current Deno fork surface does not
  provide it.
- `test-async-local-storage-weak-asyncwrap-leak.js`: async-wrap weak leak
  cleanup shape differs; observed `[] !== 0`.
- `test-eventsource.js`: Node26 expects global `EventSource` to be a function;
  current runtime reports `undefined`.
- `test-structuredClone-global.js`: error-message compatibility residual:
  current message says `can not` where Node26 expects `cannot`.

These four fixtures remain required-surface gaps until a focused implementation
fix or honest source-confirmed reclassification proves otherwise.

## Generated Evidence

Commands:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py sync
/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py validate
/opt/homebrew/bin/python3.12 scripts/runtime/node/status.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/dashboard.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/trends.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/publish_evidence.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py
```

Results:

- `scripts/runtime/node/watchpoints.py validate`:
  `validated node-compat watchpoint catalog: 147 entries`
- `scripts/runtime/node/required_surface_blockers.py`:
  `node22 required gaps: 0`, `node24 required gaps: 0`
- `tests/runtime/node/compat/node-compat-evidence/latest/status-summary.json`:
  Node26 official manifested green count is `1772 / 5578`.

Posture after regeneration:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`
- Node26 `v8_isolate_required`: `427` gaps, `80.58%`

The Node26 count moved from `458` gaps / `79.17%` to `427` gaps /
`80.58%`, burning 31 required-surface gaps in this wave. The official fixture
evidence count for Node26 moved from `1741 / 5578` to `1772 / 5578`.

The largest remaining Node26 required-surface bucket after this cycle is still
`node-compat/unpromoted-surface`, now `243` gaps.

## Verifier

Command:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
```

Result:

- Summary: `14 passed, 20 failed`.
- Step 9 passed: Node22 and Node24 V8-isolate-required fixtures are `100%`.
- Step 11 remains failed because Node26 Current evidence is still incomplete:
  Node26 is `1772` official passes and `427` required gaps, not `0` gaps /
  `100.0%`.
- The remaining verifier failures are honest red closeout/proof gaps in this
  checkout; this cycle does not claim full NDS completion.

## Next Node26 Work

Node26 remains at `427` required gaps. A fresh ROI scan should prefer the
largest coherent cluster under the remaining `node-compat/unpromoted-surface`
bucket, then the smaller root-cause groups:
`node26_current_broad_pre_run_residual` (`34`), `process-host` (`33`), `vm`
(`23`), `module` (`20`), and `domain` (`18`).
