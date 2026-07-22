# NDS3 Node26 Cycle 19: process get-builtin promotion

## Scope

This checkpoint promotes one Node26 Current process-host residual:

- `test/parallel/test-process-get-builtin.mjs`

The fixture failed because Nimbus/Deno exposed experimental `stream/iter` and
`zlib/iter` entries through `module.builtinModules` and `module.isBuiltin()`
without Node26's `--experimental-stream-iter` gate. Local Node26 reports both
entries as non-builtin by default:

```bash
/opt/homebrew/Cellar/node/26.0.0/bin/node -e "const m=require('node:module'); for (const id of ['stream/iter','zlib/iter']) console.log(id, JSON.stringify({has:m.builtinModules.includes(id), is:m.isBuiltin(id), node:m.isBuiltin('node:'+id)}));"
```

Result:

```text
stream/iter {"has":false,"is":false,"node":false}
zlib/iter {"has":false,"is":false,"node":false}
```

The fix landed in the Deno fork at `nimbus/deno` commit `89db0f0912`
(`node: gate stream iter builtin listing`) and was published as immutable tag
`v2.8.3-nimbus.51`. Nimbus was temporarily pinned to the canonical local Deno
worktree for the local proof, then restored to the published tag before the
promoted evidence and generated posture were refreshed.

No V8 or rusty_v8 changes were made. No official upstream fixture or checker was
edited. No false-green JSON edits were made.

Before this wave, Node26 `v8_isolate_required` posture was `226` gaps /
`89.26%`.

## Broad Diagnostics

Pre-fix immutable-tag broad batch on `v2.8.3-nimbus.50`:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-process-host-wave3 \
  cargo test -p nimbus-runtime --lib node26_current_lane_process_host_watchpoint -- --ignored --nocapture
```

Result:

- selected: `27`
- passed: `26`
- skipped: `0`
- failed: `1`
- failure: `test/parallel/test-process-get-builtin.mjs`
- root cause: missing Node26 default gate for `stream/iter` and `zlib/iter`
- summary:
  `/private/tmp/nds-node26-process-host-wave3/batch/node26__node26_current_lane_process_host_watchpoint__summary.json`

Local-Deno broad proof after temporarily pinning the full Deno-family patch set
to `/Users/jack/src/github.com/nimbus/deno`:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-process-host-wave3-local2 \
  cargo test -p nimbus-runtime --lib node26_current_lane_process_host_watchpoint -- --ignored --nocapture
```

Result:

- selected: `27`
- passed: `27`
- skipped: `0`
- failed: `0`
- summary:
  `/private/tmp/nds-node26-process-host-wave3-local2/batch/node26__node26_current_lane_process_host_watchpoint__summary.json`

The targeted clean used before proving the fresh embedded JS snapshot was:

```bash
cargo clean -p deno_node -p nimbus-runtime
```

Result: `Removed 7110 files, 6.1GiB total`.

## Local Promotion Proof

Local-Deno promoted process-host batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-process-host-wave3-local-promoted \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_process_host_promoted_batch_fixture -- --nocapture
```

Result:

- selected: `27`
- passed: `27`
- skipped: `0`
- failed: `0`
- summary:
  `/private/tmp/nds-node26-process-host-wave3-local-promoted/batch/node26__node26_current_lane_executes_process_host_promoted_batch__summary.json`

## Immutable Tag Proof

After publishing `v2.8.3-nimbus.51`, Nimbus was repinned from the local Deno path
back to the immutable tag. Cargo resolved the Deno-family crates to:

```text
https://github.com/nimbus/deno?tag=v2.8.3-nimbus.51#89db0f09
```

The targeted clean after repin was:

```bash
cargo clean -p deno_node -p nimbus-runtime
```

Result: `Removed 2121 files, 2.0GiB total`.

Immutable-tag broad batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-process-host-wave3-tag51 \
  cargo test -p nimbus-runtime --lib node26_current_lane_process_host_watchpoint -- --ignored --nocapture
```

Result:

- selected: `27`
- passed: `27`
- skipped: `0`
- failed: `0`
- summary:
  `/private/tmp/nds-node26-process-host-wave3-tag51/batch/node26__node26_current_lane_process_host_watchpoint__summary.json`

Immutable-tag promoted batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-process-host-wave3-tag51-promoted \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_process_host_promoted_batch_fixture -- --nocapture
```

Result:

- selected: `27`
- passed: `27`
- skipped: `0`
- failed: `0`
- summary:
  `/private/tmp/nds-node26-process-host-wave3-tag51-promoted/batch/node26__node26_current_lane_executes_process_host_promoted_batch__summary.json`

The process-host batches still print the existing inspector
`test-process-env-sideeffects.js` diagnostic diff, but the harness summary is
green in both broad and promoted runs.

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

- `scripts/runtime/node/watchpoints.py validate`:
  `validated node-compat watchpoint catalog: 150 entries`
- `scripts/runtime/node/required_surface_blockers.py`:
  Node22 required gaps: `0`; Node24 required gaps: `0`
- `docs/private/architecture/runtime/node-default-support-posture.json`:
  Node26 `v8_isolate_required` is `225` gaps / `89.31%`
- tracked public evidence:
  Node26 full official corpus manifested green count is `1880 / 5578`

Posture after regeneration:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`
- Node26 `v8_isolate_required`: `225` gaps, `89.31%`
- Node26 required passed: `1880 / 2105`

This wave moves Node26 from `226` gaps / `89.26%` to `225` gaps / `89.31%`,
burning one required-surface gap.

## Verifier

Command:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
```

Result:

- Summary: `14 passed, 20 failed`.
- Step 9 passed: Node22 and Node24 V8-isolate-required fixtures are `100%`.
- Step 11 remains failed because Node26 Current evidence is still incomplete:
  Node26 is `1880` official passes and `225` required gaps, not `0` gaps /
  `100.0%`.
- The remaining verifier failures are honest red closeout/proof gaps in this
  checkout; this cycle does not claim full NDS completion.

## Integrity Checks

Commands:

```bash
cargo fmt --all --check
git diff --check
```

Results:

- `cargo fmt --all --check`: passed.
- `git diff --check`: passed.

## Remaining Node26 Required Buckets

After this wave:

- `92` `node-compat/unpromoted-surface`
- `34` `node-compat/current-lane`
- `23` `loader-context/vm`
- `20` `loader-context/module`
- `18` `loader-context/domain`
- `15` `streams-local-io/fs-host-io`
- `7` `runtime/v8`
- `6` `process-and-timing/process-host`
- `4` `core-semantics/console`
- `3` `loader-context/util`
- `2` `core-semantics/assert`
- `1` `core-semantics/url`

Recommended next wave: return to broad Node26 diagnostics and pick a coherent
high-yield implementation cluster. The largest bucket remains
`node-compat/unpromoted-surface`, but the likely best implementation leverage is
still in fs/FileHandle/stream residuals or ESM/module loader residuals. Avoid
spending another cycle on a process-host singleton unless it is already proven
or unlocks a broader process batch.
