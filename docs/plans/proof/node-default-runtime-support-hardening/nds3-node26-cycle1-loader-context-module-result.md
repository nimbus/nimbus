# NDS3 Node26 Cycle 1: Loader Context Module Promotion

## Scope

This checkpoint extends the NDS required-surface burn-down to the Node26 Current
lane without changing the already-green Node22/Node24 gate. It promotes only
Node26 fixtures that passed a broad `loader-context/module` diagnostic batch and
then passed again in a non-ignored promotion batch.

No Deno fork changes, rusty_v8 changes, fixture edits, checker edits, or
generated false-green JSON hand edits were made.

## Broad Pre-Run

Command:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-loader-context-module-wave1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_loader_context_module_watchpoint -- --ignored --nocapture
```

Result:

- Rust test result: failed, as expected for a broad diagnostic batch.
- Fixture summary: `selected=66`, `passed=46`, `skipped=0`, `failed=20`.
- Summary artifact:
  `/private/tmp/nds-node26-loader-context-module-wave1/batch/node26__node26_current_lane_loader_context_module_watchpoint__summary.json`
- Fixture diagnostics:
  `/private/tmp/nds-node26-loader-context-module-wave1/general/`

## Promoted Fixtures

The 46 broad-batch passes were added to
`LOADER_CONTEXT_MODULE_PROMOTED_NODE26_PATHS` and enforced by
`node26_current_lane_executes_loader_context_module_promoted_batch_fixture`.

Command:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-loader-context-module-promote1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_loader_context_module_promoted_batch_fixture -- --nocapture
```

Result:

- Rust test result: `1 passed; 0 failed; 0 ignored; 918 filtered out`.
- Fixture summary: `selected=46`, `passed=46`, `skipped=0`, `failed=0`.
- Summary artifact:
  `/private/tmp/nds-node26-loader-context-module-promote1/batch/node26__node26_current_lane_executes_loader_context_module_promoted_batch__summary.json`

## Failure Grouping

The 20 non-promoted broad-batch failures remain in Node26 required-surface
blockers:

- 10 fixtures fail with `ERR_REQUIRE_ESM_RACE_CONDITION` while requiring a
  module that the loader-hook path has not fully loaded.
- 6 fixtures fail around builtin loader-hook ordering or contract shape
  (`shortCircuit` return validation, unexpected builtin target, or duplicate
  hook calls).
- 3 fixtures fail in CommonJS-from-loader-hook source handling
  (`exports`/`require` not defined or nested require from an async ESM hook).
- 1 fixture (`test/parallel/test-module-circular-dependency-warning.js`) failed
  due missing staged `test/fixtures/cycles` data and was intentionally not
  promoted without a focused rerun.

## Generated Evidence

Commands:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py sync
/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py validate
for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do
  /opt/homebrew/bin/python3.12 scripts/runtime/node/$s.py >/dev/null
done
```

Results:

- `scripts/runtime/node/watchpoints.py validate`: `validated node-compat watchpoint catalog: 135 entries`
- `tests/runtime/node/compat/node-compat-evidence/latest/status-summary.json`: warnings `0`
- `tests/runtime/node/compat/node-compat-evidence/latest/dashboard-summary.json`: warnings `0`

Posture after regeneration:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`
- Node26 `v8_isolate_required`: `1144` gaps, `47.98%`

The Node26 count moved from `1190` gaps / `45.88%` to `1144` gaps / `47.98%`,
matching the 46 dynamically proven fixture promotions in this wave.

## Verifier

Command:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
```

Result:

- Overall script result in this checkout: `14 passed, 20 failed`.
- Step 9 passed: `Node22 and Node24 V8-isolate-required fixtures are 100%`.
- The failures were the private closeout/proof-plan conditions whose
  `docs/private/...` inputs are absent in this ignored local checkout
  (`plan=missing`, required private proof files missing). Those failures do not
  reflect a Node22/Node24 posture regression.

## Next Node26 Work

Recommended next wave: stay in `loader-context/module` briefly only for the
staging-only `test/parallel/test-module-circular-dependency-warning.js` rerun
with `test/fixtures/cycles`, then move to the broadest remaining high-yield
Node26 cluster from the current posture. The largest remaining Node26 owners are
still `node-compat/unpromoted-surface`, `streams-local-io/fs-host-io`,
`loader-context/vm`, async/process timing, crypto/networking, and Web platform
surfaces.
