# NDS3 Node26 Cycle 2: Loader Context VM Promotion

## Scope

This checkpoint extends Node26 Current required-surface coverage for the
`loader-context/vm` owner. The broad diagnostic batch used the existing VM
cluster staging and the same fatal `vm.Module` prefix exclusions used by the
Node22/Node24 VM watchpoints. No Deno fork changes, rusty_v8 changes, fixture
edits, checker edits, or generated false-green JSON hand edits were made.

## Broad Pre-Run

Command:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-loader-context-vm-wave1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_loader_context_vm_watchpoint -- --ignored --nocapture
```

Result:

- Rust test result: `1 passed; 0 failed; 0 ignored; 919 filtered out`.
- Fixture summary: `selected=63`, `passed=63`, `skipped=0`, `failed=0`.
- Summary artifact:
  `/private/tmp/nds-node26-loader-context-vm-wave1/batch/node26__node26_current_lane_loader_context_vm_watchpoint__summary.json`

## Promoted Fixtures

The 63 broad-batch passes were added to
`LOADER_CONTEXT_VM_PROMOTED_NODE26_PATHS` and enforced by
`node26_current_lane_executes_loader_context_vm_promoted_batch_fixture`.

Command:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-loader-context-vm-promote1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_loader_context_vm_promoted_batch_fixture -- --nocapture
```

Result:

- Rust test result: `1 passed; 0 failed; 0 ignored; 920 filtered out`.
- Fixture summary: `selected=63`, `passed=63`, `skipped=0`, `failed=0`.
- Summary artifact:
  `/private/tmp/nds-node26-loader-context-vm-promote1/batch/node26__node26_current_lane_executes_loader_context_vm_promoted_batch__summary.json`

## Failure Grouping

The broad runnable VM slice had no failures. The `loader-context/vm` owner still
has 23 Node26 required-surface gaps after this checkpoint, all outside this
runnable batch. They are primarily the fatal-abort `test/parallel/test-vm-module-*`
surface that remains excluded from this broad VM watchpoint until a focused
`vm.Module` strategy is selected.

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

- `scripts/runtime/node/watchpoints.py validate`: `validated node-compat watchpoint catalog: 136 entries`
- `tests/runtime/node/compat/node-compat-evidence/latest/status-summary.json`: warnings `0`
- `tests/runtime/node/compat/node-compat-evidence/latest/dashboard-summary.json`: warnings `0`

Posture after regeneration:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`
- Node26 `v8_isolate_required`: `1081` gaps, `50.84%`

The Node26 count moved from `1144` gaps / `47.98%` to `1081` gaps / `50.84%`,
matching the 63 dynamically proven VM fixture promotions in this wave.

## Next Node26 Work

Recommended next wave: pivot to the largest remaining high-yield owners rather
than the residual `vm.Module` set. The biggest remaining clusters are
`node-compat/unpromoted-surface`, `streams-local-io/fs-host-io`,
`streams-local-io/stream`, `process-and-timing/diagnostics-channel`, and
`process-and-timing/timers`.
