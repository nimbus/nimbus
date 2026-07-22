# NDS3 Node26 Cycle 18: process/timing and async residual promotion

## Scope

This checkpoint promotes a dynamically green Node26 Current batch from the
remaining process/timing and unpromoted async/internal residuals. It adds a
Node26 broad process/timing watchpoint, then promotes the green subset proven by
that broad run:

- all `10` remaining `process-and-timing/perf-hooks` required gaps
- `6` trace-events fixtures from `node-compat/unpromoted-surface`
- `1` async-local-storage fixture from `node-compat/unpromoted-surface`

No V8 or rusty_v8 changes were made. No official upstream fixture or checker was
edited. No Deno fork changes or local Deno pins were used in this cycle. Nimbus
remained pinned to the published immutable Deno tag `v2.8.3-nimbus.50`.

Before this wave, Node26 `v8_isolate_required` posture was `243` gaps /
`88.46%`.

## Broad Diagnostics

Unpromoted parallel residual batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-unpromoted-residual-wave2 \
  cargo test -p nimbus-runtime --lib node26_current_lane_unpromoted_parallel_discovery_watchpoint -- --ignored --nocapture
```

Result:

- selected: `35`
- passed: `32`
- skipped: `0`
- failed: `3`
- required green promoted in this cycle:
  `test/parallel/test-async-local-storage-run-scope.js`
- failures:
  `test/parallel/test-async-local-storage-weak-asyncwrap-leak.js`,
  `test/parallel/test-eventsource.js`,
  `test/parallel/test-structuredClone-global.js`
- summary:
  `/private/tmp/nds-node26-unpromoted-residual-wave2/batch/node26__node26_current_lane_unpromoted_parallel_discovery_watchpoint__summary.json`

Process/timing residual batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-process-timing-residual-wave2 \
  cargo test -p nimbus-runtime --lib node26_current_lane_process_timing_runtime_residual_watchpoint -- --ignored --nocapture
```

Result:

- selected: `56`
- passed: `54`
- skipped: `0`
- failed: `2`
- required greens promoted in this cycle:
  all `10` remaining `process-and-timing/perf-hooks` paths plus the six
  green trace-events paths listed below
- failures:
  `test/parallel/test-trace-events-api.js` and
  `test/parallel/test-trace-events-dynamic-enable.js`
- failure root causes:
  `test-trace-events-api.js` still expects a trace output file that the current
  isolate runtime does not create; `test-trace-events-dynamic-enable.js` still
  sees `internalBinding('trace_events').trace` as non-callable
- summary:
  `/private/tmp/nds-node26-process-timing-residual-wave2/batch/node26__node26_current_lane_process_timing_runtime_residual_watchpoint__summary.json`

The promoted trace-events required paths are:

- `test/parallel/test-trace-events-all.js`
- `test/parallel/test-trace-events-async-hooks.js`
- `test/parallel/test-trace-events-file-pattern.js`
- `test/parallel/test-trace-events-get-category-enabled-buffer.js`
- `test/parallel/test-trace-events-http.js`
- `test/parallel/test-trace-events-v8.js`

## Promotion Proof

Process/timing promoted batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-process-timing-residual-wave2-promoted \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_process_timing_runtime_residual_promoted_batch_fixture -- --nocapture
```

Result:

- selected: `26`
- passed: `26`
- skipped: `0`
- failed: `0`
- summary:
  `/private/tmp/nds-node26-process-timing-residual-wave2-promoted/batch/node26__node26_current_lane_executes_process_timing_runtime_residual_promoted_batch__summary.json`

Unpromoted parallel promoted batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-unpromoted-residual-wave2-promoted \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_unpromoted_parallel_discovery_promoted_batch_fixture -- --nocapture
```

Result:

- selected: `32`
- passed: `32`
- skipped: `0`
- failed: `0`
- summary:
  `/private/tmp/nds-node26-unpromoted-residual-wave2-promoted/batch/node26__node26_current_lane_executes_unpromoted_parallel_discovery_promoted_batch__summary.json`

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
  `validated node-compat watchpoint catalog: 150 entries`
- `docs/private/architecture/runtime/node-default-support-posture.json`:
  Node26 `v8_isolate_required` is `226` gaps / `89.26%`
- tracked public evidence:
  Node26 full official corpus manifested green count is `1879 / 5578`

Posture after regeneration:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`
- Node26 `v8_isolate_required`: `226` gaps, `89.26%`
- Node26 required passed: `1879 / 2105`

This wave moves Node26 from `243` gaps / `88.46%` to `226` gaps / `89.26%`,
burning 17 required-surface gaps.

## Verifier

Command:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
```

Result:

- Summary: `14 passed, 20 failed`.
- Step 9 passed: Node22 and Node24 V8-isolate-required fixtures are `100%`.
- Step 11 remains failed because Node26 Current evidence is still incomplete:
  Node26 is `1879` official passes and `226` required gaps, not `0` gaps /
  `100.0%`.
- The remaining verifier failures are honest red closeout/proof gaps in this
  checkout; this cycle does not claim full NDS completion.

## Integrity Checks

Commands:

```bash
cargo fmt --all --check
git diff --check
```

Results:

- `cargo fmt --all --check`: passed.
- `git diff --check`: passed.

## Remaining Node26 Required Buckets

After this wave:

- `92` `node-compat/unpromoted-surface`
- `34` `node-compat/current-lane`
- `23` `loader-context/vm`
- `20` `loader-context/module`
- `18` `loader-context/domain`
- `15` `streams-local-io/fs-host-io`
- `7` `process-and-timing/process-host`
- `7` `runtime/v8`
- `4` `core-semantics/console`
- `3` `loader-context/util`
- `2` `core-semantics/assert`
- `1` `core-semantics/url`

Recommended next wave: the largest remaining single owner is still the mixed
`node-compat/unpromoted-surface` bucket, but the highest implementation leverage
may come from clustered owners with coherent root causes: fs/FileHandle, process
active resources plus `stream/iter`, or the ESM/module loader residuals. The two
trace-event failures should remain out of promotion until the trace output and
dynamic-enable APIs are fixed or honestly reclassified.
