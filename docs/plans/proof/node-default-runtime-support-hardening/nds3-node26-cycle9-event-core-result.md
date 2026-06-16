# NDS3 Node26 Cycle 9: Event and Core Utility Promotion

## Scope

This checkpoint burns Node26 Current required-surface gaps in the event,
EventEmitter/EventTarget, assert, buffer, path, URL, and util residual surface.
It adds Node26 broad ignored watchpoints for the existing event and core-util
selectors, promotes only dynamically green Current-lane fixture paths, and
leaves skips/failures counted as gaps. No Deno fork changes, rusty_v8 changes,
fixture edits, checker edits, or generated false-green JSON hand edits were
made.

Before this wave, Node26 `v8_isolate_required` posture was `744` gaps /
`66.17%`.

## Loader Diagnostic Lead

Before pivoting to this wave, two loader/context diagnostics were run and kept
as retained diagnostic roots, but they produced no promotable fixture set:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-loader-module-wave1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_loader_context_module_watchpoint -- --ignored --nocapture
```

Result: `selected=20`, `passed=0`, `skipped=0`, `failed=20`.

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-loader-vm-wave1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_loader_context_vm_watchpoint -- --ignored --nocapture
```

Result: `selected=0`, `passed=0`, `skipped=0`, `failed=0`.

No support was promoted from those loader diagnostics.

## Broad Pre-Runs

Event broad command:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-event-core-wave1-event \
  cargo test -p nimbus-runtime --lib node26_current_lane_event_required_gap_watchpoint -- --ignored --nocapture
```

Result:

- Rust test result: failed, as expected for a broad diagnostic batch with one
  residual failure.
- Fixture summary: `selected=37`, `passed=36`, `skipped=0`, `failed=1`.
- Summary artifact:
  `/private/tmp/nds-node26-event-core-wave1-event/batch/node26__node26_current_lane_event_required_gap_watchpoint__summary.json`

Core-util broad command:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-event-core-wave1-core \
  cargo test -p nimbus-runtime --lib node26_current_lane_core_semantics_util_required_gap_watchpoint -- --ignored --nocapture
```

Result:

- Rust test result: failed, as expected for a broad diagnostic batch with
  residual failures and one self-skip.
- Fixture summary: `selected=30`, `passed=24`, `skipped=1`, `failed=5`.
- Skipped fixture: `test/parallel/test-util-styletext.js` (`Could not create TTY fd`).
- Summary artifact:
  `/private/tmp/nds-node26-event-core-wave1-core/batch/node26__node26_current_lane_core_semantics_util_required_gap_watchpoint__summary.json`

## Promoted Fixtures

The 36 event broad-batch passes were added to `EVENT_PROMOTED_NODE26_PATHS` and
enforced by `node26_current_lane_executes_event_promoted_batch_fixture`.

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-event-core-wave1-event-promote1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_event_promoted_batch_fixture -- --nocapture
```

Result:

- Rust test result: `1 passed; 0 failed; 0 ignored; 936 filtered out`.
- Fixture summary: `selected=36`, `passed=36`, `skipped=0`, `failed=0`.
- Summary artifact:
  `/private/tmp/nds-node26-event-core-wave1-event-promote1/batch/node26__node26_current_lane_executes_event_promoted_batch__summary.json`

The 24 core-util broad-batch passes were added to
`CORE_SEMANTICS_UTIL_PROMOTED_NODE26_PATHS` and enforced by
`node26_current_lane_executes_core_semantics_util_promoted_batch_fixture`.

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-event-core-wave1-core-promote1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_core_semantics_util_promoted_batch_fixture -- --nocapture
```

Result:

- Rust test result: `1 passed; 0 failed; 0 ignored; 936 filtered out`.
- Fixture summary: `selected=24`, `passed=24`, `skipped=0`, `failed=0`.
- Summary artifact:
  `/private/tmp/nds-node26-event-core-wave1-core-promote1/batch/node26__node26_current_lane_executes_core_semantics_util_promoted_batch__summary.json`

## Failure Grouping

Non-promoted event broad failure:

- `test/parallel/test-eventsource.js`: `EventSource` is still undefined in the
  runtime global surface; the paired `test-eventsource-disabled.js` passed.

Non-promoted core-util broad failures/skips:

- `test/parallel/test-assert-deep.js`: assert diff formatting mismatch for
  Proxy/array comparison output.
- `test/parallel/test-assert.js`: assert diff formatting mismatch for
  symbol-keyed custom inspect output.
- `test/parallel/test-url-parse-deprecation.js`: expected URL parse
  deprecation warning was not emitted.
- `test/parallel/test-util-callbackify.js`: callbackify fixture path reporting
  used the temporary bundle path instead of the `nvx-*` test fixture path.
- `test/parallel/test-util-inspect-regexp.js`: regexp color inspect output
  differs from Node26's tokenized color shape.
- `test/parallel/test-util-styletext.js`: self-skipped because the harness
  could not create a TTY fd; it remains unpromoted.

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

- `scripts/runtime/node/watchpoints.py validate`: `validated node-compat watchpoint catalog: 144 entries`
- `tests/runtime/node/compat/node-compat-evidence/latest/status-summary.json`: warnings `[]`
- `tests/runtime/node/compat/node-compat-evidence/latest/dashboard-summary.json`: warnings `None`
- `tests/runtime/node/compat/node-compat-evidence/latest/trend-summary.json`: warnings `None`
- `scripts/runtime/node/required_surface_blockers.py`: `node22 required gaps: 0`, `node24 required gaps: 0`

Posture after regeneration:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`
- Node26 `v8_isolate_required`: `684` gaps, `68.89%`

The Node26 count moved from `744` gaps / `66.17%` to `684` gaps /
`68.89%`, burning 60 required-surface gaps in this wave.

The untracked public selector mirror
`docs/architecture/runtime/node-default-support-posture.{json,md}` was
refreshed from the generated private posture after regeneration and remains
unstaged.

## Next Node26 Work

Remaining high-yield Node26 clusters are still dominated by
`node-compat/unpromoted-surface` (`392` gaps), `streams-local-io/fs-host-io`
(`80` gaps), `streams-local-io/stream` (`41` gaps), `node26_current_required_residual`
(`34` gaps), `process-and-timing/process-host` (`33` gaps), and loader
module/VM/domain (`63` combined gaps). The best next implementation wave is
still either `stream/iter` in the Deno fork, which explains 38 stream failures
plus fs-filehandle tail failures, or the Node26 `.mjs` CommonJS named-export
interop problem seen in the fs.cp family.
