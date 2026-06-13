# NDS3 cycle 61 - public promise hooks

Date: 2026-06-13
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

The following fixtures were dynamically promoted for node22 and node24:

- `test/parallel/test-promise-hook-create-hook.js`
- `test/parallel/test-promise-hook-exceptions.js`
- `test/parallel/test-promise-hook-on-after.js`
- `test/parallel/test-promise-hook-on-resolve.js`

Gate movement from the generated private posture:

- node22: 38 -> 34 gaps, 98.56% pass rate
- node24: 45 -> 41 gaps, 98.29% pass rate
- unique remaining required fixtures: 43

Deno fork tag: `v2.8.3-nimbus.15` (`f31e3cdd80`). Nimbus was repinned to the
published tag after local proof; the temporary local Deno path override was
removed before immutable-tag verification.

## Root Cause

The public `node:v8` polyfill did not expose Node's `promiseHooks` API surface,
so the official promise-hook fixtures failed on missing API shape or callback
ordering. A first native `core.setPromiseHooks` approach proved the low-level V8
hook path was insufficient for this public surface in Nimbus because it surfaced
Deno-internal and module-loader promises in an order that the Node fixtures do
not accept.

The fork fix implements the public `v8.promiseHooks` API in
`ext/node/polyfills/v8.ts` with lazy JavaScript-level `Promise`
instrumentation. It exposes `createHook`, `onInit`, `onBefore`, `onAfter`, and
`onSettled`; validates plain functions with Node-style
`ERR_INVALID_ARG_TYPE`; routes hook exceptions through
`process._fatalException(error, false)` when available; preserves the original
`Promise` static/prototype surface; and deduplicates settled events. This keeps
the public API semantics aligned with Node while avoiding V8/rusty_v8 changes.

The fix stays in `deno_node` TypeScript/JavaScript polyfills and does not touch
V8/rusty_v8.

## Verification

Local-fork focused proof before publishing the Deno tag:

```bash
/opt/homebrew/bin/gtimeout -s KILL 120 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle61-async-lifecycle \
  cargo test -p nimbus-runtime --lib nds_cycle61_node24_public_promise_hooks_probe -- --ignored --nocapture
# node_compat nds-cycle61-node24-public-promise-hooks-probe node24 summary: selected=6, passed=6, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 857 filtered out; finished in 11.81s

/opt/homebrew/bin/gtimeout -s KILL 120 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle61-async-lifecycle \
  cargo test -p nimbus-runtime --lib nds_cycle61_node22_public_promise_hooks_probe -- --ignored --nocapture
# node_compat nds-cycle61-node22-public-promise-hooks-probe node22 summary: selected=6, passed=6, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 857 filtered out; finished in 11.40s
```

The six-fixture local probes included the four promoted fixtures plus adjacent
regression fixtures:

- `test/parallel/test-promise-hook-on-before.js`
- `test/parallel/test-promise-hook-on-init.js`

Local-fork broad async-lifecycle probe before publishing:

```bash
/opt/homebrew/bin/gtimeout -s KILL 120 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle61-async-lifecycle \
  cargo test -p nimbus-runtime --lib nds_cycle61_node22_async_lifecycle_probe -- --ignored --nocapture
# node_compat nds-cycle61-node22-async-lifecycle-probe node22 summary: selected=7, passed=4, skipped=0, failed=3

/opt/homebrew/bin/gtimeout -s KILL 120 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle61-async-lifecycle \
  cargo test -p nimbus-runtime --lib nds_cycle61_node24_async_lifecycle_probe -- --ignored --nocapture
# node_compat nds-cycle61-node24-async-lifecycle-probe node24 summary: selected=8, passed=4, skipped=0, failed=4
```

The remaining broad-batch failures are grouped as:

- promise internals / async_hooks: `test/parallel/test-heapdump-async-hooks-init-promise.js`,
  `test/parallel/test-promise-swallowed-event.js`
- perf timeout: `test/parallel/test-perf-hooks-eventlooputilization.js` on node24
- VM timeout/escape semantics: `test/parallel/test-vm-timeout-escape-promise-module.js`

Deno fork publish:

```bash
git add ext/node/polyfills/v8.ts
git commit -m "node(v8): add public promise hooks"
# [nimbus/v2.8.3 f31e3cdd80] node(v8): add public promise hooks
#  1 file changed, 178 insertions(+)

git tag v2.8.3-nimbus.15
git push origin nimbus/v2.8.3
# c99b5eb5d4..f31e3cdd80  nimbus/v2.8.3 -> nimbus/v2.8.3

git push origin v2.8.3-nimbus.15
# * [new tag]               v2.8.3-nimbus.15 -> v2.8.3-nimbus.15
```

Immutable-tag preparation:

```bash
cargo update -p deno_node
# Locking 40 packages to latest compatible versions
# deno_node v0.189.0: v2.8.3-nimbus.14#c99b5eb5 -> v2.8.3-nimbus.15#f31e3cdd

cargo clean -p deno_node
# Removed 541 files, 361.3MiB total
```

Immutable-tag focused probes after removing the local path override and
repinning Nimbus to `v2.8.3-nimbus.15`:

```bash
/opt/homebrew/bin/gtimeout -s KILL 120 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle61-async-lifecycle-repinned \
  cargo test -p nimbus-runtime --lib nds_cycle61_node24_public_promise_hooks_probe -- --ignored --nocapture
# node_compat nds-cycle61-node24-public-promise-hooks-probe node24 summary: selected=6, passed=6, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 857 filtered out; finished in 12.02s

/opt/homebrew/bin/gtimeout -s KILL 120 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle61-async-lifecycle-repinned \
  cargo test -p nimbus-runtime --lib nds_cycle61_node22_public_promise_hooks_probe -- --ignored --nocapture
# node_compat nds-cycle61-node22-public-promise-hooks-probe node22 summary: selected=6, passed=6, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 857 filtered out; finished in 11.69s
```

Immutable-tag broad async-lifecycle probe:

```bash
/opt/homebrew/bin/gtimeout -s KILL 120 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle61-async-lifecycle-repinned \
  cargo test -p nimbus-runtime --lib nds_cycle61_node22_async_lifecycle_probe -- --ignored --nocapture
# node_compat nds-cycle61-node22-async-lifecycle-probe node22 summary: selected=7, passed=4, skipped=0, failed=3

/opt/homebrew/bin/gtimeout -s KILL 120 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle61-async-lifecycle-repinned \
  cargo test -p nimbus-runtime --lib nds_cycle61_node24_async_lifecycle_probe -- --ignored --nocapture
# node_compat nds-cycle61-node24-async-lifecycle-probe node24 summary: selected=8, passed=4, skipped=0, failed=4
```

Real promotion guard:

```bash
cargo test -p nimbus-runtime --lib cycle61_public_promise_hooks_batch -- --nocapture
# node_compat node22-supported-lane-executes-cycle61-public-promise-hooks-batch node22 summary: selected=4, passed=4, skipped=0, failed=0
# node_compat node24-default-lane-executes-cycle61-public-promise-hooks-batch node24 summary: selected=4, passed=4, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 854 filtered out; finished in 15.35s
```

Regenerated lightweight posture/evidence pipeline:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do
  /opt/homebrew/bin/python3.12 scripts/runtime/node/$s.py >/dev/null
done
```

Generated private posture counts:

```text
node22 34 98.56
node24 41 98.29
unique remaining required fixtures: 43
```

The checked-in public evidence summaries moved four manifested fixtures per lane:

```text
node22 documented_manifested_green_count: 2328 -> 2332
node24 documented_manifested_green_count: 2358 -> 2362
total documented green fixtures: 6612 -> 6620
```

NDS verifier:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 13 passed, 21 failed
# Step 9 remains red because the generated posture is node22=34 / node24=41, not 0/0.
```
