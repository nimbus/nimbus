# NDS3 node26 cycle 32 - console inspect symbol-label promotion

Date: 2026-06-16
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

This checkpoint burns 4 Node26 Current required gaps from the
`core-semantics/console` owner:

- `test/parallel/test-console-diagnostics-channels.js`
- `test/parallel/test-console-issue-43095.js`
- `test/parallel/test-console-with-frozen-intrinsics.js`
- `test/parallel/test-console.js`

The implementation lives in the Nimbus Deno fork:

- Repo: `/Users/jack/src/github.com/nimbus/deno`
- Branch: `nimbus/v2.8.3`
- Commit: `bcb2fde97fe23134681cd69ed2d1bde7d866b0ba`
- Tag: `v2.8.3-nimbus.63`
- Commit subject: `node: align console inspect symbol labels by lane`
- Changed file: `ext/node/polyfills/internal/console/constructor.mjs`

Nimbus is repinned from immutable Deno tag `v2.8.3-nimbus.62` to
`v2.8.3-nimbus.63`. `rusty_v8` is unchanged at `v149.4.0-nimbus.2`.

Node26 `v8_isolate_required` posture moved from `25` gaps / `98.8%`
(`2067 / 2092`) to `21` gaps / `99.0%` (`2071 / 2092`). Node22 and Node24
remain green at `0` gaps / `100.0%`.

No V8 or rusty_v8 changes were made. No official upstream Node fixture or
checker was edited. No generated JSON was hand-edited to fake a green. No
`git add -A` was used.

## Deno Fork Change

`ext/node/polyfills/internal/console/constructor.mjs` now chooses Console's
default `nodejsSymbolKeysWithoutBrackets` option by compatibility lane:

- Node22 keeps bracketed symbol property labels, matching the v22 fixture:
  `[Symbol(nodejs.util.inspect.custom)]`.
- Node24 and Node26 use unbracketed symbol property labels, matching the v24
  and v26 fixtures: `Symbol(nodejs.util.inspect.custom)`.
- Non-Nimbus contexts fall back to `process.versions.node` and use the Node24+
  behavior for major versions 24 and newer.

The owner-wide Node26 console batch initially failed on `v2.8.3-nimbus.62`
because `test-console.js` expected the Node26 unbracketed label while the
runtime printed the older bracketed form:

```text
+  [Symbol(nodejs.util.inspect.custom)]: [Function: [nodejs.util.inspect.custom]]
-  Symbol(nodejs.util.inspect.custom): [Function: [nodejs.util.inspect.custom]]
```

The first local Deno proof used only a narrow Cargo override for `deno_node`,
which failed at compile time because it produced duplicate `deno_core`,
`node_resolver`, and `deno_fs` type identities across the Deno-family crates:

```text
/private/tmp/nds-node26-cycle32-console-local-deno-broad1
# compile failure, not a fixture failure
```

The successful local proof temporarily pinned the full Deno-family patch set to
`/Users/jack/src/github.com/nimbus/deno`, then restored Nimbus to the immutable
published tag before promotion. That local proof exposed and fixed a real Node22
regression before publication: the unguarded Node24+ behavior made Node22
`test-console.js` print unbracketed labels. The final fork change uses the
Nimbus lane global first, so Node22 and Node26 both match their official fixture
expectations.

`deno fmt --check ext/node/polyfills/internal/console/constructor.mjs` is not a
checkpoint gate because the upstream file is an IIFE-wrapped polyfill and the
formatter wants to reindent the whole file. The fork diff itself is whitespace
clean:

```bash
git diff --check v2.8.3-nimbus.62..v2.8.3-nimbus.63 -- ext/node/polyfills/internal/console/constructor.mjs
# pass
```

## Proof Commands

Initial immutable-tag broad pre-run on `v2.8.3-nimbus.62`:

```bash
env NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle32-console-broad2 \
  cargo test -p nimbus-runtime --lib node26_current_lane_core_semantics_console_required_gap_watchpoint -- --ignored --nocapture
# selected=4, passed=3, skipped=0, failed=1
# failed: test/parallel/test-console.js
```

Local Deno proof before publishing the fork tag:

```bash
env NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle32-console-local-deno-broad3 \
  cargo test -p nimbus-runtime --lib node26_current_lane_core_semantics_console_required_gap_watchpoint -- --ignored --nocapture
# selected=4, passed=4, skipped=0, failed=0
```

Node22 regression guard after adding the lane-specific default:

```bash
env NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle32-console-local-deno-node22-cycle31-v2 \
  cargo test -p nimbus-runtime --lib node22_supported_lane_executes_cycle31_console_batch -- --nocapture
# selected=1, passed=1, skipped=0, failed=0
```

Immutable tag proof after publishing `v2.8.3-nimbus.63`:

```bash
env NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle32-console-tag63-broad1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_core_semantics_console_required_gap_watchpoint -- --ignored --nocapture
# selected=4, passed=4, skipped=0, failed=0
```

```bash
env NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle32-console-tag63-node22-cycle31 \
  cargo test -p nimbus-runtime --lib node22_supported_lane_executes_cycle31_console_batch -- --nocapture
# selected=1, passed=1, skipped=0, failed=0
```

After the four console fixtures were moved into
`CORE_SEMANTICS_CONSOLE_PROMOTED_NODE26_PATHS`, the durable non-ignored
promoted batch proved the Node26 console set on the immutable tag:

```bash
env NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle32-console-tag63-promoted1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_core_semantics_console_promoted_batch_fixture -- --nocapture
# selected=4, passed=4, skipped=0, failed=0
```

Summary artifacts:

```text
/private/tmp/nds-node26-cycle32-console-tag63-broad1/batch/node26__node26_current_lane_core_semantics_console_required_gap_watchpoint__summary.json
/private/tmp/nds-node26-cycle32-console-tag63-node22-cycle31/batch/node22__node22_supported_lane_executes_cycle31_console_batch__summary.json
/private/tmp/nds-node26-cycle32-console-tag63-promoted1/batch/node26__node26_current_lane_executes_core_semantics_console_promoted_batch__summary.json
```

## Generator And Integrity Checks

```bash
python3 -B scripts/runtime/node/watchpoints.py sync
# wrote tests/runtime/node/expectations/rust-watchpoints.json

python3 -B scripts/runtime/node/watchpoints.py validate
# validated node-compat watchpoint catalog: 154 entries

python3 -B scripts/runtime/node/status.py
# wrote target/node-compat/status/status-summary.{json,md}

python3 -B scripts/runtime/node/classifications.py sync --lane node26
# wrote tests/runtime/node/classifications/node26.json

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

The downstream evidence generators were rerun after the Node26 classification
sync so posture reflected the promoted fixtures.

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

The overall verifier remains red honestly because the broader NDS closeout proof
rows are incomplete and Node26 still has `21` Current-lane required gaps. PR #10
remains draft and unmerged.

## Current Posture

Generated `docs/architecture/runtime/node-default-support-posture.json` after
this checkpoint:

```text
node22 v8_isolate_required.gaps = 0, pass_rate_percent = 100.0
node24 v8_isolate_required.gaps = 0, pass_rate_percent = 100.0
node26 v8_isolate_required.gaps = 21, pass_rate_percent = 99.0
```

Remaining Node26 required gaps by generated owner:

```text
loader-context/vm: 1
  test/parallel/test-vm-module-evaluate-while-evaluating.js

node-compat/unpromoted-surface: 8
  test/parallel/test-async-hooks-fatal-error.js
  test/parallel/test-async-local-storage-weak-asyncwrap-leak.js
  test/parallel/test-blob-file-backed.js
  test/parallel/test-stream2-basic.js
  test/parallel/test-structuredClone-global.js
  test/parallel/test-trace-events-api.js
  test/parallel/test-webstreams-clone-unref.js
  test/parallel/test-whatwg-webstreams-transform-stream-members.js

runtime/v8: 7
  test/parallel/test-v8-collect-gc-profile-exit-before-stop.js
  test/parallel/test-v8-collect-gc-profile-using.js
  test/parallel/test-v8-collect-gc-profile.js
  test/parallel/test-v8-getheapsnapshot-twice.js
  test/parallel/test-v8-global-setter.js
  test/parallel/test-v8-heap-profile.js
  test/parallel/test-v8-string-is-one-byte-representation.js

streams-local-io/fs-host-io: 5
  test/parallel/test-fs-promises-watch-ignore-invalid.mjs
  test/parallel/test-fs-promises-watch.js
  test/parallel/test-fs-sir-writes-alot.js
  test/parallel/test-fs-stat-temporal.mjs
  test/parallel/test-fs-write-buffer-large.js
```
