# Process And Timing Failure Inventory

Status: `classified`

This file is the checked-in failure inventory for the currently manifested
`NLC4` process and timing subset.

## Node22 Upstream Slice Status

- Status: `green for the currently manifested subset`
- Current measured subset:
  - `48` official files passed
  - `0` failed
  - `0` current Node22 watchpoints inside the manifested subset
- Current green subset families:
  - `process` including `loadEnvFile()`
  - `nextTick`
  - core `timers`
  - `util` (`deprecate`, `format`, `inherits`, `parseEnv`, `MIMEType`,
    `MIMEParams`, `TextDecoder`, type presence)
  - `perf_hooks` user timing (`PerformanceMark`, `PerformanceMeasure`,
    `performance.mark()`, `clearMarks()`)
  - basic `diagnostics_channel`

Integrated ownership result:

- the previous Nimbus-local `util` numeric-separator and
  `diagnostics_channel` sync-unsubscribe shims were removed from
  `node22_runtime_bootstrap.js`
- the current green Node22 lane proves those shared semantics now hold from the
  Deno fork instead:
  - `nimbus/deno v2.7.14-locker.20` moved numeric-separator formatting
    and `diagnostics_channel` stable-subscriber iteration into the fork
  - `nimbus/deno v2.7.14-locker.21` added the `%s` +
    `Symbol.toPrimitive` `util.format()` fix
  - `nimbus/deno v2.7.14-locker.22` aligned `util.parseEnv()` with the
    official Node plain-object return shape and added a fork-side regression in
    `tests/unit_node/util_test.ts`
  - `nimbus/deno v2.7.14-locker.23`,
    `nimbus/deno v2.7.14-locker.24`, and
    `nimbus/deno v2.7.14-locker.25` moved the `node:perf_hooks`
    user-timing contract into the fork by exporting
    `PerformanceMark` / `PerformanceMeasure`, seeding the minimal
    `nodeTiming` marks the official file expects, and restoring Node-style
    Symbol coercion errors for `performance.mark()` / `clearMarks()` plus
    Node-style `ERR_INVALID_ARG_TYPE` validation for `startTime`
  - `nimbus/deno v2.7.14-locker.26`,
    `nimbus/deno v2.7.14-locker.27`, and
    `nimbus/deno v2.7.14-locker.28` moved the `node:perf_hooks`
    resource-timing contract into the fork by exporting
    `PerformanceResourceTiming`, implementing
    `performance.markResourceTiming()`, fixing the web-layer enumerable
    descriptor setup, and hiding the internal `nodeTiming` bootstrap marks
    from public `performance.getEntries*()` queries
  - the final `process.loadEnvFile()` promotion was intentionally Nimbus-local:
    `source.rs` now lets explicitly loaded env-file keys surface through a
    runtime-only overlay even though ambient host env stays capability-gated,
    `node22_runtime_bootstrap.js` now adds Node-style path/default/missing-file
    and permission shaping on top of Deno's base op, and
    `bootstrap/ops/test_runtime.rs` now snapshots/restores host `cwd` and
    process-global env around spawned child executions so the imported official
    file no longer pollutes later manifested fixtures

## Node20 Validation Slice Status

- Status: `green for the currently manifested validation subset`
- Current measured subset:
  - `46` official `nodejs/node v20.20.2` files passed
  - `0` failed
  - `2` explicit Node20 divergence watchpoints outside the green subset

### Explicit Node20 Divergence Watchpoint

- `test/parallel/test-process-features.js`
  - classification: `validation_lane_divergence`
  - reason: the single Nimbus `Node22` runtime contract intentionally keeps
    `process.features.typescript`, while the official Node20 file expects the
    older key set that does not include `typescript`
  - owner: Nimbus bootstrap/target-contract layer, not the Deno fork
  - evidence:
    `runtime::tests::node_compat::node20_process_features_watchpoint`
- `test/parallel/test-perf-hooks-resourcetiming.js`
  - classification: `validation_lane_divergence`
  - reason: the single Nimbus `Node22` runtime contract intentionally keeps
    the later `PerformanceResourceTiming#toJSON()` fields `deliveryType` and
    `responseStatus`, while official Node20 still expects the older shape that
    omits them
  - owner: Nimbus target-contract layer, not the Deno fork
  - evidence:
    `runtime::tests::node_compat::node20_perf_hooks_resourcetiming_watchpoint`

## Explicit Imported Watchpoints Outside The Green Subset

- `test/parallel/test-process-finalization.mjs`
  - classification: `later_family_dependency`
  - reason: the official wrapper now runs through the Nimbus sync subprocess
    harness and the direct official fixture bodies for `close.mjs`,
    `before-exit.mjs`, and `unregister.mjs` are green; the only remaining
    failure is `different-registry-per-thread.mjs`, which depends on
    `node:worker_threads` and therefore belongs to the later VM/worker family
    instead of the `NLC4` process/timing family
  - evidence:
    `runtime::tests::node_compat::node22_process_finalization_close_fixture`,
    `runtime::tests::node_compat::node22_process_finalization_before_exit_fixture`,
    `runtime::tests::node_compat::node22_process_finalization_unregister_fixture`,
    `runtime::tests::node_compat::node22_process_finalization_watchpoint`
## Node24 Preview Divergences

- Status: `supported-lane watchpoint; not a green support claim`
- Latest explicit supported-lane watchpoint run:
  - `45` passed
  - `3` failed
- Current supported-lane failures:
  - `test/parallel/test-process-features.js`
    - classification: `supported_lane_divergence`
    - reason: Nimbus still carries the current Node22-shaped
      `process.features` contract and does not yet expose
      `openssl_is_boringssl`
  - `test/parallel/test-util-deprecate.js`
    - classification: `supported_lane_divergence`
    - reason: the embedded `internalUtil.pendingDeprecate()` surface required
      by the Node24 file is not yet implemented
  - `test/parallel/test-util-format.js`
    - classification: `supported_lane_divergence`
    - reason: SharedArrayBuffer inspect output still prints `byteLength`
      instead of Node24's `[byteLength]` formatting

## `NLC4` Closeout Note

`NLC4` no longer has an unexplained in-scope runtime gap. The imported
official files for the family now break down into three honest buckets:

- the green Node22 manifested subset (`48 / 48`)
- the green Node20 manifested validation subset (`46 / 46`)
- explicit red/skip items that are already classified as either:
  - `validation_lane_divergence`
  - `later_family_dependency`

That satisfies the family closeout contract and promotes the next active work
to `NLC5`.

## Current Local Evidence

- `runtime::tests::node_compat::node22_default_lane_executes_manifested_process_and_timing_subset`
- `runtime::tests::node_compat::node20_supported_lane_executes_official_process_and_timing_subset`
- `runtime::tests::node_compat::node20_process_features_watchpoint`
- `docs/architecture/runtime/node-lts-compat/manifests/process-and-timing.md`

## Verification Environment Note

- The Deno-fork `unit_node` harness is not the primary blocker any more:
  `CARGO_ENCODED_RUSTFLAGS` successfully removes the local macOS
  `-fuse-ld=lld` verification seam, but the fork-side harness still needs
  machine prerequisites (`cmake`, built `deno`, and built `test_server`) that
  are absent in this environment.
- The current Nimbus Node22/Node20 manifested lanes therefore remain the
  primary evidence for the integrated ownership move.
