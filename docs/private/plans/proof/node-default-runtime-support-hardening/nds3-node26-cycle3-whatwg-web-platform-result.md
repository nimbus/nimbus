# NDS3 Node26 Cycle 3: WHATWG Web Platform Promotion

## Scope

This checkpoint burns Node26 Current required-surface gaps in the
`node-compat/unpromoted-surface` owner by promoting the coherent
WHATWG/Web Platform common slice. It uses the existing WHATWG selector and
staging directories, excluding the previously identified low-ROI and
encoding-side paths. No Deno fork changes, rusty_v8 changes, fixture edits,
checker edits, or generated false-green JSON hand edits were made.

## Broad Pre-Run

Command:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-whatwg-web-platform-wave1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_whatwg_web_platform_watchpoint -- --ignored --nocapture
```

Result:

- Rust test result: failed, as expected for a broad diagnostic batch with one
  residual.
- Fixture summary: `selected=46`, `passed=45`, `skipped=0`, `failed=1`.
- Summary artifact:
  `/private/tmp/nds-node26-whatwg-web-platform-wave1/batch/node26__node26_current_lane_whatwg_web_platform_watchpoint__summary.json`
- Failing diagnostic:
  `/private/tmp/nds-node26-whatwg-web-platform-wave1/general/node26__test_parallel_test_whatwg_url_custom_inspect_js.json`

## Promoted Fixtures

The 45 broad-batch passes were added to
`WHATWG_WEB_PLATFORM_PROMOTED_NODE26_PATHS` and enforced by
`node26_current_lane_executes_whatwg_web_platform_promoted_batch_fixture`.

Command:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-whatwg-web-platform-promote1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_whatwg_web_platform_promoted_batch_fixture -- --nocapture
```

Result:

- Rust test result: `1 passed; 0 failed; 0 ignored; 922 filtered out`.
- Fixture summary: `selected=45`, `passed=45`, `skipped=0`, `failed=0`.
- Summary artifact:
  `/private/tmp/nds-node26-whatwg-web-platform-promote1/batch/node26__node26_current_lane_executes_whatwg_web_platform_promoted_batch__summary.json`

## Failure Grouping

The single non-promoted broad-batch failure is
`test/parallel/test-whatwg-url-custom-inspect.js`. It is a util-inspect output
shape mismatch for Node26's URL internals:

- actual: `[Symbol(context)]: URLContext`
- expected: `Symbol(context): URLContext`

This is a focused formatting/runtime-inspection gap, not a sandbox-boundary
reclassification.

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

- `scripts/runtime/node/watchpoints.py validate`: `validated node-compat watchpoint catalog: 137 entries`
- `tests/runtime/node/compat/node-compat-evidence/latest/status-summary.json`: warnings `0`
- `tests/runtime/node/compat/node-compat-evidence/latest/dashboard-summary.json`: warnings `0`

Posture after regeneration:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`
- Node26 `v8_isolate_required`: `1036` gaps, `52.89%`

The Node26 count moved from `1081` gaps / `50.84%` to `1036` gaps / `52.89%`,
matching the 45 dynamically proven WHATWG fixture promotions in this wave.

## Next Node26 Work

Recommended next wave: continue mining coherent subclusters from
`node-compat/unpromoted-surface` only where selectors are already bounded, or
pivot to the larger structured owners `streams-local-io/fs-host-io`,
`streams-local-io/stream`, and `process-and-timing/diagnostics-channel`.
