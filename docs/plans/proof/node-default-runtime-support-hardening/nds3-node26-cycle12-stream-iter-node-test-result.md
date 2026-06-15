# NDS3 Node26 Cycle 12: stream/iter and node:test Promotion

## Scope

This checkpoint burns Node26 Current required-surface gaps in the
`streams-local-io/stream` and WebStreams cluster. It builds on the
`v2.8.3-nimbus.48` Deno fork stream/iter work, fixes the remaining
`node:test` scheme-only builtin loading issue in the Deno fork, repins Nimbus to
the published immutable `v2.8.3-nimbus.49` tag, and promotes only fixtures that
were green in the enforced Node26 promoted batch.

No V8 or rusty_v8 changes, fixture edits, checker edits, or generated
false-green JSON hand edits were made. `test/parallel/test-stream2-basic.js`
remains intentionally unpromoted because the broad `.48` stream/WebStreams run
still failed it.

Before this wave, Node26 `v8_isolate_required` posture was `604` gaps /
`72.53%`.

## Deno Fork Change

Fork:

- Worktree: `/Users/jack/src/github.com/nimbus/deno`
- Branch: `nimbus/v2.8.3`
- Previous stream/iter tag: `v2.8.3-nimbus.48`
- New commit: `90bac4bded` (`node: allow node test builtin via node scheme`)
- New tag: `v2.8.3-nimbus.49`
- Push result: branch `nimbus/v2.8.3` and tag `v2.8.3-nimbus.49` are present on
  `origin`.

The fork change updates `ext/node/polyfills/01_require.js` so Deno's existing
`node:test` and `node:test/reporters` polyfills can be required through the
explicit `node:` scheme while the schemeless blocklist still blocks bare
`test` and `test/reporters`. The change threads a `fromNodeScheme` flag through
`nativeModuleCanBeRequiredByUsers()`, `isBuiltin()`, `_load()`, and the
`node:` branch of `_resolveFilename()`.

This keeps the behavior in the fork's existing Node builtin loader instead of
adding a Nimbus-local shim.

## Broad Pre-Run

Immutable `v2.8.3-nimbus.48` broad stream/WebStreams pre-run:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-streams-wave1-tag48 \
  cargo test -p nimbus-runtime --lib node26_current_lane_streams_web_platform_watchpoint -- --ignored --nocapture
```

Result:

- Rust test result: failed, as expected for the broad diagnostic batch.
- Fixture summary: `selected=116`, `passed=46`, `skipped=69`, `failed=1`.
- Failed fixture: `test/parallel/test-stream2-basic.js`
- Summary artifact:
  `/private/tmp/nds-node26-streams-wave1-tag48/batch/node26__node26_current_lane_streams_web_platform_watchpoint__summary.json`

The 46 broad passes were candidates for promotion. The 69 skips are still QUIC
stream skips and were not promoted.

## First Promotion Attempt

Immutable `v2.8.3-nimbus.48` enforced promoted batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-streams-wave1-tag48-promote1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_streams_web_platform_promoted_batch_fixture -- --nocapture
```

Result:

- Rust test result: failed.
- Fixture summary: `selected=118`, `passed=112`, `skipped=0`, `failed=6`.
- Failed fixtures:
  - `test/parallel/test-file-write-stream5.js`
  - `test/parallel/test-webstreams-adapters-sync-write-error.js`
  - `test/parallel/test-webstreams-adapters-writable-buffer-sources.js`
  - `test/parallel/test-webstreams-compression-bad-chunks.js`
  - `test/parallel/test-webstreams-compression-buffer-source.js`
  - `test/parallel/test-webstreams-decompression-reject-trailing.js`
- Shared root cause: `ERR_UNKNOWN_BUILTIN_MODULE: No such built-in module: node:test`.
- Summary artifact:
  `/private/tmp/nds-node26-streams-wave1-tag48-promote1/batch/node26__node26_current_lane_executes_streams_web_platform_promoted_batch__summary.json`

## Local Fork Proof

Nimbus was temporarily pinned to the canonical local Deno worktree at commit
`90bac4bded2fc48589e8d261f32ec69901960daa` while proving the fork change. To
avoid stale bundled JavaScript artifacts, the Deno JS polyfill proof used a
targeted clean before rerunning the promoted batch:

```bash
cargo clean -p deno_node -p nimbus-runtime
```

Result:

- Removed `1.9GiB`.

Local Deno path proof:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-streams-wave1-local-node-test-promote1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_streams_web_platform_promoted_batch_fixture -- --nocapture
```

Result:

- Rust test result: `1 passed; 0 failed; 0 ignored`.
- Fixture summary: `selected=118`, `passed=118`, `skipped=0`, `failed=0`.
- Summary artifact:
  `/private/tmp/nds-node26-streams-wave1-local-node-test-promote1/batch/node26__node26_current_lane_executes_streams_web_platform_promoted_batch__summary.json`

## Immutable Tag Proof

After tagging and pushing the Deno fork, Nimbus was restored to the immutable
remote tag:

```toml
deno_node = { git = "https://github.com/nimbus/deno", tag = "v2.8.3-nimbus.49" }
```

The same targeted clean was run again so the published tag, not stale local
artifacts, supplied the Deno Node polyfill code:

```bash
cargo clean -p deno_node -p nimbus-runtime
```

Result:

- Removed `854.9MiB`.

Published-tag promoted batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-streams-wave1-tag49-promote1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_streams_web_platform_promoted_batch_fixture -- --nocapture
```

Result:

- Rust test result: `1 passed; 0 failed; 0 ignored`.
- Fixture summary: `selected=118`, `passed=118`, `skipped=0`, `failed=0`.
- Summary artifact:
  `/private/tmp/nds-node26-streams-wave1-tag49-promote1/batch/node26__node26_current_lane_executes_streams_web_platform_promoted_batch__summary.json`

## Promoted Fixtures

The 46 newly promoted Node26 paths are exactly the extra paths added to
`STREAMS_WEB_PLATFORM_PROMOTED_NODE26_PATHS` after the prior cycle 7 stream
promotion. They include the stable `stream/iter` family, the
`stream.finished()` AsyncLocalStorage/AsyncResource fixtures, selected
WebStreams inspection/adapter/compression fixtures, and the stable stream core
fixtures `test-stream-readable-readable-one.js` and `test-stream2-transform.js`.

The promoted batch now enforces `118` Node26 stream/WebStreams paths and is
green on the immutable Deno tag.

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

- `scripts/runtime/node/watchpoints.py validate`: `validated node-compat watchpoint catalog: 145 entries`
- `tests/runtime/node/compat/node-compat-evidence/latest/status-summary.json`: warnings `[]`
- `scripts/runtime/node/required_surface_blockers.py`: `node22 required gaps: 0`, `node24 required gaps: 0`

Posture after regeneration:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`
- Node26 `v8_isolate_required`: `558` gaps, `74.62%`

The Node26 count moved from `604` gaps / `72.53%` to `558` gaps /
`74.62%`, burning 46 required-surface gaps in this wave. The official fixture
evidence count for Node26 moved from `1595 / 5578` to `1641 / 5578`.

## Verifier

Command:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
```

Result:

- Summary: `14 passed, 20 failed`.
- Step 9 passed: Node22 and Node24 V8-isolate-required fixtures are `100%`.
- The verifier remains red because this checkout does not contain the private
  plan/proof closeout tree and Node26 Current evidence is still incomplete.
  This is an honest red; it is not a Node22/Node24 required-surface regression.

## Stale Build Caution

Deno fork JavaScript polyfill changes can false-green if `deno_node` is not
rebuilt. This cycle therefore used `cargo clean -p deno_node -p nimbus-runtime`
before both local-Deno and immutable-tag proofs. Future Deno-owner waves should
keep this targeted clean in the proof recipe.

## Next Node26 Work

Node26 remains at `558` required gaps. The highest-yield follow-up is a fresh
ROI scan of the regenerated posture, then a broad ignored batch for the largest
remaining coherent group. Do not spend singleton time on
`test/parallel/test-stream2-basic.js` unless it is the last stream residual.
Likely high-yield waves remain loader/ESM/CJS, async lifecycle, crypto/network,
web platform/WebStreams tail, and remaining fs/stream residuals.
