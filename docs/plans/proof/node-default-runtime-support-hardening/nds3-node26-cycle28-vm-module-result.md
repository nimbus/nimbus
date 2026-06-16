# NDS3 node26 cycle 28 - VM module promotion

Date: 2026-06-15
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

This wave burns 22 Node26 Current required gaps from `loader-context/vm` by
promoting the VM module fixtures that now pass on the `v2.8.3-nimbus.60` Deno
fork tag.

Node26 `v8_isolate_required` posture moved from `63` gaps / `97.0%`
(`2034 / 2097`) to `41` gaps / `98.04%` (`2056 / 2097`). Node22 and Node24
remain green at `0` gaps / `100.0%`.

Deno was updated from `v2.8.3-nimbus.59`
(`305e355ff255a05d45456ad5576427a46d79ac23`) to `v2.8.3-nimbus.60`
(`d7edcf7ab9b49c317849601cbe359e8db1939cdf`). rusty_v8 was unchanged at
`v149.4.0-nimbus.2`.

No V8 or rusty_v8 changes were made. No official upstream Node fixture or
checker was edited. No generated JSON was hand-edited to fake a green. No
`git add -A` was used.

## Fork Change

The Deno fork commit is:

```bash
git -C /Users/jack/src/github.com/nimbus/deno show --stat --oneline -1
# d7edcf7ab9 node: accept VM dynamic import namespaces
# ext/node/polyfills/vm.js | 4 ++++
```

`ext/node/polyfills/vm.js` now accepts a module namespace object returned by a
VM `importModuleDynamically` hook. The implementation uses the existing
`core.isModuleNamespaceObject()` predicate and returns namespace objects
directly from `finishDynamicImportResult(result)`. That preserves the existing
`vm.Module` path and keeps non-module values on the `ERR_VM_MODULE_NOT_MODULE`
error path.

The fork was committed, tagged, and pushed before Nimbus was repinned:

```bash
git -C /Users/jack/src/github.com/nimbus/deno commit -m "node: accept VM dynamic import namespaces"
# [nimbus/v2.8.3 d7edcf7ab9] node: accept VM dynamic import namespaces

git -C /Users/jack/src/github.com/nimbus/deno tag -a v2.8.3-nimbus.60 -m "v2.8.3-nimbus.60"

git -C /Users/jack/src/github.com/nimbus/deno push origin nimbus/v2.8.3 v2.8.3-nimbus.60
# 305e355ff2..d7edcf7ab9  nimbus/v2.8.3 -> nimbus/v2.8.3
# [new tag]               v2.8.3-nimbus.60 -> v2.8.3-nimbus.60
```

Nimbus is pinned back to immutable `https://github.com/nimbus/deno`, tag
`v2.8.3-nimbus.60`. `Cargo.lock` resolves Deno-family crates to
`d7edcf7ab9b49c317849601cbe359e8db1939cdf`.

Fork state at checkpoint:

```bash
git -C /Users/jack/src/github.com/nimbus/deno status --short --branch
# ## nimbus/v2.8.3

git -C /Users/jack/src/github.com/nimbus/deno describe --tags --exact-match HEAD
# v2.8.3-nimbus.60
```

## Broad Batch Proof

The first broad run selected zero fixtures because the broad VM selector still
excluded all `test-vm-module-*` paths through a stale fatal-abort prefix:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave28-vm-broad1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_loader_context_vm_watchpoint -- --ignored --nocapture
# selected=0, passed=0, skipped=0, failed=0
```

The selector was corrected to exclude only the exact known fatal path,
`test/parallel/test-vm-module-evaluate-while-evaluating.js`. The broad batch
then exposed the one fork-owned failure fixed in this wave:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave28-vm-broad2 \
  cargo test -p nimbus-runtime --lib node26_current_lane_loader_context_vm_watchpoint -- --ignored --nocapture
# selected=22, passed=21, skipped=0, failed=1
# failure:
# - test/parallel/test-vm-module-dynamic-namespace.js
#   ERR_VM_MODULE_NOT_MODULE in finishDynamicImportResult()
```

After the Deno fork fix and immutable tag repin, the broad batch passed on the
published tag:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave28-vm-broad-tag1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_loader_context_vm_watchpoint -- --ignored --nocapture
# selected=22, passed=22, skipped=0, failed=0
```

The tag-pinned broad summary artifact is:

```text
/private/tmp/nds-node26-wave28-vm-broad-tag1/batch/node26__node26_current_lane_loader_context_vm_watchpoint__summary.json
```

Promoted VM module paths:

- `test/parallel/test-vm-module-after-evaluate.js`
- `test/parallel/test-vm-module-basic.js`
- `test/parallel/test-vm-module-cached-data.js`
- `test/parallel/test-vm-module-dynamic-import-promise.js`
- `test/parallel/test-vm-module-dynamic-import.js`
- `test/parallel/test-vm-module-dynamic-namespace.js`
- `test/parallel/test-vm-module-errors.js`
- `test/parallel/test-vm-module-evaluate-source-text-module.js`
- `test/parallel/test-vm-module-evaluate-synthethic-module-rejection.js`
- `test/parallel/test-vm-module-evaluate-synthethic-module.js`
- `test/parallel/test-vm-module-hasasyncgraph.js`
- `test/parallel/test-vm-module-hastoplevelawait.js`
- `test/parallel/test-vm-module-import-meta.js`
- `test/parallel/test-vm-module-instantiate.js`
- `test/parallel/test-vm-module-link-shared-deps.js`
- `test/parallel/test-vm-module-link.js`
- `test/parallel/test-vm-module-linkmodulerequests-circular.js`
- `test/parallel/test-vm-module-linkmodulerequests-deep.js`
- `test/parallel/test-vm-module-linkmodulerequests.js`
- `test/parallel/test-vm-module-reevaluate.js`
- `test/parallel/test-vm-module-referrer-realm.mjs`
- `test/parallel/test-vm-module-synthetic.js`

## Local Path And Promoted Batch Proof

Nimbus was temporarily pinned to the canonical local Deno worktree while the
fork change was developed. The local broad batch passed after the fork fix:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave28-vm-local2 \
  cargo test -p nimbus-runtime --lib node26_current_lane_loader_context_vm_watchpoint -- --ignored --nocapture
# selected=22, passed=22, skipped=0, failed=0
```

The local-path promoted batch, before the module paths were added to the
non-ignored Node26 promoted list, passed the existing 63-fixture VM batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave28-vm-promoted-local1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_loader_context_vm_promoted_batch_fixture -- --nocapture
# selected=63, passed=63, skipped=0, failed=0
```

After repinning to the immutable Deno tag, the same 63-fixture promoted batch
passed:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave28-vm-promoted-tag1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_loader_context_vm_promoted_batch_fixture -- --nocapture
# selected=63, passed=63, skipped=0, failed=0
```

During checkpoint review, the 22 newly proven VM module paths were found in the
common Node22/Node24/Node26 VM promoted list instead of the Node26-specific
promoted list. They were moved into `LOADER_CONTEXT_VM_PROMOTED_NODE26_PATHS`
so the non-ignored Node26 fixture inventory exactly matches this wave's proof
scope. The corrected tag-pinned promoted batch passed:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave28-vm-promoted-tag2 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_loader_context_vm_promoted_batch_fixture -- --nocapture
# selected=85, passed=85, skipped=0, failed=0
```

The corrected tag-pinned promoted summary artifact is:

```text
/private/tmp/nds-node26-wave28-vm-promoted-tag2/batch/node26__node26_current_lane_executes_loader_context_vm_promoted_batch__summary.json
```

## Remaining VM Gap

The only remaining Node26 `loader-context/vm` required gap is:

- `test/parallel/test-vm-module-evaluate-while-evaluating.js`

It remains excluded from the broad selector by exact path because it is the
known fatal-evaluation path for this cluster.

## Generator And Integrity Checks

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py sync
# wrote tests/runtime/node/expectations/rust-watchpoints.json

/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
# wrote node20, node22, node24, node26 classification catalogs

/opt/homebrew/bin/python3.12 scripts/runtime/node/status.py
# wrote target/node-compat/status/status-summary.{json,md}

/opt/homebrew/bin/python3.12 scripts/runtime/node/dashboard.py
# wrote target/node-compat/dashboard/dashboard-summary.{json,md}

/opt/homebrew/bin/python3.12 scripts/runtime/node/trends.py
# wrote target/node-compat/trends/trend-summary.{json,md}

/opt/homebrew/bin/python3.12 scripts/runtime/node/publish_evidence.py
# published tests/runtime/node/compat/node-compat-evidence/latest/*

/opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py
# wrote private and public node-default-support-posture artifacts

/opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py
# node22 required gaps: 0
# node24 required gaps: 0

/opt/homebrew/bin/python3.12 -B scripts/runtime/node/classifications.py sync --preserve-existing --check
# node20.json, node22.json, node24.json, node26.json are up to date

/opt/homebrew/bin/python3.12 -B scripts/runtime/node/default_support_posture.py --check
# node default support posture: pass

/opt/homebrew/bin/python3.12 -B scripts/runtime/node/required_surface_blockers.py --check
# node required-surface blocker inventory: pass

/opt/homebrew/bin/python3.12 -B scripts/runtime/node/watchpoints.py validate
# validated node-compat watchpoint catalog: 153 entries

/opt/homebrew/bin/python3.12 -B scripts/runtime/node/docs_guard.py
# Node LTS docs guard passed: public docs avoid stale pass-rate, support-priority, and host-heavy overclaim prose

cargo fmt --all --check
# pass

git diff --check
# no output
```

The NDS verifier remains red because the broader NDS closeout and Node26
Current completion work is still in progress, but the Node22/Node24 required
gate stays green:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 14 passed, 20 failed
# [9] Node22/Node24 V8-isolate-required green: PASS
```

Current public posture after this checkpoint:

```bash
jq '.lanes.node22.v8_isolate_required, .lanes.node24.v8_isolate_required, .lanes.node26.v8_isolate_required' \
  docs/architecture/runtime/node-default-support-posture.json
# node22: gaps=0, pass_rate_percent=100.0, passed=2363, total=2363
# node24: gaps=0, pass_rate_percent=100.0, passed=2400, total=2400
# node26: gaps=41, pass_rate_percent=98.04, passed=2056, total=2097
```

## Next Recommended Cluster

The remaining Node26 `v8_isolate_required` gaps are:

- `node-compat/unpromoted-surface`: 18
- `runtime/v8`: 7
- `process-and-timing/process-host`: 6
- `streams-local-io/fs-host-io`: 5
- `core-semantics/console`: 4
- `loader-context/vm`: 1

The next highest-yield implementation wave is still
`node-compat/unpromoted-surface`, but it contains known mixed root causes and
a WebStreams hang. A quick lower-risk implementation pass over
`process-and-timing/process-host`, `streams-local-io/fs-host-io`, or
`core-semantics/console` may be a better throughput move if the broad ignored
batches prove green or isolate a small root cause.
