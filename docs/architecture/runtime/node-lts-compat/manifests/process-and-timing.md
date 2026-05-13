# Process And Timing Node Test Slices

Current upstream Node test-slice manifest for `process-and-timing` family.

Source corpus:

- current Deno-family implementation baseline:
  `~/src/github.com/nimbus/deno @ v2.7.14-locker.28`
- pinned official Node22 validation corpus:
  `nodejs/node @ v22.15.0`
- pinned official Node20 supported corpus:
  `nodejs/node @ v20.20.2`
- staged future Node24 supported corpus:
  `nodejs/node @ v24.15.0`

This file records the currently manifested official-fixture subset for the
`process-and-timing` family process and timing family. The canonical source of truth for the
executed subset is
[`PROCESS_AND_TIMING_BATCH`](../../../../crates/nimbus-runtime/src/runtime/tests/node/mod.rs)
plus the explicit watchpoints in the same Rust file; this document summarizes
that state so future work can resume without rediscovering it.

## Initial Slice Map

| Family | Initial upstream test slices |
| --- | --- |
| `node:process` | `test/parallel/test-process-*.js`, `test/sequential/test-process-*.js`, `test/pummel/test-process-*.js` |
| `nextTick` / scheduling | `test/parallel/test-next-tick*.js`, `test/parallel/test-process-next-tick.js`, `test/sequential/test-next-tick*.js`, `test/pummel/test-next-tick*.js` |
| `node:timers` | `test/parallel/test-timers*.js`, `test/pummel/test-timers*.js`, `test/wpt/test-timers.js` |
| `node:util` | `test/parallel/test-util-*.js`, `test/sequential/test-util-*.js` |
| `node:diagnostics_channel` | `test/parallel/test-diagnostics-channel-*.js` |
| `node:perf_hooks` | `test/parallel/test-perf-hooks-*.js`, `test/sequential/test-perf-hooks*.js` |

## Current Manifested Official Subset

The current manifested subset is data-driven from the checked-in fixture roots
and the `PROCESS_AND_TIMING_BATCH` table in
`crates/nimbus-runtime/src/runtime/tests/node/mod.rs`.

Current manifested batch counts:

- Node22 default lane: `48` official files
- Node20 supported lane: `46` official files
- Node24 supported lane: `48` staged official files
  - latest explicit supported-lane watchpoint run: `45` passed, `3` failed

Current manifested slice coverage:

- `process`: release/default/prototype/uptime/env-symbols/warning/emitWarning,
  plus `loadEnvFile()` path/default/missing-file/permission/`--env-file`
  immutability behavior
- `nextTick`: queue ordering, starvation, error propagation, regression files
- `timers`: core `setTimeout` / `setInterval` / `setImmediate` behavior,
  callback `this`, overflow warnings, and clear/ref equivalence
- `util`: `deprecate`, `format`, `inherits`, `parseEnv`, `MIMEType`,
  `MIMEParams`, `TextDecoder`, and type presence
- `perf_hooks`: user timing, `createHistogram()`,
  `monitorEventLoopDelay()` clone/summary semantics,
  `PerformanceResourceTiming`, and `performance.markResourceTiming()` shape
  and entry-buffer semantics
- `diagnostics_channel`: has-subscribers, pub/sub, symbol/object channels,
  safe subscriber errors, and synchronous unsubscribe behavior

Family-level notes:

- The current hot-path `util.format()` numeric-separator formatting,
  `%s` + `Symbol.toPrimitive` coercion, and
  `diagnostics_channel` stable-subscriber iteration semantics are now owned by
  `nimbus/deno`, not Nimbus-local bootstrap code.
- `nimbus/deno v2.7.14-locker.22` now also owns the imported official
  `util.parseEnv()` plain-object shape contract, with a fork-side regression in
  `tests/unit_node/util_test.ts` that asserts `Object.getPrototypeOf(parsed) === Object.prototype`.
- `nimbus/deno v2.7.14-locker.23`,
  `nimbus/deno v2.7.14-locker.24`, and
  `nimbus/deno v2.7.14-locker.25` now own the imported
  `node:perf_hooks` user-timing contract:
  exporting `PerformanceMark` / `PerformanceMeasure`, seeding the minimal
  `nodeTiming` marks the Node file expects, and restoring Node-style Symbol
  coercion errors for `performance.mark()` / `clearMarks()`. The final
  `.25` repin adds Node-style `ERR_INVALID_ARG_TYPE` validation for
  `performance.mark('a', { startTime: ... })`, which moves the imported
  official `test-perf-hooks-usertiming.js` file into the manifested green
  denominator instead of leaving it as a watchpoint.
- `nimbus/deno v2.7.14-locker.26`,
  `nimbus/deno v2.7.14-locker.27`, and
  `nimbus/deno v2.7.14-locker.28` now own the imported
  `node:perf_hooks` resource-timing contract:
  exporting `PerformanceResourceTiming`, implementing
  `performance.markResourceTiming()`, aligning web-layer enumerable
  descriptors, and hiding the internal `nodeTiming` bootstrap marks from
  public `performance.getEntries*()` queries so the official Node22 file runs
  green.
- `process.loadEnvFile()` is now part of the manifested green denominator
  instead of a separate watchpoint. The final enabling fixes stayed local to
  Nimbus because the remaining seam was embedder-owned: the embedded
  `Node22` bootstrap now layers a runtime-only env overlay plus Node-style
  missing-file and permission error shaping on top of Deno's base op, and the
  `node_compat` subprocess helper now snapshots and restores host `cwd` and
  process-global env after each spawned child run so `loadEnvFile()` no longer
  contaminates later manifested fixtures in the same Rust process.
- `perf_hooks` histogram is now part of the manifested green denominator
  instead of a separate watchpoint. `process.finalization.*` is no longer an
  active `process-and-timing` family seam in its own right: the direct official fixture bodies
  (`close.mjs`, `before-exit.mjs`, and `unregister.mjs`) now run green through
  the Nimbus-owned sync subprocess harness, and the only remaining failure in
  the top-level official wrapper file `test-process-finalization.mjs` is
  `different-registry-per-thread.mjs`, which depends on `node:worker_threads`
  and is therefore owned by the later VM/worker family rather than this
  process/timing item.
- Official `test-perf-hooks-resourcetiming.js` is now part of the Node22
  default lane and the staged Node24 supported lane, but it remains outside the
  Node20 supported green subset because official `v20.20.2`
  `PerformanceResourceTiming#toJSON()` omits the later `deliveryType` and
  `responseStatus` fields that Nimbus intentionally keeps in its single
  Node22-shaped runtime contract.
- Official `test-process-load-env-file.js` files are now manifested in the
  Node22 default lane, the official Node20 supported lane, and the staged
  Node24 supported lane. The imported file remains a good example of the owner
  split rule: the underlying Deno op still loads env content, but the final
  batch-stability and Node-style runtime contract fixes belong in Nimbus's
  embedded bootstrap and subprocess harness rather than the Deno fork.

## Current Local Evidence

- `runtime::tests::node_compat::node22_default_lane_executes_manifested_process_and_timing_subset`
- `runtime::tests::node_compat::node20_supported_lane_executes_official_process_and_timing_subset`
- `runtime::tests::node_compat::node24_supported_lane_executes_manifested_process_and_timing_subset`
  *(ignored by default; supported-lane watchpoint, not a green support claim)*

## Notes

- The `process-and-timing` family now follows the same fast path that worked for the `core-semantics` family: official Node
  files are imported as data, run through a manifested subset, and widened in
  batches by shared runtime seam instead of adding bespoke Rust test wrappers.
- Local Deno-fork `unit_node` verification is partially blocked on this machine:
  `CARGO_ENCODED_RUSTFLAGS` cleanly removes the repo's checked-in macOS
  `-fuse-ld=lld` flag for a single local command, but the full Deno `unit_node`
  harness still requires extra machine prerequisites such as `cmake` and the
  prebuilt `deno` / `test_server` binaries. The current Nimbus manifest lanes
  are therefore the primary integration proof for this slice.
