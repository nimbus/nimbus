# NDS3 cycle 39 result - fs WriteStream lifecycle drain

Date: 2026-06-13

## Scope

Promoted the required fixture on both default lanes:

- `test/parallel/test-file-write-stream5.js` (node22, node24)

Fork state:

- Deno fork unchanged at `v2.8.3-nimbus.6` (`7a0edfb282`).
- rusty_v8 unchanged at `v149.4.0-nimbus.1`.
- No local Cargo path override remains.

Nimbus changes:

- Moved `test-file-write-stream5.js` from the single-emit
  `ProcessLifecycleDrain` postlude to the existing
  `ProcessBeforeExitReentry` postlude.
- Added a non-ignored cycle-39 guard for node22 and node24.

## Root Cause

The fixture starts real asynchronous `fs.WriteStream` work from a synchronous
`node:test` body:

- a write callback at line 25
- a `finish` callback at line 20

Node keeps the process alive and only exits after the stream callbacks drain.
Nimbus already mapped this fixture to a process-lifecycle postlude, but the
single-emit variant could run `beforeExit`/`exit` and common call checks before
the `fs.WriteStream` callbacks fired. The existing reentry postlude repeatedly
pumps loop work before the terminal exit emission, which matches the fixture's
process-liveness requirement without changing the upstream fixture.

Pre-fix focused census:

```bash
gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-file-write-stream5.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node24 summary: selected=1, passed=0, skipped=0, failed=1
runtime JavaScript error: AssertionError [ERR_ASSERTION]: Mismatched function calls:
Expected <anonymous> to be called exactly 1, actual 0.
Expected noop to be called exactly 1, actual 0.
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 829 filtered out; finished in 1.93s
```

## Proof

Focused post-fix censuses:

```bash
gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-file-write-stream5.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 829 filtered out; finished in 3.02s
```

```bash
gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-file-write-stream5.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 829 filtered out; finished in 2.93s
```

Promoted non-ignored guard:

```bash
gtimeout -s KILL 120 cargo test -p nimbus-runtime --lib \
  cycle39_file_write_stream -- --nocapture
```

```text
node_compat node22-supported-lane-executes-cycle39-file-write-stream-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle39-file-write-stream-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 829 filtered out; finished in 5.97s
```

Generated pipeline:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do \
  /opt/homebrew/bin/python3.12 scripts/runtime/node/$s.py >/dev/null; \
done
```

Generated posture after cycle 39:

```text
node22 60 97.47
node24 70 97.1
```

## Result

`test-file-write-stream5.js` is no longer a `v8_isolate_required` gap in either
default lane. Gate remains red and honest:

- node22: 60 gaps
- node24: 70 gaps
