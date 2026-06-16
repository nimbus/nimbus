# NDS3 node26 cycle 27 - URLPattern and permission diagnostics

Date: 2026-06-15
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

This wave burns 4 Node26 Current required gaps from
`node-compat/unpromoted-surface`:

- `test/parallel/test-permission-diagnostics-channel.js`
- `test/parallel/test-urlpattern-invalidthis.js`
- `test/parallel/test-urlpattern-types.js`
- `test/parallel/test-urlpattern.js`

Node26 `v8_isolate_required` posture moved from `67` gaps / `96.8%`
(`2030 / 2097`) to `63` gaps / `97.0%` (`2034 / 2097`). Node22 and Node24
remain green at `0` gaps / `100.0%`.

Deno was updated from `v2.8.3-nimbus.58`
(`cf321f2394ffd51ca56fffe7636f52beb7174f2a`) to `v2.8.3-nimbus.59`
(`305e355ff255a05d45456ad5576427a46d79ac23`). rusty_v8 was unchanged at
`v149.4.0-nimbus.2`.

No V8 or rusty_v8 changes were made. No official upstream Node fixture or
checker was edited. No generated JSON was hand-edited to fake a green. No
`git add -A` was used.

## Fork Changes

The Deno fork commit is:

```bash
git -C /Users/jack/src/github.com/nimbus/deno show --stat --oneline -1
# 305e355ff2 node: expose URLPattern and permission diagnostics
# ext/node/polyfills/process.ts | 106 +++++++++++++++++++++
# ext/node/polyfills/url.ts     | 215 ++++++++++++++++++++++++++++++++++++++++++
```

`ext/node/polyfills/url.ts` now exposes `URLPattern` from `node:url` by
delegating parsing and matching to Deno's upstream web `URLPattern`
implementation while adding the Node API boundary behavior required by the
fixtures: construct-call enforcement, Node argument type errors, Node-style
three-argument baseURL handling, copied prototype descriptors for brand/getter
shape, and `ERR_INVALID_URL_PATTERN` / `ERR_OPERATION_FAILED` error codes where
Node's native URLPattern reports them.

`ext/node/polyfills/process.ts` now exposes `process.permission.has()` and
`process.permission.drop()` for the Node permission diagnostic surface used by
the fixture. The implementation is intentionally bounded to the Node fixture's
permission-model diagnostics path: fs read/write scopes, fixture `execArgv`
flags, and `node:permission-model:fs` diagnostic-channel publishing on denied
access. It does not grant host process capabilities or mutate Nimbus sandbox
boundaries.

Nimbus also added the permission-related Node flags to the node-compat harness
allowlist so fixture headers such as `// Flags: --permission
--allow-fs-read=*` reach `process.execArgv`:

- `--permission`
- `--permission-audit`
- `--allow-fs-read=...`
- `--allow-fs-write=...`

## Local Path Proof

Nimbus was temporarily pinned to the canonical local Deno worktree while the
fork change was developed. The final local-path focused proof passed:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave27-urlpattern-permission-local4 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_unpromoted_surface_promoted_batch_fixture -- --nocapture
# selected=18, passed=18, skipped=0, failed=0
```

The focused summary artifact is:

```text
/private/tmp/nds-node26-wave27-urlpattern-permission-local4/batch/node26__node26_current_lane_executes_unpromoted_surface_promoted_batch__summary.json
```

An earlier local iteration failed with the two root causes fixed in this wave:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave27-urlpattern-permission-local3 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_unpromoted_surface_promoted_batch_fixture -- --nocapture
# selected=18, passed=16, skipped=0, failed=2
# failures:
# - test/parallel/test-permission-diagnostics-channel.js
# - test/parallel/test-urlpattern-types.js
```

`test-permission-diagnostics-channel.js` was failing because Nimbus filtered
the fixture's permission flags out of `process.execArgv`. `test-urlpattern-types.js`
was failing on the dict-input-plus-baseURL `ERR_OPERATION_FAILED` assertion.

## Broad Batch Proof

The owner-wide broad watchpoint was rerun against the local Deno path after the
focused batch passed:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave27-unpromoted-surface-broad-local1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_unpromoted_surface_required_gap_watchpoint -- --ignored --nocapture
# selected=22
# interrupted with ctrl-c after test/parallel/test-webstreams-clone-unref.js hung
```

The run reached and moved past all four promoted candidates:

- `test/parallel/test-permission-diagnostics-channel.js`
- `test/parallel/test-urlpattern-invalidthis.js`
- `test/parallel/test-urlpattern-types.js`
- `test/parallel/test-urlpattern.js`

The interrupt happened only after `test/parallel/test-webstreams-clone-unref.js`
stayed live in the known WebStreams hang. A process check after interrupt found
no leftover `cargo`, `rustc`, `nextest`, `nimbus_runtime`, or `nds_probe`
process.

The broad diagnostic root retained diagnostics for unrelated remaining
required-gap groups:

- DNS async-resource accounting:
  `test/async-hooks/test-getaddrinforeqwrap.js`,
  `test/async-hooks/test-getnameinforeqwrap.js`,
  `test/async-hooks/test-querywrap.js`.
- Async lifecycle residuals:
  `test/parallel/test-async-hooks-fatal-error.js`,
  `test/parallel/test-async-local-storage-weak-asyncwrap-leak.js`.
- FFI/embedding helper or host-surface gaps:
  `test/ffi/test-ffi-module.js`,
  `test/ffi/test-ffi-shared-buffer.js`,
  `test/embedding/test-embedding-snapshot-vm.js`.
- Residual API/error-shape gaps:
  `test/parallel/test-blob-file-backed.js`,
  `test/parallel/test-stream2-basic.js`,
  `test/parallel/test-structuredClone-global.js`,
  `test/parallel/test-trace-events-api.js`.
- WebStreams hang:
  `test/parallel/test-webstreams-clone-unref.js`.

Diagnostic root:

```text
/private/tmp/nds-node26-wave27-unpromoted-surface-broad-local1
```

## Immutable Tag Proof

After the local proof, the Deno fork was committed, tagged, and pushed:

```bash
git -C /Users/jack/src/github.com/nimbus/deno commit -m "node: expose URLPattern and permission diagnostics"
# [nimbus/v2.8.3 305e355ff2] node: expose URLPattern and permission diagnostics

git -C /Users/jack/src/github.com/nimbus/deno tag -a v2.8.3-nimbus.59 -m "v2.8.3-nimbus.59"

git -C /Users/jack/src/github.com/nimbus/deno push origin nimbus/v2.8.3 v2.8.3-nimbus.59
# cf321f2394..305e355ff2  nimbus/v2.8.3 -> nimbus/v2.8.3
# [new tag]               v2.8.3-nimbus.59 -> v2.8.3-nimbus.59
```

Nimbus was repinned from the temporary local Deno path back to immutable
`https://github.com/nimbus/deno`, tag `v2.8.3-nimbus.59`. Cargo.lock resolved
the Deno-family crates to `305e355f`.

The tag-pinned focused proof passed:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave27-urlpattern-permission-tag1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_unpromoted_surface_promoted_batch_fixture -- --nocapture
# selected=18, passed=18, skipped=0, failed=0
```

The tag-pinned summary artifact is:

```text
/private/tmp/nds-node26-wave27-urlpattern-permission-tag1/batch/node26__node26_current_lane_executes_unpromoted_surface_promoted_batch__summary.json
```

## Fork Integrity Checks

```bash
git -C /Users/jack/src/github.com/nimbus/deno diff --check
# no output

LC_ALL=C grep -n '[^ -~]' ext/node/polyfills/url.ts ext/node/polyfills/process.ts
# no output

git -C /Users/jack/src/github.com/nimbus/deno status --short --branch
# ## nimbus/v2.8.3

git -C /Users/jack/src/github.com/nimbus/deno describe --tags --exact-match HEAD
# v2.8.3-nimbus.59
```

`deno fmt --check ext/node/polyfills/url.ts ext/node/polyfills/process.ts` was
not used as a pass/fail gate for this fork checkpoint because the current Deno
bootstrap IIFE formatting causes whole-file reindent diffs unrelated to this
wave.

## Generator and Integrity Checks

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
# wrote node20, node22, node24, node26 classification catalogs

/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py sync
# wrote tests/runtime/node/expectations/rust-watchpoints.json

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

cargo fmt --all --check
# no output

/opt/homebrew/bin/python3.12 -B scripts/runtime/node/docs_guard.py
# Node LTS docs guard passed: public docs avoid stale pass-rate, support-priority, and host-heavy overclaim prose

git diff --check
# no output
```

Regenerated public posture:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`, `2363 / 2363`.
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`, `2400 / 2400`.
- Node26 `v8_isolate_required`: `63` gaps, `97.0%`, `2034 / 2097`.

Verifier checkpoint:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 14 passed, 20 failed
```

Step 9 remains green for Node22/Node24. The verifier remains red honestly
because the broader NDS closeout proof rows are incomplete and Node26 still has
`63` required gaps.

## Remaining Node26 Required Gaps

After this wave:

- `loader-context/vm`: 23
- `node-compat/unpromoted-surface`: 18
- `runtime/v8`: 7
- `process-and-timing/process-host`: 6
- `streams-local-io/fs-host-io`: 5
- `core-semantics/console`: 4

Recommended next action: stay on `node-compat/unpromoted-surface` while the
cluster is hot. The remaining high-yield groups are async-resource DNS
accounting, async lifecycle residuals, Blob/structuredClone/stream2/trace-events
error-shape gaps, and the WebStreams hang. Keep VM-module work separate because
it includes the known V8/deno_core boundary blockers.
