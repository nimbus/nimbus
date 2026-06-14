# NDS3 Node26 Cycle 4: Async Hooks Nonblocking Promotion

## Scope

This checkpoint burns Node26 Current required-surface gaps in the
async-hooks lifecycle cluster. It starts from the existing async-hooks
required-gap selector, excludes the previously documented socket-bind
networking subset so the broad batch can finish, and promotes only the
dynamically green Node26 fixture paths. No Deno fork changes, rusty_v8
changes, fixture edits, checker edits, or generated false-green JSON hand
edits were made.

## Broad Pre-Run

Command:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-async-hooks-nonblocking-wave1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_async_hooks_nonblocking_required_gap_watchpoint -- --ignored --nocapture
```

Result:

- Rust test result: failed, as expected for a broad diagnostic batch with
  residual failures.
- Fixture summary: `selected=77`, `passed=73`, `skipped=0`, `failed=4`.
- Summary artifact:
  `/private/tmp/nds-node26-async-hooks-nonblocking-wave1/batch/node26__node26_current_lane_async_hooks_nonblocking_required_gap_watchpoint__summary.json`

Failing diagnostics:

- `/private/tmp/nds-node26-async-hooks-nonblocking-wave1/event_loop/node26__test_async_hooks_test_getaddrinforeqwrap_js.json`
- `/private/tmp/nds-node26-async-hooks-nonblocking-wave1/event_loop/node26__test_async_hooks_test_getnameinforeqwrap_js.json`
- `/private/tmp/nds-node26-async-hooks-nonblocking-wave1/event_loop/node26__test_async_hooks_test_querywrap_js.json`
- `/private/tmp/nds-node26-async-hooks-nonblocking-wave1/event_loop/node26__test_parallel_test_async_hooks_fatal_error_js.json`

## Promoted Fixtures

The 73 broad-batch passes were added to `ASYNC_HOOKS_PROMOTED_NODE26_PATHS`
and enforced by
`node26_current_lane_executes_async_hooks_promoted_batch_fixture`.

Command:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-async-hooks-nonblocking-promote1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_async_hooks_promoted_batch_fixture -- --nocapture
```

Result:

- Rust test result: `1 passed; 0 failed; 0 ignored; 924 filtered out`.
- Fixture summary: `selected=73`, `passed=73`, `skipped=0`, `failed=0`.
- Summary artifact:
  `/private/tmp/nds-node26-async-hooks-nonblocking-promote1/batch/node26__node26_current_lane_executes_async_hooks_promoted_batch__summary.json`

## Residual Cleanup Check

`test/parallel/test-async-hooks-enable-recursive.js` was still listed in
the Node26 Current broad residual source after the async-hooks focused guard
proved it green. The source residual list in `scripts/runtime/node/classifications.py`
and the matching Rust exclusion helper were updated to stop forcing that path
red. This was source-level cleanup only; generated JSON was regenerated from
the updated sources.

Command:

```bash
cargo test -p nimbus-runtime --lib node26_current_lane_executes_manifested_process_and_timing_subset -- --nocapture
```

Result:

- Rust test result: `1 passed; 0 failed; 0 ignored; 924 filtered out`.
- Fixture summary: `passed: 46`, `skipped: 0`, `excluded: 2`, `failed: 0`.
- Excluded fixtures remain:
  - `test/parallel/test-process-load-env-file.js`
  - `test/parallel/test-util-parse-env.js`

## Failure Grouping

The four non-promoted broad-batch failures are:

- DNS async provider wrappers:
  - `test/async-hooks/test-getaddrinforeqwrap.js`
  - `test/async-hooks/test-getnameinforeqwrap.js`
  - `test/async-hooks/test-querywrap.js`
  - Symptom: async-hooks assertion `0 !== 1` from the wrapper callback,
    indicating missing async provider lifecycle accounting for DNS request
    wrappers.
- Fatal-error destroy accounting:
  - `test/parallel/test-async-hooks-fatal-error.js`
  - Symptom: destroy count assertion `0 !== 1`.

The socket-bind networking async-hooks subset remains outside this nonblocking
probe and was not promoted by this wave.

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
cargo fmt --all --check
git diff --check
```

Results:

- `scripts/runtime/node/watchpoints.py validate`: `validated node-compat watchpoint catalog: 138 entries`
- `tests/runtime/node/compat/node-compat-evidence/latest/status-summary.json`: warnings `0`
- `tests/runtime/node/compat/node-compat-evidence/latest/dashboard-summary.json`: warnings `0`
- `scripts/runtime/node/required_surface_blockers.py`: `node22 required gaps: 0`, `node24 required gaps: 0`
- `cargo fmt --all --check`: passed
- `git diff --check`: passed

Posture after regeneration:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`
- Node26 `v8_isolate_required`: `963` gaps, `56.21%`

The Node26 count moved from `1036` gaps / `52.89%` to `963` gaps /
`56.21%`, burning 73 required-surface gaps in this wave.

## Next Node26 Work

Recommended next wave: continue with the largest remaining coherent clusters
from the refreshed Node26 posture. Good candidates are `streams-local-io/fs-host-io`,
`streams-local-io/stream`, `process-and-timing/diagnostics-channel`, and the
remaining bounded `node-compat/unpromoted-surface` subclusters. Keep using the
broad-batch first pattern and promote only dynamically green fixture subsets.
