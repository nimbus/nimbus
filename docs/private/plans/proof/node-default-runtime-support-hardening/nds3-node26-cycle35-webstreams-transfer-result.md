# NDS3 node26 cycle 35 - WebStreams transfer promotion

Date: 2026-06-16
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

This checkpoint burns 3 Node26 Current required gaps from the
`node-compat/unpromoted-surface` cluster:

- `test/parallel/test-structuredClone-global.js`
- `test/parallel/test-webstreams-clone-unref.js`
- `test/parallel/test-whatwg-webstreams-transform-stream-members.js`

The implementation lives in the Nimbus Deno fork:

- Repo: `/Users/jack/src/github.com/nimbus/deno`
- Branch: `nimbus/v2.8.3`
- Commit: `a68de0585022f957331125b51c3e9355b9aeb5ff`
- Tag: `v2.8.3-nimbus.67`
- Commit subject: `web: fix node26 stream transfer liveness`
- Changed files:
  - `ext/web/06_streams.js`
  - `ext/web/13_message_port.js`

Nimbus is repinned from immutable Deno tag `v2.8.3-nimbus.66` to
`v2.8.3-nimbus.67`. `rusty_v8` is unchanged at `v149.4.0-nimbus.2`.

Node26 `v8_isolate_required` posture moved from `13` gaps / `99.38%`
(`2079 / 2092`) to `10` gaps / `99.52%` (`2082 / 2092`). Node22 and Node24
remain green at `0` gaps / `100.0%`.

No V8 or rusty_v8 changes were made. No official upstream Node fixture or
checker was edited. No generated JSON was hand-edited to fake a green. No
`git add -A` was used.

## Root Cause

Deno's WebStreams transfer bridge in `ext/web/06_streams.js` creates internal
`MessagePort` pairs for cross-realm stream transfer. Cycle 76 already unrefed
the bridge ports after `port.start()`, but the readable and writable request
paths still called `port.ref()` before posting `pull` or `chunk` messages.

Node26's WebStreams transfer fixtures expose that remaining liveness edge: an
otherwise idle transferred stream can enqueue an internal pull request and leave
the bridge port keeping the isolate alive even after the fixture assertions have
completed.

Node26 also updated the structuredClone transfer-options error text from
`can not` to `cannot`, while Node22 and Node24 still assert the older wording.

## Fork Fix

`ext/web/06_streams.js` now preserves the unrefed bridge lifecycle by removing
the extra `port.ref()` calls from the cross-realm readable pull and writable
write request paths. This aligns those request paths with upstream Deno's shape
while keeping the Nimbus-specific bridge `unref()` safety points from cycle 76.

`ext/web/13_message_port.js` now keeps the structuredClone transfer-sequence
error message lane-aware:

- Node26 gets `transfer in Options cannot be converted to sequence.`
- Node22 and Node24 keep `transfer in Options can not be converted to sequence.`

The change preserves sandbox boundaries. It only changes internal WebStreams
bridge liveness and Node-compatible error text; it does not add host process,
signal, subprocess, filesystem, network, or native authority.

## Proof Commands

Focused local Deno proof before publishing `.67`:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle35-webstreams-transfer-local-deno-4 \
  cargo test -p nimbus-runtime \
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
    --config 'patch.crates-io.deno_maybe_sync.path="/Users/jack/src/github.com/nimbus/deno/libs/maybe_sync"' \
    --config 'patch.crates-io.deno_napi.path="/Users/jack/src/github.com/nimbus/deno/ext/napi"' \
    --config 'patch.crates-io.deno_net.path="/Users/jack/src/github.com/nimbus/deno/ext/net"' \
    --config 'patch.crates-io.deno_node.path="/Users/jack/src/github.com/nimbus/deno/ext/node"' \
    --config 'patch.crates-io.deno_node_crypto.path="/Users/jack/src/github.com/nimbus/deno/ext/node_crypto"' \
    --config 'patch.crates-io.deno_node_sqlite.path="/Users/jack/src/github.com/nimbus/deno/ext/node_sqlite"' \
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
    --lib node26_current_lane_executes_cycle35_webstreams_transfer_batch -- --nocapture
# selected=3, passed=3, skipped=0, failed=0
```

Summary artifact:

```text
/private/tmp/nds-node26-cycle35-webstreams-transfer-local-deno-4/batch/node26__node26_current_lane_executes_cycle35_webstreams_transfer_batch__summary.json
```

Local Deno regression guards:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle35-structuredclone-regression-local-deno-1 \
  cargo test -p nimbus-runtime [full local Deno patch list] --lib cycle27_structured_clone -- --nocapture
# selected=2, passed=2, skipped=0, failed=0

NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle35-webstreams-unref-regression-local-deno-1 \
  cargo test -p nimbus-runtime [full local Deno patch list] --lib cycle76_webstreams_clone_unref -- --nocapture
# selected=2, passed=2, skipped=0, failed=0
```

Local broad unpromoted-surface batch after the fix:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle35-unpromoted-surface-local-deno-2 \
  cargo test -p nimbus-runtime [full local Deno patch list] \
    --lib node26_current_lane_unpromoted_surface_required_gap_watchpoint -- --ignored --nocapture
# selected=7, passed=4, skipped=0, failed=3
```

`test-stream2-basic.js` passed inside that broad local batch, but it failed as a
focused promotion guard and failed again in the immutable `.67` broad batch.
It was therefore not promoted in this checkpoint:

```text
/private/tmp/nds-node26-cycle35-webstreams-transfer-local-deno-3/batch/node26__node26_current_lane_executes_cycle35_webstreams_transfer_batch__summary.json
# selected=4, passed=3, skipped=0, failed=1
# failing fixture: test/parallel/test-stream2-basic.js
```

Immutable `.67` focused proof:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle35-webstreams-transfer-tag67-1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_cycle35_webstreams_transfer_batch -- --nocapture
# selected=3, passed=3, skipped=0, failed=0
```

Summary artifact:

```text
/private/tmp/nds-node26-cycle35-webstreams-transfer-tag67-1/batch/node26__node26_current_lane_executes_cycle35_webstreams_transfer_batch__summary.json
```

Immutable `.67` regression guards:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle35-structuredclone-regression-tag67-1 \
  cargo test -p nimbus-runtime --lib cycle27_structured_clone -- --nocapture
# selected=2, passed=2, skipped=0, failed=0

NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle35-webstreams-unref-regression-tag67-1 \
  cargo test -p nimbus-runtime --lib cycle76_webstreams_clone_unref -- --nocapture
# selected=2, passed=2, skipped=0, failed=0
```

Immutable `.67` broad unpromoted-surface batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle35-unpromoted-surface-tag67-1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_unpromoted_surface_required_gap_watchpoint -- --ignored --nocapture
# selected=7, passed=3, skipped=0, failed=4
```

Failed fixtures in the immutable broad batch:

```text
test/parallel/test-async-hooks-fatal-error.js
test/parallel/test-async-local-storage-weak-asyncwrap-leak.js
test/parallel/test-stream2-basic.js
test/parallel/test-trace-events-api.js
```

## Regeneration and Checks

Commands:

```bash
python3 -B scripts/runtime/node/classifications.py sync --lane node26
python3 -B scripts/runtime/node/status.py
python3 -B scripts/runtime/node/dashboard.py
python3 -B scripts/runtime/node/trends.py
python3 -B scripts/runtime/node/publish_evidence.py
python3 -B scripts/runtime/node/default_support_posture.py
python3 -B scripts/runtime/node/required_surface_blockers.py
python3 -B scripts/runtime/node/watchpoints.py sync
```

Validation:

```bash
python3 -B scripts/runtime/node/classifications.py sync --lane node26 --check
# tests/runtime/node/classifications/node26.json is up to date

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

(cd /Users/jack/src/github.com/nimbus/deno && git diff --check)
# pass
```

Full verifier:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 14 passed, 20 failed
# [9] Node22/Node24 V8-isolate-required green: PASS
```

The overall verifier remains red honestly. Node26 still has 10 required gaps,
and the broader NDS proof/closeout rows are not complete. PR #10 remains draft
and unmerged.

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
    "gaps": 10,
    "pass_rate_percent": 99.52,
    "passed": 2082,
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
test/parallel/test-trace-events-api.js
test/parallel/test-fs-promises-watch-ignore-invalid.mjs
test/parallel/test-fs-promises-watch.js
test/parallel/test-fs-sir-writes-alot.js
test/parallel/test-fs-stat-temporal.mjs
test/parallel/test-fs-write-buffer-large.js
```

## Next Recommended Cluster

Continue with the remaining 10 Node26 required gaps. Best next clusters by
likely shared root cause:

- Async lifecycle and tracing: `test-async-hooks-fatal-error.js`,
  `test-async-local-storage-weak-asyncwrap-leak.js`, and
  `test-trace-events-api.js`.
- Local fs/host I/O residual: the five `test-fs-*` entries.
- Stream semantics residual: `test-stream2-basic.js`.
- Loader/vm residual: `test-vm-module-evaluate-while-evaluating.js`.
