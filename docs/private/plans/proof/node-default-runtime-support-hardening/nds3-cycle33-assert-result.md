# NDS3 cycle 33 result - assert promotion

Date: 2026-06-13

## Scope

Promoted the already-dynamically-green node22 required fixture:

- `test/parallel/test-assert.js`

No Deno fork, rusty_v8, or Nimbus runtime behavior change was needed. The fixture was an unpromoted-support gap.

## Proof

Focused node22 census:

```bash
gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-assert.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture 2>&1 \
  | grep -iE 'summary: selected|test result|should execute|error\[|FAILED|deep-equal|\+ actual|- expected|AssertionError|at async.*test-assert|ERR_'
```

Result:

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 818 filtered out
```

Promoted non-ignored guard:

```bash
gtimeout -s KILL 90 cargo test -p nimbus-runtime --lib cycle33_assert -- --nocapture 2>&1 \
  | grep -iE 'summary: selected|test result|should execute|error\[|FAILED|failed='
```

Result:

```text
node_compat node22-supported-lane-executes-cycle33-assert-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 818 filtered out
```

Generated posture after classification sync and evidence regeneration:

```text
node22: v8_isolate_required.gaps = 66, pass_rate_percent = 97.22
node24: v8_isolate_required.gaps = 76, pass_rate_percent = 96.85
unique required fixtures remaining: 78
```

## Guardrails

- No V8 or rusty_v8 changes.
- No Deno fork changes.
- No official fixture or checker edits.
- No hand-edited false-green JSON.
- Scratch `nds_probe` include/file was removed before promotion.
- PR #10 remains draft; the gate is still red and honest.
