# NDS3 node26 cycle 36 - fs residual and queryObjects promotion

Date: 2026-06-16
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

This checkpoint burns 5 Node26 Current required gaps:

- `test/parallel/test-fs-promises-watch-ignore-invalid.mjs`
- `test/parallel/test-fs-promises-watch.js`
- `test/parallel/test-fs-sir-writes-alot.js`
- `test/parallel/test-fs-write-buffer-large.js`
- `test/parallel/test-async-local-storage-weak-asyncwrap-leak.js`

The fs fixtures did not require a new implementation change in this cycle. They
were the passing members of the remaining Node26 fs-host-io residual batch and
are now enforced by the promoted fs-host-io batch. The one remaining fs member,
`test/parallel/test-fs-stat-temporal.mjs`, still skips because Temporal support
is not available and remains a required red path.

The `test-async-local-storage-weak-asyncwrap-leak.js` fix lives in the Nimbus
Deno fork:

- Repo: `/Users/jack/src/github.com/nimbus/deno`
- Branch: `nimbus/v2.8.3`
- Commit: `943929a4b57a0c44499ba4cfa8e53d7a05c44271`
- Tag: `v2.8.3-nimbus.68`
- Commit subject: `node: default v8 queryObjects to count`
- Changed files:
  - `ext/node/polyfills/v8.ts`
  - `tests/unit_node/util_test.ts`
  - `tests/unit_node/v8_test.ts`

Nimbus is repinned from immutable Deno tag `v2.8.3-nimbus.67` to
`v2.8.3-nimbus.68`. `rusty_v8` is unchanged at `v149.4.0-nimbus.2`.

Node26 `v8_isolate_required` posture moved from `10` gaps / `99.52%`
(`2082 / 2092`) to `5` gaps / `99.76%` (`2087 / 2092`). Node22 and Node24
remain green at `0` gaps / `100.0%`.

No V8 or rusty_v8 changes were made. No official upstream Node fixture or
checker was edited. No generated JSON was hand-edited to fake a green. No
`git add -A` was used.

## Root Cause

Deno's `v8.queryObjects()` polyfill defaulted to the summary array form when
callers omitted `options.format`. Node defaults to the count form unless
`{ format: "summary" }` is explicitly requested.

Node26's `test-async-local-storage-weak-asyncwrap-leak.js` calls
`v8.queryObjects(AsyncWrap)` without a format option and expects a number. The
old fork behavior returned an array, so the fixture failed before it could use
the count as the weak-reference leak sentinel.

## Fork Fix

`ext/node/polyfills/v8.ts` now defaults `format` to `"count"`:

```text
const format = options?.format ?? "count";
```

The summary-object path remains available only for `{ format: "summary" }`.
The Deno unit tests for both `node:v8` and `node:util` now assert the default
count form.

The change preserves sandbox boundaries. It only changes the return shape of an
in-isolate heap query polyfill. It does not add host process, signal,
subprocess, filesystem, network, or native authority.

## Proof Commands

Node26 fs residual broad batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle36-fs-residual-broad1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_fs_host_io_residual_watchpoint -- --ignored --nocapture
# selected=5, passed=4, skipped=1, failed=0
```

Summary artifact:

```text
/private/tmp/nds-node26-cycle36-fs-residual-broad1/batch/node26__node26_current_lane_fs_host_io_residual_watchpoint__summary.json
```

Promoted fs-host-io batch after adding the four passing residual fixtures:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle36-fs-residual-promoted1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_fs_host_io_promoted_batch_fixture -- --nocapture
# selected=146, passed=146, skipped=0, failed=0
```

Summary artifact:

```text
/private/tmp/nds-node26-cycle36-fs-residual-promoted1/batch/node26__node26_current_lane_executes_fs_host_io_promoted_batch__summary.json
```

Unpromoted-surface broad baseline before the Deno fix:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle36-unpromoted-surface-broad1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_unpromoted_surface_required_gap_watchpoint -- --ignored --nocapture
# selected=4, passed=0, skipped=0, failed=4
```

Summary artifact:

```text
/private/tmp/nds-node26-cycle36-unpromoted-surface-broad1/batch/node26__node26_current_lane_unpromoted_surface_required_gap_watchpoint__summary.json
```

Local Deno proof before publishing `.68`:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle36-unpromoted-surface-local-v8-queryobjects1 \
  cargo test -p nimbus-runtime [full local Deno patch list] \
    --lib node26_current_lane_unpromoted_surface_required_gap_watchpoint -- --ignored --nocapture
# selected=4, passed=1, skipped=0, failed=3
```

Summary artifact:

```text
/private/tmp/nds-node26-cycle36-unpromoted-surface-local-v8-queryobjects1/batch/node26__node26_current_lane_unpromoted_surface_required_gap_watchpoint__summary.json
```

The one passing fixture in that broad local proof was
`test/parallel/test-async-local-storage-weak-asyncwrap-leak.js`. The remaining
failures were:

```text
test/parallel/test-async-hooks-fatal-error.js
test/parallel/test-stream2-basic.js
test/parallel/test-trace-events-api.js
```

Exploratory local Deno edits to fs async drain behavior and child-process cwd
defaults were rerun and did not move the remaining failures. Those edits were
reverted before publishing `.68`; they are not part of the fork tag or Nimbus
checkpoint.

Deno fork hygiene:

```bash
git diff --check
# passed

cargo test -p unit_node_tests --test unit_node queryObjects -- --nocapture
# failed on macOS link flag: -fuse-ld=lld

CARGO_ENCODED_RUSTFLAGS= cargo test -p unit_node_tests --test unit_node queryObjects -- --nocapture
# exited 0, but did not provide a behavioral unit-test count because the local
# deno/test_server binaries were absent; this was treated only as a compile
# precheck, not as fixture proof.
```

Published fork tag:

```bash
git show --stat --oneline --decorate v2.8.3-nimbus.68
# 943929a4b5 (HEAD -> nimbus/v2.8.3, tag: v2.8.3-nimbus.68, origin/nimbus/v2.8.3, origin/HEAD) node: default v8 queryObjects to count
#  ext/node/polyfills/v8.ts     | 13 +++++--------
#  tests/unit_node/util_test.ts |  3 +++
#  tests/unit_node/v8_test.ts   |  3 +++
#  3 files changed, 11 insertions(+), 8 deletions(-)
```

Immutable `.68` broad proof after repinning Nimbus:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle36-unpromoted-surface-tag68 \
  cargo test -p nimbus-runtime --lib node26_current_lane_unpromoted_surface_required_gap_watchpoint -- --ignored --nocapture
# selected=4, passed=1, skipped=0, failed=3
```

Summary artifact:

```text
/private/tmp/nds-node26-cycle36-unpromoted-surface-tag68/batch/node26__node26_current_lane_unpromoted_surface_required_gap_watchpoint__summary.json
```

Promoted unpromoted-surface batch after adding the ALS weak AsyncWrap fixture:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle36-unpromoted-surface-promoted-tag68 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_unpromoted_surface_promoted_batch_fixture -- --nocapture
# selected=21, passed=21, skipped=0, failed=0
```

Summary artifact:

```text
/private/tmp/nds-node26-cycle36-unpromoted-surface-promoted-tag68/batch/node26__node26_current_lane_executes_unpromoted_surface_promoted_batch__summary.json
```

## Regeneration and Checks

Commands:

```bash
python3 -B scripts/runtime/node/classifications.py sync --lane node26
python3 -B scripts/runtime/node/watchpoints.py sync
python3 -B scripts/runtime/node/status.py
python3 -B scripts/runtime/node/dashboard.py
python3 -B scripts/runtime/node/trends.py
python3 -B scripts/runtime/node/publish_evidence.py
python3 -B scripts/runtime/node/default_support_posture.py
python3 -B scripts/runtime/node/required_surface_blockers.py
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
# Node LTS docs guard passed: public docs avoid stale pass-rate, support-priority,
# and host-heavy overclaim prose
```

Aggregate verifier:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 14 passed, 20 failed
```

The aggregate verifier remains red by design because Node26 still has required
gaps and the broader PR closeout proof rows are not complete. This checkpoint is
not a final gate claim.

## Remaining Node26 Required Gaps

Current `docs/architecture/runtime/node-default-support-posture.json` reports:

```json
{
  "gaps": 5,
  "pass_rate_percent": 99.76,
  "passed": 2087,
  "total": 2092
}
```

The five remaining Node26 `v8_isolate_required` gaps are:

| Owner | Fixture | Source classification |
| --- | --- | --- |
| `streams-local-io/fs-host-io` | `test/parallel/test-fs-stat-temporal.mjs` | `rust_watchpoint_expected_failure` |
| `loader-context/vm` | `test/parallel/test-vm-module-evaluate-while-evaluating.js` | `requires_unpromoted_node_surface` |
| `node-compat/unpromoted-surface` | `test/parallel/test-async-hooks-fatal-error.js` | `requires_unpromoted_node_surface` |
| `node-compat/unpromoted-surface` | `test/parallel/test-stream2-basic.js` | `requires_unpromoted_node_surface` |
| `node-compat/unpromoted-surface` | `test/parallel/test-trace-events-api.js` | `requires_unpromoted_node_surface` |

Recommended next wave:

1. Re-run the remaining five as one broad residual batch with a fresh
   diagnostic root.
2. Investigate `test-fs-stat-temporal.mjs` first only if Temporal support is
   genuinely present or can be made present without faking
   `process.config.variables.v8_enable_temporal_support`.
3. Prefer a coherent async/stream/trace wave for the three
   `node-compat/unpromoted-surface` failures:
   - `test-async-hooks-fatal-error.js` still fails the destroy count.
   - `test-stream2-basic.js` still has readable chunk ordering/size drift.
   - `test-trace-events-api.js` still fails to observe the trace output file.
4. Keep `test-vm-module-evaluate-while-evaluating.js` separate unless the VM
   module work clearly shares a root cause with the async lifecycle fixes.
