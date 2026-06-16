# NDS3 node26 cycle 33 - file-backed Blob promotion

Date: 2026-06-16
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

This checkpoint burns 1 Node26 Current required gap from the
`node-compat/unpromoted-surface` / web-platform residual set:

- `test/parallel/test-blob-file-backed.js`

The implementation lives in the Nimbus Deno fork:

- Repo: `/Users/jack/src/github.com/nimbus/deno`
- Branch: `nimbus/v2.8.3`
- Commit: `a51bd04f0a6de0ed231e320288550dba3fdcc539`
- Tag: `v2.8.3-nimbus.65`
- Commit subject: `web: expose Node file-backed Blob clone errors`
- Changed file: `ext/web/13_message_port.js`

This tag includes the immediately previous Deno fork commit:

- Commit: `06e7b4739a`
- Tag: `v2.8.3-nimbus.64`
- Commit subject: `web: validate file-backed Blob reads`
- Changed file: `ext/web/09_file.js`

`v2.8.3-nimbus.64` is published but incomplete for Nimbus promotion: it fixed
the stale file-backed read path but did not convert file-backed Blob clone
failures to Node's `ERR_INVALID_STATE` error shape. Nimbus is pinned to
`v2.8.3-nimbus.65`, not `.64`.

Nimbus is repinned from immutable Deno tag `v2.8.3-nimbus.63` to
`v2.8.3-nimbus.65`. `rusty_v8` is unchanged at `v149.4.0-nimbus.2`.

Node26 `v8_isolate_required` posture moved from `21` gaps / `99.0%`
(`2071 / 2092`) to `20` gaps / `99.04%` (`2072 / 2092`). Node22 and Node24
remain green at `0` gaps / `100.0%`.

No V8 or rusty_v8 changes were made. No official upstream Node fixture or
checker was edited. No generated JSON was hand-edited to fake a green. No
`git add -A` was used.

## Deno Fork Change

`ext/web/09_file.js` now preserves the checker passed by Node's
`fs.openAsBlob()` path:

- `Blob` stores a file-backed readability checker.
- `Blob.slice()` propagates the checker.
- `Blob.stream()` and `Blob#u8Array()` call the checker before each read.
- Failed checks map to a `NotReadableError` DOMException for the file-backed
  read path.

`ext/web/13_message_port.js` now maps the file-backed Blob structured-clone
failure into Node's expected error shape when Nimbus is running a Node
compatibility lane:

- message: `Invalid state: File-backed Blobs are not cloneable`
- error type: `TypeError`
- error code: `ERR_INVALID_STATE`

The lane guard intentionally keeps this compatibility mapping out of generic
Deno web contexts.

## Proof Commands

Local Deno proof before publishing `.64` / `.65`:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle33-blob-file-backed-local-deno-focused3 \
  cargo test -p nimbus-runtime --lib node26_current_lane_blob_file_backed_watchpoint -- --ignored --nocapture
# selected=1, passed=1, skipped=0, failed=0
```

Summary artifact:

```text
/private/tmp/nds-node26-cycle33-blob-file-backed-local-deno-focused3/batch/node26__node26_current_lane_blob_file_backed_watchpoint__summary.json
```

Immutable `.64` proof, retained as the superseded-tag failure:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle33-blob-file-backed-tag64-focused1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_blob_file_backed_watchpoint -- --ignored --nocapture
# selected=1, passed=0, skipped=0, failed=1
```

The diagnostic confirms why `.64` was not promotable:

```text
/private/tmp/nds-node26-cycle33-blob-file-backed-tag64-focused1/general/node26__test_parallel_test_blob_file_backed_js.json

AssertionError [ERR_ASSERTION]: Expected values to be strictly deep-equal:
+ actual - expected

  Comparison {
+   code: 25,
-   code: 'ERR_INVALID_STATE',
    message: 'Invalid state: File-backed Blobs are not cloneable'
  }
```

Immutable `.65` focused proof:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle33-blob-file-backed-tag65-focused1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_blob_file_backed_watchpoint -- --ignored --nocapture
# selected=1, passed=1, skipped=0, failed=0
```

Summary artifact:

```text
/private/tmp/nds-node26-cycle33-blob-file-backed-tag65-focused1/batch/node26__node26_current_lane_blob_file_backed_watchpoint__summary.json
```

After promotion, `test/parallel/test-blob-file-backed.js` was added to the
existing non-ignored Node26 streams/web-platform promoted batch. The durable
promoted proof on immutable `.65` passed:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle33-blob-file-backed-tag65-promoted1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_streams_web_platform_promoted_batch_fixture -- --nocapture
# selected=119, passed=119, skipped=0, failed=0
```

Summary artifact:

```text
/private/tmp/nds-node26-cycle33-blob-file-backed-tag65-promoted1/batch/node26__node26_current_lane_executes_streams_web_platform_promoted_batch__summary.json
```

The structuredClone sibling was not promoted. The focused local-Deno diagnostic
remains a timeout and needs separate root-cause work:

```text
/private/tmp/nds-node26-cycle33-structured-clone-local-deno-focused2/general/node26__test_parallel_test_structuredClone_global_js.json
# outcome=wall_clock_timeout, timeout_ms=35000, elapsed_ms=35003
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

Several generator commands were rerun with narrow filesystem escalation because
the worktree `target/` directory had been cleaned for disk recovery and the
managed sandbox blocked recreated target/generated-file writes. The commands
and outputs above are the effective generator results.

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

The overall verifier remains red honestly. In this checkout, the red conditions
include broader NDS closeout/proof-row gates and Node26 Current evidence
completion. PR #10 remains draft and unmerged.

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
    "gaps": 20,
    "pass_rate_percent": 99.04,
    "passed": 2072,
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
test/parallel/test-v8-collect-gc-profile-exit-before-stop.js
test/parallel/test-v8-collect-gc-profile-using.js
test/parallel/test-v8-collect-gc-profile.js
test/parallel/test-v8-getheapsnapshot-twice.js
test/parallel/test-v8-global-setter.js
test/parallel/test-v8-heap-profile.js
test/parallel/test-v8-string-is-one-byte-representation.js
test/parallel/test-fs-promises-watch-ignore-invalid.mjs
test/parallel/test-fs-promises-watch.js
test/parallel/test-fs-sir-writes-alot.js
test/parallel/test-fs-stat-temporal.mjs
test/parallel/test-fs-write-buffer-large.js
```

## Next Recommended Cluster

Continue with a broad Node26 ROI scan over the remaining 20 gaps. Best next
clusters by likely shared root cause:

- Async lifecycle: `test-async-hooks-fatal-error.js`,
  `test-async-local-storage-weak-asyncwrap-leak.js`.
- WebStreams/structuredClone/stream residual: `test-stream2-basic.js`,
  `test-structuredClone-global.js`, `test-webstreams-clone-unref.js`,
  `test-whatwg-webstreams-transform-stream-members.js`.
- Local fs/host I/O residual: the five `test-fs-*` entries.
- V8/native profiler surface: the six `test-v8-*` entries likely need
  structural reclassification or native support analysis, but do not patch V8
  or rusty_v8 on this host.
