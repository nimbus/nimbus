# NDS3 Node26 Cycle 6: Diagnostics Channel Promotion

## Scope

This checkpoint burns Node26 Current required-surface gaps in the
`process-and-timing/diagnostics-channel` owner. It mirrors the existing
Node22/Node24 diagnostics-channel watchpoint shape, adds a Node26 broad ignored
watchpoint, and promotes only the dynamically green Node26 fixture paths. No
Deno fork changes, rusty_v8 changes, fixture edits, checker edits, or generated
false-green JSON hand edits were made.

Before this wave, Node26 `v8_isolate_required` posture was `896` gaps /
`59.25%`.

## Broad Pre-Run

Command:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-diagnostics-channel-wave1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_process_diagnostics_channel_watchpoint -- --ignored --nocapture
```

Result:

- Rust test result: failed, as expected for a broad diagnostic batch with
  residual failures.
- Fixture summary: `selected=55`, `passed=41`, `skipped=0`, `failed=14`.
- Summary artifact:
  `/private/tmp/nds-node26-diagnostics-channel-wave1/batch/node26__node26_current_lane_process_diagnostics_channel_watchpoint__summary.json`

## Promoted Fixtures

The 41 broad-batch passes were added to
`PROCESS_DIAGNOSTICS_CHANNEL_PROMOTED_NODE26_PATHS` and enforced by
`node26_current_lane_executes_process_diagnostics_channel_promoted_batch_fixture`.

Command:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-diagnostics-channel-promote1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_process_diagnostics_channel_promoted_batch_fixture -- --nocapture
```

Result:

- Rust test result: `1 passed; 0 failed; 0 ignored; 928 filtered out`.
- Fixture summary: `selected=41`, `passed=41`, `skipped=0`, `failed=0`.
- Summary artifact:
  `/private/tmp/nds-node26-diagnostics-channel-promote1/batch/node26__node26_current_lane_executes_process_diagnostics_channel_promoted_batch__summary.json`

## Failure Grouping

The 14 non-promoted broad-batch failures group as follows:

- `diagnostics_channel.boundedChannel`: 7 failures.
  - `test-diagnostics-channel-bounded-channel-run-transform-error.js`
  - `test-diagnostics-channel-bounded-channel-run.js`
  - `test-diagnostics-channel-bounded-channel-scope-error.js`
  - `test-diagnostics-channel-bounded-channel-scope-nested.js`
  - `test-diagnostics-channel-bounded-channel-scope-transform-error.js`
  - `test-diagnostics-channel-bounded-channel-scope.js`
  - `test-diagnostics-channel-bounded-channel.js`
  - Symptom: `dc.boundedChannel is not a function` / API is undefined.
- HTTP/2 client stream created/start payload shape: 2 failures.
  - `test-diagnostics-channel-http2-client-stream-created.js`
  - `test-diagnostics-channel-http2-client-stream-start.js`
  - Symptom: created/start event payload reports the wrong header surface
    (`pushheader` / sensitive-header shape instead of the expected
    `requestHeader` payload).
- `diagnostics_channel.withStoreScope`: 2 failures.
  - `test-diagnostics-channel-run-stores-scope-transform-error.js`
  - `test-diagnostics-channel-run-stores-scope.js`
  - Symptom: `channel.withStoreScope is not a function`.
- Tracing-channel promise/runStores semantics: 3 failures.
  - `test-diagnostics-channel-tracing-channel-promise-non-thenable.js`
  - `test-diagnostics-channel-tracing-channel-promise-run-stores.js`
  - `test-diagnostics-channel-tracing-channel-promise-thenable.js`
  - Symptoms: non-thenable return shape mismatch, run-stores subscriber not
    invoked, and thenable result not preserving the expected instance shape.

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

- `scripts/runtime/node/watchpoints.py validate`: `validated node-compat watchpoint catalog: 140 entries`
- `tests/runtime/node/compat/node-compat-evidence/latest/status-summary.json`: warnings `0`
- `tests/runtime/node/compat/node-compat-evidence/latest/dashboard-summary.json`: warnings `0`
- `scripts/runtime/node/required_surface_blockers.py`: `node22 required gaps: 0`, `node24 required gaps: 0`

Posture after regeneration:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`
- Node26 `v8_isolate_required`: `855` gaps, `61.12%`

The Node26 count moved from `896` gaps / `59.25%` to `855` gaps /
`61.12%`, burning 41 required-surface gaps in this wave.

## Next Node26 Work

The highest-value diagnostics-channel follow-up is a Deno fork implementation
wave for the remaining API and payload gaps:

- Add or align `diagnostics_channel.boundedChannel`.
- Add or align `channel.withStoreScope`.
- Correct HTTP/2 client stream diagnostics payload shape for created/start.
- Tighten tracing-channel promise/runStores behavior.

Broader high-yield Node26 waves remain `streams-local-io/stream`,
`process-and-timing/timers`, and the CommonJS named-export interop root cause
seen in the fs-host-io tail.
