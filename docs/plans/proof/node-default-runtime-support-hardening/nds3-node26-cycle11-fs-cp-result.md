# NDS3 Node26 Cycle 11: fs.cp and CJS Export Interop Promotion

## Scope

This checkpoint burns Node26 Current required-surface gaps in the
`streams-local-io/fs-host-io` cluster, mainly the official `fs.cp` family. The
wave fixes a Deno fork CJS named-export interop regression that prevented the
Node26 `test/common/fs.js` helper from exposing object-literal `module.exports`
names to ES module imports, stages the official `test/fixtures/copy` support
directory in the Nimbus fs-host-io harness, and promotes only fixtures that were
dynamically green on the published Deno tag.

No V8 or rusty_v8 changes, fixture edits, checker edits, or generated
false-green JSON hand edits were made.

Before this wave, Node26 `v8_isolate_required` posture was `669` gaps /
`69.58%`.

## Deno Fork Change

Fork:

- Worktree: `/Users/jack/src/github.com/nimbus/deno`
- Branch: `nimbus/v2.8.3`
- Previous tag: `v2.8.3-nimbus.46`
- New commit: `719a55d2c2` (`node: keep object-literal CJS named exports`)
- New tag: `v2.8.3-nimbus.47`
- Push result: `nimbus/v2.8.3 -> nimbus/v2.8.3`, new tag
  `v2.8.3-nimbus.47 -> v2.8.3-nimbus.47`

The fork change removes the Nimbus-only stripping of object-literal
`module.exports = { ... }` names in
`libs/resolver/cjs/analyzer/deno_ast.rs`. A direct Node v26 probe of the
vendored helper had shown that named imports from
`test/common/fs.js` expose `nextdir`, `assertDirEquivalent`, and
`collectEntries`, so keeping those analyzer names matches Node's observed
behavior for this shape.

Focused fork proof:

```bash
CARGO_ENCODED_RUSTFLAGS= cargo test -p deno_resolver --features deno_ast cjs_analysis_keeps_module_exports_object_literal_names
```

Result:

- `1 passed; 0 failed; 0 ignored; 36 filtered out`

## Broad Pre-Runs

Initial immutable-tag baseline on `v2.8.3-nimbus.46`:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-fs-cp-wave1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_fs_host_io_watchpoint -- --ignored --nocapture
```

Result:

- Rust test result: failed, as expected for the broad diagnostic batch.
- Fixture summary: `selected=76`, `passed=0`, `skipped=1`, `failed=75`.
- Dominant failure: import-time `SyntaxError` for named imports from
  `../common/fs.js` (`nextdir`, `assertDirEquivalent`, `collectEntries`).
- Skipped fixture: `test/parallel/test-fs-stat-temporal.mjs`
- Summary artifact:
  `/private/tmp/nds-node26-fs-cp-wave1/batch/node26__node26_current_lane_fs_host_io_watchpoint__summary.json`

Local Deno path pin after the analyzer fix, before copy fixture staging:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-fs-cp-wave1-local-deno1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_fs_host_io_watchpoint -- --ignored --nocapture
```

Result:

- Rust test result: failed, as expected for the broad diagnostic batch.
- Fixture summary: `selected=76`, `passed=29`, `skipped=1`, `failed=46`.
- Residual cp failures had moved from import-time failures to missing official
  support data such as `test/fixtures/copy/kitchen-sink`.
- Summary artifact:
  `/private/tmp/nds-node26-fs-cp-wave1-local-deno1/batch/node26__node26_current_lane_fs_host_io_watchpoint__summary.json`

Local Deno path pin after staging the official copy fixture directory:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-fs-cp-wave1-local-deno2 \
  cargo test -p nimbus-runtime --lib node26_current_lane_fs_host_io_watchpoint -- --ignored --nocapture
```

Result:

- Rust test result: failed, as expected for the broad diagnostic batch.
- Fixture summary: `selected=76`, `passed=65`, `skipped=1`, `failed=10`.
- Skipped fixture: `test/parallel/test-fs-stat-temporal.mjs`
- Summary artifact:
  `/private/tmp/nds-node26-fs-cp-wave1-local-deno2/batch/node26__node26_current_lane_fs_host_io_watchpoint__summary.json`

Immutable published-tag rerun after repinning Nimbus to `v2.8.3-nimbus.47`:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-fs-cp-wave1-tag47 \
  cargo test -p nimbus-runtime --lib node26_current_lane_fs_host_io_watchpoint -- --ignored --nocapture
```

Result:

- Rust test result: failed, as expected for the broad diagnostic batch.
- Fixture summary: `selected=76`, `passed=65`, `skipped=1`, `failed=10`.
- Skipped fixture: `test/parallel/test-fs-stat-temporal.mjs`
- Summary artifact:
  `/private/tmp/nds-node26-fs-cp-wave1-tag47/batch/node26__node26_current_lane_fs_host_io_watchpoint__summary.json`

## Promoted Fixtures

The 65 published-tag broad-batch passes were added to
`FS_HOST_IO_PROMOTED_NODE26_PATHS`; `FS_HOST_IO_EXTRA_DIRS` now stages
`test/fixtures/copy` for the fs-host-io batch alongside `test/common`.

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-fs-cp-wave1-tag47-promote1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_fs_host_io_promoted_batch_fixture -- --nocapture
```

Result:

- Rust test result: `1 passed; 0 failed; 0 ignored; 938 filtered out`.
- Fixture summary: `selected=132`, `passed=132`, `skipped=0`, `failed=0`.
- Summary artifact:
  `/private/tmp/nds-node26-fs-cp-wave1-tag47-promote1/batch/node26__node26_current_lane_executes_fs_host_io_promoted_batch__summary.json`

The promoted 65 new paths are exactly the `passed_paths` entries in the
published-tag broad summary. They are enforced by the non-ignored promoted
batch and removed from the Node26 unpromoted-surface classification by the
generator sync.

## Failure Grouping

Non-promoted fs-host-io broad failures/skips after `v2.8.3-nimbus.47`:

- `stream/iter` builtin missing: `test-fs-promises-file-handle-pull.js`,
  `test-fs-promises-file-handle-pullsync.js`, and
  `test-fs-promises-file-handle-writer.js`.
- `fs.rmdir` callback/error-shape mismatches:
  `test-fs-rmdir-recursive-error.js`,
  `test-fs-rmdir-throws-not-found.js`, and
  `test-fs-rmdir-throws-on-file.js`.
- `test-fs-lchmod.js`: invalid-argument error shape mismatch.
- `test-fs-promises.js`: async stack expectation mismatch for
  `assert.rejects`.
- `test-fs-stat-date.mjs`: `Stats` date field enumerability mismatch.
- `test-fs-symlink-dir-junction.js`: junction symlink semantics mismatch.
- `test-fs-stat-temporal.mjs`: self-skipped because Temporal support is
  unavailable.

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

- `scripts/runtime/node/watchpoints.py validate`: `validated node-compat watchpoint catalog: 145 entries`
- `tests/runtime/node/compat/node-compat-evidence/latest/status-summary.json`: warnings `[]`
- `tests/runtime/node/compat/node-compat-evidence/latest/dashboard-summary.json`: warnings `None`
- `tests/runtime/node/compat/node-compat-evidence/latest/trend-summary.json`: warnings `None`
- `scripts/runtime/node/required_surface_blockers.py`: `node22 required gaps: 0`, `node24 required gaps: 0`

Posture after regeneration:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`
- Node26 `v8_isolate_required`: `604` gaps, `72.53%`

The Node26 count moved from `669` gaps / `69.58%` to `604` gaps /
`72.53%`, burning 65 required-surface gaps in this wave.

The untracked public selector mirror
`docs/architecture/runtime/node-default-support-posture.{json,md}` was
refreshed from the generated private posture after regeneration and remains
unstaged.

## Next Node26 Work

The next highest-yield implementation wave is likely `stream/iter`, because it
would address the three fs filehandle residual failures and belongs to the
larger remaining stream/WebStreams cluster. After that, rerun broad batches for
stream/webstream and loader/CJS/ESM surfaces because the `v2.8.3-nimbus.47`
CJS object-literal export change may unlock additional fixtures outside this
fs-host-io checkpoint.
