# Node Compatibility Harness Timeouts And Hangs

The Node compatibility harness treats every official fixture as a bounded
measurement. A fixture may become a support claim only when it runs inside the
lane's runtime budget, avoids the ignored-watchpoint catalog, and has no
unclassified remainder in the generated status report.

## Wall-Clock Budget

The harness derives each fixture's wall-clock timeout from the runtime limits
selected for that exact lane and fixture:

```text
fixture wall-clock timeout = RuntimeLimits.execution_timeout + 5 seconds
```

Most fixtures use the application Node budget of 30 seconds, so their harness
deadline is 35 seconds. Nested subprocess-style fixtures such as
`test-runner-reporters.js` raise the runtime budget to 120 seconds and therefore
still have a finite 125 second harness deadline.

The wall-clock timeout wraps the full `invoke_bundle(...)` call. Runtime
execution timeout remains the primary cancellation mechanism; the harness
deadline is the outer guard that prevents a stalled fixture from looking like a
test process hang.

## Diagnostic Artifacts

Fixture runtime errors, non-OK payloads, mismatched fixture payloads, non-zero
process exits, and wall-clock timeouts write a structured diagnostic artifact.

Default artifact root:

```text
target/node-compat/diagnostics/
```

`scripts/runtime/node/report.sh` sets the diagnostic root to
`<output-root>/diagnostics` so live slice captures keep diagnostics beside the
plan and observed-result reports.

Each artifact uses `report_kind:
node_compat_fixture_execution_diagnostic` and records:

- lane
- fixture path
- bundle path
- diagnostic family
- outcome
- timeout and elapsed milliseconds
- detail
- exit criterion

## Diagnostic Families

The harness classifies failure artifacts into these families:

| Family | Examples | Exit Criterion |
| --- | --- | --- |
| `event_loop` | `test-next-tick-doesnt-hang.js`, timer, promise, async fixtures | Timer, microtask, and nextTick drains settle inside the per-fixture wall-clock budget. |
| `vm` | `test-vm-basic.js`, VM context regressions | Dynamic-code execution stays bounded by timeout diagnostics and production admission policy. |
| `worker` | `test-worker.js`, cluster-worker lifecycle fixtures | Worker lifecycle, cancellation, and policy inheritance are bounded by production profile tests. |
| `message_port` | `test-worker-message-port.js` | Worker MessagePort fixtures are promoted only after NLRT8 proves production in-process profiles do not grant `worker_threads`, or after bounded teardown diagnostics and the watchpoint catalog remove the ignore. |
| `subprocess` | `test-runner-reporters.js`, `test-process-*`, WASI self-exec fixtures | Execution stays on the `$runtime_self_exec` seam or a service/microVM profile, never ambient process execution. |
| `general` | Unclassified fixture failures | A specific owner classification is required before the fixture can become a support claim. |

## Watchpoint Accounting

Ignored Rust node-compat tests are watchpoints, not green support. The current
guard path is:

```bash
python3 scripts/runtime/node/watchpoints.py validate
python3 scripts/runtime/node/classifications.py sync --preserve-existing --check
python3 scripts/runtime/node/status.py --output-root target/node-compat/harness-hardening-status
```

The generated status report must keep `rust_ignore_count` equal to the
watchpoint catalog entry count, produce zero unexpected passes, and leave zero
unclassified fixtures for supported LTS lanes before support evidence is
published.

## Worker MessagePort / VM Promotion Gate

`test-worker-message-port.js` is part of the current measured worker basics
contract, but production trust is stricter than fixture greenness. Worker and
MessagePort behavior must not imply production in-process `worker_threads`
authority. NLRT8 owns the permission profile split that proves production
in-process Node profiles exclude worker authority unless a later profile
explicitly routes that work to a bounded service or microVM execution model.
