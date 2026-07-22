# NDS3 Node26 Cycle 8: Process Timers Promotion

## Scope

This checkpoint burns Node26 Current required-surface gaps in the
`process-and-timing/timers` owner. It adds a Node26 broad ignored timers
watchpoint, proves that entire broad batch green, and promotes the exact same
39 fixture paths through a non-ignored enforced batch. No Deno fork changes,
rusty_v8 changes, fixture edits, checker edits, or generated false-green JSON
hand edits were made.

Before this wave, Node26 `v8_isolate_required` posture was `783` gaps /
`64.39%`.

## Broad Pre-Run

Command:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-process-timers-wave1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_process_timers_watchpoint -- --ignored --nocapture
```

Result:

- Rust test result: `1 passed; 0 failed; 0 ignored; 931 filtered out`.
- Fixture summary: `selected=39`, `passed=39`, `skipped=0`, `failed=0`.
- Summary artifact:
  `/private/tmp/nds-node26-process-timers-wave1/batch/node26__node26_current_lane_process_timers_watchpoint__summary.json`

## Promoted Fixtures

The 39 broad-batch passes were added to
`PROCESS_TIMERS_PROMOTED_NODE26_PATHS` and enforced by
`node26_current_lane_executes_process_timers_promoted_batch_fixture`.

Command:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-process-timers-promote1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_process_timers_promoted_batch_fixture -- --nocapture
```

Result:

- Rust test result: `1 passed; 0 failed; 0 ignored; 932 filtered out`.
- Fixture summary: `selected=39`, `passed=39`, `skipped=0`, `failed=0`.
- Summary artifact:
  `/private/tmp/nds-node26-process-timers-promote1/batch/node26__node26_current_lane_executes_process_timers_promoted_batch__summary.json`

## Failure Grouping

There were no skipped or failed fixtures in either the broad batch or the
promoted batch.

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

- `scripts/runtime/node/watchpoints.py validate`: `validated node-compat watchpoint catalog: 142 entries`
- `tests/runtime/node/compat/node-compat-evidence/latest/status-summary.json`: warnings `0`
- `tests/runtime/node/compat/node-compat-evidence/latest/dashboard-summary.json`: warnings `0`
- `scripts/runtime/node/required_surface_blockers.py`: `node22 required gaps: 0`, `node24 required gaps: 0`

Posture after regeneration:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`
- Node26 `v8_isolate_required`: `744` gaps, `66.17%`

The Node26 count moved from `783` gaps / `64.39%` to `744` gaps /
`66.17%`, burning 39 required-surface gaps in this wave.

## Next Node26 Work

The next high-yield implementation wave should target a broader residual
cluster: either `stream/iter` in the Deno fork, the loader/VM cluster, or the
networking/http/http2/tls groups. Process/timing residual work remains in
`diagnostics_channel`, `perf_hooks`, `os`, and process-host surfaces, but the
pure timers owner is now green for Node26.
