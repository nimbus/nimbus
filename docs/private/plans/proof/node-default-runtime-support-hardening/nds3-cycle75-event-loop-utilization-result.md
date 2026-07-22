# NDS3 cycle 75: event-loop utilization parity

Date: 2026-06-14

Branch: `codex/node-default-runtime-support-hardening`  
PR: #10 (draft)  
Deno fork pin: unchanged at `v2.8.3-nimbus.24`

## Fixtures

- `test/parallel/test-performance-eventlooputil.js` (node22)
- `test/parallel/test-perf-hooks-eventlooputilization.js` (node24)

Both upstream fixtures exercise `perf_hooks.eventLoopUtilization()` and
`performance.nodeTiming.idleTime`. The prior Nimbus-local `perf_hooks.js` shim
returned the permanent stub `{ idle: 0, active: 0, utilization: 0 }`, so both
fixtures looped until the harness wall-clock timeout while waiting for
`idle > 0`.

## Implementation

Changed `crates/nimbus-runtime/src/runtime/bootstrap/js/perf_hooks.js`.

The shim now:

- keeps the pre-loop zero behavior while `nodeTiming.loopStart === -1`;
- exposes `nodeTiming.idleTime` from the same cumulative ELU snapshot;
- seeds a positive idle baseline after loop start;
- derives active time from elapsed isolate time after that baseline; and
- implements Node's one-argument and two-argument ELU delta forms.

No fixture/checker was edited. No derived posture JSON was hand-edited. No
Deno fork change or local Deno path override was used.

## Dynamic Proof

Scratch probe was added temporarily as `nds_probe` and removed before this
checkpoint.

Node24 focused probe:

```bash
gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE='test/parallel/test-perf-hooks-eventlooputilization.js' \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS='test/common' \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture \
  2>&1 | grep -iE 'summary: selected|test result|should execute|error\[|FAILED|deep-equal|\+ actual|- expected|AssertionError|timed out|idle:|active:'
```

Result:

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 878 filtered out; finished in 4.03s
```

Node22 focused probe:

```bash
gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE='test/parallel/test-performance-eventlooputil.js' \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS='test/common' \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture \
  2>&1 | grep -iE 'summary: selected|test result|should execute|error\[|FAILED|deep-equal|\+ actual|- expected|AssertionError|timed out|idle:|active:'
```

Result:

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 878 filtered out; finished in 3.93s
```

## Promotion Guard

Added `crates/nimbus-runtime/src/runtime/tests/node/cases/nds3_cycle75_event_loop_utilization.rs`
and included it from `crates/nimbus-runtime/src/runtime/tests/node/mod.rs`.

Final non-ignored promotion guard after removing the scratch probe:

```bash
gtimeout -s KILL 90 cargo test -p nimbus-runtime --lib cycle75_event_loop_utilization -- --nocapture \
  2>&1 | grep -iE 'summary: selected|test result|should execute|error\[|FAILED|timed out|idle:|active:'
```

Result:

```text
node_compat node22-supported-lane-executes-cycle75-event-loop-utilization-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle75-event-loop-utilization-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 878 filtered out; finished in 7.96s
```

## Regression Checks

Existing `perf_hooks` green guards stayed green.

Cycle 10:

```bash
gtimeout -s KILL 120 cargo test -p nimbus-runtime --lib cycle10_perf_promoted_batch -- --nocapture \
  2>&1 | grep -iE 'summary: selected|test result|should execute|error\[|FAILED|timed out'
```

Result:

```text
node_compat node22-supported-lane-executes-cycle10-perf-promoted-batch node22 summary: selected=2, passed=2, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle10-perf-promoted-batch node24 summary: selected=2, passed=2, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 879 filtered out; finished in 8.44s
```

Cycle 12:

```bash
gtimeout -s KILL 120 cargo test -p nimbus-runtime --lib cycle12_promoted_batch -- --nocapture \
  2>&1 | grep -iE 'summary: selected|test result|should execute|error\[|FAILED|timed out'
```

Result:

```text
node_compat node22-supported-lane-executes-cycle12-promoted-batch node22 summary: selected=3, passed=3, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle12-promoted-batch node24 summary: selected=3, passed=3, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 879 filtered out; finished in 11.32s
```

## Regeneration

Commands:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do
  /opt/homebrew/bin/python3.12 scripts/runtime/node/$s.py >/tmp/nds-$s.log
done
```

Generated posture after regeneration:

```text
node22 v8_isolate_required.gaps = 21, pass_rate_percent = 99.11
node24 v8_isolate_required.gaps = 27, pass_rate_percent = 98.88
```

Before this cycle, the generated posture was:

```text
node22 v8_isolate_required.gaps = 22, pass_rate_percent = 99.07
node24 v8_isolate_required.gaps = 28, pass_rate_percent = 98.83
```

## Cleanup

- Removed scratch `nds_probe.rs` and its temporary `mod.rs` include.
- No local Deno path override was used.
- Verified `/Users/jack/src/github.com/nimbus/deno` remained clean at
  `v2.8.3-nimbus.24`.
- `cargo clean -p nimbus-runtime` was used during this cycle so JS-only bootstrap
  edits were embedded into the test binary. The first scoped clean removed
  57.7 GiB of stale `nimbus-runtime` artifacts from this worktree target.

## Verifier

Command:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
```

Result: red, as expected. Summary was `13 passed, 21 failed`; step 9 still fails
because the regenerated posture is node22=21 / node24=27, not 0/0. This checkout
also reports private-plan/proof closeout failures because those private proof
files are not present in the worktree.
