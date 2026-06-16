# NDS3 node26 cycle 20 - fs host I/O stream-iter promotion

Date: 2026-06-15
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

This wave promoted 10 additional Node26 `streams-local-io/fs-host-io`
fixtures. Node26 `v8_isolate_required` posture moved from `225` gaps /
`89.31%` to `215` gaps / `89.79%`.

Deno fork tag: `v2.8.3-nimbus.52`
Commit: `1d9810a91affd20c4c00572b6140454126b956c1`

Nimbus was temporarily pinned to the canonical local Deno worktree while proving
the fork-owned changes, then repinned to the immutable published tag before
promotion and regeneration.

## Promoted Fixtures

- `test/parallel/test-fs-lchmod.js`
- `test/parallel/test-fs-promises-file-handle-pull.js`
- `test/parallel/test-fs-promises-file-handle-pullsync.js`
- `test/parallel/test-fs-promises-file-handle-writer.js`
- `test/parallel/test-fs-promises.js`
- `test/parallel/test-fs-rmdir-recursive-error.js`
- `test/parallel/test-fs-rmdir-throws-not-found.js`
- `test/parallel/test-fs-rmdir-throws-on-file.js`
- `test/parallel/test-fs-stat-date.mjs`
- `test/parallel/test-fs-symlink-dir-junction.js`

`test/parallel/test-fs-stat-temporal.mjs` remained skipped because Temporal is
not available and was not promoted.

## Root Cause

The fixtures exposed a composed Node26 fs-host-io gap:

- Deno's FileHandle lacked Node26 stream-iter `pull()`, `pullSync()`, and
  `writer()` methods.
- `fs.rmdir` / `fs.promises.rmdir` needed Node26's removed
  `options.recursive` validation while preserving sandboxed directory-only
  removal instead of falling through to file unlink behavior.
- `Stats` lazy date fields were materialized as non-enumerable own properties.
- `assert.rejects()` async stack labels needed the Node26 `assert.rejects`
  receiver spelling. The existing Nimbus marker supported the Node22
  `Function.rejects` spelling; this wave generalized it and had the Node26
  harness request `assert`.
- The symlink junction fixture needed `test/fixtures/cycles` staged in the
  copied fixture bundle.
- `fs.promises.lchmod()` needed promise-side mode/path validation before routing
  to the sandboxed `Deno.lchmod` op.

The sandbox boundary stayed intact. The new rmdir path calls the existing
runtime-local remove op with `directory_only: true`, which maps to
`std::fs::remove_dir`; it does not grant host-cwd mutation, host-process exit,
signals, subprocesses, or unbounded filesystem access.

## Verification

Fork-local checks before publishing the Deno tag:

```bash
deno fmt --check ext/node/polyfills/internal/fs/handle.ts ext/node/polyfills/internal/fs/utils.mjs
# Checked 2 files

git diff --check
# no output

CARGO_ENCODED_RUSTFLAGS='' cargo check -p deno_node
# Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.49s
```

Local Nimbus proof against the local Deno path pin:

```bash
cargo test -p nimbus-runtime --lib node26_current_lane_fs_host_io_watchpoint -- --ignored --nocapture
# node_compat node26-current-lane-fs-host-io-watchpoint node26 summary:
# selected=11, passed=10, skipped=1, failed=0
```

After publishing `v2.8.3-nimbus.52`, Nimbus was repinned to the immutable tag
and checked:

```bash
cargo check -p nimbus-runtime
# Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 52s
```

Immutable-tag focused proof:

```bash
cargo test -p nimbus-runtime --lib node26_current_lane_fs_host_io_watchpoint -- --ignored --nocapture
# node_compat node26-current-lane-fs-host-io-watchpoint node26 summary:
# selected=11, passed=10, skipped=1, failed=0
```

Promoted non-ignored batch proof:

```bash
cargo test -p nimbus-runtime --lib node26_current_lane_executes_fs_host_io_promoted_batch_fixture -- --nocapture
# node_compat node26-current-lane-executes-fs-host-io-promoted-batch node26 summary:
# selected=142, passed=142, skipped=0, failed=0
```

Generator pipeline:

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

- `watchpoints.py validate`: `validated node-compat watchpoint catalog: 150 entries`
- `required_surface_blockers.py`: `node22 required gaps: 0`, `node24 required gaps: 0`
- `default_support_posture.py --check`: `node default support posture: pass`

Generated posture:

- Node22 `v8_isolate_required`: `0` gaps, `100%`
- Node24 `v8_isolate_required`: `0` gaps, `100%`
- Node26 `v8_isolate_required`: `215` gaps, `89.79%`
- Node26 required passed: `1890 / 2105`

Verifier:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 14 passed, 20 failed
```

Step 9 remains green for Node22/Node24. Step 11 remains red honestly because
Node26 still has `215` required gaps, not `0`.

## Diagnostics

Useful diagnostic artifacts from this wave:

- `/private/tmp/nds-node26-broad-refresh-wave20`
- `/private/tmp/nds-node26-fs-host-io-wave20-local1`
- `/private/tmp/nds-node26-fs-host-io-wave20-local2`
- `/private/tmp/nds-node26-fs-host-io-wave20-local3`
- `/private/tmp/nds-node26-fs-host-io-wave20-local4`
- `/private/tmp/nds-node26-fs-host-io-wave20-local5`
- `target/node-compat/diagnostics/batch/node26__node26_current_lane_fs_host_io_watchpoint__summary.json`
- `target/node-compat/diagnostics/batch/node26__node26_current_lane_executes_fs_host_io_promoted_batch__summary.json`

The custom diagnostic-root env form for the final reruns was blocked by the
managed sandbox escalation policy, so the final local/tag proof summaries live
under the default target diagnostics root.

## Remaining Node26 fs-host-io Gaps

After regeneration, `streams-local-io/fs-host-io` has 5 remaining Node26
required gaps:

- `test/parallel/test-fs-promises-watch-ignore-invalid.mjs`
- `test/parallel/test-fs-promises-watch.js`
- `test/parallel/test-fs-sir-writes-alot.js`
- `test/parallel/test-fs-stat-temporal.mjs`
- `test/parallel/test-fs-write-buffer-large.js`

The refreshed owner map for all remaining Node26 required gaps is:

- `92` `node-compat/unpromoted-surface`
- `34` `node-compat/current-lane`
- `23` `loader-context/vm`
- `20` `loader-context/module`
- `18` `loader-context/domain`
- `7` `runtime/v8`
- `6` `process-and-timing/process-host`
- `5` `streams-local-io/fs-host-io`
- `4` `core-semantics/console`
- `3` `loader-context/util`
- `2` `core-semantics/assert`
- `1` `core-semantics/url`

Good next waves are loader/module, loader/vm, and high-yield unpromoted-surface
clusters. The remaining fs-host-io paths are now smaller and lower-yield unless
the Temporal/watch support work is tackled as a coherent wave.

## Integrity

- No V8 or rusty_v8 changes were made.
- No official upstream Node fixture or checker was edited.
- No generated JSON was hand-edited to fake a green result.
- No local Deno path pin remains in `Cargo.toml` or `Cargo.lock`.
- `measure_ah.sh` and other scratch files remain untracked.
