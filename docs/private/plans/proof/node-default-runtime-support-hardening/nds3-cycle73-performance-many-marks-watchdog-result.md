# NDS3 cycle 73: performance-many-marks watchdog reclassification

Date: 2026-06-14

Branch: `codex/node-default-runtime-support-hardening`  
PR: #10 (draft)  
Deno fork pin: unchanged at `v2.8.3-nimbus.24`

## Fixture

- `test/parallel/test-performance-many-marks.js` (node22 + node24)

The upstream fixture source is a host-scale performance timeline stressor:

```js
require('../common');

for (let i = 0; i < 1e6; i++) {
  performance.mark(`mark-${i}`);
}

performance.getEntriesByName('mark-0');
performance.clearMarks();
```

It has no observable API assertion beyond completing one million synchronous
`performance.mark()` calls, then one lookup and clear. This is the same class of
multi-tenant isolate fairness boundary as the existing
`isolate_execution_termination_watchdog` precedent: the default V8 isolate must
retain a wall-clock execution deadline and cannot promise completion of
host-scale synchronous diagnostic loops as required Application behavior.

## Dynamic census

Scratch probe was added temporarily as `nds_probe` and removed before this
checkpoint.

Node24 command:

```bash
gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-performance-many-marks.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture \
  2>&1 | grep -iE 'summary: selected|should execute|error\[|FAILED|test-performance-many-marks|terminated|Cannot evaluate|panicked'
```

Result:

```text
node_compat nds-probe node24 summary: selected=1, passed=0, skipped=0, failed=1
runtime JavaScript error: Cannot evaluate dynamically imported module, because JavaScript execution has been terminated
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 878 filtered out; finished in 2.87s
```

Node22 command was identical except `NIMBUS_RECENSUS_LANE=node22`.

Result:

```text
node_compat nds-probe node22 summary: selected=1, passed=0, skipped=0, failed=1
runtime JavaScript error: Cannot evaluate dynamically imported module, because JavaScript execution has been terminated
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 878 filtered out; finished in 2.76s
```

## Disposition

Reclassified in `scripts/runtime/node/default_support_posture.py`:

- denominator: `upstream_or_platform_boundary`
- reason code: `isolate_execution_termination_watchdog`
- shim classification: `unsupported`

No fixture/checker was edited. No derived posture JSON was hand-edited.

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
node22 v8_isolate_required.gaps = 23, pass_rate_percent = 99.03
node24 v8_isolate_required.gaps = 29, pass_rate_percent = 98.79
```

Before this cycle, the generated posture was:

```text
node22 v8_isolate_required.gaps = 24, pass_rate_percent = 98.99
node24 v8_isolate_required.gaps = 30, pass_rate_percent = 98.75
```

## Cleanup

- Removed scratch `nds_probe.rs`.
- Removed the temporary local Deno path override.
- Reverted exploratory `deno_web` implementation changes after they did not
  make the fixture dynamically green.
- Verified `/Users/jack/src/github.com/nimbus/deno` is clean at
  `v2.8.3-nimbus.24`.

## Verifier

Command:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
```

Result: red, as expected. Summary was `13 passed, 21 failed`; step 9 still fails
because the regenerated posture is node22=23 / node24=29, not 0/0. This checkout
also reports private-plan/proof closeout failures because those private proof
files are not present in the worktree.
