# NDS3 node26 cycle 34 - runtime/v8 promotion

Date: 2026-06-16
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

This checkpoint burns 7 Node26 Current required gaps from the `runtime/v8`
owner cluster:

- `test/parallel/test-v8-collect-gc-profile-exit-before-stop.js`
- `test/parallel/test-v8-collect-gc-profile-using.js`
- `test/parallel/test-v8-collect-gc-profile.js`
- `test/parallel/test-v8-getheapsnapshot-twice.js`
- `test/parallel/test-v8-global-setter.js`
- `test/parallel/test-v8-heap-profile.js`
- `test/parallel/test-v8-string-is-one-byte-representation.js`

The implementation lives in the Nimbus Deno fork:

- Repo: `/Users/jack/src/github.com/nimbus/deno`
- Branch: `nimbus/v2.8.3`
- Commit: `1b9984b1b1059c7ad59d36d564eeeffa5947c341`
- Tag: `v2.8.3-nimbus.66`
- Commit subject: `node:v8 add heap profile and string representation APIs`
- Changed file: `ext/node/polyfills/v8.ts`

Nimbus is repinned from immutable Deno tag `v2.8.3-nimbus.65` to
`v2.8.3-nimbus.66`. `rusty_v8` is unchanged at `v149.4.0-nimbus.2`.

Node26 `v8_isolate_required` posture moved from `20` gaps / `99.04%`
(`2072 / 2092`) to `13` gaps / `99.38%` (`2079 / 2092`). Node22 and Node24
remain green at `0` gaps / `100.0%`.

No V8 or rusty_v8 changes were made. No official upstream Node fixture or
checker was edited. No generated JSON was hand-edited to fake a green. No
`git add -A` was used.

## Deno Fork Change

`ext/node/polyfills/v8.ts` now exposes the two Node `v8` APIs needed by this
cluster:

- `startHeapProfile(options)`
- `isStringOneByteRepresentation(content)`

`startHeapProfile()` mirrors Node's public validation and lifecycle surface for
the tested options:

- `sampleInterval` must be an integer >= 1.
- `stackDepth` must be a signed 32-bit integer >= 0.
- `treatGlobalObjectsAsRoots`, `trackAllocations`, and
  `exposeInternals` must be booleans when provided.
- A second active profile throws `ERR_HEAP_PROFILE_HAVE_BEEN_STARTED`.
- `stop()` is idempotent and returns a syntactically valid JSON heap profile.

The returned heap profile is intentionally minimal. Deno's current Node
polyfill layer does not expose V8's sampling heap profiler through `rusty_v8`,
and this fixture cluster only requires Node-compatible validation, active-handle
lifecycle, and JSON parseability.

`isStringOneByteRepresentation(content)` validates `content` as a string and
returns whether every UTF-16 code unit is <= `0xff`.

## Proof Commands

Broad ignored batch on the previous immutable tag `.65`:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle34-runtime-v8-broad1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_runtime_v8_required_gap_watchpoint -- --ignored --nocapture
# selected=7, passed=5, failed=2
```

Summary artifact:

```text
/private/tmp/nds-node26-cycle34-runtime-v8-broad1/batch/node26__node26_current_lane_runtime_v8_required_gap_watchpoint__summary.json
```

Failure diagnostics on `.65`:

```text
/private/tmp/nds-node26-cycle34-runtime-v8-broad1/general/node26__test_parallel_test_v8_heap_profile_js.json
# expected ERR_INVALID_ARG_TYPE, actual error had no code

/private/tmp/nds-node26-cycle34-runtime-v8-broad1/general/node26__test_parallel_test_v8_string_is_one_byte_representation_js.json
# TypeError: isStringOneByteRepresentation is not a function
```

Local Deno proof before publishing `.66`:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle34-runtime-v8-local-deno-broad2 \
  cargo test -p nimbus-runtime \
    --config 'patch.crates-io.deno_cache_dir.path="/Users/jack/src/github.com/nimbus/deno/libs/cache_dir"' \
    --config 'patch.crates-io.deno_config.path="/Users/jack/src/github.com/nimbus/deno/libs/config"' \
    --config 'patch.crates-io.deno_core.path="/Users/jack/src/github.com/nimbus/deno/libs/core"' \
    --config 'patch.crates-io.deno_crypto.path="/Users/jack/src/github.com/nimbus/deno/ext/crypto"' \
    --config 'patch.crates-io.deno_crypto_provider.path="/Users/jack/src/github.com/nimbus/deno/libs/crypto"' \
    --config 'patch.crates-io.deno_dotenv.path="/Users/jack/src/github.com/nimbus/deno/libs/dotenv"' \
    --config 'patch.crates-io.deno_features.path="/Users/jack/src/github.com/nimbus/deno/runtime/features"' \
    --config 'patch.crates-io.deno_fetch.path="/Users/jack/src/github.com/nimbus/deno/ext/fetch"' \
    --config 'patch.crates-io.deno_fs.path="/Users/jack/src/github.com/nimbus/deno/ext/fs"' \
    --config 'patch.crates-io.deno_http.path="/Users/jack/src/github.com/nimbus/deno/ext/http"' \
    --config 'patch.crates-io.deno_inspector_server.path="/Users/jack/src/github.com/nimbus/deno/libs/inspector_server"' \
    --config 'patch.crates-io.deno_io.path="/Users/jack/src/github.com/nimbus/deno/ext/io"' \
    --config 'patch.crates-io.deno_lockfile.path="/Users/jack/src/github.com/nimbus/deno/libs/lockfile"' \
    --config 'patch.crates-io.deno_maybe_sync.path="/Users/jack/src/github.com/nimbus/deno/libs/maybe_sync"' \
    --config 'patch.crates-io.deno_napi.path="/Users/jack/src/github.com/nimbus/deno/ext/napi"' \
    --config 'patch.crates-io.deno_net.path="/Users/jack/src/github.com/nimbus/deno/ext/net"' \
    --config 'patch.crates-io.deno_node.path="/Users/jack/src/github.com/nimbus/deno/ext/node"' \
    --config 'patch.crates-io.deno_node_crypto.path="/Users/jack/src/github.com/nimbus/deno/ext/node_crypto"' \
    --config 'patch.crates-io.deno_node_sqlite.path="/Users/jack/src/github.com/nimbus/deno/ext/node_sqlite"' \
    --config 'patch.crates-io.deno_npm.path="/Users/jack/src/github.com/nimbus/deno/libs/npm"' \
    --config 'patch.crates-io.deno_npmrc.path="/Users/jack/src/github.com/nimbus/deno/libs/npmrc"' \
    --config 'patch.crates-io.deno_ops.path="/Users/jack/src/github.com/nimbus/deno/libs/ops"' \
    --config 'patch.crates-io.deno_os.path="/Users/jack/src/github.com/nimbus/deno/ext/os"' \
    --config 'patch.crates-io.deno_package_json.path="/Users/jack/src/github.com/nimbus/deno/libs/package_json"' \
    --config 'patch.crates-io.deno_permissions.path="/Users/jack/src/github.com/nimbus/deno/runtime/permissions"' \
    --config 'patch.crates-io.deno_process.path="/Users/jack/src/github.com/nimbus/deno/ext/process"' \
    --config 'patch.crates-io.deno_resolver.path="/Users/jack/src/github.com/nimbus/deno/libs/resolver"' \
    --config 'patch.crates-io.deno_signals.path="/Users/jack/src/github.com/nimbus/deno/ext/signals"' \
    --config 'patch.crates-io.deno_subprocess_windows.path="/Users/jack/src/github.com/nimbus/deno/runtime/subprocess_windows"' \
    --config 'patch.crates-io.deno_telemetry.path="/Users/jack/src/github.com/nimbus/deno/ext/telemetry"' \
    --config 'patch.crates-io.deno_tls.path="/Users/jack/src/github.com/nimbus/deno/ext/tls"' \
    --config 'patch.crates-io.deno_web.path="/Users/jack/src/github.com/nimbus/deno/ext/web"' \
    --config 'patch.crates-io.deno_webidl.path="/Users/jack/src/github.com/nimbus/deno/ext/webidl"' \
    --config 'patch.crates-io.deno_websocket.path="/Users/jack/src/github.com/nimbus/deno/ext/websocket"' \
    --config 'patch.crates-io.node_resolver.path="/Users/jack/src/github.com/nimbus/deno/libs/node_resolver"' \
    --config 'patch.crates-io.node_shim.path="/Users/jack/src/github.com/nimbus/deno/libs/node_shim"' \
    --config 'patch.crates-io.serde_v8.path="/Users/jack/src/github.com/nimbus/deno/libs/serde_v8"' \
    --lib node26_current_lane_runtime_v8_required_gap_watchpoint -- --ignored --nocapture
# selected=7, passed=7, failed=0
```

Summary artifact:

```text
/private/tmp/nds-node26-cycle34-runtime-v8-local-deno-broad2/batch/node26__node26_current_lane_runtime_v8_required_gap_watchpoint__summary.json
```

The first attempted local proof patched only `deno_node` and failed to compile
because it mixed local and tagged Deno-family crates. That transient failure did
not produce fixture diagnostics, and `Cargo.lock` was restored before the full
Deno-family local proof:

```text
/private/tmp/nds-node26-cycle34-runtime-v8-local-deno-broad1
```

Immutable `.66` broad proof:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle34-runtime-v8-tag66-broad1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_runtime_v8_required_gap_watchpoint -- --ignored --nocapture
# selected=7, passed=7, failed=0
```

Summary artifact:

```text
/private/tmp/nds-node26-cycle34-runtime-v8-tag66-broad1/batch/node26__node26_current_lane_runtime_v8_required_gap_watchpoint__summary.json
```

After promotion, the seven fixtures were added to a non-ignored Node26
runtime/v8 promoted batch. The durable promoted proof on immutable `.66`
passed:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle34-runtime-v8-tag66-promoted1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_runtime_v8_promoted_batch_fixture -- --nocapture
# selected=7, passed=7, failed=0
```

Summary artifact:

```text
/private/tmp/nds-node26-cycle34-runtime-v8-tag66-promoted1/batch/node26__node26_current_lane_executes_runtime_v8_promoted_batch__summary.json
```

## Generator And Integrity Checks

Generator sequence after the promoted proof:

```bash
python3 -B scripts/runtime/node/watchpoints.py sync
# wrote tests/runtime/node/expectations/rust-watchpoints.json

python3 -B scripts/runtime/node/watchpoints.py validate
# validated node-compat watchpoint catalog: 154 entries

python3 -B scripts/runtime/node/classifications.py sync --lane node26
# wrote tests/runtime/node/classifications/node26.json

python3 -B scripts/runtime/node/status.py
# wrote target/node-compat/status/status-summary.{json,md}

python3 -B scripts/runtime/node/dashboard.py
# wrote target/node-compat/dashboard/dashboard-summary.{json,md}

python3 -B scripts/runtime/node/trends.py
# wrote target/node-compat/trends/trend-summary.{json,md}

python3 -B scripts/runtime/node/publish_evidence.py
# published tests/runtime/node/compat/node-compat-evidence/latest/*

python3 -B scripts/runtime/node/default_support_posture.py
# wrote private and public node-default-support-posture artifacts

python3 -B scripts/runtime/node/required_surface_blockers.py
# node22 required gaps: 0
# node24 required gaps: 0
```

Some generator commands were rerun with narrow filesystem escalation because
the managed sandbox blocked writes under `docs/private`.

Integrity checks:

```bash
python3 -B scripts/runtime/node/classifications.py sync --preserve-existing --check
# node20.json, node22.json, node24.json, node26.json are up to date

python3 -B scripts/runtime/node/default_support_posture.py --check
# node default support posture: pass

python3 -B scripts/runtime/node/required_surface_blockers.py --check
# node required-surface blocker inventory: pass

python3 -B scripts/runtime/node/watchpoints.py validate
# validated node-compat watchpoint catalog: 154 entries

python3 -B scripts/runtime/node/docs_guard.py
# Node LTS docs guard passed

cargo fmt --all --check
# pass

git diff --check
# pass
```

Verifier:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 14 passed, 20 failed
# [9] Node22/Node24 V8-isolate-required green: PASS
```

The overall verifier remains red honestly. Node26 still has 13 required gaps,
and the broader NDS proof/closeout rows are not complete. PR #10 remains draft
and unmerged.

`deno fmt --check ext/node/polyfills/v8.ts` in the Deno fork wanted to
reindent the entire extension IIFE, including thousands of untouched lines.
That repo-wide churn was intentionally not applied. `git diff --check` in the
Deno fork passed before tagging `.66`.

## Current Posture

Generated `docs/architecture/runtime/node-default-support-posture.json` after
this checkpoint:

```json
{
  "node22": {
    "gaps": 0,
    "pass_rate_percent": 100.0,
    "passed": 2363,
    "total": 2363
  },
  "node24": {
    "gaps": 0,
    "pass_rate_percent": 100.0,
    "passed": 2400,
    "total": 2400
  },
  "node26": {
    "gaps": 13,
    "pass_rate_percent": 99.38,
    "passed": 2079,
    "total": 2092
  }
}
```

Remaining Node26 `v8_isolate_required` entries:

```text
test/parallel/test-vm-module-evaluate-while-evaluating.js
test/parallel/test-async-hooks-fatal-error.js
test/parallel/test-async-local-storage-weak-asyncwrap-leak.js
test/parallel/test-stream2-basic.js
test/parallel/test-structuredClone-global.js
test/parallel/test-trace-events-api.js
test/parallel/test-webstreams-clone-unref.js
test/parallel/test-whatwg-webstreams-transform-stream-members.js
test/parallel/test-fs-promises-watch-ignore-invalid.mjs
test/parallel/test-fs-promises-watch.js
test/parallel/test-fs-sir-writes-alot.js
test/parallel/test-fs-stat-temporal.mjs
test/parallel/test-fs-write-buffer-large.js
```

## Next Recommended Cluster

Continue with a broad Node26 ROI scan over the remaining 13 gaps. Best next
clusters by likely shared root cause:

- Local fs/host I/O residual: the five `test-fs-*` entries.
- WebStreams/structuredClone/stream residual: `test-stream2-basic.js`,
  `test-structuredClone-global.js`, `test-webstreams-clone-unref.js`, and
  `test-whatwg-webstreams-transform-stream-members.js`.
- Async lifecycle and tracing: `test-async-hooks-fatal-error.js`,
  `test-async-local-storage-weak-asyncwrap-leak.js`, and
  `test-trace-events-api.js`.
- Loader/vm residual: `test-vm-module-evaluate-while-evaluating.js`.
