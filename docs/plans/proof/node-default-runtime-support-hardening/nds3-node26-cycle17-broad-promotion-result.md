# NDS3 Node26 Cycle 17: broad runtime promotion wave

## Scope

This checkpoint promotes the dynamically green Node26 Current fixtures found by
the broad process-host, parallel JS platform, and WebCrypto batches. It also
keeps the Node26 stream/WebStreams promotion honest by proving the existing
promoted batch after leaving `test/parallel/test-stream2-basic.js` out of the
Node26 promoted set; that fixture still fails on Node26 and remains a required
gap.

No V8 or rusty_v8 changes were made. No official upstream fixture or checker was
edited. No Deno fork changes or local Deno pins were used in this cycle. Nimbus
remained pinned to the published immutable Deno tag `v2.8.3-nimbus.50`.

Before this wave, Node26 `v8_isolate_required` posture was `319` gaps /
`84.85%`.

## Broad Diagnostics

Unpromoted parallel discovery:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-unpromoted-parallel-discovery-wave1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_unpromoted_parallel_discovery_watchpoint -- --ignored --nocapture
```

Result:

- selected: `35`
- passed: `32`
- skipped: `0`
- failed: `3`
- failures:
  `test/parallel/test-async-local-storage-weak-asyncwrap-leak.js`,
  `test/parallel/test-eventsource.js`,
  `test/parallel/test-structuredClone-global.js`

Process-host broad batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-process-host-wave1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_process_host_watchpoint -- --ignored --nocapture
```

Result:

- selected: `27`
- passed: `26`
- skipped: `0`
- failed: `1`
- failure: `test/parallel/test-process-get-builtin.mjs`, which still needs
  `stream/iter`

Loader/module broad batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-loader-module-wave1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_loader_context_module_watchpoint -- --ignored --nocapture
```

Result:

- selected: `20`
- passed: `0`
- skipped: `0`
- failed: `20`
- root-cause groups: loader-hook require/ESM race, built-in hook call-count
  mismatch, and missing fixture directory topology for one circular warning

Core/util broad batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-core-util-wave1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_core_semantics_util_required_gap_watchpoint -- --ignored --nocapture
```

Result:

- selected: `6`
- passed: `0`
- skipped: `1`
- failed: `5`
- skip: `test/parallel/test-util-styletext.js`
- failure families: assert, util, URL parse, callbackify, regexp inspect

Fs-host broad batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-fs-host-wave1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_fs_host_io_watchpoint -- --ignored --nocapture
```

Result:

- selected: `11`
- passed: `0`
- skipped: `1`
- failed: `10`
- root-cause groups: missing FileHandle `readableWebStream()` pull,
  `pullSync()`, and writer support; remaining fs rmdir/error/stat/symlink
  semantics

WebCrypto broad batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-webcrypto-wave1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_webcrypto_required_gap_watchpoint -- --ignored --nocapture
```

Result:

- selected: `29`
- passed: `1`
- skipped: `1`
- failed: `27`
- pass promoted in this cycle:
  `test/parallel/test-webcrypto-promise-prototype-pollution.mjs`
- skip: `test/parallel/test-webcrypto-derivebits-argon2.js`

WHATWG web-platform broad batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-whatwg-web-platform-wave1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_whatwg_web_platform_watchpoint -- --ignored --nocapture
```

Result:

- selected: `1`
- passed: `0`
- skipped: `0`
- failed: `1`
- failure: `test/parallel/test-whatwg-url-custom-inspect.js`

Event broad batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-event-wave1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_event_required_gap_watchpoint -- --ignored --nocapture
```

Result:

- selected: `1`
- passed: `0`
- skipped: `0`
- failed: `1`
- failure: `test/parallel/test-eventsource.js`, where `EventSource` is not a
  Node26 global in the current bootstrap

Parallel JS platform broad batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-parallel-js-platform-wave1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_parallel_js_platform_required_gap_watchpoint -- --ignored --nocapture
```

Result:

- selected: `55`
- passed: `49`
- skipped: `1`
- failed: `5`
- skip: `test/parallel/test-util-styletext.js`
- failures:
  `test/parallel/test-global.js`,
  `test/parallel/test-performance-resourcetimingbuffersize.js`,
  `test/parallel/test-util-callbackify.js`,
  `test/parallel/test-util-inspect-regexp.js`,
  `test/parallel/test-util-parse-env.js`

Streams/WebStreams broad batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-streams-web-platform-wave1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_streams_web_platform_watchpoint -- --ignored --nocapture
```

Result:

- selected: `116`
- passed: `46`
- skipped: `69`
- failed: `1`
- the `69` skips are disabled QUIC fixtures and were not promoted
- failure: `test/parallel/test-stream2-basic.js`

## Promotion Proof

Process-host promoted batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-process-host-wave1-promoted \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_process_host_promoted_batch_fixture -- --nocapture
```

Result:

- selected: `26`
- passed: `26`
- skipped: `0`
- failed: `0`

WebCrypto promoted batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-webcrypto-cycle71-promoted \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_cycle71_webcrypto_promise_prototype_pollution_batch -- --nocapture
```

Result:

- selected: `1`
- passed: `1`
- skipped: `0`
- failed: `0`

Parallel JS platform promoted batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-parallel-js-platform-wave1-promoted \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_parallel_js_platform_promoted_batch_fixture -- --nocapture
```

Result:

- selected: `49`
- passed: `49`
- skipped: `0`
- failed: `0`

Stream/WebStreams honesty rerun:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-streams-web-platform-promoted-honesty \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_streams_web_platform_promoted_batch_fixture -- --nocapture
```

Result:

- selected: `118`
- passed: `118`
- skipped: `0`
- failed: `0`

The stream rerun proves the already-promoted Node26 stream/WebStreams batch
still stays green after keeping `test/parallel/test-stream2-basic.js` out of the
Node26 promoted set. That fixture remains in the required blockers inventory.

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
  `validated node-compat watchpoint catalog: 149 entries`
- `docs/private/architecture/runtime/node-default-support-posture.json`:
  Node26 `v8_isolate_required` is `243` gaps / `88.46%`
- tracked public evidence:
  Node26 full official corpus manifested green count is `1862 / 5578`

Posture after regeneration:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`
- Node26 `v8_isolate_required`: `243` gaps, `88.46%`
- Node26 required passed: `1862 / 2105`

This wave moves Node26 from `319` gaps / `84.85%` to `243` gaps / `88.46%`,
burning 76 required-surface gaps.

## Verifier

Command:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
```

Result:

- Summary: `14 passed, 20 failed`.
- Step 9 passed: Node22 and Node24 V8-isolate-required fixtures are `100%`.
- Step 11 remains failed because Node26 Current evidence is still incomplete:
  Node26 is `1862` official passes and `243` required gaps, not `0` gaps /
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

- `99` `node-compat/unpromoted-surface`
- `34` `node-compat/current-lane`
- `23` `loader-context/vm`
- `20` `loader-context/module`
- `18` `loader-context/domain`
- `15` `streams-local-io/fs-host-io`
- `10` `process-and-timing/perf-hooks`
- `7` `process-and-timing/process-host`
- `7` `runtime/v8`
- `4` `core-semantics/console`
- `3` `loader-context/util`
- `2` `core-semantics/assert`
- `1` `core-semantics/url`

Recommended next wave: start from the remaining `node-compat/unpromoted-surface`
and `node-compat/current-lane` buckets, but bias toward implementation clusters
that can unlock multiple fixtures at once. The `process-host` residual is now
small enough to fold into a broader `stream/iter` or process builtin wave rather
than spending a singleton cycle on `test-process-get-builtin.mjs`.
