# NDS3 Node26 Cycle 5: FS Host I/O Promotion

## Scope

This checkpoint burns Node26 Current required-surface gaps in the
`streams-local-io/fs-host-io` owner. It mirrors the existing fs-host-io
watchpoint shape used for Node22 and Node24, keeps the low-ROI watch/stress
paths excluded by the selector, and promotes only the dynamically green Node26
fixture paths. No Deno fork changes, rusty_v8 changes, fixture edits, checker
edits, or generated false-green JSON hand edits were made.

Before running the broad batch, the untracked local selector mirror
`docs/architecture/runtime/node-default-support-posture.json` was refreshed
from the generated private posture so the Rust selector used the current
Node26 `963`-gap state. The mirror remains untracked and was not staged.

## Broad Pre-Run

Command:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-fs-host-io-wave1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_fs_host_io_watchpoint -- --ignored --nocapture
```

Result:

- Rust test result: failed, as expected for a broad diagnostic batch with
  residual failures.
- Fixture summary: `selected=143`, `passed=67`, `skipped=1`, `failed=75`.
- Skipped fixture: `test/parallel/test-fs-stat-temporal.mjs`
- Summary artifact:
  `/private/tmp/nds-node26-fs-host-io-wave1/batch/node26__node26_current_lane_fs_host_io_watchpoint__summary.json`

## Promoted Fixtures

The 67 broad-batch passes were added to `FS_HOST_IO_PROMOTED_NODE26_PATHS`
and enforced by `node26_current_lane_executes_fs_host_io_promoted_batch_fixture`.

Command:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-fs-host-io-promote1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_fs_host_io_promoted_batch_fixture -- --nocapture
```

Result:

- Rust test result: `1 passed; 0 failed; 0 ignored; 926 filtered out`.
- Fixture summary: `selected=67`, `passed=67`, `skipped=0`, `failed=0`.
- Summary artifact:
  `/private/tmp/nds-node26-fs-host-io-promote1/batch/node26__node26_current_lane_executes_fs_host_io_promoted_batch__summary.json`

## Failure Grouping

The 75 non-promoted broad-batch failures group as follows:

- `fs.cp` family: 65 failures.
  - Dominant symptom: Node26 `.mjs` fixtures import named exports from
    `../common/fs.js`, but the runtime reports that the CommonJS helper does
    not expose those named exports.
  - Adjacent symptom: identical source/destination checks report `ENOENT`
    where Node26 expects `ERR_FS_CP_EINVAL`.
- `stream/iter` module gap: 3 failures.
  - `test-fs-promises-file-handle-pull.js`
  - `test-fs-promises-file-handle-pullsync.js`
  - `test-fs-promises-file-handle-writer.js`
  - Symptom: `Cannot find module 'stream/iter'`.
- `fs.rmdir` error/callback shape: 3 failures.
  - `test-fs-rmdir-recursive-error.js`
  - `test-fs-rmdir-throws-not-found.js`
  - `test-fs-rmdir-throws-on-file.js`
- Singleton fs behavior mismatches:
  - `test-fs-lchmod.js`: invalid-argument error shape mismatch.
  - `test-fs-promises.js`: rejected stack/message comparison shape mismatch.
  - `test-fs-stat-date.mjs`: `Stats` date fields are not enumerable as expected.
  - `test-fs-symlink-dir-junction.js`: junction symlink target existence mismatch.

The skipped `test-fs-stat-temporal.mjs` source checks `common.hasTemporal`
and self-skips with `Temporal support unavailable`; it was not promoted in
this wave.

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

- `scripts/runtime/node/watchpoints.py validate`: `validated node-compat watchpoint catalog: 139 entries`
- `tests/runtime/node/compat/node-compat-evidence/latest/status-summary.json`: warnings `0`
- `tests/runtime/node/compat/node-compat-evidence/latest/dashboard-summary.json`: warnings `0`
- `scripts/runtime/node/required_surface_blockers.py`: `node22 required gaps: 0`, `node24 required gaps: 0`

Posture after regeneration:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`
- Node26 `v8_isolate_required`: `896` gaps, `59.25%`

The Node26 count moved from `963` gaps / `56.21%` to `896` gaps /
`59.25%`, burning 67 required-surface gaps in this wave.

## Next Node26 Work

The highest-value follow-up from this fs wave is the CommonJS named-export
interop root cause for Node26 `.mjs` fs fixtures importing `../common/fs.js`.
That one root cause accounts for most of the remaining fs-host-io tail and may
also help the `node-compat/unpromoted-surface` ES module cluster. Other good
parallel waves remain `streams-local-io/stream`, `process-and-timing/diagnostics-channel`,
and `process-and-timing/timers`.
